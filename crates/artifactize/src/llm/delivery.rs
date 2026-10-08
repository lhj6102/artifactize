//! Transparent public-summary observation and normalized text delivery. The provider bytes,
//! errors, backpressure and retry decisions remain rig's; private reasoning never reaches UI.
#[cfg(test)]
mod framing_tests;
use crate::agent::session::{Delivery, DeliveryKind, DeliveryState};
use bytes::Bytes;
use futures_util::StreamExt;
use rig_core::{
    http_client::{
        HttpClientExt, LazyBody, MultipartForm, Request, Response, Result as HttpResult,
        StreamingResponse, framing::SseFramer,
    },
    message::{AssistantContent, ReasoningContent},
    streaming::{Item, StreamEvent},
};
use serde::Deserialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Mutex},
    time::Duration,
};

/// Flush display updates at ten frames per second, independently of transport polling.
pub(super) const CADENCE: Duration = Duration::from_millis(100);
/// Observer-only framing cap. Oversized frames degrade observation, never the original stream.
const FRAME_BYTES: usize = 256 * 1024;
/// Bound coalesced deltas until the consuming task drains them.
const PENDING_BYTES: usize = 64 * 1024;
/// Framing slices bound allocations even when the transport yields a very large chunk.
const FRAME_SLICE: usize = 4096;
/// Bound pathological empty summary parts and provider item identities independently.
const SUMMARY_PARTS: usize = 256;
const ITEM_ID_BYTES: usize = 1024;

struct Pending {
    text: String,
    state: DeliveryState,
    order: usize,
}
struct Summary {
    item: Option<String>,
    index: usize,
    text: String,
}
#[derive(Default)]
struct Buffer {
    pending: BTreeMap<(DeliveryKind, String), Pending>,
    summaries: BTreeMap<String, Summary>,
    ended: BTreeSet<(DeliveryKind, usize)>,
    dropped: bool,
    ordinal: usize,
}
#[derive(Clone, Default)]
pub(crate) struct Sink(Arc<Mutex<Buffer>>);
tokio::task_local! { static ACTIVE: Sink; }
impl Sink {
    pub async fn scope<F: std::future::Future>(&self, future: F) -> F::Output {
        ACTIVE.scope(self.clone(), future).await
    }
    fn push(&self, kind: DeliveryKind, block: String, text: &str) {
        if text.is_empty() {
            return;
        }
        let mut buffer = self.0.lock().unwrap();
        if buffer
            .pending
            .values()
            .map(|entry| entry.text.len())
            .sum::<usize>()
            .saturating_add(text.len())
            > PENDING_BYTES
        {
            buffer.dropped = true;
            return;
        }
        if buffer.pending.len() >= SUMMARY_PARTS
            && !buffer.pending.contains_key(&(kind, block.clone()))
        {
            buffer.dropped = true;
            return;
        }
        let order = buffer.ordinal;
        buffer.ordinal = buffer.ordinal.saturating_add(1);
        let entry = buffer.pending.entry((kind, block)).or_insert(Pending {
            text: String::new(),
            state: DeliveryState::Delta,
            order,
        });
        entry.text.push_str(text);
    }
    fn complete(&self, kind: DeliveryKind, block: String, text: String) {
        let mut buffer = self.0.lock().unwrap();
        let order = buffer
            .pending
            .get(&(kind, block.clone()))
            .map_or(buffer.ordinal, |entry| entry.order);
        buffer.ordinal = buffer.ordinal.saturating_add(1);
        buffer.pending.insert(
            (kind, block),
            Pending {
                text,
                state: DeliveryState::Complete,
                order,
            },
        );
    }
    pub fn ready(&self) -> bool {
        self.0
            .lock()
            .unwrap()
            .pending
            .values()
            .map(|entry| entry.text.len())
            .sum::<usize>()
            >= PENDING_BYTES / 2
    }
    pub fn take(&self, turn: usize, attempt: usize) -> Vec<Delivery> {
        let mut buffer = self.0.lock().unwrap();
        let mut pending = std::mem::take(&mut buffer.pending)
            .into_iter()
            .collect::<Vec<_>>();
        pending.sort_by_key(|(_, entry)| entry.order);
        pending
            .into_iter()
            .map(|((kind, block), entry)| Delivery {
                turn,
                attempt,
                block,
                kind,
                text: entry.text,
                state: entry.state,
            })
            .collect()
    }
    fn summary(&self, item: Option<String>, output: Option<usize>, index: usize, text: String) {
        if item.as_ref().is_some_and(|id| id.len() > ITEM_ID_BYTES) {
            return;
        }
        let Some(identity) = item
            .as_ref()
            .map(|id| format!("item-{id}"))
            .or_else(|| output.map(|index| format!("slot-{index}")))
        else {
            return;
        };
        let block = format!("summary-{identity}-{index}");
        {
            let mut buffer = self.0.lock().unwrap();
            let observed = buffer
                .summaries
                .values()
                .map(|entry| entry.text.len())
                .sum::<usize>();
            if buffer.summaries.len() < SUMMARY_PARTS
                && observed.saturating_add(text.len()) <= PENDING_BYTES
            {
                buffer
                    .summaries
                    .entry(block.clone())
                    .or_insert(Summary {
                        item,
                        index,
                        text: String::new(),
                    })
                    .text
                    .push_str(&text);
            } else {
                buffer.dropped = true;
            }
        }
        self.push(DeliveryKind::Summary, block, &text);
    }
    fn final_summary(&self, part: usize, content: &AssistantContent) {
        let AssistantContent::Reasoning(sealed) = content else {
            return;
        };
        let Some(reasoning) = sealed.open(sealed.issuer()) else {
            return;
        };
        let summaries = reasoning
            .content
            .iter()
            .filter_map(|part| match part {
                ReasoningContent::Summary(text) => Some(text),
                _ => None,
            })
            .collect::<Vec<_>>();
        for (index, text) in summaries.into_iter().enumerate() {
            // Prefer recorded item identity+summary index. Equality is a safe fallback for
            // providers omitting item identity; never join a summary to "the latest" block.
            let block = {
                let buffer = self.0.lock().unwrap();
                buffer
                    .summaries
                    .iter()
                    .find(|(_, entry)| {
                        entry.index == index
                            && (reasoning
                                .id
                                .as_ref()
                                .is_some_and(|id| entry.item.as_ref() == Some(id))
                                || entry.text == *text)
                    })
                    .map(|(block, _)| block.clone())
            };
            self.complete(
                DeliveryKind::Summary,
                block.unwrap_or_else(|| format!("summary-final-{part}-{index}")),
                text.clone(),
            );
        }
    }
    pub fn normalized(&self, item: &Item<StreamEvent>) {
        match item {
            Item::Event(StreamEvent::Text { part, text }) => {
                self.push(DeliveryKind::Text, format!("text-{}", part.index()), text)
            }
            Item::Event(StreamEvent::End { part, content }) => match content {
                AssistantContent::Text(text) => {
                    self.complete(
                        DeliveryKind::Text,
                        format!("text-{}", part.index()),
                        text.text.clone(),
                    );
                    self.0
                        .lock()
                        .unwrap()
                        .ended
                        .insert((DeliveryKind::Text, part.index()));
                }
                AssistantContent::Reasoning(_) => {
                    self.final_summary(part.index(), content);
                    self.0
                        .lock()
                        .unwrap()
                        .ended
                        .insert((DeliveryKind::Summary, part.index()));
                }
                _ => {}
            },
            // Reasoning deltas conflate raw thought and public summaries in rig 0.43.
            // Only the wire allowlist and typed Summary finalization are displayable.
            _ => {}
        }
    }
    /// Release the mutex before any completion branch reacquires it.
    fn ended(&self, kind: DeliveryKind, index: usize) -> bool {
        self.0.lock().unwrap().ended.contains(&(kind, index))
    }
    pub fn final_response(&self, choice: &[AssistantContent]) {
        for (index, content) in choice.iter().enumerate() {
            match content {
                AssistantContent::Text(text) => {
                    if !self.ended(DeliveryKind::Text, index) {
                        self.complete(
                            DeliveryKind::Text,
                            format!("text-{index}"),
                            text.text.clone(),
                        );
                    }
                }
                AssistantContent::Reasoning(_) if !self.ended(DeliveryKind::Summary, index) => {
                    self.final_summary(index, content);
                }
                _ => {}
            }
        }
    }
}

#[derive(Clone)]
pub(crate) struct Tap(rig_reqwest::ReqwestClient);
impl Tap {
    pub fn new(client: reqwest::Client) -> Self {
        Self(rig_reqwest::ReqwestClient::from(client))
    }
}
impl HttpClientExt for Tap {
    fn send<T, U>(
        &self,
        request: Request<T>,
    ) -> impl std::future::Future<Output = HttpResult<Response<LazyBody<U>>>> + Send + 'static
    where
        T: Into<Bytes> + Send,
        U: From<Bytes> + Send + 'static,
    {
        self.0.send(request)
    }
    fn send_multipart<U>(
        &self,
        request: Request<MultipartForm>,
    ) -> impl std::future::Future<Output = HttpResult<Response<LazyBody<U>>>> + Send + 'static
    where
        U: From<Bytes> + Send + 'static,
    {
        self.0.send_multipart(request)
    }
    async fn send_streaming<T>(&self, request: Request<T>) -> HttpResult<StreamingResponse>
    where
        T: Into<Bytes> + Send,
    {
        let sink = ACTIVE.try_with(Clone::clone).ok();
        let response = self.0.send_streaming(request).await?;
        let (parts, body) = response.into_parts();
        let mut observer = Observer::default();
        let stream = body.inspect(move |chunk| {
            if let (Some(sink), Ok(bytes)) = (&sink, chunk) {
                observer.push(bytes, sink);
            }
        });
        Ok(Response::from_parts(parts, Box::pin(stream)))
    }
}

#[derive(Default)]
struct Observer {
    framer: SseFramer,
    discarding: bool,
    previous: u8,
}
/// Tagged allowlist avoids even allocating unrelated raw thought/unknown payload fields.
#[derive(Deserialize)]
#[serde(tag = "type")]
enum SummaryFrame {
    #[serde(rename = "response.reasoning_summary_text.delta")]
    Delta {
        output_index: Option<usize>,
        item_id: Option<String>,
        summary_index: Option<usize>,
        delta: String,
    },
    #[serde(other)]
    Other,
}
impl Observer {
    fn push(&mut self, bytes: &[u8], sink: &Sink) {
        for chunk in bytes.chunks(FRAME_SLICE) {
            if self.discarding {
                let mut resume = None;
                for (index, &byte) in chunk.iter().enumerate() {
                    if byte == b'\n' && self.previous == b'\n' {
                        self.discarding = false;
                        self.framer = SseFramer::new();
                        resume = Some(index + 1);
                        break;
                    }
                    if byte != b'\r' {
                        self.previous = byte;
                    }
                }
                if let Some(index) = resume {
                    self.push(&chunk[index..], sink);
                }
                continue;
            }
            for frame in self.framer.push(chunk) {
                if let Ok(SummaryFrame::Delta {
                    output_index,
                    item_id,
                    summary_index,
                    delta,
                }) = serde_json::from_str(&frame.data)
                {
                    sink.summary(item_id, output_index, summary_index.unwrap_or(0), delta);
                }
            }
            if self.framer.pending() > FRAME_BYTES {
                self.framer = SseFramer::new();
                self.discarding = true;
                self.previous = chunk.last().copied().unwrap_or_default();
            }
        }
    }
}
