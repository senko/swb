//! Tests of SVG decoding, natural sizes, limits and rendering. Expected
//! sizes and pixels come from Chromium 148 where noted.

use std::fmt::Write as _;

use super::*;

fn svg(attributes: &str, content: &str) -> String {
    format!(r#"<svg xmlns="http://www.w3.org/2000/svg" {attributes}>{content}</svg>"#)
}

fn natural(attributes: &str) -> NaturalSize {
    decode(svg(attributes, "").as_bytes())
        .unwrap_or_else(|e| panic!("{attributes}: {e}"))
        .natural_size()
}

fn sized(width: Option<f32>, height: Option<f32>, ratio: Option<f32>) -> NaturalSize {
    NaturalSize {
        width,
        height,
        ratio,
    }
}

#[test]
fn natural_sizes_follow_chromium() {
    assert_eq!(
        natural(r#"viewBox="0 0 40 20""#),
        sized(None, None, Some(2.0))
    );
    assert_eq!(natural(r#"width="60""#), sized(Some(60.0), None, None));
    assert_eq!(natural(r#"height="30""#), sized(None, Some(30.0), None));
    assert_eq!(
        natural(r#"width="50%" height="40""#),
        sized(None, Some(40.0), None)
    );
    assert_eq!(
        natural(r#"width="60" height="30""#),
        NaturalSize::fixed(60.0, 30.0)
    );
    assert_eq!(
        natural(r#"width="60" viewBox="0 0 40 20""#),
        sized(Some(60.0), None, Some(2.0))
    );
    assert_eq!(
        natural(r#"width="auto" height="30" viewBox="0 0 40 20""#),
        sized(None, Some(30.0), Some(2.0))
    );
    assert_eq!(
        natural(r#"width="60" height="30" viewBox="0 0 10 10""#),
        NaturalSize::fixed(60.0, 30.0)
    );
    assert_eq!(natural(""), NaturalSize::default());
    assert_eq!(
        natural(r#"width="10em" height="2pc""#),
        NaturalSize::fixed(160.0, 32.0)
    );
    let units = natural(r#"width="2in" height="1cm""#);
    assert_eq!(units.width, Some(192.0));
    assert!((units.height.unwrap_or(0.0) - 37.795).abs() < 0.01);
    // A viewBox without area is ignored.
    assert_eq!(natural(r#"viewBox="0 0 0 20""#), NaturalSize::default());
}

#[test]
fn zero_negative_and_huge_sizes() {
    // Chromium: 0x20 for width 0 or -5; nothing is drawn.
    for width in ["0", "-5"] {
        let image = decode(svg(&format!(r#"width="{width}" height="20""#), "").as_bytes()).unwrap();
        assert_eq!(image.natural_size(), sized(Some(0.0), Some(20.0), None));
        let size = IntSize::from_wh(10, 10).unwrap();
        let pixmap = image.render(size, (10.0, 10.0)).unwrap();
        assert!(pixmap.data().iter().all(|&b| b == 0));
    }
    // Huge sizes are clamped like layout lengths; rendering is bounded by
    // the size it is drawn at.
    let huge = decode(
        svg(
            r#"width="1e9" height="1e30" viewBox="0 0 1 1""#,
            r#"<rect width="1" height="1" fill="red"/>"#,
        )
        .as_bytes(),
    )
    .unwrap();
    let max = swb_style::Length::MAX_PX;
    assert_eq!(huge.natural_size().width, Some(max));
    assert_eq!(huge.natural_size().height, Some(max));
    let pixmap = huge
        .render(IntSize::from_wh(4, 4).unwrap(), (4.0, 4.0))
        .unwrap();
    assert_eq!(
        pixmap.pixel(2, 2).map(tiny_skia::PremultipliedColorU8::red),
        Some(255)
    );
}

fn error(data: &[u8]) -> String {
    match decode(data) {
        Ok(_) => panic!(
            "decoded: {}",
            String::from_utf8_lossy(&data[..data.len().min(100)])
        ),
        Err(e) => e.to_string(),
    }
}

#[test]
fn invalid_sources_are_errors() {
    error(b"");
    error(b"not xml");
    error(b"<svg xmlns='http://www.w3.org/2000/svg'><g></svg>");
    error(b"<html xmlns='http://www.w3.org/2000/svg'/>");
    error(b"<svg/>");
    error(&[0xff, 0xfe, b'<', 0]);
    // svgz without Content-Encoding.
    assert!(error(&[0x1f, 0x8b, 8, 0, 0, 0, 0, 0]).contains("gzip"));
    // A byte order mark is fine.
    let mut bom = b"\xEF\xBB\xBF".to_vec();
    bom.extend_from_slice(svg("", "").as_bytes());
    assert!(decode(&bom).is_ok());
}

#[test]
fn limits_are_enforced() {
    let large = svg("", &" ".repeat(MAX_SOURCE_BYTES));
    assert!(error(large.as_bytes()).contains("larger than"));

    let nodes = svg("", &"<g/>".repeat(MAX_NODES as usize));
    assert!(error(nodes.as_bytes()).contains("nodes limit"));

    let deep = svg(
        "",
        &format!("{}{}", "<g>".repeat(MAX_DEPTH), "</g>".repeat(MAX_DEPTH)),
    );
    assert!(error(deep.as_bytes()).contains("nested deeper"));
    let ok = svg(
        "",
        &format!(
            "{}{}",
            "<g>".repeat(MAX_DEPTH - 1),
            "</g>".repeat(MAX_DEPTH - 1)
        ),
    );
    assert!(decode(ok.as_bytes()).is_ok());

    let mut dtd = String::from("<!ENTITY a0 \"aaaaaaaaaa\">");
    for i in 1..8 {
        let refs = format!("&a{};", i - 1).repeat(10);
        write!(dtd, "<!ENTITY a{i} \"{refs}\">").unwrap();
    }
    let bomb = format!("<!DOCTYPE svg [{dtd}]>{}", svg("", "<desc>&a7;</desc>"));
    assert!(error(bomb.as_bytes()).contains("entity"));
    // roxmltree rejects entity references nested more than 10 levels.
    let mut dtd = String::from("<!ENTITY e0 \"x\">");
    for i in 1..=12 {
        write!(dtd, "<!ENTITY e{i} \"&e{};\">", i - 1).unwrap();
    }
    let chain = format!("<!DOCTYPE svg [{dtd}]>{}", svg("", "<desc>&e12;</desc>"));
    error(chain.as_bytes());
}

#[test]
fn illustrator_entities_work() {
    let source = r#"<?xml version="1.0"?>
<!DOCTYPE svg PUBLIC "-//W3C//DTD SVG 1.1//EN" "http://www.w3.org/Graphics/SVG/1.1/DTD/svg11.dtd" [
    <!ENTITY ns_svg "http://www.w3.org/2000/svg">
]>
<svg xmlns="&ns_svg;" width="4" height="2"/>"#;
    let image = decode(source.as_bytes()).unwrap();
    assert_eq!(image.natural_size(), NaturalSize::fixed(4.0, 2.0));
}

/// Deeply nested content (the depth limit and `<use>` copies) does not
/// overflow a 2 MiB stack, the default size for Rust threads.
#[test]
fn deep_nesting_fits_a_small_stack() {
    // With the root, the outer group and the rectangle: MAX_DEPTH levels.
    let depth = MAX_DEPTH - 3;
    let groups = format!(
        "{}<rect width='1' height='1'/>{}",
        "<g opacity='0.9'>".repeat(depth),
        "</g>".repeat(depth)
    );
    // Each <use> copies the previous group, three levels deeper in usvg's
    // tree: 256 copies come close to usvg's depth limit of 1024 (258 fail).
    let mut uses = String::new();
    for i in 0..256 {
        write!(uses, "<g id='u{}'><use href='#u{i}'/></g>", i + 1).unwrap();
    }
    let source = svg(
        "viewBox='0 0 1 1'",
        &format!("<g id='u0'>{groups}</g>{uses}"),
    );
    std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(move || {
            let image = decode(source.as_bytes()).unwrap();
            let pixmap = image.render(IntSize::from_wh(8, 8).unwrap(), (8.0, 8.0));
            assert!(pixmap.is_some());
        })
        .unwrap()
        .join()
        .unwrap();
}

/// The pixel at (x, y) as (r, g, b, a), not premultiplied.
fn pixel(pixmap: &Pixmap, x: u32, y: u32) -> (u8, u8, u8, u8) {
    let p = pixmap.pixel(x, y).unwrap().demultiply();
    (p.red(), p.green(), p.blue(), p.alpha())
}

fn render(source: &str, width: u32, height: u32) -> Pixmap {
    decode(source.as_bytes())
        .unwrap()
        .render(
            IntSize::from_wh(width, height).unwrap(),
            (width as f32, height as f32),
        )
        .unwrap()
}

/// The bounding box (x, y, width, height) of the pixels that match.
fn bounds(
    pixmap: &Pixmap,
    matches: impl Fn((u8, u8, u8, u8)) -> bool,
) -> Option<(u32, u32, u32, u32)> {
    let mut found: Option<(u32, u32, u32, u32)> = None;
    for y in 0..pixmap.height() {
        for x in 0..pixmap.width() {
            if matches(pixel(pixmap, x, y)) {
                let (x0, y0, x1, y1) = found.unwrap_or((x, y, x, y));
                found = Some((x0.min(x), y0.min(y), x1.max(x), y1.max(y)));
            }
        }
    }
    found.map(|(x0, y0, x1, y1)| (x0, y0, x1 - x0 + 1, y1 - y0 + 1))
}

const RED: (u8, u8, u8, u8) = (255, 0, 0, 255);
const BLACK: (u8, u8, u8, u8) = (0, 0, 0, 255);

/// A red rectangle with a black quarter in the top-left corner.
const QUARTERS: &str =
    r#"<rect width="60" height="30" fill="red"/><rect width="30" height="15" fill="black"/>"#;

#[test]
fn rendering_follows_chromium() {
    let red = |p: &Pixmap| bounds(p, |c| c == RED);
    let black = |p: &Pixmap| bounds(p, |c| c == BLACK);
    // No viewBox: both axes scale to the concrete size.
    let p = render(&svg(r#"width="60" height="30""#, QUARTERS), 120, 30);
    assert_eq!(
        (red(&p), black(&p)),
        (Some((0, 0, 120, 30)), Some((0, 0, 60, 15)))
    );
    // No viewBox, only a width: only the x axis scales.
    let p = render(&svg(r#"width="60""#, QUARTERS), 120, 60);
    assert_eq!(black(&p), Some((0, 0, 60, 15)));
    assert_eq!(bounds(&p, |c| c.3 > 0), Some((0, 0, 120, 30)));
    // No size: no scaling.
    let p = render(&svg("", QUARTERS), 120, 60);
    assert_eq!(bounds(&p, |c| c.3 > 0), Some((0, 0, 60, 30)));
    // viewBox: centered, uniform scale (meet).
    let p = render(&svg(r#"viewBox="0 0 60 30""#, QUARTERS), 120, 30);
    assert_eq!(black(&p), Some((30, 0, 30, 15)));
    let p = render(&svg(r#"viewBox="0 0 60 30""#, QUARTERS), 120, 120);
    assert_eq!(black(&p), Some((0, 30, 60, 30)));
    // slice, aligned at the top left.
    let source = svg(
        r#"viewBox="0 0 60 30" preserveAspectRatio="xMinYMin slice""#,
        r#"<rect x="-100" y="-100" width="300" height="300" fill="red"/>
           <rect width="30" height="15" fill="black"/>"#,
    );
    let p = render(&source, 120, 120);
    assert_eq!(black(&p), Some((0, 0, 120, 60)));
    // The viewBox does not depend on the root's width and height.
    let p = render(
        &svg(r#"width="10" height="99" viewBox="0 0 60 30""#, QUARTERS),
        120,
        60,
    );
    assert_eq!(black(&p), Some((0, 0, 60, 30)));
}

#[test]
fn the_root_viewport_clips() {
    // Hacker News' triangle.svg: the path extends below the viewBox and
    // the viewport cuts it at the bottom.
    let source = r##"<svg height="32" viewBox="0 0 32 16" width="32" xmlns="http://www.w3.org/2000/svg"><path d="m2 27 14-29 14 29z" fill="#999"/></svg>"##;
    let p = render(source, 32, 32);
    let grey = bounds(&p, |c| c.3 > 0).unwrap();
    // Apex at y = 6 (viewBox centered vertically: offset 8), cut at 32.
    assert_eq!((grey.1, grey.1 + grey.3), (6, 32));
    assert_eq!(pixel(&p, 16, 31), (153, 153, 153, 255));
}

#[test]
fn renders_at_device_resolution() {
    // A circle rendered at 3x for 10 CSS px: 30 pixels across.
    let source = svg(r#"viewBox="0 0 10 10""#, r#"<circle cx="5" cy="5" r="5"/>"#);
    let image = decode(source.as_bytes()).unwrap();
    let pixmap = image
        .render(IntSize::from_wh(30, 30).unwrap(), (10.0, 10.0))
        .unwrap();
    assert_eq!(bounds(&pixmap, |c| c.3 > 128), Some((0, 0, 30, 30)));
    assert_eq!(pixel(&pixmap, 15, 15), BLACK);
    assert_eq!(pixel(&pixmap, 1, 1).3, 0, "outside the circle");
}

fn base64(data: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// An SVG that shows `href` in a 10x10 `<image>`.
fn with_image(href: &str) -> String {
    svg(
        r#"width="10" height="10""#,
        &format!(r#"<image href="{href}" width="10" height="10"/>"#),
    )
}

#[test]
fn embedded_data_images_are_drawn() {
    let png = crate::image::tests::png(2, 2, [255, 0, 0, 255]);
    let p = render(
        &with_image(&format!("data:image/png;base64,{}", base64(&png))),
        10,
        10,
    );
    assert_eq!(pixel(&p, 5, 5), RED);
    let inner = svg(r#"viewBox="0 0 1 1""#, r#"<rect width="1" height="1"/>"#);
    let p = render(
        &with_image(&format!(
            "data:image/svg+xml;base64,{}",
            base64(inner.as_bytes())
        )),
        10,
        10,
    );
    assert_eq!(pixel(&p, 5, 5), BLACK);
}

/// A PNG signature and header that declare a `width` × `height` image.
fn png_header(width: u32, height: u32) -> Vec<u8> {
    let mut chunk = b"IHDR".to_vec();
    chunk.extend_from_slice(&width.to_be_bytes());
    chunk.extend_from_slice(&height.to_be_bytes());
    chunk.extend_from_slice(&[8, 6, 0, 0, 0]); // 8-bit RGBA
    let mut crc = 0xffff_ffff_u32;
    for &byte in &chunk {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xedb8_8320
            } else {
                crc >> 1
            };
        }
    }
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    out.extend_from_slice(&13u32.to_be_bytes());
    out.extend_from_slice(&chunk);
    out.extend_from_slice(&(!crc).to_be_bytes());
    out
}

#[test]
fn embedded_images_are_checked() {
    // A PNG that declares 20000x20000 pixels is not decoded.
    let href = format!(
        "data:image/png;base64,{}",
        base64(&png_header(20_000, 20_000))
    );
    let p = render(&with_image(&href), 10, 10);
    assert!(p.data().iter().all(|&b| b == 0));
    // SVG images nested deeper than MAX_NESTING are not drawn.
    let mut nested = svg(r#"viewBox="0 0 1 1""#, r#"<rect width="1" height="1"/>"#);
    for _ in 0..MAX_NESTING {
        nested = with_image(&format!(
            "data:image/svg+xml;base64,{}",
            base64(nested.as_bytes())
        ));
    }
    assert_eq!(pixel(&render(&nested, 10, 10), 5, 5), BLACK);
    let deeper = with_image(&format!(
        "data:image/svg+xml;base64,{}",
        base64(nested.as_bytes())
    ));
    assert_eq!(pixel(&render(&deeper, 10, 10), 5, 5).3, 0);
}

#[test]
fn files_are_not_loaded() {
    let dir = std::env::temp_dir().join(format!("swb-svg-files-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("red.png");
    std::fs::write(&file, crate::image::tests::png(2, 2, [255, 0, 0, 255])).unwrap();
    let path = file.to_string_lossy().into_owned();
    for href in [path.clone(), format!("file://{path}"), "red.png".to_owned()] {
        let p = render(&with_image(&href), 10, 10);
        assert!(p.data().iter().all(|&b| b == 0), "{href} was loaded");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn expansion_limits_are_enforced() {
    // 10^6 rectangles through <use>: rejected before usvg copies them.
    let mut defs = String::from("<g id='l0'><rect width='1' height='1'/></g>");
    for i in 1..7 {
        let uses = format!("<use href='#l{}'/>", i - 1).repeat(10);
        write!(defs, "<g id='l{i}'>{uses}</g>").unwrap();
    }
    let bomb = svg("", &format!("<defs>{defs}</defs><use href='#l6'/>"));
    assert!(error(bomb.as_bytes()).contains("too many elements"));
    // A marker on each of 100,000 vertices.
    let marker = format!("<marker id='m'>{}</marker>", "<rect/>".repeat(10));
    let points = "1,1 ".repeat(100_000);
    let markers = svg(
        "",
        &format!("{marker}<polyline points='{points}' marker-mid='url(#m)'/>"),
    );
    assert!(error(markers.as_bytes()).contains("too many elements"));
    // Embedded SVG images have the same limits.
    let inner = with_image(&format!(
        "data:image/svg+xml;base64,{}",
        base64(bomb.as_bytes())
    ));
    assert_eq!(pixel(&render(&inner, 10, 10), 5, 5).3, 0);
}

#[test]
fn expensive_renderings_get_a_lower_resolution() {
    // A morphology radius of 3 user units is 300 px at 1000 px: 360,000
    // neighbors per pixel.
    let source = svg(
        r#"viewBox="0 0 10 10""#,
        "<filter id='f'><feMorphology radius='3'/></filter>
         <rect width='10' height='10' filter='url(#f)'/>",
    );
    let image = decode(source.as_bytes()).unwrap();
    let size = image
        .affordable_size(IntSize::from_wh(1000, 500).unwrap(), 1.0)
        .unwrap();
    assert!(size.width() < 200 && size.width() > 50, "{size:?}");
    assert_eq!(size.width(), size.height() * 2);
    // Cheap images keep their size.
    let cheap =
        decode(svg(r#"viewBox="0 0 1 1""#, "<rect width='1' height='1'/>").as_bytes()).unwrap();
    let full = IntSize::from_wh(4000, 4000).unwrap();
    assert_eq!(cheap.affordable_size(full, 1.0), Some(full));
    // A pattern tile of 6000 x 6000 user units in a 10 x 10 image is not
    // rendered at all.
    let tile = svg(
        r#"viewBox="0 0 10 10""#,
        "<pattern id='p' width='6000000' height='6000000' patternUnits='userSpaceOnUse'>
           <rect width='1' height='1'/></pattern>
         <rect width='10' height='10' fill='url(#p)'/>",
    );
    let image = decode(tile.as_bytes()).unwrap();
    let pixmap = image
        .render(IntSize::from_wh(10, 10).unwrap(), (10.0, 10.0))
        .unwrap();
    assert_eq!((pixmap.width(), pixmap.height()), (1, 1));
}

#[test]
fn reduced_sizes_stay_within_the_pixel_budget() {
    let fit = |w, h, max| {
        fit_pixels(IntSize::from_wh(w, h).unwrap(), max).map(|s| (s.width(), s.height()))
    };
    assert_eq!(fit(100, 50, 5000.0), Some((100, 50)));
    assert_eq!(fit(100, 50, 1250.0), Some((50, 25)));
    // A very thin strip: the short side stays 1 px and the long side
    // takes the budget, not its proportional length.
    assert_eq!(fit(1_000_000, 10, 1000.0), Some((1000, 1)));
    assert_eq!(fit(10, 1_000_000, 1000.5), Some((1, 1000)));
    assert_eq!(fit(16_384, 16_384, 1.0), Some((1, 1)));
    assert_eq!(fit(10, 10, 0.5), None);
    assert_eq!(fit(10, 10, f64::NAN), None);
    assert_eq!(fit(10, 10, f64::INFINITY), Some((10, 10)));
    for (w, h, max) in [
        (16_384, 3, 7.0),
        (4_000, 4_000, 999_999.9),
        (7, 16_000, 33.3),
    ] {
        let (fw, fh) = fit(w, h, max).unwrap();
        assert!(
            f64::from(fw) * f64::from(fh) <= max,
            "{w}x{h} in {max}: {fw}x{fh}"
        );
    }
}

/// The pixels of the canvas per pixel of the drawn box.
fn overdraw(source: &str, concrete: (f32, f32)) -> f64 {
    let image = decode(source.as_bytes()).unwrap();
    let content = image.content_transform(concrete).unwrap();
    image.overdraw(&content, concrete)
}

#[test]
fn the_cost_counts_canvas_outside_the_box() {
    let full = "<rect width='100' height='100'/>";
    // The view box fits the root: one canvas pixel per pixel.
    let fitting = svg(r#"width="100" height="100" viewBox="0 0 100 100""#, full);
    assert!((overdraw(&fitting, (1000.0, 1000.0)) - 1.0).abs() < 1e-6);
    // A root 1000 times taller than the view box: the canvas covers 1000
    // times the box (centered view box, scale 1).
    let tall = svg(r#"width="100" height="100000" viewBox="0 0 100 100""#, full);
    let k = overdraw(&tall, (1000.0, 1000.0));
    assert!((k - 1000.0).abs() < 1.0, "{k}");
    // `slice` into a wide box scales the view box by the larger factor.
    let slice = svg(
        r#"viewBox="0 0 100 100" preserveAspectRatio="xMidYMid slice""#,
        full,
    );
    let k = overdraw(&slice, (16384.0, 64.0));
    assert!((k - 256.0).abs() < 1.0, "{k}");
    // A height without natural size (a percentage) is not scaled; usvg's
    // canvas is then the content's bounding box.
    let percent = svg(
        r#"width="100" height="100000%""#,
        "<rect width='100' height='150000'/>",
    );
    assert!(overdraw(&percent, (100.0, 100.0)) > 1000.0);
    // The rendering budget accounts for it.
    let many = svg(
        r#"width="100" height="100000" viewBox="0 0 100 100""#,
        &"<rect width='100' height='100' fill-opacity='.5'/>".repeat(1000),
    );
    let image = decode(many.as_bytes()).unwrap();
    let size = IntSize::from_wh(1000, 1000).unwrap();
    assert!(image.render_work(size, (1000.0, 1000.0)) <= MAX_RENDER_WORK * 1.01);
    // 1000 blended layers of the whole box: about 250,000 pixels fit.
    let (planned, _) = image.plan(size, (1000.0, 1000.0)).unwrap();
    assert!(planned.width() < 600, "{planned:?}");
}

#[test]
fn tiny_roots_with_large_view_boxes_draw() {
    // Chromium draws this; inverting usvg's root transform would fail.
    let source = svg(
        r#"width="0.001" height="0.001" viewBox="0 0 1e6 1e6""#,
        "<rect width='1e6' height='1e6' fill='red'/>",
    );
    let p = render(&source, 100, 100);
    assert_eq!(pixel(&p, 50, 50), RED);
}

#[test]
fn extreme_ratios_are_limited() {
    let max = swb_style::Length::MAX_PX;
    let thin = natural(r#"viewBox="0 0 1e-18 1e19""#);
    assert_eq!(thin.ratio, Some(1.0 / max));
    let flat = natural(r#"width="1e7" height="1e-30""#);
    assert!(
        flat.ratio.is_none_or(|r| r.is_finite() && r <= max),
        "{flat:?}"
    );
}

/// A chain of `links` elements of `kind`, each referring to the previous
/// one, and a rectangle that uses the last one.
fn chain(kind: &str, links: usize) -> String {
    let mut content = String::new();
    for i in 0..links {
        let (open, close) = match kind {
            "mask" => (format!("<mask id='l{i}'>"), "</mask>"),
            "clip-path" => (format!("<clipPath id='l{i}'>"), "</clipPath>"),
            "fill" => (
                format!("<pattern id='l{i}' width='1' height='1' patternUnits='userSpaceOnUse'>"),
                "</pattern>",
            ),
            _ => (format!("<marker id='l{i}'>"), "</marker>"),
        };
        let inner = if i == 0 {
            "<rect width='1' height='1'/>".to_owned()
        } else if kind == "marker" {
            format!("<path d='M0 0 L1 1' marker-end='url(#l{})'/>", i - 1)
        } else {
            format!("<rect width='1' height='1' {kind}='url(#l{})'/>", i - 1)
        };
        write!(content, "{open}{inner}{close}").unwrap();
    }
    let last = links - 1;
    if kind == "marker" {
        write!(content, "<path d='M0 0 L1 1' marker-end='url(#l{last})'/>").unwrap();
    } else {
        write!(
            content,
            "<rect width='1' height='1' {kind}='url(#l{last})'/>"
        )
        .unwrap();
    }
    svg("viewBox='0 0 1 1'", &content)
}

/// Reference chains up to the depth limit fit a 2 MiB stack (the default
/// for Rust threads); longer chains are rejected, not followed. Nested
/// markers count as multiplying, so their chains stop at the element
/// budget much earlier.
#[test]
fn reference_chains_fit_a_small_stack() {
    let longest = (CONVERSION_LIMITS.depth / (expansion::LINK_DEPTH + 2.0)) as usize - 2;
    std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(move || {
            for (kind, links) in [
                ("mask", longest),
                ("clip-path", longest),
                ("fill", longest),
                ("marker", 6),
            ] {
                let image =
                    decode(chain(kind, links).as_bytes()).unwrap_or_else(|e| panic!("{kind}: {e}"));
                let pixmap = image.render(IntSize::from_wh(8, 8).unwrap(), (8.0, 8.0));
                assert!(pixmap.is_some(), "{kind}");
                assert!(decode(chain(kind, 1000).as_bytes()).is_err(), "{kind}");
            }
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn embedded_images_share_one_budget() {
    // An embedded image with about 32,000 elements, copied 20 times: each
    // copy is converted again, and together they exceed the budget.
    let mut defs = String::from("<g id='l0'><rect width='1' height='1'/></g>");
    for i in 1..5 {
        let uses = format!("<use href='#l{}'/>", i - 1).repeat(10);
        write!(defs, "<g id='l{i}'>{uses}</g>").unwrap();
    }
    let inner = svg(
        "viewBox='0 0 1 1'",
        &format!("<defs>{defs}</defs><use href='#l4'/>"),
    );
    let image = format!(
        "<g id='i'><image href='data:image/svg+xml;base64,{}' width='1' height='1'/></g>",
        base64(inner.as_bytes())
    );
    let budget = Arc::new(Mutex::new(BUDGET));
    let outer = svg(
        "viewBox='0 0 1 1'",
        &format!("<defs>{image}</defs>{}", "<use href='#i'/>".repeat(20)),
    );
    let converted = convert(outer.as_bytes(), 0, CONVERSION_LIMITS.depth, &budget);
    assert!(converted.is_ok());
    let left = *budget.lock().unwrap();
    assert!(left.elements >= 0.0 && left.elements < 70_000.0, "{left:?}");
}

#[test]
fn the_budget_is_taken_only_if_it_suffices() {
    let budget = Arc::new(Mutex::new(Budget {
        elements: 10.0,
        segments: 10.0,
        raster_pixels: 100.0,
    }));
    assert!(take(&budget, 5.0, 5.0, 60.0));
    assert!(!take(&budget, 0.0, 0.0, 60.0));
    assert!(!take(&budget, f64::NAN, 0.0, 0.0));
    let left = *budget.lock().unwrap();
    assert_eq!((left.elements, left.raster_pixels), (5.0, 40.0));
}
