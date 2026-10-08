//! Web fonts (`@font-face`): the face set, loading requests, composite
//! fonts, matching, variation axes and synthesis. The expected values come
//! from Chromium 148 (`tools/probes/web-fonts.json`).

use std::path::Path;

use swb_text::{
    FamilyName, Feature, FontContext, FontId, FontQuery, FontStyle, GenericFamily, ShapeOptions,
    WebFaceId, WebFontFace, WebFontStyle, decode_web_font,
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
        sources: vec![source.to_owned()],
        ..WebFontFace::default()
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

// ----- Part 2: variation and feature settings, metric descriptors,
// `local()`. The expected values come from Chromium 148
// (`tools/probes/web-fonts-2.json`). -----

/// The variable test font (`crates/text/tests/webfonts/README.md`): the
/// advance of `A` is 600 units at the defaults, 400 to 1000 over `wght`
/// 100 to 900, and 400 to 800 over `wdth` 75 to 125; `SWBX` adds 0.5
/// units per unit.
const VAR: &str = "crates/text/tests/webfonts/swb-variable.ttf";

fn var_face(family: &str) -> WebFontFace {
    WebFontFace {
        weight: Some((100.0, 900.0)),
        stretch: Some((75.0, 125.0)),
        ..face(family, VAR)
    }
}

/// Loads the faces that `q` needs and shapes `text` at `size` px.
fn advance_of(
    ctx: &mut FontContext,
    q: &FontQuery<'_>,
    size: f32,
    text: &str,
    features: &[Feature],
) -> f32 {
    ctx.itemize(text, q);
    load_requests(ctx);
    let runs = ctx.itemize(text, q);
    let options = ShapeOptions {
        features,
        ..ShapeOptions::default()
    };
    runs.iter()
        .map(|run| {
            ctx.shape(run.font, size, &text[run.range.clone()], &options)
                .advance
        })
        .sum()
}

/// The advance of `A` at 100 px, for the CSS properties given.
fn a_width(
    ctx: &mut FontContext,
    family: &str,
    weight: f32,
    stretch: f32,
    variations: &[([u8; 4], f32)],
) -> f32 {
    let families = [Named(family)];
    let mut q = query(&families, weight, FontStyle::Normal);
    q.stretch = stretch;
    q.variations = variations;
    advance_of(ctx, &q, 100.0, "A", &[])
}

fn assert_close(actual: f32, expected: f32) {
    assert!((actual - expected).abs() < 0.02, "{actual} != {expected}");
}

#[test]
fn variation_settings_come_after_the_matched_axes() {
    let mut ctx = FontContext::for_tests();
    ctx.set_web_fonts(vec![var_face("V"), weighted("V4", VAR, (400.0, 400.0))]);
    let mut w = |family: &str, weight: f32, stretch: f32, v: &[([u8; 4], f32)]| {
        a_width(&mut ctx, family, weight, stretch, v)
    };
    assert_close(w("V", 400.0, 100.0, &[]), 60.0);
    assert_close(w("V", 900.0, 100.0, &[]), 100.0);
    assert_close(w("V", 700.0, 100.0, &[]), 84.0);
    // Chromium: `font-weight: 700; font-variation-settings: "wght" 660`
    // (the Ars Technica headings) is 80.81 px, not 84.
    assert_close(w("V", 700.0, 100.0, &[(*b"wght", 660.0)]), 80.8);
    assert_close(w("V", 400.0, 100.0, &[(*b"wght", 900.0)]), 100.0);
    // Values outside the axis range are clamped.
    assert_close(w("V", 400.0, 100.0, &[(*b"wght", 1000.0)]), 100.0);
    assert_close(w("V", 400.0, 100.0, &[(*b"wght", 0.0)]), 40.0);
    assert_close(w("V", 400.0, 100.0, &[(*b"wght", -50.0)]), 40.0);
    assert_close(w("V", 400.0, 100.0, &[(*b"wght", 500.5)]), 68.0);
    // The last setting of a tag wins.
    assert_close(
        w("V", 400.0, 100.0, &[(*b"wght", 900.0), (*b"wght", 100.0)]),
        40.0,
    );
    // `wdth` from font-stretch, and from the property over it.
    assert_close(w("V", 400.0, 75.0, &[]), 40.0);
    assert_close(w("V", 400.0, 75.0, &[(*b"wdth", 125.0)]), 80.0);
    assert_close(
        w("V", 400.0, 100.0, &[(*b"wdth", 125.0), (*b"wght", 900.0)]),
        120.0,
    );
    // A custom axis; tags are case-sensitive; axes that the font lacks are
    // ignored.
    assert_close(w("V", 400.0, 100.0, &[(*b"SWBX", 1000.0)]), 110.0);
    assert_close(w("V", 400.0, 100.0, &[(*b"swbx", 1000.0)]), 60.0);
    assert_close(
        w("V", 400.0, 100.0, &[(*b"slnt", 5.0), (*b"XXXX", 5.0)]),
        60.0,
    );
    // The descriptor range of the face clamps the weight before the
    // property applies, which can go beyond it.
    assert_close(w("V4", 900.0, 100.0, &[]), 60.0);
    assert_close(w("V4", 900.0, 100.0, &[(*b"wght", 900.0)]), 100.0);
}

#[test]
fn variation_settings_make_distinct_fonts() {
    let mut ctx = FontContext::for_tests();
    ctx.set_web_fonts(vec![var_face("V")]);
    let families = [Named("V")];
    let mut q = query(&families, 400.0, FontStyle::Normal);
    let plain = font_of(&mut ctx, "V", 400.0, FontStyle::Normal);
    let settings = [(*b"wght", 900.0)];
    q.variations = &settings;
    let heavy = ctx.itemize("A", &q)[0].font;
    let again = ctx.itemize("A", &q)[0].font;
    assert_ne!(plain, heavy);
    assert_eq!(heavy, again);
}

#[test]
fn synthetic_bold_ignores_variation_settings() {
    // Measured in Chromium 148 with the ink of the glyphs: the decision
    // uses the weight that matching sets, before `font-variation-settings`.
    let mut ctx = FontContext::for_tests();
    ctx.set_web_fonts(vec![weighted("V4", VAR, (400.0, 400.0)), var_face("V")]);
    let bold = |ctx: &mut FontContext, family: &str, weight: f32, v: &[([u8; 4], f32)]| {
        let families = [Named(family)];
        let mut q = query(&families, weight, FontStyle::Normal);
        q.variations = v;
        ctx.itemize("A", &q);
        load_requests(ctx);
        let font = ctx.itemize("A", &q)[0].font;
        ctx.synthesis(font).bold
    };
    let w900 = [(*b"wght", 900.0)];
    let w400 = [(*b"wght", 400.0)];
    assert!(bold(&mut ctx, "V4", 700.0, &[]));
    assert!(bold(&mut ctx, "V4", 700.0, &w400));
    assert!(bold(&mut ctx, "V4", 700.0, &w900));
    assert!(!bold(&mut ctx, "V4", 400.0, &w900));
    assert!(!bold(&mut ctx, "V", 700.0, &w400));
}

#[test]
fn font_face_variation_settings_apply_before_the_property() {
    let mut ctx = FontContext::for_tests();
    let descriptor = |family: &str, settings: &[([u8; 4], f32)]| WebFontFace {
        variations: settings.to_vec(),
        ..var_face(family)
    };
    ctx.set_web_fonts(vec![
        descriptor("D1", &[(*b"wght", 900.0)]),
        descriptor("D2", &[(*b"wght", 900.0), (*b"wdth", 125.0)]),
        descriptor("D3", &[(*b"wght", 900.0), (*b"wght", 100.0)]),
    ]);
    let mut w = |family: &str, weight: f32, v: &[([u8; 4], f32)]| {
        a_width(&mut ctx, family, weight, 100.0, v)
    };
    // The descriptor wins over font-weight.
    assert_close(w("D1", 400.0, &[]), 100.0);
    assert_close(w("D1", 100.0, &[]), 100.0);
    // The property sets other axes in addition, and wins for the same tag.
    assert_close(w("D1", 400.0, &[(*b"wdth", 125.0)]), 120.0);
    assert_close(w("D1", 400.0, &[(*b"wght", 200.0)]), 46.7);
    assert_close(w("D2", 400.0, &[]), 120.0);
    assert_close(w("D3", 400.0, &[]), 40.0);
}

#[test]
fn variable_system_fonts_take_variation_settings() {
    use swb_text::GenericFamilyMap;
    let dir = std::env::temp_dir().join(format!("swb-text-variable-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(VAR),
        dir.join("swb-variable.ttf"),
    )
    .unwrap();
    let map = GenericFamilyMap {
        generics: Vec::new(),
        default_family: "SWB Variable".to_owned(),
        fallback: Vec::new(),
        aliases: Vec::new(),
    };
    let ctx = FontContext::from_directory(&dir, map);
    std::fs::remove_dir_all(&dir).unwrap();
    let mut ctx = ctx.unwrap();
    let mut w =
        |weight: f32, v: &[([u8; 4], f32)]| a_width(&mut ctx, "SWB Variable", weight, 100.0, v);
    assert_close(w(400.0, &[]), 60.0);
    assert_close(w(900.0, &[]), 100.0);
    assert_close(w(400.0, &[(*b"wght", 900.0)]), 100.0);
    assert_close(w(900.0, &[(*b"wght", 100.0)]), 40.0);
    // A static font ignores the settings.
    let mut ctx = FontContext::for_tests();
    let families = [Named("Liberation Sans")];
    let mut q = query(&families, 400.0, FontStyle::Normal);
    let plain = advance_of(&mut ctx, &q, 16.0, "Hamburgefonstiv", &[]);
    let settings = [(*b"wght", 900.0)];
    q.variations = &settings;
    assert_close(
        advance_of(&mut ctx, &q, 16.0, "Hamburgefonstiv", &[]),
        plain,
    );
}

#[test]
fn font_feature_settings_reach_the_shaper() {
    let mut ctx = FontContext::for_tests();
    let descriptor = |family: &str, features: &[Feature]| WebFontFace {
        features: features.to_vec(),
        ..face(family, DV)
    };
    let f = |tag: &[u8; 4], value| Feature { tag: *tag, value };
    ctx.set_web_fonts(vec![
        face("P", DV),
        descriptor("D", &[f(b"liga", 0)]),
        descriptor("E", &[f(b"liga", 0), f(b"kern", 0)]),
    ]);
    let text = "fi fl ffi AV To";
    let mut width = |family: &str, features: &[Feature]| {
        let families = [Named(family)];
        let q = query(&families, 400.0, FontStyle::Normal);
        advance_of(&mut ctx, &q, 100.0, text, features)
    };
    assert_close(width("P", &[]), 585.5);
    assert_close(width("P", &[f(b"liga", 0)]), 587.02);
    assert_close(width("P", &[f(b"kern", 0)]), 608.89);
    assert_close(width("P", &[f(b"kern", 0), f(b"liga", 0)]), 610.41);
    // The last setting of a feature wins.
    assert_close(width("P", &[f(b"liga", 0), f(b"liga", 1)]), 585.5);
    assert_close(width("P", &[f(b"liga", 1), f(b"liga", 0)]), 587.02);
    // The descriptor applies first; the property overrides it per tag.
    assert_close(width("D", &[]), 587.02);
    assert_close(width("D", &[f(b"liga", 1)]), 585.5);
    assert_close(width("E", &[f(b"liga", 1)]), 608.89);
    assert_close(width("E", &[]), 610.41);
}

#[test]
fn size_adjust_scales_shaping_metrics_and_glyphs() {
    let mut ctx = FontContext::for_tests();
    let adjusted = |family: &str, size_adjust: f32| WebFontFace {
        size_adjust,
        ..face(family, VAR)
    };
    ctx.set_web_fonts(vec![
        face("P", VAR),
        adjusted("Half", 0.5),
        adjusted("Zero", 0.0),
        WebFontFace {
            ascent_override: Some(0.5),
            ..adjusted("Asc", 0.5)
        },
        WebFontFace {
            descent_override: Some(0.0),
            line_gap_override: Some(0.2),
            ..adjusted("Gap", 0.5)
        },
    ]);
    let mut metrics = |family: &str| {
        let font = font_of(&mut ctx, family, 400.0, FontStyle::Normal);
        let m = ctx.metrics(font, 100.0);
        (m.ascent, m.descent, m.line_gap)
    };
    assert_eq!(metrics("P"), (80.0, 20.0, 0.0));
    // The glyphs and all metrics scale.
    assert_eq!(metrics("Half"), (40.0, 10.0, 0.0));
    assert_eq!(metrics("Zero"), (0.0, 0.0, 0.0));
    // Overrides are ratios of the adjusted size (measured in Chromium).
    assert_eq!(metrics("Asc"), (25.0, 10.0, 0.0));
    assert_eq!(metrics("Gap"), (40.0, 0.0, 10.0));
    assert_close(a_width(&mut ctx, "P", 400.0, 100.0, &[]), 60.0);
    assert_close(a_width(&mut ctx, "Half", 400.0, 100.0, &[]), 30.0);
    assert_close(a_width(&mut ctx, "Zero", 400.0, 100.0, &[]), 0.0);
    // The glyph masks scale too: `H` at 100 px with `size-adjust: 50%`
    // looks like `H` at 50 px.
    let mask = |ctx: &mut FontContext, family: &str, size: f32| {
        let font = font_of(ctx, family, 400.0, FontStyle::Normal);
        let glyph = ctx.shape(font, size, "H", &ShapeOptions::default()).glyphs[0].glyph;
        let mask = ctx.glyph_mask(font, glyph, size, 0).expect("an outline");
        (mask.width, mask.height)
    };
    assert_eq!(mask(&mut ctx, "Half", 100.0), mask(&mut ctx, "P", 50.0));
}

#[test]
fn local_sources_match_full_and_postscript_names() {
    // Measured in Chromium 148 with the test fonts: full names and
    // PostScript names match, ignoring ASCII case and spaces; family
    // names that differ from the full name, aliases and styles do not.
    let matches = [
        ("Liberation Sans", "LiberationSans-Regular.ttf"),
        ("liberation sans", "LiberationSans-Regular.ttf"),
        ("L i b e r a t i o n S a n s", "LiberationSans-Regular.ttf"),
        ("LiberationSans", "LiberationSans-Regular.ttf"),
        ("Liberation Sans Bold", "LiberationSans-Bold.ttf"),
        ("LIBERATIONSANS-BOLD", "LiberationSans-Bold.ttf"),
        ("Liberation Sans  Bold", "LiberationSans-Bold.ttf"),
        ("LiberationSans Bold", "LiberationSans-Bold.ttf"),
        (
            "Liberation Sans Bold Italic",
            "LiberationSans-BoldItalic.ttf",
        ),
        ("Liberation Serif Italic", "LiberationSerif-Italic.ttf"),
        ("DejaVu Sans", "DejaVuSans.ttf"),
        ("DejaVuSans-Bold", "DejaVuSans-Bold.ttf"),
        ("DejaVuSansBold", "DejaVuSans-Bold.ttf"),
        ("Liberation Mono Bold", "LiberationMono-Bold.ttf"),
    ];
    let no_match = [
        "Arial",
        "Arial Bold",
        "Arimo",
        "Helvetica",
        "Times New Roman",
        "Times New Roman Bold",
        "Courier New",
        "Liberation Sans Regular",
        "LiberationSans-Regular",
        "DejaVu Sans Book",
        "DejaVu",
        "Liberation",
        "Liberation-Sans",
        "Liberation\tSans",
        "DejaVu Sans Bold Oblique",
        "",
        "   ",
    ];
    let families = [Named("L"), Generic(GenericFamily::Monospace)];
    let q = query(&families, 400.0, FontStyle::Normal);
    let requested = || {
        let mut ctx = FontContext::for_tests();
        ctx.set_web_fonts(vec![face("L", "local")]);
        ctx.itemize("x", &q);
        let id = ctx.take_web_font_requests()[0];
        (ctx, id)
    };
    let (mut ctx, id) = requested();
    for name in no_match {
        assert!(ctx.web_font_local(id, name).is_err(), "{name:?}");
    }
    // A failed lookup leaves the face requested: the next source can still
    // load it.
    for (name, file) in matches {
        let (mut ctx, id) = requested();
        assert!(ctx.web_font_local(id, "Nope").is_err());
        ctx.web_font_local(id, name).expect(name);
        assert_eq!(run_sources(&mut ctx, "x", &q), [file], "{name}");
    }
}

#[test]
fn local_faces_follow_the_rules_of_url_faces() {
    // Measured in Chromium 148: a local bold face with `font-weight: 400`
    // is not emboldened again, and a local regular face is not slanted for
    // an italic descriptor; the descriptors of the rule apply.
    let mut ctx = FontContext::for_tests();
    ctx.set_web_fonts(vec![
        weighted("A", "local", (400.0, 400.0)),
        WebFontFace {
            style: WebFontStyle::Italic,
            ..face("D", "local")
        },
        WebFontFace {
            size_adjust: 0.5,
            ..face("S", "local")
        },
    ]);
    let local = |ctx: &mut FontContext, family: &str, name: &str, weight, style| {
        let families = [Named(family)];
        let q = query(&families, weight, style);
        ctx.itemize("H", &q);
        let id = ctx.take_web_font_requests()[0];
        ctx.web_font_local(id, name).unwrap();
        let font = ctx.itemize("H", &q)[0].font;
        (ctx.synthesis(font), source(ctx, font), font)
    };
    let (synthesis, file, _) = local(
        &mut ctx,
        "A",
        "Liberation Sans Bold",
        700.0,
        FontStyle::Normal,
    );
    assert_eq!(file, "LiberationSans-Bold.ttf");
    assert!(!synthesis.bold);
    let (synthesis, _, _) = local(&mut ctx, "D", "Liberation Sans", 400.0, FontStyle::Italic);
    assert!(!synthesis.oblique);
    let (_, _, font) = local(&mut ctx, "S", "Liberation Sans", 400.0, FontStyle::Normal);
    let mut plain = FontContext::for_tests();
    let families = [Named("Liberation Sans")];
    let q = query(&families, 400.0, FontStyle::Normal);
    let reference = plain.itemize("H", &q)[0].font;
    let half = ctx.metrics(font, 100.0).ascent;
    let full = plain.metrics(reference, 100.0).ascent;
    assert_close(half, full / 2.0);
}
