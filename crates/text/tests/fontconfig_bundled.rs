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

/// The file of the face that `local(name)` loads, if any.
fn local_font_file(ctx: &mut FontContext, n: usize, name: &str) -> Option<String> {
    // A new family name makes a new face, which a context requests again.
    let family = format!("L{n}");
    ctx.set_web_fonts(vec![swb_text::WebFontFace {
        family: family.clone(),
        sources: vec!["local".to_owned()],
        ..swb_text::WebFontFace::default()
    }]);
    let families = [Named(family.as_str())];
    let query = FontQuery::new(&families);
    ctx.itemize("x", &query);
    let id = *ctx.take_web_font_requests().first()?;
    ctx.web_font_local(id, name).ok()?;
    let font = ctx.itemize("x", &query).first()?.font;
    Some(file(ctx, font))
}

/// `local()` finds the same faces through fontconfig and in directory
/// mode, with the names that Chromium 148 accepts and rejects.
#[test]
fn local_names_match_in_fontconfig_mode() {
    if std::env::var_os(CHILD_MARKER).is_none() {
        let config = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/fonts/fonts.conf");
        let status = Command::new(std::env::current_exe().expect("the test binary has a path"))
            .args([
                "--exact",
                "local_names_match_in_fontconfig_mode",
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
    let names = [
        ("Liberation Sans", Some("LiberationSans-Regular.ttf")),
        ("liberationsans", Some("LiberationSans-Regular.ttf")),
        ("Liberation Sans Bold", Some("LiberationSans-Bold.ttf")),
        ("LiberationSans-Bold", Some("LiberationSans-Bold.ttf")),
        ("DejaVu Sans", Some("DejaVuSans.ttf")),
        ("Liberation Sans Regular", None),
        ("Arial", None),
        ("Times New Roman", None),
    ];
    for (n, (name, expected)) in names.into_iter().enumerate() {
        let expected = expected.map(str::to_owned);
        assert_eq!(local_font_file(&mut system, n, name), expected, "{name}");
        assert_eq!(local_font_file(&mut bundled, n, name), expected, "{name}");
    }
}
