//! [`ExtendingFetcher`]: adds missing responses to a fixture.

use std::fmt;

use super::{RecordingFetcher, ReplayFetcher};
use crate::cookies::CookieJar;
use crate::error::NetError;
use crate::fetch::Fetcher;
use crate::request::Request;
use crate::response::Response;

/// Answers requests from a fixture directory and fetches only the requests
/// that it does not have, with another fetcher. Those responses are added
/// to the fixture; existing entries do not change.
///
/// This updates a fixture when swb starts to load more resources (for
/// example after support for a new CSS feature), without a new capture
/// that could get a newer version of a dynamic page.
pub struct ExtendingFetcher<F: Fetcher> {
    replay: ReplayFetcher,
    recorder: RecordingFetcher<F>,
}

impl<F: Fetcher> ExtendingFetcher<F> {
    /// Loads the fixture in `dir` and records into it with `inner`.
    pub fn new(inner: F, dir: impl AsRef<std::path::Path>) -> Result<Self, NetError> {
        let dir = dir.as_ref();
        Ok(ExtendingFetcher {
            replay: ReplayFetcher::load(dir)?,
            recorder: RecordingFetcher::new(inner, dir)?,
        })
    }
}

impl<F: Fetcher> Fetcher for ExtendingFetcher<F> {
    fn fetch(&self, request: &Request) -> Result<Response, NetError> {
        match self.replay.fetch(request) {
            Err(NetError::NotInFixture(url)) => {
                log::info!("not in the fixture, fetching: {url}");
                self.recorder.fetch(request)
            }
            other => other,
        }
    }

    fn cookie_jar(&self) -> Option<&CookieJar> {
        self.recorder.cookie_jar()
    }
}

impl<F: Fetcher + fmt::Debug> fmt::Debug for ExtendingFetcher<F> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ExtendingFetcher")
            .field("dir", &self.replay.dir())
            .field("inner", self.recorder.inner())
            .finish()
    }
}
