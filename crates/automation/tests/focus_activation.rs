//! M14 feature 6b (review fix, ADR 0047 section 10): the scripts of the
//! automation's own, like the one of the `focus` step, give the page no user
//! activation (Playwright's `locator.focus()` gives none); the requests of the
//! client (the `eval` step, `page.evaluate`, `--eval`) give it.

// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

mod common;
use common::{OnePage, on_page_thread};

use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use swb_automation::run_steps;
use swb_engine::{Evaluation, FontContext, JsonForm, Page, PageConfig, Size, Url};

const URL: &str = "https://example.test/";

fn open() -> Page {
    let config = PageConfig {
        fetcher: Arc::new(OnePage {
            url: URL,
            html: "<!DOCTYPE html><input id=a><p>hi</p>".to_owned(),
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
    page.set_javascript(true);
    page.navigate(Url::parse(URL).unwrap());
    assert!(page.wait_until_loaded(Duration::from_secs(30)));
    page
}

/// `navigator.userActivation` and the focused element, read without giving
/// an activation.
fn state(page: &mut Page) -> String {
    match page.evaluate_internal(
        "[navigator.userActivation.hasBeenActive, navigator.userActivation.isActive, \
         document.activeElement.id]",
        JsonForm::Strict,
    ) {
        Some(Evaluation::Value(text)) => text,
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_focus_step_gives_no_user_activation_and_the_eval_step_gives_one() {
    on_page_thread(|| {
        let mut page = open();
        assert_eq!(state(&mut page), r#"[false,false,""]"#);
        run_steps(&mut page, &[json!({ "focus": "#a" })]).unwrap();
        // The element has the focus; the page has no activation.
        assert_eq!(state(&mut page), r#"[false,false,"a"]"#);
        run_steps(&mut page, &[json!({ "eval": "1" })]).unwrap();
        assert_eq!(state(&mut page), r#"[true,true,"a"]"#);
    });
}
