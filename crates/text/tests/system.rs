//! Tests with the system fonts (fontconfig). They depend on the installed
//! fonts, so they check only properties that hold on any machine, and skip
//! when there are no fonts.

use swb_text::{FamilyName, FontContext, FontQuery, GenericFamily, ShapeOptions};

use FamilyName::{Generic, Named};

/// The system context, or `None` (with a message) if it has no fonts.
fn system() -> Option<FontContext> {
    let mut ctx = FontContext::system();
    let families = [Generic(GenericFamily::SansSerif)];
    let font = ctx.select(&FontQuery::new(&families));
    if ctx.font_info(font).is_none() {
        eprintln!("skipping: fontconfig found no fonts");
        return None;
    }
    Some(ctx)
}

#[test]
fn selects_sans_serif() {
    let Some(mut ctx) = system() else { return };
    let families = [Generic(GenericFamily::SansSerif)];
    let query = FontQuery::new(&families);
    let font = ctx.select(&query);
    let info = ctx.font_info(font).unwrap();
    assert!(info.path.exists(), "{}", info.path.display());
    let metrics = ctx.metrics(font, 16.0);
    assert!(metrics.ascent > 0.0 && metrics.descent > 0.0);
    let run = ctx.shape(font, 16.0, "Hello", &ShapeOptions::default());
    assert_eq!(run.glyphs.len(), 5);
    assert!(run.advance > 0.0);
    assert_eq!(ctx.itemize("Hello world", &query).len(), 1);
}

#[test]
fn every_generic_family_resolves() {
    let Some(mut ctx) = system() else { return };
    for generic in GenericFamily::ALL {
        let families = [Generic(generic)];
        let font = ctx.select(&FontQuery::new(&families));
        assert!(ctx.font_info(font).is_some(), "{generic:?}");
    }
}

#[test]
fn missing_family_falls_through_to_next() {
    let Some(mut ctx) = system() else { return };
    let with_missing = [
        Named("No Such Font Family 7f3a"),
        Generic(GenericFamily::Monospace),
    ];
    let monospace = [Generic(GenericFamily::Monospace)];
    assert_eq!(
        ctx.select(&FontQuery::new(&with_missing)),
        ctx.select(&FontQuery::new(&monospace))
    );
}

#[test]
fn itemize_covers_mixed_text() {
    let Some(mut ctx) = system() else { return };
    let families = [Generic(GenericFamily::Serif)];
    let query = FontQuery::new(&families);
    let text = "Latin Кирилица Ελληνικά ☃ 中文 ไทย";
    let runs = ctx.itemize(text, &query);
    assert_eq!(runs.first().unwrap().range.start, 0);
    assert_eq!(runs.last().unwrap().range.end, text.len());
    for pair in runs.windows(2) {
        assert_eq!(pair[0].range.end, pair[1].range.start);
    }
}
