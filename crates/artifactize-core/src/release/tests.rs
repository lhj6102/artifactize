use super::*;
use futures_util::FutureExt;
use std::panic::{AssertUnwindSafe, resume_unwind};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::time::UNIX_EPOCH;
use tokio_util::sync::CancellationToken;

/// A loopback stand-in for GitHub's latest-release API answering `body`, and how many
/// requests it answered.
struct Releases {
    url: String,
    requests: Arc<AtomicUsize>,
    stop: CancellationToken,
    task: tokio::task::JoinHandle<()>,
}

impl Releases {
    async fn answering(status: u16, body: &'static str, test: impl AsyncFnOnce(&Self)) {
        let requests = Arc::new(AtomicUsize::new(0));
        let counted = requests.clone();
        let app = axum::Router::new().route(
            "/latest",
            axum::routing::get(move || {
                counted.fetch_add(1, Ordering::SeqCst);
                async move {
                    (
                        axum::http::StatusCode::from_u16(status).unwrap(),
                        [("content-type", "application/json")],
                        body,
                    )
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/latest", listener.local_addr().unwrap());
        let stop = CancellationToken::new();
        let shutdown = stop.clone().cancelled_owned();
        let task = tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(shutdown)
                .await
                .unwrap();
        });
        let releases = Self {
            url,
            requests,
            stop,
            task,
        };
        // An assertion failure must still join the server and the connections it accepted.
        let outcome = AssertUnwindSafe(test(&releases)).catch_unwind().await;
        releases.close().await;
        if let Err(panic) = outcome {
            resume_unwind(panic);
        }
    }

    fn source(&self, site: &str) -> Source {
        Source {
            url: self.url.clone(),
            site: site.into(),
        }
    }

    async fn close(mut self) {
        self.stop.cancel();
        (&mut self.task).await.unwrap();
    }

    fn requests(&self) -> usize {
        self.requests.load(Ordering::SeqCst)
    }
}

impl Drop for Releases {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

fn version(text: &str) -> Version {
    Version::parse(text).unwrap()
}

/// A fixed time, and the same time `seconds` later.
fn at(seconds: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(1_800_000_000 + seconds)
}

const DAY: u64 = 24 * 60 * 60;

#[test]
fn update_links_are_tied_to_the_version_the_person_chose() {
    let old = version("0.16.0");
    let next = version("0.17.0");
    let old_page = Channel::Unknown.update(&old).unwrap();
    assert_eq!(
        old_page,
        Update::Page {
            url: format!("{SITE}/releases/tag/v0.16.0")
        }
    );
    assert_ne!(Channel::Unknown.update(&next).unwrap(), old_page);
    assert!(
        matches!(Channel::Deb.update(&old).unwrap(), Update::Download { url, .. }
        if url == format!("{SITE}/releases/download/v0.16.0/Artifactize-amd64.deb"))
    );
}

#[tokio::test]
async fn a_newer_release_comes_with_the_channels_update_and_is_asked_once_a_day() {
    Releases::answering(
        200,
        r#"{"tag_name":"v0.16.0","name":"0.16.0"}"#,
        async |releases| {
            let state = crate::test_os::tempdir();
            let running = version("0.15.1");
            let newer = Outcome::Newer {
                latest: version("0.16.0"),
                update: Update::Page {
                    url: "https://github.com/lhj6102/artifactize/releases/tag/v0.16.0".into(),
                },
            };
            let source = releases.source(SITE);
            let look = |now| look_up(&running, Channel::Unknown, &source, state.path(), now);
            assert_eq!(look(at(0)).await, newer);
            assert_eq!(look(at(DAY - 1)).await, newer);
            assert_eq!(releases.requests(), 1);
            assert_eq!(look(at(DAY)).await, newer);
            assert_eq!(releases.requests(), 2);
        },
    )
    .await;
}

#[tokio::test]
async fn the_latest_release_or_an_older_one_is_no_news() {
    Releases::answering(200, r#"{"tag_name":"v0.15.1"}"#, async |releases| {
        let state = crate::test_os::tempdir();
        for running in ["0.15.1", "0.16.0-rc.1"] {
            assert_eq!(
                look_up(
                    &version(running),
                    Channel::InstallSh,
                    &releases.source(SITE),
                    state.path(),
                    at(0),
                )
                .await,
                Outcome::Latest(version("0.15.1")),
                "{running}"
            );
        }
        assert_eq!(
            releases.requests(),
            1,
            "the second lookup reused the kept answer"
        );
        // An answer is kept for the site that gave it: another site is asked anew.
        look_up(
            &version("0.15.1"),
            Channel::InstallSh,
            &releases.source("http://127.0.0.1:1/stand-in"),
            state.path(),
            at(0),
        )
        .await;
        assert_eq!(releases.requests(), 2);
    })
    .await;
}

#[tokio::test]
async fn a_failed_lookup_is_reported_and_asked_again() {
    Releases::answering(
        403,
        r#"{"message":"API rate limit exceeded"}"#,
        async |releases| {
            let state = crate::test_os::tempdir();
            let running = version("0.15.1");
            for _ in 0..2 {
                let outcome = look_up(
                    &running,
                    Channel::Cargo,
                    &releases.source(SITE),
                    state.path(),
                    at(0),
                )
                .await;
                assert!(
                    matches!(&outcome, Outcome::Failed(error) if error.contains("403")),
                    "{outcome:?}"
                );
            }
            assert_eq!(releases.requests(), 2);
        },
    )
    .await;
}

#[test]
fn a_development_build_and_the_store_are_not_looked_up() {
    let unset = |_: &str| None;
    assert_eq!(
        source(&version("0.0.0-dev"), Channel::Cargo, unset).err(),
        Some(Outcome::Unchecked(Unchecked::Development))
    );
    assert_eq!(
        source(&version("0.15.1"), Channel::Store, unset).err(),
        Some(Outcome::Unchecked(Unchecked::Store))
    );
}

#[test]
fn an_answer_kept_at_an_impossible_time_is_not_current() {
    assert!(fresh(at(0), at(DAY - 1)));
    assert!(!fresh(at(DAY), at(0)));
    let kept = format!(
        r#"{{"checkedAt":{},"site":"{SITE}","latest":"1.0.0"}}"#,
        u64::MAX
    );
    assert!(serde_json::from_str::<Cached>(&kept).is_err());
}
