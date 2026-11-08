//! M10 sub-feature 3b-2 (ADR 0034 "3b-2 implementation notes") for the front
//! page of github.com/senko/swb with JavaScript on: the local fixture
//! `fixtures/js/github-swb` (git-ignored; set `SWB_JS_FIXTURES` to the
//! directory `fixtures/js` of another checkout). The page has an inline import
//! map (`react`, `react-dom` and others), 111 `modulepreload` links and 9
//! module scripts whose graphs have 95 modules. The test checks that
//!
//! - the 9 module scripts ran, in document order (Chromium's order, ADR 0034
//!   section 10), each once;
//! - no module script failed to load, link or parse, and the console has no
//!   message about a module or an import map.
//!
//! How the order shows: swb logs `module script URL: run` at the `debug` level
//! when a module script's graph is evaluated and `... : error to rethrow`
//! when its graph has a parse, resolution or link error
//! (`scripts/modules.rs`); the test installs a logger that keeps those lines.
//! The expected order is the order of the `script type=module src` elements in
//! the document. The test skips when the fixture is missing.

// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

mod common;

use std::sync::Mutex;
use std::time::Duration;

use common::{console_errors, fixture, load_at_capture, on_page_thread, value};

/// The lines of the module script log.
static LINES: Mutex<Vec<String>> = Mutex::new(Vec::new());

struct Capture;

/// The module path of the logger calls that the test keeps.
const MODULE_LOG: &str = "swb_web::scripts::modules";

impl log::Log for Capture {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.target() == MODULE_LOG
    }

    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) {
            LINES.lock().unwrap().push(record.args().to_string());
        }
    }

    fn flush(&self) {}
}

/// Words of the console messages about modules and import maps.
const MODULE_MESSAGES: &[&str] = &[
    "module specifier",
    "does not provide an export",
    "dynamically imported module",
    "module script",
    "import map",
    "Failed to load module",
];

#[test]
fn github_module_scripts_run_in_document_order() {
    let Some(dir) = fixture("github-swb") else {
        eprintln!("skipped: fixtures/js/github-swb is missing");
        return;
    };
    on_page_thread(move || {
        let _ = log::set_logger(&Capture);
        log::set_max_level(log::LevelFilter::Debug);
        let mut page = load_at_capture(&dir, Duration::from_millis(1500));
        assert!(page.has_script_runtime());
        let expected: Vec<String> = serde_json::from_value(value(
            &mut page,
            "Array.from(document.querySelectorAll('script[type=module][src]')).map(s => s.src)",
        ))
        .unwrap();
        assert_eq!(expected.len(), 9, "{expected:?}");
        let lines = LINES.lock().unwrap().clone();
        let errors: Vec<&String> = lines
            .iter()
            .filter(|line| line.ends_with(": error to rethrow"))
            .collect();
        assert!(errors.is_empty(), "module scripts with errors: {errors:?}");
        let ran: Vec<String> = lines
            .iter()
            .filter_map(|line| line.strip_suffix(": run"))
            .filter_map(|line| line.strip_prefix("module script "))
            .map(str::to_owned)
            .collect();
        assert_eq!(ran, expected);
        let module_errors: Vec<String> = console_errors(&page)
            .into_iter()
            .filter(|text| MODULE_MESSAGES.iter().any(|word| text.contains(word)))
            .collect();
        assert!(
            module_errors.is_empty(),
            "console errors about modules: {module_errors:?}"
        );
    });
}
