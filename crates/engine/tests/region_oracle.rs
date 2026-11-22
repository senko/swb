//! The oracle of the region raster (ADR 0044 section 18), on the unit
//! level: a full render of a page fixture into A; B is a copy of A with
//! random rectangles filled with garbage; `Page::render_regions` draws those
//! rectangles into B; B must equal A bit for bit. The fixtures are the page
//! fixtures (JavaScript off) and the JS fixtures when `fixtures/js` exists
//! (not committed), at scales 1 and 1.5, at the top and at two scroll
//! offsets, with many rectangles each (small, thin, large, and ones that cut
//! text, images, shadows and groups wherever they fall).

// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

use swb_engine::{Page, Pixmap, Point, Size};
use swb_paint::PixelRect;

mod common;
use common::damage::first_difference;
use common::on_page_threads;

const VIEW: Size = Size::new(1280.0, 800.0);

/// A small deterministic generator of random numbers (xorshift64*).
pub(crate) struct Rng(u64);

impl Rng {
    pub(crate) fn new(seed: u64) -> Rng {
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    pub(crate) fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// A number in `0..n` (`n` > 0).
    pub(crate) fn below(&mut self, n: u32) -> u32 {
        (self.next() % u64::from(n.max(1))) as u32
    }
}

/// Random rectangles inside a target of `width` x `height`: small, thin,
/// medium and large ones.
fn random_rects(rng: &mut Rng, width: u32, height: u32) -> Vec<PixelRect> {
    let count = 1 + rng.below(6);
    (0..count)
        .map(|_| {
            let (w, h) = match rng.below(5) {
                0 => (1 + rng.below(12), 1 + rng.below(12)),
                1 => (1 + rng.below(3), 20 + rng.below(300)),
                2 => (20 + rng.below(300), 1 + rng.below(3)),
                3 => (20 + rng.below(200), 20 + rng.below(200)),
                _ => (100 + rng.below(width), 50 + rng.below(height / 2)),
            };
            let x = rng.below(width);
            let y = rng.below(height);
            PixelRect::from_edges(x, y, (x + w).min(width), (y + h).min(height))
        })
        .collect()
}

/// Fills `rects` of `pixmap` with garbage.
fn spoil(pixmap: &mut Pixmap, rects: &[PixelRect], rng: &mut Rng) {
    let width = pixmap.width() as usize;
    let data = pixmap.data_mut();
    for r in rects {
        for y in r.y..r.bottom() {
            for x in r.x..r.right() {
                let i = (y as usize * width + x as usize) * 4;
                let v = rng.next().to_le_bytes();
                // Premultiplied: no channel above the alpha.
                let a = v[3];
                if let Some(px) = data.get_mut(i..i + 4) {
                    px.copy_from_slice(&[v[0].min(a), v[1].min(a), v[2].min(a), a]);
                }
            }
        }
    }
}

/// Renders the page in full, then checks `rounds` rounds of random parts.
fn check_parts(page: &mut Page, label: &str, rng: &mut Rng, rounds: usize) -> usize {
    let (width, height) = swb_engine::device_size(page.viewport(), page.scale()).unwrap();
    let mut full = Pixmap::new(width, height).unwrap();
    page.render(&mut full);
    let mut parts = 0;
    for round in 0..rounds {
        let rects = random_rects(rng, width, height);
        let mut spoiled = full.clone();
        spoil(&mut spoiled, &rects, rng);
        page.render_regions(&mut spoiled, &rects);
        if let Some((x, y, want, got)) = first_difference(&full, &spoiled) {
            panic!(
                "{label}, round {round}: pixel ({x}, {y}) is {got:?}, a full raster gives \
                 {want:?}; parts {rects:?}"
            );
        }
        parts += rects.len();
    }
    parts
}

/// The fixture at scales 1 and 1.5, at three scroll offsets. `None` without
/// the fixture.
fn run_fixture(name: &str, javascript: bool, seed: u64) -> Option<usize> {
    let rounds = std::env::var("REGION_ROUNDS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(12);
    let mut rng = Rng::new(seed);
    let mut parts = 0;
    for scale in [1.0, 1.5] {
        let mut page =
            common::load_fixture(name, javascript, VIEW, |p| p.set_viewport(VIEW, scale))?;
        page.update_layout();
        for offset in [0.0, 700.0, 1900.0] {
            page.scroll_to(Point::new(0.0, offset));
            let label = format!("{name} (js {javascript}, scale {scale}, scroll {offset})");
            parts += check_parts(&mut page, &label, &mut rng, rounds);
        }
    }
    Some(parts)
}

/// A page with the drawing that a part can cut badly: rounded images and
/// clips, shadows, tiled gradients and images, filters, backdrop filters,
/// masks, rotations, scales and skews, inline SVG, blend modes, a frame,
/// text strokes and shadows, dashed rounded borders, outlines, clip paths.
fn features_page() -> String {
    let svg = "data:image/svg+xml,<svg xmlns='http://www.w3.org/2000/svg' width='40' \
        height='30'><circle cx='20' cy='15' r='12' fill='%23c03'/><path d='M0 30L40 0' \
        stroke='%2300f' stroke-width='3'/></svg>";
    let png = format!("data:image/png;base64,{}", base64(&gradient_png(23, 17)));
    let text = "Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod";
    format!(
        "<!DOCTYPE html><style>body{{margin:0;font:15px sans-serif}}\
         .b{{position:absolute;width:220px;height:140px}}\
         </style>\
         <div class=b style='left:10px;top:10px;border-radius:30px;overflow:hidden'>\
         <img src=\"{png}\" style='width:220px;height:140px'></div>\
         <div class=b style='left:250px;top:10px;box-shadow:0 10px 30px rgba(0,0,0,.6),\
         inset 0 0 12px red;border-radius:14px;background:#fff'>{text}</div>\
         <div class=b style='left:490px;top:10px;background:linear-gradient(33deg,red,blue 30%,\
         transparent),url(\"{png}\");background-size:37px 23px,19px 13px'></div>\
         <div class=b style='left:730px;top:10px;filter:blur(5px) drop-shadow(6px 4px 3px green)'>\
         <b>{text}</b><img src=\"{svg}\" width=80></div>\
         <div class=b style='left:970px;top:10px;background:url(\"{png}\") repeat;\
         background-size:13px 9px'>{text}</div>\
         <div class=b style='left:980px;top:40px;backdrop-filter:blur(7px) invert(1);\
         border-radius:20px;width:180px;height:90px'></div>\
         <div class=b style='left:10px;top:190px;-webkit-mask-image:linear-gradient(black,\
         transparent);mask-image:linear-gradient(black,transparent);background:#06c'>{text}</div>\
         <div class=b style='left:260px;top:200px;transform:rotate(17deg) scale(1.2);\
         background:#fd6;border:3px solid #333'>{text}<img src=\"{png}\" width=60></div>\
         <div class=b style='left:520px;top:200px;transform:skew(12deg,4deg);\
         background:linear-gradient(90deg,#0a0,#aa0)'>{text}</div>\
         <svg class=b style='left:770px;top:190px' viewBox='0 0 100 60'><circle cx='30' \
         cy='30' r='25' fill='#f0a' stroke='black' stroke-width='2' stroke-dasharray='5 3'/>\
         <path d='M50 5 Q90 30 60 55 T95 50' fill='none' stroke='teal' stroke-width='4'/></svg>\
         <div class=b style='left:1010px;top:190px;background:#c0c'>\
         <div style='opacity:.6;mix-blend-mode:multiply;background:#0cc;height:80px'>{text}</div>\
         </div>\
         <iframe class=b style='left:10px;top:370px;border:2px solid' srcdoc=\"<p \
         style='color:red'>frame <b>text</b></p><div style='filter:blur(2px)'>blurred</div>\
         <div style='backdrop-filter:invert(1);height:20px'>backdrop</div>\"></iframe>\
         <p class=b style='left:260px;top:370px;font-size:28px;-webkit-text-stroke:1px red;\
         text-shadow:2px 3px 4px blue'>{text}</p>\
         <div class=b style='left:520px;top:380px;border:6px dashed red;\
         border-radius:40px 10px'>{text}</div>\
         <div class=b style='left:780px;top:380px;outline:3px solid blue;outline-offset:4px;\
         clip-path:circle(45%);background:#9c3'>{text}</div>\
         <div class=b style='left:1020px;top:380px;opacity:.5;border-radius:50%;\
         background:conic-gradient(red,blue);filter:saturate(3)'>{text}</div>\
         <div style='position:absolute;top:560px;left:10px;width:1200px;columns:3'>{}</div>",
        text.repeat(30)
    )
}

/// The bytes of a PNG of `width` x `height` RGBA pixels with a gradient
/// (stored deflate blocks).
fn gradient_png(width: u32, height: u32) -> Vec<u8> {
    let mut raw = Vec::new();
    for y in 0..height {
        raw.push(0);
        for x in 0..width {
            let a = if (x + y) % 5 == 0 { 120 } else { 255 };
            raw.extend([(x * 11) as u8, (y * 15) as u8, ((x + y) * 7) as u8, a]);
        }
    }
    let mut z = vec![0x78, 0x01];
    for (i, chunk) in raw.chunks(65_535).enumerate() {
        let last = (i + 1) * 65_535 >= raw.len();
        z.push(u8::from(last));
        let len = chunk.len() as u16;
        z.extend(len.to_le_bytes());
        z.extend((!len).to_le_bytes());
        z.extend(chunk);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &v in &raw {
        a = (a + u32::from(v)) % 65_521;
        b = (b + a) % 65_521;
    }
    z.extend(((b << 16) | a).to_be_bytes());
    let mut png = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    let mut ihdr = Vec::new();
    ihdr.extend(width.to_be_bytes());
    ihdr.extend(height.to_be_bytes());
    ihdr.extend([8, 6, 0, 0, 0]);
    for (kind, data) in [(*b"IHDR", ihdr), (*b"IDAT", z), (*b"IEND", Vec::new())] {
        png.extend((data.len() as u32).to_be_bytes());
        let mut body = kind.to_vec();
        body.extend(&data);
        png.extend(&body);
        png.extend(crc32(&body).to_be_bytes());
    }
    png
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

fn base64(data: &[u8]) -> String {
    const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in data.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, &b)| n | u32::from(b) << (16 - 8 * i));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(ABC[(n >> (18 - 6 * i) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[test]
fn region_raster_equals_full_raster_on_a_page_of_effects() {
    let rounds = std::env::var("REGION_ROUNDS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(40);
    let runs = on_page_threads([1.0f32, 1.5, 2.0].map(|scale| {
        move || {
            let mut page = common::new_page_at_scale(
                std::sync::Arc::new(common::OnePage::with_data_urls(features_page())),
                2,
                VIEW,
                scale,
            );
            page.navigate(swb_engine::Url::parse(common::CASE_PAGE).unwrap());
            assert!(page.wait_until_loaded(std::time::Duration::from_secs(20)));
            let mut rng = Rng::new(7 + scale as u64);
            let mut parts = 0;
            for offset in [0.0, 150.0] {
                page.scroll_to(Point::new(0.0, offset));
                let label = format!("effects (scale {scale}, scroll {offset})");
                parts += check_parts(&mut page, &label, &mut rng, rounds);
            }
            parts
        }
    }));
    eprintln!("parts compared: {}", runs.iter().sum::<usize>());
}

const PAGE_FIXTURES: [&str; 9] = [
    "ars-technica",
    "bbc",
    "craigslist-posting",
    "github-swb",
    "hacker-news",
    "osm-zagreb",
    "senko-net",
    "videojs",
    "wikipedia-web-browser",
];

#[test]
fn region_raster_equals_full_raster_on_page_fixtures() {
    let runs = on_page_threads(
        PAGE_FIXTURES
            .iter()
            .enumerate()
            .map(|(i, name)| move || run_fixture(name, false, i as u64 + 1)),
    );
    assert!(
        runs.iter().all(Option::is_some),
        "a page fixture is missing"
    );
    let parts: usize = runs.iter().flatten().sum();
    eprintln!("parts compared: {parts}");
}

#[test]
fn region_raster_equals_full_raster_on_js_fixtures() {
    let runs = on_page_threads(
        PAGE_FIXTURES
            .iter()
            .enumerate()
            .map(|(i, name)| move || run_fixture(name, true, i as u64 + 101)),
    );
    let parts: usize = runs.iter().flatten().sum();
    eprintln!("parts compared: {parts} (0: no JS fixture)");
}
