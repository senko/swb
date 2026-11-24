//! The values and pixels of the probe pages of CSS animations and
//! transitions (ADR 0044 section 12): the tolerances of numbers and colors,
//! the pages of the probe cases, and the colors of their screenshots
//! (`css_animations.rs`, `css_transitions.rs`).

use std::sync::Arc;

use serde_json::Value;
use swb_engine::{Page, Size, Url};

use super::{DataUrls, OnePage, SKIP_QUERY as SKIP, open_script_page};

/// A line of an expected value that any line matches.
pub(crate) const ANY_LINE: &str = "\u{1}any";

/// `text` with each `oklab(L a b)` and `oklab(L a b / A)` written as the
/// 8-bit sRGB `rgb()` or `rgba()` that swb computes (ADR 0044 section 4:
/// swb stores the result of an interpolation between non-legacy colors as
/// 8-bit sRGB; tests compare such values with a tolerance of 1 per
/// channel). The conversion is the one of CSS Color 4 §9.2
/// (<https://www.w3.org/TR/css-color-4/#color-conversion-code>).
#[allow(clippy::many_single_char_names)] // The names of the formulas.
pub(crate) fn oklab_as_rgb(text: &str) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    let mut rest = text;
    while let Some(at) = rest.find("oklab(") {
        out.push_str(&rest[..at]);
        let after = &rest[at + "oklab(".len()..];
        let Some(end) = after.find(')') else {
            out.push_str(&rest[at..]);
            return out;
        };
        let inner = &after[..end];
        let (lab, alpha) = match inner.split_once('/') {
            Some((lab, alpha)) => (lab, alpha.trim().parse::<f64>().ok()),
            None => (inner, None),
        };
        let v: Vec<f64> = lab
            .split_whitespace()
            .filter_map(|n| n.parse().ok())
            .collect();
        let [l, a, b] = v[..] else {
            out.push_str(&rest[at..at + "oklab(".len()]);
            rest = after;
            continue;
        };
        let l_ = (l + 0.396_337_777_4 * a + 0.215_803_757_3 * b).powi(3);
        let m_ = (l - 0.105_561_345_8 * a - 0.063_854_172_8 * b).powi(3);
        let s_ = (l - 0.089_484_177_5 * a - 1.291_485_548 * b).powi(3);
        let linear = [
            4.076_741_662_1 * l_ - 3.307_711_591_3 * m_ + 0.230_969_929_2 * s_,
            -1.268_438_004_6 * l_ + 2.609_757_401_1 * m_ - 0.341_319_396_5 * s_,
            -0.004_196_086_3 * l_ - 0.703_418_614_7 * m_ + 1.707_614_701 * s_,
        ];
        let channel = |c: f64| {
            let c = if c <= 0.003_130_8 {
                12.92 * c
            } else {
                1.055 * c.powf(1.0 / 2.4) - 0.055
            };
            (c.clamp(0.0, 1.0) * 255.0).round()
        };
        let [r, g, b] = linear.map(channel);
        match alpha {
            // An alpha that rounds to 255 is opaque in 8 bits: `rgb()`.
            Some(alpha) if alpha_as_8_bits(alpha) < 255 => {
                write!(out, "rgba({r}, {g}, {b}, {alpha})").unwrap();
            }
            _ => write!(out, "rgb({r}, {g}, {b})").unwrap(),
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    out
}

/// An alpha in 0 to 1 as the 8-bit value that swb stores for it.
fn alpha_as_8_bits(alpha: f64) -> u8 {
    (alpha.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// Splits `text` into the parts between numbers and the numbers.
pub(crate) fn tokens(text: &str) -> (Vec<String>, Vec<f64>) {
    let mut words = vec![String::new()];
    let mut numbers = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        let starts_number = c.is_ascii_digit()
            || (c == b'-' || c == b'.')
                && bytes
                    .get(i + 1)
                    .is_some_and(|n| n.is_ascii_digit() || *n == b'.');
        let after_word = i > 0 && (bytes[i - 1].is_ascii_alphabetic() || bytes[i - 1] == b'_');
        if starts_number && !after_word {
            let mut j = i + 1;
            while j < bytes.len()
                && (bytes[j].is_ascii_digit()
                    || bytes[j] == b'.'
                    || ((bytes[j] == b'e' || bytes[j] == b'E')
                        && bytes
                            .get(j + 1)
                            .is_some_and(|n| n.is_ascii_digit() || *n == b'-' || *n == b'+'))
                    || ((bytes[j] == b'-' || bytes[j] == b'+')
                        && (bytes[j - 1] == b'e' || bytes[j - 1] == b'E')))
            {
                j += 1;
            }
            if let Ok(n) = text[i..j].parse::<f64>() {
                numbers.push(n);
                words.push(String::new());
                i = j;
                continue;
            }
        }
        words.last_mut().unwrap().push(c as char);
        i += 1;
    }
    (words, numbers)
}

/// `text` with each `rgba(r, g, b, 1)` written as `rgb(r, g, b)`.
fn opaque_rgba_as_rgb(text: &str) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    let mut rest = text;
    while let Some(at) = rest.find("rgba(") {
        out.push_str(&rest[..at]);
        let after = &rest[at + "rgba(".len()..];
        let Some(end) = after.find(')') else {
            out.push_str(&rest[at..]);
            return out;
        };
        let parts: Vec<&str> = after[..end].split(',').map(str::trim).collect();
        let rest_after = &after[end + 1..];
        match parts[..] {
            [r, g, b, "1"] => write!(out, "rgb({r}, {g}, {b})").unwrap(),
            _ => out.push_str(&rest[at..rest.len() - rest_after.len()]),
        }
        rest = rest_after;
    }
    out.push_str(rest);
    out
}

/// True if `a` and `b` are equal with the tolerances of ADR 0044 section
/// 12: the text without numbers equal; numbers within a relative 1e-5 or
/// an absolute 1e-4 (Chromium prints 6 significant digits); the channels
/// of `rgb()` and `rgba()` within 1 and their alpha within one 8-bit step;
/// `rgba(r, g, b, 1)` is `rgb(r, g, b)`
/// (a color in the middle of an interpolation whose alpha rounds to 1:
/// Chromium keeps the `rgba()` form, swb's 8-bit color has no alpha left).
pub(crate) fn close(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    let (a, b) = (opaque_rgba_as_rgb(a), opaque_rgba_as_rgb(b));
    let (wa, na) = tokens(&a);
    let (wb, nb) = tokens(&b);
    if wa != wb || na.len() != nb.len() {
        return false;
    }
    let mut in_color = false;
    let mut channel = 0;
    for (i, (x, y)) in na.iter().zip(&nb).enumerate() {
        let before = &wa[i];
        if before.ends_with("rgb(") || before.ends_with("rgba(") {
            in_color = true;
            channel = 0;
        } else if in_color && !before.starts_with(',') {
            in_color = false;
        }
        let tolerance = if in_color && channel < 3 {
            1.0
        } else if in_color && channel == 3 {
            // An 8-bit alpha within one step, as the channels: swb writes
            // the shortest decimal of its 8-bit value (71 is `0.28`),
            // Chromium the float (0.277837).
            1.0 / 255.0 + 1e-4
        } else {
            (1e-5 * x.abs().max(y.abs())).max(1e-4)
        };
        channel += 1;
        if (x - y).abs() > tolerance {
            return false;
        }
    }
    true
}

pub(crate) fn same(want: &Value, got: &Value) -> bool {
    if want.as_str() == Some(SKIP) || want == got {
        return true;
    }
    match (want.as_str(), got.as_str()) {
        (Some(a), Some(b)) if a.contains(ANY_LINE) => {
            let wa: Vec<&str> = a.split('\n').collect();
            let wb: Vec<&str> = b.split('\n').collect();
            wa.len() == wb.len()
                && wa
                    .iter()
                    .zip(&wb)
                    .all(|(x, y)| *x == ANY_LINE || close(x, y))
        }
        (Some(a), Some(b)) => close(a, b),
        _ => false,
    }
}

pub(crate) fn open(case: &Value) -> Page {
    let url = "file:///case.html";
    let fetcher = DataUrls(OnePage {
        url,
        html: case["html"].as_str().unwrap().to_owned(),
    });
    let default = serde_json::json!([400, 300]);
    let viewport = case.get("viewport").unwrap_or(&default).as_array().unwrap();
    open_script_page(
        Arc::new(fetcher),
        Url::parse(url).unwrap(),
        Size::new(
            viewport[0].as_f64().unwrap() as f32,
            viewport[1].as_f64().unwrap() as f32,
        ),
    )
}

/// The case `name` of the probe file `tools/probes/{file}.json`.
pub(crate) fn probe_case(file: &str, name: &str) -> Value {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let text = std::fs::read_to_string(root.join(format!("tools/probes/{file}.json"))).unwrap();
    let probe: Value = serde_json::from_str(&text).unwrap();
    probe["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == name)
        .unwrap()
        .clone()
}

/// The colors of the screenshot of `page` at `points` relative to the
/// top-left corner of the first element of `selector`.
pub(crate) fn colors(page: &mut Page, selector: &str, points: &[(u32, u32)]) -> Vec<[u8; 3]> {
    let origin = super::value(
        page,
        &format!(
            "(() => {{ const r = document.querySelector('{selector}').getBoundingClientRect(); \
             return [Math.round(r.x), Math.round(r.y)]; }})()"
        ),
    );
    let (x0, y0) = (
        origin[0].as_u64().unwrap() as u32,
        origin[1].as_u64().unwrap() as u32,
    );
    let shot = page.screenshot(false).unwrap();
    points
        .iter()
        .map(|&(x, y)| {
            let p = shot.pixel(x0 + x, y0 + y).unwrap().demultiply();
            [p.red(), p.green(), p.blue()]
        })
        .collect()
}

pub(crate) fn near(got: &[[u8; 3]], want: &[[u8; 3]], tolerance: u8) -> bool {
    got.len() == want.len()
        && got
            .iter()
            .zip(want)
            .all(|(g, w)| g.iter().zip(w).all(|(a, b)| a.abs_diff(*b) <= tolerance))
}

/// The page of `case` with a virtual clock and painted frames, so that its
/// animations run (and their transform groups are composited, ADR 0044
/// section 19) instead of being paused by `seek()`.
pub(crate) fn open_running(case: &Value) -> Page {
    let url = "file:///case.html";
    let fetcher = DataUrls(OnePage {
        url,
        html: case["html"].as_str().unwrap().to_owned(),
    });
    let config = swb_engine::PageConfig {
        fetcher: Arc::new(fetcher),
        notify: Arc::new(|| {}),
        network_threads: 2,
        scripts_allowed: true,
    };
    let mut page = Page::new(
        config,
        swb_engine::FontContext::for_tests(),
        Size::new(400.0, 300.0),
        1.0,
    );
    page.set_javascript(true);
    page.set_animations(true);
    page.set_paints_frames(true);
    page.set_clock(swb_engine::PageClock::new(
        swb_engine::ClockMode::Virtual,
        None,
    ));
    page.navigate(Url::parse(url).unwrap());
    assert!(page.wait_until_loaded(std::time::Duration::from_secs(30)));
    page
}
