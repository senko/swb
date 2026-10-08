//! Web fonts (`@font-face`): the face set, loading requests, composite
//! fonts, matching, variation axes and synthesis. The expected values come
//! from Chromium 148 (`tools/probes/web-fonts.json`).

use std::path::Path;

use swb_text::{
    FamilyName, FontContext, FontId, FontQuery, FontStyle, GenericFamily, ShapeOptions, WebFaceId,
    WebFontFace, WebFontStyle, decode_web_font,
};

use FamilyName::{Generic, Named};

const DV: &str = "fixtures/fonts/DejaVuSans.ttf";
const DVB: &str = "fixtures/fonts/DejaVuSans-Bold.ttf";
const SERIF: &str = "fixtures/fonts/LiberationSerif-Regular.ttf";
const SANS_ITALIC: &str = "fixtures/fonts/LiberationSans-Italic.ttf";
/// Exo 2 (variable, `wght` 100 to 900, default 400), latin subset.
const EXO: &str = "fixtures/pages/ars-technica/files/d2f675f4572825d0.woff2";
/// Faustina (variable, `wght` 300 to 800, default 300), latin subset.
const FAUSTINA: &str = "fixtures/pages/ars-technica/files/6cf30bdff2b30f61.woff2";
const MISSING: &str = "fixtures/fonts/missing.ttf";

fn face(family: &str, source: &str) -> WebFontFace {
    WebFontFace {
        family: family.to_owned(),
        weight: None,
        stretch: None,
        style: WebFontStyle::Auto,
        unicode_range: vec![(0, 0x10_FFFF)],
        sources: vec![source.to_owned()],
    }
}

fn ranged(family: &str, source: &str, range: &[(u32, u32)]) -> WebFontFace {
    WebFontFace {
        unicode_range: range.to_vec(),
        ..face(family, source)
    }
}

fn weighted(family: &str, source: &str, weight: (f32, f32)) -> WebFontFace {
    WebFontFace {
        weight: Some(weight),
        ..face(family, source)
    }
}

/// Loads every requested face from its first source that works (a path
/// relative to the repository root), as the engine does. Returns the
/// requested sources.
fn load_requests(ctx: &mut FontContext) -> Vec<String> {
    let mut loaded = Vec::new();
    for id in ctx.take_web_font_requests() {
        let sources = ctx
            .web_font_sources(id)
            .expect("a requested face has sources")
            .to_vec();
        loaded.push(sources[0].clone());
        if !sources.iter().any(|source| load(ctx, id, source)) {
            ctx.web_font_failed(id);
        }
    }
    loaded
}

fn load(ctx: &mut FontContext, id: WebFaceId, source: &str) -> bool {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(source);
    let Ok(bytes) = std::fs::read(path) else {
        return false;
    };
    decode_web_font(source, &bytes)
        .and_then(|data| ctx.web_font_loaded(id, source, &data))
        .is_ok()
}

fn query<'a>(families: &'a [FamilyName<'a>], weight: f32, style: FontStyle) -> FontQuery<'a> {
    let mut q = FontQuery::new(families);
    q.weight = weight;
    q.style = style;
    q
}

/// The source (or file name) of each run's font.
fn run_sources(ctx: &mut FontContext, text: &str, q: &FontQuery<'_>) -> Vec<String> {
    ctx.itemize(text, q)
        .iter()
        .map(|run| source(ctx, run.font))
        .collect()
}

fn selected(ctx: &mut FontContext, q: &FontQuery<'_>) -> String {
    let font = ctx.select(q);
    source(ctx, font)
}

fn source(ctx: &FontContext, font: FontId) -> String {
    let info = ctx.font_info(font).expect("not a placeholder");
    let path = info.path.to_string_lossy();
    path.rsplit('/').next().unwrap_or_default().to_owned()
}

#[test]
fn a_face_loads_when_text_needs_it() {
    let mut ctx = FontContext::for_tests();
    ctx.set_web_fonts(vec![face("W", DV)]);
    let families = [Named("W"), Generic(GenericFamily::SansSerif)];
    let q = query(&families, 400.0, FontStyle::Normal);
    // Until the face arrives, text uses the next family.
    assert_eq!(
        run_sources(&mut ctx, "Hello", &q),
        ["LiberationSans-Regular.ttf"]
    );
    assert_eq!(load_requests(&mut ctx), [DV]);
    assert_eq!(run_sources(&mut ctx, "Hello", &q), ["DejaVuSans.ttf"]);
    let primary = ctx.select(&q);
    assert_eq!(ctx.font_info(primary).unwrap().family, "W");
    // Each face is requested once.
    assert_eq!(ctx.take_web_font_requests().len(), 0);
}

#[test]
fn web_families_shadow_system_families_but_not_generics() {
    let mut ctx = FontContext::for_tests();
    ctx.set_web_fonts(vec![face("liberation serif", DV)]);
    let named = [Named("Liberation Serif")];
    let q = query(&named, 400.0, FontStyle::Normal);
    ctx.itemize("x", &q);
    load_requests(&mut ctx);
    assert_eq!(selected(&mut ctx, &q), "DejaVuSans.ttf");
    let generic = [Generic(GenericFamily::Serif)];
    let q = query(&generic, 400.0, FontStyle::Normal);
    assert_eq!(selected(&mut ctx, &q), "LiberationSerif-Regular.ttf");
}

#[test]
fn failed_families_are_missing() {
    let mut ctx = FontContext::for_tests();
    // A web family whose only face fails hides the system family too.
    ctx.set_web_fonts(vec![face("Liberation Mono", MISSING)]);
    let families = [Named("Liberation Mono"), Generic(GenericFamily::Serif)];
    let q = query(&families, 400.0, FontStyle::Normal);
    ctx.itemize("x", &q);
    assert_eq!(load_requests(&mut ctx), [MISSING]);
    assert_eq!(
        run_sources(&mut ctx, "x", &q),
        ["LiberationSerif-Regular.ttf"]
    );
    assert_eq!(ctx.take_web_font_requests().len(), 0);
}

#[test]
fn unicode_ranges_decide_which_faces_load() {
    let latin = ranged("U", DV, &[(0, 0xFF)]);
    let cyrillic = ranged("U", SERIF, &[(0x400, 0x4FF)]);
    let families = [Named("U"), Generic(GenericFamily::Monospace)];
    let q = query(&families, 400.0, FontStyle::Normal);

    let mut ctx = FontContext::for_tests();
    ctx.set_web_fonts(vec![latin.clone(), cyrillic.clone()]);
    ctx.select(&q);
    ctx.itemize("abc def", &q);
    assert_eq!(load_requests(&mut ctx), [DV]);

    // Text that needs only the cyrillic face does not load the latin face,
    // although that face covers U+0020 (measured in Chromium 148).
    let mut ctx = FontContext::for_tests();
    ctx.set_web_fonts(vec![latin.clone(), cyrillic.clone()]);
    ctx.select(&q);
    ctx.itemize("абвгд", &q);
    assert_eq!(load_requests(&mut ctx), [SERIF]);
    // The first available font is then the loaded cyrillic face.
    assert_eq!(selected(&mut ctx, &q), "LiberationSerif-Regular.ttf");
    assert_eq!(load_requests(&mut ctx).len(), 0);

    // A line box without text loads the face for U+0020.
    let mut ctx = FontContext::for_tests();
    ctx.set_web_fonts(vec![latin, cyrillic]);
    ctx.select(&q);
    assert_eq!(load_requests(&mut ctx), [DV]);
    assert_eq!(selected(&mut ctx, &q), "DejaVuSans.ttf");

    // A face whose range excludes U+0020 is not a first available font.
    let mut ctx = FontContext::for_tests();
    ctx.set_web_fonts(vec![ranged("C", DV, &[(0x400, 0x4FF)])]);
    let families = [Named("C"), Generic(GenericFamily::Monospace)];
    let q = query(&families, 400.0, FontStyle::Normal);
    assert_eq!(selected(&mut ctx, &q), "LiberationMono-Regular.ttf");
    assert_eq!(load_requests(&mut ctx).len(), 0);
}

#[test]
fn composite_fonts_check_later_rules_first() {
    let mut ctx = FontContext::for_tests();
    ctx.set_web_fonts(vec![
        face("O", DV),
        ranged("O", SERIF, &[(0x41, 0x5A)]),
        face("Q", DV),
        ranged("Q", SERIF, &[(0x2600, 0x26FF)]),
    ]);
    let o = [Named("O")];
    let q = query(&o, 400.0, FontStyle::Normal);
    ctx.itemize("Ab", &q);
    load_requests(&mut ctx);
    assert_eq!(
        run_sources(&mut ctx, "Ab", &q),
        ["LiberationSerif-Regular.ttf", "DejaVuSans.ttf"]
    );
    // The later face has the range but not the glyph: the earlier face
    // draws it. Both load.
    let qq = [Named("Q")];
    let q = query(&qq, 400.0, FontStyle::Normal);
    ctx.itemize("\u{2603}", &q);
    let mut requested = load_requests(&mut ctx);
    requested.sort();
    assert_eq!(requested, [DV, SERIF]);
    assert_eq!(run_sources(&mut ctx, "\u{2603}", &q), ["DejaVuSans.ttf"]);
}

#[test]
fn faces_are_matched_by_their_descriptors() {
    let mut ctx = FontContext::for_tests();
    ctx.set_web_fonts(vec![
        weighted("M", DV, (400.0, 400.0)),
        weighted("M", DVB, (700.0, 700.0)),
    ]);
    let m = [Named("M")];
    for (weight, expected) in [
        (100.0, "DejaVuSans.ttf"),
        (500.0, "DejaVuSans.ttf"),
        (550.0, "DejaVuSans-Bold.ttf"),
        (900.0, "DejaVuSans-Bold.ttf"),
    ] {
        let q = query(&m, weight, FontStyle::Normal);
        ctx.itemize("x", &q);
        load_requests(&mut ctx);
        assert_eq!(run_sources(&mut ctx, "x", &q), [expected], "{weight}");
    }
}

/// The font of family `family` for `weight` and `style`, after loading.
fn font_of(ctx: &mut FontContext, family: &str, weight: f32, style: FontStyle) -> FontId {
    let families = [Named(family)];
    let q = query(&families, weight, style);
    ctx.itemize("H", &q);
    load_requests(ctx);
    ctx.itemize("H", &q)[0].font
}

#[test]
fn synthetic_bold_follows_descriptor_and_font_weight() {
    let mut ctx = FontContext::for_tests();
    ctx.set_web_fonts(vec![
        weighted("S400", DV, (400.0, 400.0)),
        weighted("S600", DV, (600.0, 600.0)),
        face("Sauto", DV),
        face("Bauto", DVB),
        weighted("Bb400", DVB, (400.0, 400.0)),
    ]);
    let bold = |ctx: &mut FontContext, family: &str, weight: f32| {
        let font = font_of(ctx, family, weight, FontStyle::Normal);
        ctx.synthesis(font).bold
    };
    assert!(!bold(&mut ctx, "S400", 550.0));
    assert!(bold(&mut ctx, "S400", 600.0));
    assert!(!bold(&mut ctx, "S600", 900.0));
    assert!(bold(&mut ctx, "Sauto", 600.0));
    assert!(!bold(&mut ctx, "Bauto", 700.0));
    assert!(!bold(&mut ctx, "Bb400", 700.0));
}

#[test]
fn variable_axes_follow_the_descriptors() {
    let mut ctx = FontContext::for_tests();
    ctx.set_web_fonts(vec![
        face("Eauto", EXO),
        weighted("E400", EXO, (400.0, 400.0)),
        weighted("E500", EXO, (500.0, 600.0)),
        weighted("F400", FAUSTINA, (400.0, 400.0)),
    ]);
    let info = |ctx: &mut FontContext, family: &str, weight: f32| {
        let font = font_of(ctx, family, weight, FontStyle::Normal);
        (
            ctx.font_info(font).unwrap().weight,
            ctx.synthesis(font).bold,
        )
    };
    assert_eq!(info(&mut ctx, "Eauto", 900.0), (900.0, false));
    assert_eq!(info(&mut ctx, "E400", 700.0), (400.0, true));
    assert_eq!(info(&mut ctx, "E500", 300.0), (500.0, false));
    assert_eq!(info(&mut ctx, "E500", 900.0), (600.0, false));
    // The default instance of Faustina is 300; the descriptor sets 400.
    assert_eq!(info(&mut ctx, "F400", 400.0), (400.0, false));
    // Shaping uses the same coordinates: wider glyphs at a higher weight.
    let light = font_of(&mut ctx, "Eauto", 300.0, FontStyle::Normal);
    let heavy = font_of(&mut ctx, "Eauto", 900.0, FontStyle::Normal);
    let text = "Hamburgefonstiv";
    let light = ctx
        .shape(light, 16.0, text, &ShapeOptions::default())
        .advance;
    let heavy = ctx
        .shape(heavy, 16.0, text, &ShapeOptions::default())
        .advance;
    // Chromium 148: 125.19 px and 137.78 px.
    assert!((light - 125.19).abs() < 0.01, "{light}");
    assert!((heavy - 137.78).abs() < 0.01, "{heavy}");
}

#[test]
fn synthetic_oblique_follows_descriptor_and_font_style() {
    let mut ctx = FontContext::for_tests();
    ctx.set_web_fonts(vec![
        face("I", DV),
        WebFontFace {
            style: WebFontStyle::Italic,
            ..face("J", DV)
        },
        WebFontFace {
            style: WebFontStyle::Normal,
            ..face("N", SANS_ITALIC)
        },
    ]);
    let oblique = |ctx: &mut FontContext, family: &str, style: FontStyle| {
        let font = font_of(ctx, family, 400.0, style);
        ctx.synthesis(font).oblique
    };
    assert!(oblique(&mut ctx, "I", FontStyle::Italic));
    assert!(oblique(&mut ctx, "I", FontStyle::Oblique));
    assert!(!oblique(&mut ctx, "I", FontStyle::Normal));
    assert!(!oblique(&mut ctx, "J", FontStyle::Italic));
    assert!(!oblique(&mut ctx, "N", FontStyle::Italic));
}

#[test]
fn a_new_face_set_keeps_loaded_faces_and_drops_removed_ones() {
    let mut ctx = FontContext::for_tests();
    let w = face("W", DV);
    ctx.set_web_fonts(vec![w.clone()]);
    let families = [Named("W")];
    let q = query(&families, 400.0, FontStyle::Normal);
    ctx.itemize("x", &q);
    load_requests(&mut ctx);
    ctx.set_web_fonts(vec![face("Other", SERIF), w]);
    assert_eq!(run_sources(&mut ctx, "x", &q), ["DejaVuSans.ttf"]);
    assert_eq!(ctx.take_web_font_requests().len(), 0);
    let old = ctx.select(&q);
    ctx.set_web_fonts(Vec::new());
    // The family is gone; the old font is a placeholder now.
    assert_ne!(selected(&mut ctx, &q), "DejaVuSans.ttf");
    assert!(ctx.metrics(old, 16.0).ascent > 0.0);
    assert!(
        ctx.glyph_mask(old, swb_text::GlyphId(36), 16.0, 0)
            .is_none()
    );
}

#[test]
fn many_faces_and_ranges_stay_fast() {
    let mut ctx = FontContext::for_tests();
    let ranges: Vec<(u32, u32)> = (0..100_000).map(|i| (i * 4, i * 4 + 1)).collect();
    // More faces than a family may have: the last ones are ignored. The
    // face with the ranges is the last that counts, so it is checked first.
    let missing = |i: u32| {
        ranged(
            "Many",
            &format!("missing-{i}.ttf"),
            &[(i * 2 + 1, i * 2 + 1)],
        )
    };
    let mut faces: Vec<WebFontFace> = (0..999).map(missing).collect();
    faces.push(ranged("Many", DV, &ranges));
    faces.extend((999..2_000).map(missing));
    ctx.set_web_fonts(faces);
    let families = [Named("Many")];
    let q = query(&families, 400.0, FontStyle::Normal);
    let text: String = (0..2_000)
        .map(|i| char::from_u32(0x20 + i % 90).unwrap())
        .collect();
    let started = std::time::Instant::now();
    ctx.itemize(&text, &q);
    let requested = load_requests(&mut ctx);
    assert!(requested.len() <= swb_text::MAX_FACES_PER_FAMILY);
    assert!(requested.contains(&DV.to_owned()));
    ctx.itemize(&text, &q);
    assert!(started.elapsed().as_secs() < 5);
}

#[test]
fn a_face_that_loads_after_a_lookup_is_used_by_the_next_itemization() {
    let mut ctx = FontContext::for_tests();
    // Same descriptors: one composite font; the later rule is checked first.
    ctx.set_web_fonts(vec![face("W", SERIF), face("W", DV)]);
    let families = [Named("W"), Generic(GenericFamily::SansSerif)];
    let q = query(&families, 400.0, FontStyle::Normal);
    // Repeated clusters: the lookup of a cluster is shared within a call.
    let text = "ab".repeat(50);
    assert_eq!(
        run_sources(&mut ctx, &text, &q),
        ["LiberationSans-Regular.ttf"]
    );
    let ids = ctx.take_web_font_requests();
    assert_eq!(ids.len(), 2);
    // The first rule loads first; the later face is still pending.
    assert!(load(&mut ctx, ids[1], SERIF) || load(&mut ctx, ids[0], SERIF));
    assert_eq!(
        run_sources(&mut ctx, &text, &q),
        ["LiberationSerif-Regular.ttf"]
    );
    // The later rule loads: it takes over on the next call.
    let other = if ctx.web_font_sources(ids[0]).is_some_and(|s| s[0] == DV) {
        ids[0]
    } else {
        ids[1]
    };
    assert!(load(&mut ctx, other, DV));
    assert_eq!(run_sources(&mut ctx, &text, &q), ["DejaVuSans.ttf"]);
}

#[test]
fn composite_lookups_are_bounded() {
    let mut ctx = FontContext::for_tests();
    let faces: Vec<WebFontFace> = (0..1_000)
        .map(|i| face("C", &format!("missing-{i}.ttf")))
        .collect();
    ctx.set_web_fonts(faces);
    let families = [Named("C"), Generic(GenericFamily::SansSerif)];
    let q = query(&families, 400.0, FontStyle::Normal);
    let text: String = (0..20_000)
        .map(|i| char::from_u32(0x4E00 + i).unwrap())
        .collect();
    let started = std::time::Instant::now();
    ctx.itemize(&text, &q);
    assert!(ctx.take_web_font_requests().len() <= 256);
    assert!(started.elapsed().as_secs() < 5);
}
