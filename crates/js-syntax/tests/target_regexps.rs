//! Compiles every regular expression literal of the target scripts with
//! `swb-js-regexp` and runs it once (M7 feature 8a). The scripts are not
//! in the repository: the test is ignored and reads the directories named
//! in `SWB_TARGET_SCRIPTS` (separated by `:`), for example
//!
//! ```text
//! SWB_TARGET_SCRIPTS=out/bbc-js:out/ars-js \
//!     cargo test -p swb-js-syntax --test target_regexps -- --ignored --nocapture
//! ```

use swb_js_regexp::Regex;
use swb_js_syntax::{ExprKind, parse_script};
use swb_js_text::{RecursionBudget, Str16, String16};

#[test]
#[ignore = "needs the target scripts (SWB_TARGET_SCRIPTS)"]
fn every_literal_compiles() {
    let Ok(dirs) = std::env::var("SWB_TARGET_SCRIPTS") else {
        eprintln!("SWB_TARGET_SCRIPTS is not set");
        return;
    };
    let (mut scripts, mut literals, mut failures) = (0, 0, Vec::new());
    let sample: String16 =
        "The quick brown fox, https://www.bbc.co.uk/news?a=1&b=2 #x 12:30".into();
    for dir in dirs.split(':') {
        let Ok(entries) = std::fs::read_dir(dir) else {
            panic!("cannot read {dir}");
        };
        let mut paths: Vec<_> = entries.filter_map(Result::ok).map(|e| e.path()).collect();
        paths.sort();
        for path in paths
            .iter()
            .filter(|p| p.extension().is_some_and(|e| e == "js"))
        {
            let Ok(text) = std::fs::read_to_string(path) else {
                continue;
            };
            let source = String16::from(text.as_str());
            let mut budget = RecursionBudget::default();
            let script = match parse_script(source.as_str16(), &mut budget) {
                Ok(script) => script,
                Err(error) => {
                    failures.push(format!("{}: {error}", path.display()));
                    continue;
                }
            };
            scripts += 1;
            for id in script.ast.expr_ids() {
                let ExprKind::RegExp { pattern, flags } = script.ast.expr(id).kind else {
                    continue;
                };
                literals += 1;
                let pattern = script.ast.string(pattern);
                match Regex::new(pattern.as_str16(), flags) {
                    Ok(regex) => {
                        if let Err(error) = regex.exec(sample.as_str16(), 0, 10_000_000, 1 << 24) {
                            failures.push(format!("{}: /{pattern}/: {error}", path.display()));
                        }
                    }
                    Err(error) => failures.push(format!(
                        "{}: /{pattern}/: {}",
                        path.display(),
                        error.message(pattern.as_str16(), Str16::Latin1(b""))
                    )),
                }
            }
        }
    }
    eprintln!("{scripts} scripts, {literals} regular expression literals");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
