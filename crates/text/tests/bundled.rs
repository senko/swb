//! Tests with the bundled fonts in `fixtures/fonts` (directory mode).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use swb_text::{
    Direction, FamilyName, Feature, FontContext, FontId, FontQuery, FontStyle, GenericFamily,
    GenericFamilyMap, ShapeOptions, Synthesis, TextError,
};

use FamilyName::{Generic, Named};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/fonts")
}

/// The file name of the font's face.
fn file(ctx: &FontContext, font: FontId) -> String {
    let info = ctx
        .font_info(font)
        .expect("bundled fonts are never placeholders");
    info.path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default()
        .to_owned()
}

fn select(
    ctx: &mut FontContext,
    families: &[FamilyName<'_>],
    weight: f32,
    style: FontStyle,
) -> FontId {
    let mut query = FontQuery::new(families);
    query.weight = weight;
    query.style = style;
    ctx.select(&query)
}

/// A directory with copies of some bundled fonts.
fn font_dir(name: &str, files: &[&str]) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("the test directory can be created");
    for file in files {
        std::fs::copy(fixtures().join(file), dir.join(file)).expect("bundled fonts can be copied");
    }
    dir
}

#[test]
fn selects_named_family() {
    let mut ctx = FontContext::for_tests();
    let font = select(
        &mut ctx,
        &[Named("Liberation Serif")],
        400.0,
        FontStyle::Normal,
    );
    assert_eq!(file(&ctx, font), "LiberationSerif-Regular.ttf");
    assert_eq!(ctx.font_info(font).unwrap().family, "Liberation Serif");
    // Family names ignore ASCII case.
    let again = select(
        &mut ctx,
        &[Named("liberation SERIF")],
        400.0,
        FontStyle::Normal,
    );
    assert_eq!(font, again);
}

#[test]
fn skips_missing_families() {
    let mut ctx = FontContext::for_tests();
    let families = [
        Named("No Such Font"),
        Named("Liberation Mono"),
        Named("DejaVu Sans"),
    ];
    let font = select(&mut ctx, &families, 400.0, FontStyle::Normal);
    assert_eq!(file(&ctx, font), "LiberationMono-Regular.ttf");
}

#[test]
fn generic_families_use_the_map() {
    let mut ctx = FontContext::for_tests();
    let cases = [
        (GenericFamily::Serif, "LiberationSerif-Regular.ttf"),
        (GenericFamily::SansSerif, "LiberationSans-Regular.ttf"),
        (GenericFamily::Monospace, "LiberationMono-Regular.ttf"),
        (GenericFamily::Cursive, "LiberationSans-Regular.ttf"),
        (GenericFamily::SystemUi, "LiberationSans-Regular.ttf"),
    ];
    for (generic, expected) in cases {
        let font = select(&mut ctx, &[Generic(generic)], 400.0, FontStyle::Normal);
        assert_eq!(file(&ctx, font), expected, "{generic:?}");
    }
}

#[test]
fn aliases_resolve_to_metric_compatible_fonts() {
    let mut ctx = FontContext::for_tests();
    let cases = [
        ("Arial", "LiberationSans-Regular.ttf"),
        ("Helvetica", "LiberationSans-Regular.ttf"),
        ("Times New Roman", "LiberationSerif-Regular.ttf"),
        ("Courier", "LiberationMono-Regular.ttf"),
    ];
    for (name, expected) in cases {
        let font = select(&mut ctx, &[Named(name)], 400.0, FontStyle::Normal);
        assert_eq!(file(&ctx, font), expected, "{name}");
    }
}

#[test]
fn unknown_family_uses_standard_serif_family() {
    // Blink's standard font is serif (checked against Chromium).
    let mut ctx = FontContext::for_tests();
    let font = select(
        &mut ctx,
        &[Named("Linux Libertine")],
        400.0,
        FontStyle::Normal,
    );
    assert_eq!(file(&ctx, font), "LiberationSerif-Regular.ttf");
    let font = select(&mut ctx, &[], 400.0, FontStyle::Normal);
    assert_eq!(file(&ctx, font), "LiberationSerif-Regular.ttf");
}

#[test]
fn matches_weight_and_style() {
    let mut ctx = FontContext::for_tests();
    let sans = [Named("Liberation Sans")];
    let cases = [
        (400.0, FontStyle::Normal, "LiberationSans-Regular.ttf"),
        (700.0, FontStyle::Normal, "LiberationSans-Bold.ttf"),
        (900.0, FontStyle::Normal, "LiberationSans-Bold.ttf"),
        (600.0, FontStyle::Normal, "LiberationSans-Bold.ttf"),
        (500.0, FontStyle::Normal, "LiberationSans-Regular.ttf"),
        (100.0, FontStyle::Normal, "LiberationSans-Regular.ttf"),
        (400.0, FontStyle::Italic, "LiberationSans-Italic.ttf"),
        (400.0, FontStyle::Oblique, "LiberationSans-Italic.ttf"),
        (700.0, FontStyle::Italic, "LiberationSans-BoldItalic.ttf"),
    ];
    for (weight, style, expected) in cases {
        let font = select(&mut ctx, &sans, weight, style);
        assert_eq!(file(&ctx, font), expected, "{weight} {style:?}");
        let synthesis = ctx.synthesis(font);
        assert!(!synthesis.bold && !synthesis.oblique, "{weight} {style:?}");
    }
}

#[test]
fn synthesizes_missing_styles() {
    let dir = font_dir("regular-only", &["LiberationSans-Regular.ttf"]);
    let mut ctx = FontContext::from_directory(&dir, GenericFamilyMap::bundled()).unwrap();
    let sans = [Named("Liberation Sans")];

    let bold = select(&mut ctx, &sans, 700.0, FontStyle::Normal);
    assert_eq!(file(&ctx, bold), "LiberationSans-Regular.ttf");
    assert!(ctx.synthesis(bold).bold);
    assert!(!ctx.synthesis(bold).oblique);

    // Chromium synthesizes bold only above face weight + 200.
    let semibold = select(&mut ctx, &sans, 600.0, FontStyle::Normal);
    assert!(!ctx.synthesis(semibold).bold);

    let italic = select(&mut ctx, &sans, 400.0, FontStyle::Italic);
    assert!(ctx.synthesis(italic).oblique);
    assert!(!ctx.synthesis(italic).bold);

    let regular = select(&mut ctx, &sans, 400.0, FontStyle::Normal);
    assert_eq!(ctx.synthesis(regular), Synthesis::default());
    assert_ne!(regular, bold);
    assert_ne!(bold, italic);
}

#[test]
fn itemizes_with_fallback() {
    let mut ctx = FontContext::for_tests();
    let families = [Named("Liberation Sans")];
    let query = FontQuery::new(&families);
    // Latin, Cyrillic and Greek are in Liberation Sans; the snowman and the
    // heavy box-drawing line are only in DejaVu Sans.
    let text = "Hi Жж Ωω ☃━ end";
    let runs = ctx.itemize(text, &query);
    let described: Vec<(&str, String)> = runs
        .iter()
        .map(|run| (&text[run.range.clone()], file(&ctx, run.font)))
        .collect();
    assert_eq!(
        described,
        vec![
            ("Hi Жж Ωω ", "LiberationSans-Regular.ttf".to_owned()),
            ("☃━ ", "DejaVuSans.ttf".to_owned()),
            ("end", "LiberationSans-Regular.ttf".to_owned()),
        ]
    );
}

#[test]
fn runs_cover_the_text() {
    let mut ctx = FontContext::for_tests();
    let families = [Generic(GenericFamily::Monospace)];
    let query = FontQuery::new(&families);
    let text = "a☃b\u{20D7}\n中文 x";
    let runs = ctx.itemize(text, &query);
    assert_eq!(runs.first().unwrap().range.start, 0);
    assert_eq!(runs.last().unwrap().range.end, text.len());
    for pair in runs.windows(2) {
        assert_eq!(pair[0].range.end, pair[1].range.start);
        assert_ne!(pair[0].font, pair[1].font);
    }
}

#[test]
fn combining_marks_stay_with_base() {
    let mut ctx = FontContext::for_tests();
    let families = [Named("Liberation Sans")];
    let query = FontQuery::new(&families);
    // U+20D7 COMBINING RIGHT ARROW ABOVE is only in DejaVu Sans, so the
    // whole cluster "x⃗" moves there.
    let text = "ax\u{20D7}b";
    let runs = ctx.itemize(text, &query);
    let ranges: Vec<_> = runs.iter().map(|r| r.range.clone()).collect();
    assert_eq!(ranges, vec![0..1, 1..5, 5..6]);
    assert_eq!(file(&ctx, runs[1].font), "DejaVuSans.ttf");
    // A mark that the primary font has stays in one run.
    let runs = ctx.itemize("e\u{0301}x", &query);
    assert_eq!(runs.len(), 1);
}

#[test]
fn decomposable_characters_stay_in_the_primary_font() {
    let mut ctx = FontContext::for_tests();
    let families = [Named("Liberation Sans")];
    let query = FontQuery::new(&families);
    // Liberation Sans has no U+2249 NOT ALMOST EQUAL TO, but it has its
    // decomposition U+2248 U+0338. The shaper uses those glyphs, as
    // HarfBuzz does in Chromium, so no fallback font is needed.
    let text = "a\u{2249}b";
    let runs = ctx.itemize(text, &query);
    assert_eq!(runs.len(), 1);
    assert_eq!(file(&ctx, runs[0].font), "LiberationSans-Regular.ttf");
    let run = ctx.shape(runs[0].font, 16.0, text, &ShapeOptions::default());
    assert!(run.glyphs.iter().all(|g| g.glyph.0 != 0), "{run:?}");
}

/// Copies `source` from the bundled fonts to `dir/name` with `unitsPerEm`
/// set to 8, which the font loader rejects. The file still describes
/// itself and maps characters.
fn write_unloadable_copy(source: &str, dir: &Path, name: &str) {
    let mut data = std::fs::read(fixtures().join(source)).expect("bundled fonts are readable");
    let be16 = |data: &[u8], at: usize| u16::from_be_bytes([data[at], data[at + 1]]);
    let be32 = |data: &[u8], at: usize| {
        u32::from_be_bytes([data[at], data[at + 1], data[at + 2], data[at + 3]])
    };
    let head = (0..usize::from(be16(&data, 4)))
        .map(|i| 12 + 16 * i)
        .find(|&record| &data[record..record + 4] == b"head")
        .map(|record| be32(&data, record + 8) as usize)
        .expect("the font has a head table");
    data[head + 18..head + 20].copy_from_slice(&8u16.to_be_bytes());
    std::fs::write(dir.join(name), data).expect("the test font can be written");
}

#[test]
fn fallback_skips_faces_that_fail_to_load() {
    let dir = font_dir(
        "fallback-skips-broken",
        &["LiberationSans-Regular.ttf", "DejaVuSans-Bold.ttf"],
    );
    // The regular face of the fallback family "DejaVu Sans" cannot be
    // loaded. Fallback must continue with the next candidate, the bold
    // face, instead of giving up.
    write_unloadable_copy("DejaVuSans.ttf", &dir, "DejaVuSans-Broken.ttf");
    let mut ctx = FontContext::from_directory(&dir, GenericFamilyMap::bundled()).unwrap();
    let families = [Named("Liberation Sans")];
    let runs = ctx.itemize("A☃", &FontQuery::new(&families));
    assert_eq!(runs.len(), 2);
    assert_eq!(file(&ctx, runs[1].font), "DejaVuSans-Bold.ttf");
}

#[test]
fn fallback_in_bold_text_uses_synthetic_bold() {
    let mut ctx = FontContext::for_tests();
    let families = [Named("Liberation Sans")];
    let mut query = FontQuery::new(&families);
    query.weight = 700.0;
    let runs = ctx.itemize("A☃", &query);
    assert_eq!(runs.len(), 2);
    assert_eq!(file(&ctx, runs[0].font), "LiberationSans-Bold.ttf");
    // Like Chromium on Linux: system fallback takes the regular face and
    // emboldens it.
    assert_eq!(file(&ctx, runs[1].font), "DejaVuSans.ttf");
    assert!(ctx.synthesis(runs[1].font).bold);
}

#[test]
fn query_families_are_tried_before_fallback() {
    let mut ctx = FontContext::for_tests();
    let families = [Named("Liberation Serif"), Named("DejaVu Sans")];
    let query = FontQuery::new(&families);
    let runs = ctx.itemize("x☃", &query);
    assert_eq!(file(&ctx, runs[0].font), "LiberationSerif-Regular.ttf");
    assert_eq!(file(&ctx, runs[1].font), "DejaVuSans.ttf");
}

#[test]
fn ascii_and_empty_text() {
    let mut ctx = FontContext::for_tests();
    let families = [Named("Liberation Sans")];
    let query = FontQuery::new(&families);
    let runs = ctx.itemize("Plain ASCII text.\n", &query);
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].range, 0..18);
    assert_eq!(ctx.itemize("", &query), vec![]);
}

#[test]
fn metrics_of_liberation_sans() {
    let mut ctx = FontContext::for_tests();
    let font = select(
        &mut ctx,
        &[Named("Liberation Sans")],
        400.0,
        FontStyle::Normal,
    );
    let m = ctx.metrics(font, 16.0);
    let px = |units: f32| units * 16.0 / 2048.0;
    assert_eq!(m.units_per_em, 2048);
    // hhea ascender 1854, descender -434, lineGap 67: 14.48, 3.39, 0.52 px.
    assert_eq!(m.ascent, px(1854.0));
    assert_eq!(m.descent, px(434.0));
    assert_eq!(m.line_gap, px(67.0));
    // OS/2 sxHeight 1082, sCapHeight 1409.
    assert_eq!(m.x_height, px(1082.0));
    assert_eq!(m.cap_height, px(1409.0));
    // post underlinePosition -67, underlineThickness 150.
    assert_eq!(m.underline_offset, px(67.0));
    assert_eq!(m.underline_thickness, px(150.0));
    // OS/2 yStrikeoutPosition 530, yStrikeoutSize 102.
    assert_eq!(m.strikeout_offset, px(-530.0));
    assert_eq!(m.strikeout_thickness, px(102.0));
    // Metrics scale linearly.
    assert_eq!(ctx.metrics(font, 32.0).ascent, 2.0 * m.ascent);
}

#[test]
fn shapes_hello() {
    let mut ctx = FontContext::for_tests();
    let font = select(
        &mut ctx,
        &[Named("Liberation Sans")],
        400.0,
        FontStyle::Normal,
    );
    let run = ctx.shape(font, 16.0, "Hello", &ShapeOptions::default());
    // hmtx advances H 1479, e 1139, l 455, l 455, o 1139: 4667 units.
    assert_eq!(run.glyphs.len(), 5);
    assert_eq!(run.advance, 4667.0 * 16.0 / 2048.0);
    let clusters: Vec<u32> = run.glyphs.iter().map(|g| g.cluster).collect();
    assert_eq!(clusters, vec![0, 1, 2, 3, 4]);
    assert_eq!(run.glyphs[2].glyph, run.glyphs[3].glyph);
}

#[test]
fn kerning_narrows_av() {
    let mut ctx = FontContext::for_tests();
    let font = select(
        &mut ctx,
        &[Named("Liberation Sans")],
        400.0,
        FontStyle::Normal,
    );
    let a = ctx.shape(font, 16.0, "A", &ShapeOptions::default()).advance;
    let v = ctx.shape(font, 16.0, "V", &ShapeOptions::default()).advance;
    let av = ctx
        .shape(font, 16.0, "AV", &ShapeOptions::default())
        .advance;
    assert!(av < a + v, "{av} < {a} + {v}");
    let no_kern = [Feature {
        tag: *b"kern",
        value: 0,
    }];
    let options = ShapeOptions {
        features: &no_kern,
        ..ShapeOptions::default()
    };
    assert_eq!(ctx.shape(font, 16.0, "AV", &options).advance, a + v);
}

#[test]
fn clusters_are_byte_offsets() {
    let mut ctx = FontContext::for_tests();
    let font = select(&mut ctx, &[Named("DejaVu Sans")], 400.0, FontStyle::Normal);
    let run = ctx.shape(font, 16.0, "aЖ☃b", &ShapeOptions::default());
    let clusters: Vec<u32> = run.glyphs.iter().map(|g| g.cluster).collect();
    assert_eq!(clusters, vec![0, 1, 3, 6]);
}

#[test]
fn rtl_runs_are_in_visual_order() {
    let mut ctx = FontContext::for_tests();
    let font = select(
        &mut ctx,
        &[Named("Liberation Sans")],
        400.0,
        FontStyle::Normal,
    );
    let options = ShapeOptions {
        direction: Direction::Rtl,
        language: Some("he"),
        features: &[],
    };
    // Four Hebrew letters, two bytes each.
    let run = ctx.shape(font, 16.0, "שלום", &options);
    let clusters: Vec<u32> = run.glyphs.iter().map(|g| g.cluster).collect();
    assert_eq!(clusters, vec![6, 4, 2, 0]);
}

#[test]
fn glyph_masks() {
    let mut ctx = FontContext::for_tests();
    let font = select(
        &mut ctx,
        &[Named("Liberation Sans")],
        400.0,
        FontStyle::Normal,
    );
    let run = ctx.shape(font, 16.0, "A ", &ShapeOptions::default());
    let (a, space) = (run.glyphs[0].glyph, run.glyphs[1].glyph);

    let mask = ctx.glyph_mask(font, a, 16.0, 0).expect("A has an outline");
    assert_eq!(mask.data.len(), (mask.width * mask.height) as usize);
    assert!(mask.top < 0, "the glyph is above the baseline");
    assert!(mask.data.contains(&255));
    // The cap height is 11 px; the mask covers it.
    assert!(mask.height >= 11 && mask.height <= 13);

    assert!(ctx.glyph_mask(font, space, 16.0, 0).is_none());

    let shifted = ctx.glyph_mask(font, a, 16.0, 2).unwrap();
    assert_ne!(mask.data, shifted.data);
    // Cached: the same mask comes back.
    let cached = ctx.glyph_mask(font, a, 16.0, 0).unwrap();
    assert!(Arc::ptr_eq(&mask, &cached));

    // Masks larger than 256 × 256 pixels are rasterized on every call.
    let large = ctx.glyph_mask(font, a, 500.0, 0).unwrap();
    assert!(large.data.len() > 256 * 256);
    let again = ctx.glyph_mask(font, a, 500.0, 0).unwrap();
    assert!(!Arc::ptr_eq(&large, &again));
    assert_eq!(large, again);
}

#[test]
fn synthetic_styles_change_masks() {
    let dir = font_dir("regular-only-masks", &["LiberationSans-Regular.ttf"]);
    let mut ctx = FontContext::from_directory(&dir, GenericFamilyMap::bundled()).unwrap();
    let sans = [Named("Liberation Sans")];
    let regular = select(&mut ctx, &sans, 400.0, FontStyle::Normal);
    let bold = select(&mut ctx, &sans, 700.0, FontStyle::Normal);
    let italic = select(&mut ctx, &sans, 400.0, FontStyle::Italic);
    let glyph = ctx
        .shape(regular, 32.0, "l", &ShapeOptions::default())
        .glyphs[0]
        .glyph;

    let coverage =
        |mask: &swb_text::GlyphMask| mask.data.iter().map(|&c| u32::from(c)).sum::<u32>();
    let plain = ctx.glyph_mask(regular, glyph, 32.0, 0).unwrap();
    let heavy = ctx.glyph_mask(bold, glyph, 32.0, 0).unwrap();
    assert!(coverage(&heavy) > coverage(&plain));
    let slanted = ctx.glyph_mask(italic, glyph, 32.0, 0).unwrap();
    // A vertical stem becomes wider when sheared.
    assert!(slanted.width > plain.width + 3);
}

#[test]
fn directory_errors() {
    let missing = Path::new(env!("CARGO_TARGET_TMPDIR")).join("no-such-dir");
    assert!(matches!(
        FontContext::from_directory(&missing, GenericFamilyMap::bundled()),
        Err(TextError::Io { .. })
    ));
    let empty = font_dir("empty-font-dir", &[]);
    assert!(matches!(
        FontContext::from_directory(&empty, GenericFamilyMap::bundled()),
        Err(TextError::NoFonts(_))
    ));
}
