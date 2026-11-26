//! M14 feature 6b (ADR 0047 section 10): an evaluation (`--eval`, an `eval`
//! step, `page.evaluate`) gives the document a user activation first, as
//! Playwright's `evaluate` does, so that a script of a test can `play()` media
//! with sound. `Page::set_evaluation_activation(false)` turns it off (a test
//! of a page that must see no activation). The probe rows
//! `evaluate-carries-user-activation` show Chromium's rule.

#![allow(clippy::unwrap_used)]

mod common;

use std::sync::Arc;
use std::time::Duration;

use common::{CASE_PAGE, OnePage, new_page, on_page_thread};
use swb_engine::{Evaluation, JsonForm, Page, Size, Url};

fn page_with_script() -> Page {
    let mut page = new_page(
        Arc::new(OnePage::new("<!DOCTYPE html><p>x</p>")),
        1,
        Size::new(800.0, 600.0),
    );
    page.set_javascript(true);
    page.navigate(Url::parse(CASE_PAGE).unwrap());
    assert!(page.wait_until_loaded(Duration::from_secs(10)));
    page
}

fn value(page: &mut Page, js: &str) -> String {
    match page.evaluate(js, JsonForm::Strict) {
        Some(Evaluation::Value(text)) => text,
        other => panic!("{js}: {other:?}"),
    }
}

#[test]
fn an_evaluation_gives_the_document_a_user_activation_unless_it_is_turned_off() {
    on_page_thread(|| {
        let read = "[navigator.userActivation.hasBeenActive, navigator.userActivation.isActive]";
        // The page loaded without one; the first evaluation gives it.
        let mut page = page_with_script();
        assert_eq!(value(&mut page, read), "[true,true]");
        let mut page = page_with_script();
        page.set_evaluation_activation(false);
        assert_eq!(value(&mut page, read), "[false,false]");
        // Input gives it as before.
        page.click(5.0, 5.0);
        assert_eq!(value(&mut page, read), "[true,true]");
    });
}
