//! The specified value of `clip-path` as `element.style` and style rules
//! write it (measured in Chromium 148, `tools/probes/clip-path.json`):
//! function names and keywords in lower case, lengths as in other
//! properties (`0` is `0px`, `calc()` simplified), the sides of `inset()`
//! and the corner radii as short as the box shorthands, a default radius
//! (`closest-side`) and the reference box `border-box` after a shape left
//! out, a position of keywords as lengths, and `path()` data with one
//! space between tokens and `Z` for a close. `rect()` and `xywh()` stay as
//! written. The tokens passed the property's parser.

use swb_css::{ComponentValue, serialize_string};

use super::value::{Style, serialize};
use crate::properties::path_data;
use crate::values::GeometryBox;

/// Serializes the tokens of a `clip-path` value.
pub(super) fn serialize_clip_path(tokens: &[ComponentValue]) -> String {
    let mut shape = None;
    let mut geometry_box = None;
    for token in tokens {
        match token {
            ComponentValue::Whitespace => {}
            ComponentValue::Ident(i) => match GeometryBox::from_ident(i) {
                Some(b) => geometry_box = Some(b),
                None => return serialize(tokens, Style::PLAIN),
            },
            ComponentValue::Function(f) if !f.name.eq_ignore_ascii_case("url") => {
                shape = Some(function(&f.name.to_ascii_lowercase(), &f.arguments));
            }
            _ => return serialize(tokens, Style::PLAIN),
        }
    }
    match (shape, geometry_box) {
        (Some(shape), Some(b)) if b != GeometryBox::BorderBox => format!("{shape} {}", b.as_str()),
        (Some(shape), _) => shape,
        (None, Some(b)) => b.as_str().to_owned(),
        (None, None) => String::new(),
    }
}

fn function(name: &str, args: &[ComponentValue]) -> String {
    let body = match name {
        "inset" => inset(args),
        "rect" | "xywh" => rect_like(args),
        "circle" | "ellipse" => radii_and_position(args),
        "polygon" => polygon(args),
        "path" => path(args),
        _ => serialize(args, Style::LENGTH),
    };
    format!("{name}({body})")
}

/// The top-level space-separated items of `tokens`, each serialized as a
/// length (a keyword stays a keyword in lower case).
fn words(tokens: &[ComponentValue]) -> Vec<String> {
    tokens
        .split(|t| matches!(t, ComponentValue::Whitespace))
        .filter(|w| !w.is_empty())
        .map(|w| serialize(w, Style::LENGTH))
        .collect()
}

/// Splits at the first token for which `is_separator` holds.
fn split_at_token(
    tokens: &[ComponentValue],
    is_separator: impl Fn(&ComponentValue) -> bool,
) -> (&[ComponentValue], Option<&[ComponentValue]>) {
    match tokens.iter().position(is_separator) {
        Some(i) => (&tokens[..i], tokens.get(i + 1..)),
        None => (tokens, None),
    }
}

/// `<sides> [round <radii>]?` of `inset()`.
fn inset(args: &[ComponentValue]) -> String {
    let (sides, radii) = split_at_token(args, |t| t.is_ident("round"));
    let sides = short_list(&expand(&words(sides)));
    with_radii(sides.join(" "), radii)
}

/// `rect()` and `xywh()`: the four values as written, then the radii.
fn rect_like(args: &[ComponentValue]) -> String {
    let (values, radii) = split_at_token(args, |t| t.is_ident("round"));
    with_radii(words(values).join(" "), radii)
}

fn with_radii(mut text: String, radii: Option<&[ComponentValue]>) -> String {
    if let Some(radii) = radii.and_then(radii_text) {
        text.push_str(" round ");
        text.push_str(&radii);
    }
    text
}

/// Four values from one to four, as the box shorthands expand them.
fn expand(w: &[String]) -> [String; 4] {
    let get = |i: usize| w.get(i).cloned().unwrap_or_default();
    match w.len() {
        1 => [get(0), get(0), get(0), get(0)],
        2 => [get(0), get(1), get(0), get(1)],
        3 => [get(0), get(1), get(2), get(1)],
        _ => [get(0), get(1), get(2), get(3)],
    }
}

/// The values as short as the box shorthands write them (also for the
/// computed value, `resolved/shape.rs`).
#[allow(clippy::many_single_char_names)]
pub(super) fn short_list(v: &[String; 4]) -> Vec<String> {
    let [a, b, c, d] = v;
    if b != d {
        vec![a.clone(), b.clone(), c.clone(), d.clone()]
    } else if a != c {
        vec![a.clone(), b.clone(), c.clone()]
    } else if a != b {
        vec![a.clone(), b.clone()]
    } else {
        vec![a.clone()]
    }
}

/// `<radii> [/ <radii>]?`: each list short; the vertical one is left out
/// if it is the horizontal one. `None` if all radii are zero (Chromium
/// leaves out `round 0`).
fn radii_text(tokens: &[ComponentValue]) -> Option<String> {
    let (h, v) = split_at_token(tokens, |t| t.is_delim('/'));
    let h = expand(&words(h));
    let v = v.map_or_else(|| h.clone(), |v| expand(&words(v)));
    if h.iter().chain(&v).all(|r| r == "0px") {
        return None;
    }
    let (horizontal, vertical) = (short_list(&h).join(" "), short_list(&v).join(" "));
    Some(if horizontal == vertical {
        horizontal
    } else {
        format!("{horizontal} / {vertical}")
    })
}

/// `circle()` and `ellipse()`: the radii (left out if all are
/// `closest-side`), then `at <position>`.
fn radii_and_position(args: &[ComponentValue]) -> String {
    let (radii, position) = split_at_token(args, |t| t.is_ident("at"));
    let mut parts = words(radii);
    if parts.iter().all(|r| r == "closest-side") {
        parts.clear();
    }
    if let Some(position) = position {
        parts.push(format!("at {}", position_text(&words(position))));
    }
    parts.join(" ")
}

/// A position of one, two or four values as Chromium writes it: keywords
/// as percentages, the edges `left` and `top` with their offsets as the
/// offsets alone, and the other keyword forms with both keywords.
#[allow(clippy::many_single_char_names)]
fn position_text(words: &[String]) -> String {
    let w: Vec<String> = words.iter().map(|s| s.to_ascii_lowercase()).collect();
    let horizontal = |s: &str| matches!(s, "left" | "right");
    let vertical = |s: &str| matches!(s, "top" | "bottom");
    let keyword = |s: &str| matches!(s, "left" | "right" | "top" | "bottom" | "center");
    let percent = |s: &str| match s {
        "left" | "top" => "0%".to_owned(),
        "right" | "bottom" => "100%".to_owned(),
        "center" => "50%".to_owned(),
        other => other.to_owned(),
    };
    // A zero offset is a percentage (`0` is `0%`).
    let zero = |s: &str| matches!(s, "0px" | "calc(0px)");
    let value = |s: &str| if zero(s) { "0%".to_owned() } else { percent(s) };
    match w.as_slice() {
        [one] if vertical(one) => format!("50% {}", value(one)),
        [one] => format!("{} 50%", value(one)),
        [a, b] => {
            let swap = keyword(a) && keyword(b) && (vertical(a) || horizontal(b));
            let (x, y) = if swap { (b, a) } else { (a, b) };
            format!("{} {}", value(x), value(y))
        }
        [a, b, c, d] => {
            // Two keyword and offset pairs, in either order. A zero offset
            // is a percentage from the start edge: `right 0` is `100%`.
            let (x, y) = if vertical(a) {
                ((c, d), (a, b))
            } else {
                ((a, b), (c, d))
            };
            let start = |edge: &String, offset: &String, near: &str, far: &str| {
                if !zero(offset) {
                    (edge.clone(), offset.clone())
                } else if edge == near {
                    (near.to_owned(), "0%".to_owned())
                } else {
                    (
                        near.to_owned(),
                        if edge == far { "100%" } else { "0%" }.to_owned(),
                    )
                }
            };
            let (x, y) = (
                start(x.0, x.1, "left", "right"),
                start(y.0, y.1, "top", "bottom"),
            );
            if x.0 == "left" && y.0 == "top" {
                format!("{} {}", x.1, y.1)
            } else {
                format!("{} {} {} {}", x.0, x.1, y.0, y.1)
            }
        }
        other => other.join(" "),
    }
}

/// The optional fill rule and the rest of `polygon()` and `path()`:
/// `nonzero` is left out.
fn rule_and_rest(args: &[ComponentValue]) -> (&'static str, &[ComponentValue]) {
    let (first, rest) = split_at_token(args, ComponentValue::is_comma);
    let rule = match first {
        [ComponentValue::Ident(i)] if i.eq_ignore_ascii_case("evenodd") => "evenodd, ",
        [ComponentValue::Ident(i)] if i.eq_ignore_ascii_case("nonzero") => "",
        _ => return ("", args),
    };
    (rule, rest.unwrap_or_default())
}

fn polygon(args: &[ComponentValue]) -> String {
    let (rule, rest) = rule_and_rest(args);
    let points: Vec<String> = rest
        .split(ComponentValue::is_comma)
        .map(|point| words(point).join(" "))
        .collect();
    format!("{rule}{}", points.join(", "))
}

fn path(args: &[ComponentValue]) -> String {
    let (rule, rest) = rule_and_rest(args);
    let data = rest.iter().find_map(|t| match t {
        ComponentValue::String(s) => Some(s),
        _ => None,
    });
    let text = data
        .and_then(|d| path_data::normalize(d))
        .map(|n| n.specified)
        .unwrap_or_default();
    let mut quoted = String::new();
    serialize_string(&text, &mut quoted);
    format!("{rule}{quoted}")
}
