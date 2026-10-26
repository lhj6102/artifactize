//! Newer releases: whether one is out, and how to update the way this copy was installed.
//!
//! artifactize never updates itself. Every install channel keeps its own update path, and
//! only the Microsoft Store updates on its own, so a newer release is told together with the
//! update command of the channel this copy came from (#316). The desktop app and
//! `artifactize doctor` show the same lookup, made at most once a day and kept in the state.

#[cfg(test)]
mod tests;

use std::{
    io::Write,
    path::Path,
    time::{Duration, SystemTime},
};

use semver::Version;
use serde::{Deserialize, Serialize};

/// GitHub's latest stable release of the repository: drafts and prereleases are never latest.
const LATEST_URL: &str = "https://api.github.com/repos/lhj6102/artifactize/releases/latest";
/// The repository's site, below which its releases' pages and files are.
const SITE: &str = "https://github.com/lhj6102/artifactize";
/// Where a stand-in site answers the latest-release lookup, below its root.
const TEST_LATEST_PATH: &str = "/api/releases/latest";
/// Set to anything but empty, artifactize does not look up releases or contact GitHub.
pub const OFF_VARIABLE: &str = "ARTIFACTIZE_NO_UPDATE_CHECK";
/// A loopback URL standing in for the repository's site, for tests: it answers the lookup
/// below [`TEST_LATEST_PATH`], and the release's pages and files are linked below it. It is
/// asked even by a development build; its answer is kept as GitHub's is, for its own site.
pub const TEST_ENDPOINT_VARIABLE: &str = "ARTIFACTIZE_RELEASES_URL";
/// About once a day: a release is not urgent, and GitHub's API allows an address 60
/// unauthenticated requests an hour.
const INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
/// How often a long-running app asks again: hourly finds a release out that day, while the
/// lookup itself reaches GitHub at most once a day.
const RECHECK: Duration = Duration::from_secs(60 * 60);
/// How often it asks with a test endpoint, so a test sees the app ask again soon.
const TEST_RECHECK: Duration = Duration::from_secs(1);
/// A lookup still unanswered then is abandoned; the next one tries again.
const TIMEOUT: Duration = Duration::from_secs(10);
/// The last answer, in the state directory.
const CACHE: &str = "release-check.json";

/// How this copy of artifactize was installed. Each channel has its own update path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Channel {
    /// The Microsoft Store package, which the Store updates.
    Store,
    /// winget's own package `lhj6102.Artifactize`.
    Winget,
    /// `install.sh`, on Linux or macOS.
    InstallSh,
    /// `install.ps1`, on Windows.
    InstallPs1,
    /// The Homebrew formula.
    Homebrew,
    /// The Homebrew cask, with the desktop app.
    HomebrewCask,
    /// `cargo install` or `cargo binstall`.
    Cargo,
    /// The macOS disk image, dragged to Applications.
    Dmg,
    /// The Linux `.deb` package.
    Deb,
    /// A Linux AppImage, or the commands it set up.
    AppImage,
    /// Anything else, such as an unpacked archive or a source build.
    Unknown,
}

/// How to update to a newer release.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Update {
    /// Run this command.
    Command { command: String },
    /// Download this file and install it by hand, with the command when there is one.
    Download {
        url: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        command: Option<String>,
    },
    /// See the release page: the install channel is unknown.
    Page { url: String },
}

impl Channel {
    /// How to update this channel's install to `latest`; `None` when the Store does it.
    pub fn update(self, latest: &Version) -> Option<Update> {
        crate::platform::install::update(self, latest, SITE)
    }
}

/// Why releases were not looked up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Unchecked {
    /// The Microsoft Store updates this copy.
    Store,
    /// [`OFF_VARIABLE`] is set.
    Off,
    /// A development build, whose version is not a release's.
    Development,
}

/// What the lookup found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Releases were not looked up.
    Unchecked(Unchecked),
    /// This copy is the latest release, or newer.
    Latest(Version),
    /// A newer release is out.
    Newer { latest: Version, update: Update },
    /// The lookup failed; the next one tries again.
    Failed(String),
}

/// This copy, how it was installed and what its release lookup found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    pub running: Version,
    pub channel: Channel,
    pub outcome: Outcome,
}

/// Where a lookup asks for the latest release, and the site its links point into.
struct Source {
    url: String,
    site: String,
}

/// Look up whether a newer release is out for this copy, reusing an answer kept in `state`
/// for a day. It never fails: a failed lookup is its [`Outcome::Failed`].
pub async fn check(state: &Path) -> Status {
    let running = running();
    let channel = crate::platform::install::channel();
    let outcome = match source(&running, channel, crate::platform::environment::var) {
        Ok(source) => look_up(&running, channel, &source, state, SystemTime::now()).await,
        Err(outcome) => outcome,
    };
    Status {
        running,
        channel,
        outcome,
    }
}

/// How long a long-running app waits before it asks [`check`] again.
pub fn recheck_interval() -> Duration {
    if crate::platform::environment::var(TEST_ENDPOINT_VARIABLE)
        .is_some_and(|value| !value.is_empty())
    {
        TEST_RECHECK
    } else {
        RECHECK
    }
}

/// This copy's version.
pub fn running() -> Version {
    Version::parse(env!("CARGO_PKG_VERSION")).expect("the crate version is semantic")
}

/// Where to look up the latest release, or why not to, with the variables in `var`.
fn source(
    running: &Version,
    channel: Channel,
    var: impl Fn(&str) -> Option<std::ffi::OsString>,
) -> Result<Source, Outcome> {
    if var(OFF_VARIABLE).is_some_and(|value| !value.is_empty()) {
        return Err(Outcome::Unchecked(Unchecked::Off));
    }
    if channel == Channel::Store {
        return Err(Outcome::Unchecked(Unchecked::Store));
    }
    match crate::llm::variable_endpoint_in(TEST_ENDPOINT_VARIABLE, var) {
        Ok(Some(site)) => Ok(Source {
            url: format!("{site}{TEST_LATEST_PATH}"),
            site: site.root().to_owned(),
        }),
        // The workspace's own version, which a release replaces.
        Ok(None) if running.pre.as_str() == "dev" => {
            Err(Outcome::Unchecked(Unchecked::Development))
        }
        Ok(None) => Ok(Source {
            url: LATEST_URL.into(),
            site: SITE.into(),
        }),
        Err(error) => Err(Outcome::Failed(error)),
    }
}

async fn look_up(
    running: &Version,
    channel: Channel,
    source: &Source,
    state: &Path,
    now: SystemTime,
) -> Outcome {
    match latest(source, state, now).await {
        Ok(latest) => decide(running, channel, latest, &source.site),
        Err(error) => Outcome::Failed(error),
    }
}

/// What `latest` means for this copy, whose updates link into `site`.
fn decide(running: &Version, channel: Channel, latest: Version, site: &str) -> Outcome {
    if latest <= *running {
        return Outcome::Latest(latest);
    }
    match crate::platform::install::update(channel, &latest, site) {
        Some(update) => Outcome::Newer { latest, update },
        None => Outcome::Unchecked(Unchecked::Store),
    }
}

/// The last answer, which site gave it and when.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Cached {
    #[serde(with = "epoch_seconds")]
    checked_at: SystemTime,
    site: String,
    latest: Version,
}

/// The latest release: the source's kept answer while it is younger than a day, else a new one.
async fn latest(source: &Source, state: &Path, now: SystemTime) -> Result<Version, String> {
    let cache = state.join(CACHE);
    if let Some(cached) = read_cache(&cache)
        .filter(|cached| cached.site == source.site && fresh(cached.checked_at, now))
    {
        return Ok(cached.latest);
    }
    let latest = fetch(&source.url).await?;
    // An answer that cannot be kept only means asking again next time.
    let _ = write_cache(
        state,
        &Cached {
            checked_at: now,
            site: source.site.clone(),
            latest: latest.clone(),
        },
    );
    Ok(latest)
}

/// Whether an answer from `checked_at` is still current at `now`. One from the future, after
/// the clock went back, is not.
fn fresh(checked_at: SystemTime, now: SystemTime) -> bool {
    now.duration_since(checked_at)
        .is_ok_and(|age| age < INTERVAL)
}

/// `checkedAt` in the kept answer: whole seconds since the Unix epoch.
mod epoch_seconds {
    use serde::{Deserialize, Deserializer, Serializer, de::Error};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    pub fn serialize<S: Serializer>(time: &SystemTime, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u64(
            time.duration_since(UNIX_EPOCH)
                .map_or(0, |since| since.as_secs()),
        )
    }

    /// A time no clock can show is refused, so an answer kept with one is asked again.
    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<SystemTime, D::Error> {
        let seconds = u64::deserialize(deserializer)?;
        UNIX_EPOCH
            .checked_add(Duration::from_secs(seconds))
            .ok_or_else(|| D::Error::custom("checkedAt is past any clock"))
    }
}

/// The kept answer; a missing or unreadable one is none.
fn read_cache(path: &Path) -> Option<Cached> {
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

/// Keep `cached` in `state`, which must exist: a lookup never creates the state directory.
fn write_cache(state: &Path, cached: &Cached) -> Result<(), String> {
    let mut file = tempfile::NamedTempFile::new_in(state).map_err(|e| e.to_string())?;
    serde_json::to_writer(&mut file, cached).map_err(|e| e.to_string())?;
    file.flush().map_err(|e| e.to_string())?;
    file.persist(state.join(CACHE))
        .map_err(|e| e.error.to_string())?;
    Ok(())
}

/// Ask `url`, GitHub's latest-release API or its stand-in, for the latest release's version.
async fn fetch(url: &str) -> Result<Version, String> {
    #[derive(Deserialize)]
    struct Latest {
        tag_name: String,
    }
    let client = reqwest::Client::builder()
        .timeout(TIMEOUT)
        .user_agent(concat!("artifactize/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| e.to_string())?;
    let latest: Latest = client
        .get(url)
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|e| e.without_url().to_string())?
        .json()
        .await
        .map_err(|e| e.without_url().to_string())?;
    let tag = &latest.tag_name;
    Version::parse(tag.strip_prefix('v').unwrap_or(tag))
        .map_err(|e| format!("the latest release's tag {tag:?} is not a version: {e}"))
}
