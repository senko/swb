//! Playwright's selector forms in the steps of `swb_automation::run_steps`
//! (M12 feature 4): a trailing `:visible`, `S >> nth=N` and `S >> nth=-1`;
//! other uses of `>>` and `:visible` are an error. The page has buttons that
//! are hidden in different ways; each click appends the button's name to
//! `window.log`. `:visible` follows the rule of Playwright's documentation
//! (a non-empty bounding box and `visibility` other than `hidden`); the
//! boxes of the cases below are the ones that Chromium reports
//! (`getBoundingClientRect`, probes of M12 feature 4).

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

const HTML: &str = "<!DOCTYPE html><style>
button { display: block; width: 100px; height: 20px; margin: 0; padding: 0; border: 0; }
</style>
<script>window.log = []; window.onclick = (e) => log.push(e.target.id);</script>
<button id='op' class='o' style='opacity:0'>x</button>
<div style='display:contents'><button id='dc' class='o'>x</button></div>
<button id='col' class='o' style='visibility:collapse'>x</button>
<details><summary>s</summary><button id='det' class='o'>x</button></details>
<button id='none' class='b' style='display:none'>x</button>
<button id='hidden' class='b' style='visibility:hidden'>x</button>
<button id='empty' class='b' style='height:0'>x</button>
<button id='one' class='b'>x</button>
<button id='two' class='b'>x</button>
<button id='three' class='b'>x</button>";

fn open() -> Page {
    let config = PageConfig {
        fetcher: Arc::new(OnePage {
            url: URL,
            html: HTML.to_owned(),
        }),
        notify: Arc::new(|| {}),
        network_threads: 2,
        scripts_allowed: true,
    };
    let mut page = Page::new(
        config,
        FontContext::for_tests(),
        Size::new(300.0, 300.0),
        1.0,
    );
    page.set_javascript(true);
    page.navigate(Url::parse(URL).unwrap());
    assert!(page.wait_until_loaded(Duration::from_secs(30)));
    page
}

fn log(page: &mut Page) -> String {
    match page.evaluate("log.join(',')", JsonForm::Strict) {
        Some(Evaluation::Value(text)) => text,
        other => panic!("{other:?}"),
    }
}

fn click(page: &mut Page, selector: &str) -> Result<(), String> {
    run_steps(page, &[json!({ "click": selector })])
}

#[test]
fn visible_skips_boxes_that_are_not_shown() {
    on_page_thread(|| {
        let mut page = open();
        click(&mut page, ".b:visible").unwrap();
        assert_eq!(log(&mut page), "\"one\"");
    });
}

#[test]
fn nth_counts_from_zero_in_document_order() {
    on_page_thread(|| {
        let mut page = open();
        click(&mut page, ".b >> nth=3").unwrap();
        click(&mut page, ".b >> nth=5").unwrap();
        assert_eq!(log(&mut page), "\"one,three\"");
    });
}

#[test]
fn nth_applies_after_visible() {
    on_page_thread(|| {
        let mut page = open();
        click(&mut page, ".b:visible >> nth=1").unwrap();
        click(&mut page, ".b:visible >> nth=2").unwrap();
        assert_eq!(log(&mut page), "\"two,three\"");
    });
}

#[test]
fn a_selector_that_finds_nothing_fails() {
    on_page_thread(|| {
        let mut page = open();
        let error = click(&mut page, ".b:visible >> nth=3").unwrap_err();
        assert!(error.contains("no element matches"), "{error}");
        let error = click(&mut page, "#hidden:visible").unwrap_err();
        assert!(error.contains("no element matches"), "{error}");
        let error = click(&mut page, ".nothing >> nth=0").unwrap_err();
        assert!(error.contains("no element matches"), "{error}");
        assert_eq!(log(&mut page), "\"\"");
    });
}

#[test]
fn opacity_zero_and_display_contents_are_visible_collapse_is_not() {
    on_page_thread(|| {
        let mut page = open();
        // The boxes of these buttons are not empty in Chromium: `opacity: 0`
        // and a `display: contents` parent change nothing; `visibility:
        // collapse` hides; the button in a closed `details` has the box of
        // the open layout.
        click(&mut page, ".o:visible >> nth=0").unwrap();
        click(&mut page, ".o:visible >> nth=1").unwrap();
        // The third match is the button of the closed `details`: found (a
        // hover moves the pointer, nothing is clicked).
        run_steps(&mut page, &[json!({ "hover": ".o:visible >> nth=2" })]).unwrap();
        assert_eq!(log(&mut page), "\"op,dc\"");
        let error = click(&mut page, ".o:visible >> nth=3").unwrap_err();
        assert!(error.contains("no element matches"), "{error}");
    });
}

#[test]
fn nth_minus_one_is_the_last_match() {
    on_page_thread(|| {
        let mut page = open();
        click(&mut page, ".b >> nth=-1").unwrap();
        click(&mut page, ".b:visible >> nth=-1").unwrap();
        assert_eq!(log(&mut page), "\"three,three\"");
    });
}

#[test]
fn unsupported_forms_give_an_error_that_says_so() {
    on_page_thread(|| {
        let mut page = open();
        for selector in [
            ".b >> button",
            ".b:visible .b",
            ".b >> nth=1 >> nth=0",
            ".b >> nth=-2",
        ] {
            let error = click(&mut page, selector).unwrap_err();
            assert!(error.contains("not supported"), "{selector}: {error}");
        }
        assert_eq!(log(&mut page), "\"\"");
    });
}

#[test]
fn focus_takes_the_forms_of_the_pointer_steps() {
    on_page_thread(|| {
        let mut page = open();
        run_steps(&mut page, &[json!({ "focus": ".b:visible >> nth=1" })]).unwrap();
        let active = page.evaluate("document.activeElement.id", JsonForm::Strict);
        assert_eq!(active, Some(Evaluation::Value("\"two\"".to_owned())));
        let error = run_steps(&mut page, &[json!({ "focus": ".b >> nth=9" })]).unwrap_err();
        assert!(error.contains("no element matches"), "{error}");
    });
}
