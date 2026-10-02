//! Checks that system mode with `fixtures/fonts/fonts.conf` picks the same
//! fonts as directory mode with the bundled map. This tests the fontconfig
//! code path deterministically, and that `fonts.conf` (which Chromium can
//! use) agrees with `GenericFamilyMap::bundled()`.
//!
//! fontconfig reads `FONTCONFIG_FILE` once per process, so the test runs
//! itself again in a child process with that variable set.

use std::path::Path;
use std::process::Command;

use swb_text::{FamilyName, FontContext, FontId, FontQuery, FontStyle, GenericFamily};

use FamilyName::{Generic, Named};

const CHILD_MARKER: &str = "SWB_TEXT_FONTCONFIG_CHILD";

fn file(ctx: &FontContext, font: FontId) -> String {
    ctx.font_info(font)
        .and_then(|info| info.path.file_name()?.to_str().map(str::to_owned))
        .unwrap_or_default()
}

#[test]
fn fontconfig_with_bundled_config_matches_directory_mode() {
    if std::env::var_os(CHILD_MARKER).is_none() {
        let config = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/fonts/fonts.conf");
        let status = Command::new(std::env::current_exe().expect("the test binary has a path"))
            .args([
                "--exact",
                "fontconfig_with_bundled_config_matches_directory_mode",
                "--nocapture",
            ])
            .env(CHILD_MARKER, "1")
            .env("FONTCONFIG_FILE", config)
            .status()
            .expect("the test binary can run itself");
        assert!(status.success());
        return;
    }

    let mut system = FontContext::system();
    let mut bundled = FontContext::for_tests();
    let probe = [Generic(GenericFamily::SansSerif)];
    let probe_font = system.select(&FontQuery::new(&probe));
    if system.font_info(probe_font).is_none() {
        eprintln!("skipping: fontconfig is not available");
        return;
    }

    let generics = GenericFamily::ALL.map(|g| vec![Generic(g)]);
    let named = [
        vec![Named("Arial")],
        vec![Named("Helvetica")],
        vec![Named("Times")],
        vec![Named("Courier New")],
        vec![Named("DejaVu Sans")],
        vec![Named("Linux Libertine"), Generic(GenericFamily::Monospace)],
        vec![Named("Linux Libertine")],
    ];
    let styles = [
        (400.0, FontStyle::Normal),
        (700.0, FontStyle::Normal),
        (400.0, FontStyle::Italic),
        (700.0, FontStyle::Italic),
    ];
    for families in generics.iter().chain(named.iter()) {
        for (weight, style) in styles {
            let mut query = FontQuery::new(families);
            query.weight = weight;
            query.style = style;
            let (a, b) = (system.select(&query), bundled.select(&query));
            assert_eq!(
                file(&system, a),
                file(&bundled, b),
                "{families:?} {weight} {style:?}"
            );
            assert_eq!(system.synthesis(a), bundled.synthesis(b));
        }
    }

    let families = [Named("Liberation Serif")];
    let mut query = FontQuery::new(&families);
    query.weight = 700.0;
    let text = "Hi Жж Ωω ☃━ x\u{20D7} end";
    let describe = |ctx: &mut FontContext| {
        let runs = ctx.itemize(text, &query);
        runs.into_iter()
            .map(|run| (run.range, file(ctx, run.font), ctx.synthesis(run.font)))
            .collect::<Vec<_>>()
    };
    assert_eq!(describe(&mut system), describe(&mut bundled));
}
