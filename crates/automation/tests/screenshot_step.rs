//! The `screenshot` step of `swb_automation::run_steps` (M12 feature 1b):
//! it writes a PNG of the viewport and fails for a bad argument.

// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

mod common;
use common::{OnePage, on_page_thread};

use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use swb_automation::run_steps;
use swb_engine::{FontContext, Page, PageConfig, Size, Url};

const URL: &str = "https://example.test/";

fn open() -> Page {
    let config = PageConfig {
        fetcher: Arc::new(OnePage {
            url: URL,
            html: "<!DOCTYPE html><body style='background:#336699'><p>hi</p>".to_owned(),
        }),
        notify: Arc::new(|| {}),
        network_threads: 2,
        scripts_allowed: true,
    };
    let mut page = Page::new(
        config,
        FontContext::for_tests(),
        Size::new(200.0, 100.0),
        1.0,
    );
    page.navigate(Url::parse(URL).unwrap());
    assert!(page.wait_until_loaded(Duration::from_secs(30)));
    page
}

#[test]
fn screenshot_step_writes_a_png() {
    on_page_thread(|| {
        let mut page = open();
        let dir = std::env::temp_dir().join(format!("swb-shot-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("a.png");
        let steps = [json!({ "screenshot": file.to_str().unwrap() })];
        run_steps(&mut page, &steps).unwrap();
        let bytes = std::fs::read(&file).unwrap();
        assert_eq!(&bytes[1..4], b"PNG");
        std::fs::remove_dir_all(&dir).unwrap();
    });
}

#[test]
fn screenshot_step_reports_a_bad_argument_and_an_unwritable_file() {
    on_page_thread(|| {
        let mut page = open();
        let error = run_steps(&mut page, &[json!({ "screenshot": 1 })]).unwrap_err();
        assert!(error.contains("file name"), "{error}");
        let missing = "/nonexistent-dir-of-swb-test/a.png";
        let error = run_steps(&mut page, &[json!({ "screenshot": missing })]).unwrap_err();
        assert!(error.contains("cannot write"), "{error}");
    });
}
