//! Keep rig's Codex contract, restoring only its discarded final-answer format.

use rig_core::{
    completion::CompletionRequest,
    error::EncodeError,
    operation::Completion,
    providers::openai,
    wire::{Body, Descriptor, Encoded, Mode, Wire, WireFrame},
};

#[derive(Clone)]
pub(crate) struct Responses(pub openai::responses_api::wire::Responses);

impl Wire for Responses {
    type Op = Completion;
    type Payload = Encoded;
    type Frame = WireFrame;
    type Decoder<'id> = <openai::responses_api::wire::Responses as Wire>::Decoder<'id>;

    fn describe(&self) -> Descriptor<'_> {
        self.0.describe()
    }

    fn encode(&self, request: CompletionRequest, mode: Mode) -> Result<Encoded, EncodeError> {
        let text = request
            .additional_params
            .as_ref()
            .and_then(|params| params.get("text"))
            .cloned();
        let mut encoded = self.0.encode(request, mode)?;
        if let Some(text) = text {
            // rig 0.43's ResponsesContract::Codex drops text unconditionally. Retain its
            // header, instructions, encrypted-replay and relaxed-SSE behavior, and add
            // just Responses text.format back after its encoder has applied that contract.
            let Body::Bytes(bytes) = encoded.request.body_mut() else {
                unreachable!("Responses encodes JSON bytes")
            };
            let mut body: serde_json::Value = serde_json::from_slice(bytes)?;
            body["text"] = text;
            *bytes = serde_json::to_vec(&body)?;
        }
        Ok(encoded)
    }

    fn decoder<'id>(&self) -> Self::Decoder<'id> {
        self.0.decoder()
    }
}
