//! Helpers of the integration tests on the local JavaScript fixtures
//! (`fixtures/js/<name>`, git-ignored, ADR 0028 section 17). Set
//! `SWB_JS_FIXTURES` to the directory `fixtures/js` of another checkout;
//! a test skips when its fixture is missing.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::Value;
use swb_engine::{Evaluation, FontContext, JsonForm, Page, PageConfig, Size, Url};
use swb_net::ReplayFetcher;

/// The directory of the fixture `name`, or `None` (the test then skips).
pub(crate) fn fixture(name: &str) -> Option<PathBuf> {
    let root = std::env::var_os("SWB_JS_FIXTURES").map_or_else(
        || Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/js"),
        PathBuf::from,
    );
    let dir = root.join(name);
    dir.join("fixture.json").is_file().then_some(dir)
}

/// Runs `f` on a thread with the stack of the page thread (16 MiB).
pub(crate) fn on_page_thread(f: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new()
        .stack_size(16 << 20)
        .spawn(f)
        .unwrap()
        .join()
        .unwrap();
}

/// The fixture's `fixture.json`.
pub(crate) fn fixture_meta(dir: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(dir.join("fixture.json")).unwrap()).unwrap()
}

/// A page of 1280x800 with the test fonts that replays the fixture in
/// `dir`, with JavaScript on or off, and the URL of the fixture. The
/// caller navigates.
pub(crate) fn replay_page(dir: &Path, javascript: bool) -> (Page, Url) {
    let url = Url::parse(fixture_meta(dir)["url"].as_str().unwrap()).unwrap();
    let config = PageConfig {
        fetcher: Arc::new(ReplayFetcher::load(dir).unwrap()),
        notify: Arc::new(|| {}),
        network_threads: 2,
        scripts_allowed: true,
    };
    let mut page = Page::new(
        config,
        FontContext::for_tests(),
        Size::new(1280.0, 800.0),
        1.0,
    );
    page.set_javascript(javascript);
    (page, url)
}

/// The value of the expression `source` in the page, as JSON.
pub(crate) fn value(page: &mut Page, source: &str) -> Value {
    match page.evaluate(source, JsonForm::Strict) {
        Some(Evaluation::Value(text)) => serde_json::from_str(&text).unwrap(),
        other => panic!("{source}: {other:?}"),
    }
}
