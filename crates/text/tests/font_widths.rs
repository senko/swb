//! Compares the widths of text shaped by swb with the widths in Chromium 148
//! at CSS font sizes from 9 to 24 px. The data is
//! `chromium-font-widths.txt`, made by `swbtools measure font-size-sweep`
//! (the method is in `tools/swbtools/font_sizes.py`): 20 times `0` in three
//! families and 100 times `AV` in Liberation Sans (strong kerning), each
//! alone on a page.

use swb_text::{FamilyName, FontContext, FontId, FontQuery, ShapeOptions};

const MEASURED: &str = include_str!("chromium-font-widths.txt");

/// The families of the columns, after the size.
const FAMILIES: [&str; 4] = [
    "Liberation Mono",
    "Liberation Serif",
    "DejaVu Sans",
    "Liberation Sans",
];

fn select(ctx: &mut FontContext, family: &str) -> FontId {
    ctx.select(&FontQuery::new(&[FamilyName::Named(family)]))
}

/// The width of a text item in Chromium, in 1/64 px: the sum of the glyph
/// advances (exact: they are multiples of 1/65536 px) as an `f32`, rounded
/// up to 1/64 px.
fn width_64th(ctx: &mut FontContext, font: FontId, size: f32, text: &str) -> i64 {
    let run = ctx.shape(font, size, text, &ShapeOptions::default());
    let sum: f64 = run.glyphs.iter().map(|g| f64::from(g.x_advance)).sum();
    (f64::from(sum as f32) * 64.0).ceil() as i64
}

#[test]
fn widths_match_chromium() {
    let mut ctx = FontContext::for_tests();
    let fonts = FAMILIES.map(|family| select(&mut ctx, family));
    let texts = [
        "0".repeat(20),
        "0".repeat(20),
        "0".repeat(20),
        "AV".repeat(100),
    ];
    let mut checked = 0;
    let mut differences = Vec::new();
    for line in MEASURED.lines() {
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        let numbers: Vec<i64> = line
            .split_whitespace()
            .map(|n| n.parse().expect("the data file has only numbers"))
            .collect();
        let [milli, widths @ ..] = &numbers[..] else {
            panic!("a line has numbers: {line}");
        };
        assert_eq!(widths.len(), FAMILIES.len(), "{line}");
        let size: f32 = format!("{}.{:03}", milli / 1000, milli % 1000)
            .parse()
            .expect("a decimal number is a valid f32");
        // The data has Mono, Serif, DejaVu and then Sans: the same order.
        for (i, expected) in widths.iter().enumerate() {
            let width = width_64th(&mut ctx, fonts[i], size, &texts[i]);
            if width != *expected {
                differences.push(format!(
                    "{size} px, {}: {width}/64, Chromium {expected}/64",
                    FAMILIES[i]
                ));
            }
        }
        checked += 1;
    }
    assert!(checked > 1000, "{checked} sizes");
    assert!(
        differences.is_empty(),
        "{} of {checked} sizes differ:\n{}",
        differences.len(),
        differences.join("\n")
    );
}
