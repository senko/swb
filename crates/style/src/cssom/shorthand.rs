//! Shorthands in declaration blocks: the values that a shorthand gives
//! its longhands ([`expand`]) and the shorthand's value put together from
//! its longhands ([`value_of`]), as Chromium does it (measured,
//! `tools/probes/web-cssom-style.json`).
//!
//! A value has already passed swb's parser of the shorthand. The parts of
//! the value go to the longhands by position (sides, pairs, grid lines) or
//! by trying each part with the longhands' parsers (`outline: 1px solid`,
//! `border`, `background`); a longhand that the value leaves out gets
//! Chromium's implicit value. When the parts cannot be told apart (a case
//! that swb's parser accepts and this module does not know), the
//! shorthand is kept as written, as for `var()`.

use std::sync::Arc;

use swb_css::ComponentValue;

use super::Value;
use super::longhand::serialize_longhand;
use super::props::{Kind, Ser, Shorthand, TABLE};
use super::value::{Style, serialize, trimmed};
use crate::parse::ParserContext;
use crate::properties::parse_declaration;

/// The values that the shorthand `def` (index `index`) with the value
/// `tokens` gives its longhands, in the order of `def.longhands`.
pub(crate) fn expand(
    index: usize,
    def: &Shorthand,
    tokens: &[ComponentValue],
    quirks: bool,
) -> Vec<Value> {
    let cx = Parts {
        def,
        cx: ParserContext::author(None, quirks),
    };
    let values = match def.kind {
        Kind::Sides => cx.sides(tokens),
        Kind::Pair => cx.pair(tokens),
        Kind::Radius => Parts::radius(tokens),
        Kind::BorderSide => cx.border(tokens),
        Kind::AnyOrder => cx.any_order(tokens),
        Kind::Flex => Parts::flex(tokens),
        Kind::Font => cx.font(tokens),
        Kind::Background => cx.background(tokens),
        Kind::Position => Parts::position(tokens),
        Kind::GridLines => cx.grid_lines(tokens),
        Kind::Single => Some(vec![cx.text(0, tokens)]),
        Kind::BorderImage | Kind::Opaque => None,
    };
    values
        .filter(|v| v.len() == def.longhands.len())
        .unwrap_or_else(|| {
            let text: Arc<str> = Arc::from(serialize(trimmed(tokens), Style::MIXED));
            vec![Value::Pending(index, text); def.longhands.len()]
        })
}

/// The top-level parts of a value: runs of tokens between whitespace,
/// with `/` and `,` as parts of their own.
fn parts(tokens: &[ComponentValue]) -> Vec<&[ComponentValue]> {
    let mut out = Vec::new();
    let mut start = None;
    for (i, t) in tokens.iter().enumerate() {
        let separator = matches!(
            t,
            ComponentValue::Whitespace | ComponentValue::Delim('/') | ComponentValue::Comma
        );
        if separator {
            if let Some(s) = start.take() {
                out.extend(tokens.get(s..i));
            }
            if !matches!(t, ComponentValue::Whitespace) {
                out.extend(tokens.get(i..=i));
            }
        } else if start.is_none() {
            start = Some(i);
        }
    }
    if let Some(s) = start {
        out.extend(tokens.get(s..));
    }
    out
}

fn is_slash(part: &[ComponentValue]) -> bool {
    matches!(part, [ComponentValue::Delim('/')])
}

fn is_comma(part: &[ComponentValue]) -> bool {
    matches!(part, [ComponentValue::Comma])
}

/// The keyword of a part, lower case.
fn keyword(part: &[ComponentValue]) -> Option<String> {
    match part {
        [ComponentValue::Ident(s)] => Some(s.to_ascii_lowercase()),
        _ => None,
    }
}

/// Joins parts with single spaces into one token list.
fn join(parts: &[&[ComponentValue]]) -> Vec<ComponentValue> {
    let mut out = Vec::new();
    for (i, p) in parts.iter().enumerate() {
        if i > 0 {
            out.push(ComponentValue::Whitespace);
        }
        out.extend(p.iter().cloned());
    }
    out
}

struct Parts<'a> {
    def: &'a Shorthand,
    cx: ParserContext,
}

impl Parts<'_> {
    /// The serialization class of longhand `i` of the shorthand.
    fn ser(&self, i: usize) -> Ser {
        self.def
            .longhands
            .get(i)
            .and_then(|l| TABLE.longhand(l))
            .and_then(|j| TABLE.longhands.get(j))
            .map_or(Ser::Plain, |l| l.ser)
    }

    fn text(&self, i: usize, tokens: &[ComponentValue]) -> Value {
        Value::Text(serialize_longhand(self.ser(i), tokens))
    }

    /// True if swb's parser of `property` accepts `tokens`.
    fn accepts(&self, property: &str, tokens: &[ComponentValue]) -> bool {
        parse_declaration(property, tokens, &self.cx, &mut Vec::new())
    }

    /// One to four parts for top, right, bottom and left.
    fn sides(&self, tokens: &[ComponentValue]) -> Option<Vec<Value>> {
        let p = parts(tokens);
        let pick: [usize; 4] = match p.len() {
            1 => [0, 0, 0, 0],
            2 => [0, 1, 0, 1],
            3 => [0, 1, 2, 1],
            4 => [0, 1, 2, 3],
            _ => return None,
        };
        pick.iter()
            .enumerate()
            .map(|(i, &j)| Some(self.text(i, p.get(j)?)))
            .collect()
    }

    /// One or two values. `contain-intrinsic-size` keeps `auto` with the
    /// length after it.
    fn pair(&self, tokens: &[ComponentValue]) -> Option<Vec<Value>> {
        let mut p: Vec<Vec<ComponentValue>> = Vec::new();
        let mut auto = false;
        for part in parts(tokens) {
            if auto && let Some(last) = p.last_mut() {
                last.push(ComponentValue::Whitespace);
                last.extend(part.iter().cloned());
                auto = false;
                continue;
            }
            auto = self.def.name == "contain-intrinsic-size"
                && keyword(part).as_deref() == Some("auto");
            p.push(part.to_vec());
        }
        let first = p.first()?;
        let second = p.get(1).unwrap_or(first);
        if p.len() > 2 {
            return None;
        }
        Some(vec![self.text(0, first), self.text(1, second)])
    }

    /// `border-radius`: horizontal radii, then after `/` vertical radii,
    /// one to four each.
    fn radius(tokens: &[ComponentValue]) -> Option<Vec<Value>> {
        let p = parts(tokens);
        let slash = p.iter().position(|x| is_slash(x));
        let (h, v) = match slash {
            Some(i) => (p.get(..i)?, p.get(i + 1..)?),
            None => (p.as_slice(), p.as_slice()),
        };
        let corners = |list: &[&[ComponentValue]]| -> Option<[String; 4]> {
            let words: Vec<String> = list.iter().map(|x| serialize(x, Style::LENGTH)).collect();
            let pick: [usize; 4] = match words.len() {
                1 => [0, 0, 0, 0],
                2 => [0, 1, 0, 1],
                3 => [0, 1, 2, 1],
                4 => [0, 1, 2, 3],
                _ => return None,
            };
            Some(pick.map(|i| words.get(i).cloned().unwrap_or_default()))
        };
        let (h, v) = (corners(h)?, corners(v)?);
        Some(
            h.iter()
                .zip(v.iter())
                .map(|(a, b)| {
                    Value::Text(if a == b {
                        a.clone()
                    } else {
                        format!("{a} {b}")
                    })
                })
                .collect(),
        )
    }

    /// `border` and the shorthands of sides: width, style and color in
    /// any order; each side gets them; `border` resets `border-image`.
    fn border(&self, tokens: &[ComponentValue]) -> Option<Vec<Value>> {
        let mut width = None;
        let mut style = None;
        let mut color = None;
        for part in parts(tokens) {
            if style.is_none() && self.accepts("border-top-style", part) {
                style = Some(serialize_longhand(Ser::Plain, part));
            } else if color.is_none() && self.accepts("border-top-color", part) {
                color = Some(serialize_longhand(Ser::Color, part));
            } else if width.is_none() && self.accepts("border-top-width", part) {
                width = Some(serialize_longhand(Ser::Length, part));
            } else {
                return None;
            }
        }
        let value = |found: &Option<String>, implicit: &'static str| {
            found.clone().map_or(Value::Implicit(implicit), Value::Text)
        };
        Some(
            self.def
                .longhands
                .iter()
                .map(|l| {
                    if l.starts_with("border-image") {
                        Value::Implicit("initial")
                    } else if l.ends_with("-width") {
                        value(&width, "medium")
                    } else if l.ends_with("-style") {
                        value(&style, "none")
                    } else {
                        value(&color, "currentcolor")
                    }
                })
                .collect(),
        )
    }

    /// The implicit value of longhand `i` of an any-order shorthand.
    fn implicit(&self, i: usize) -> &'static str {
        match (self.def.name, i) {
            ("flex-flow", 0) => "row",
            ("flex-flow", _) => "nowrap",
            ("columns", _) => "auto",
            _ => "initial",
        }
    }

    /// Parts in any order, each to the first longhand that takes it; a
    /// part that no free longhand takes extends the longhand of the part
    /// before it (`underline overline`).
    fn any_order(&self, tokens: &[ComponentValue]) -> Option<Vec<Value>> {
        let n = self.def.longhands.len();
        // `auto` is an `outline-color` and an `outline-style`; Chromium
        // gives it to the style, so the style is tried first.
        let order: Vec<usize> = match self.def.name {
            "outline" => vec![1, 0, 2],
            _ => (0..n).collect(),
        };
        let mut found: Vec<Option<Vec<&[ComponentValue]>>> = vec![None; n];
        let mut last: Option<usize> = None;
        for part in parts(tokens) {
            let free = order.iter().copied().find(|&i| {
                found.get(i).is_some_and(Option::is_none)
                    && self
                        .def
                        .longhands
                        .get(i)
                        .is_some_and(|l| self.accepts(l, part))
            });
            if let Some(i) = free {
                if let Some(slot) = found.get_mut(i) {
                    *slot = Some(vec![part]);
                }
                last = Some(i);
            } else {
                let i = last?;
                let longer = found.get_mut(i)?.as_mut()?;
                longer.push(part);
                let name = self.def.longhands.get(i)?;
                if !self.accepts(name, &join(longer)) {
                    return None;
                }
            }
        }
        // `list-style: none` sets the type when the image is not given
        // otherwise (Chromium).
        if self.def.name == "list-style"
            && let (Some(Some(image)), Some(None)) = (found.get(1), found.get(2))
            && image.len() == 1
            && image.first().and_then(|p| keyword(p)).as_deref() == Some("none")
        {
            found.swap(1, 2);
        }
        Some(
            found
                .iter()
                .enumerate()
                .map(|(i, f)| match f {
                    Some(p) => self.text(i, &join(p)),
                    None => Value::Implicit(self.implicit(i)),
                })
                .collect(),
        )
    }

    /// `flex`: `none`, `auto`, or grow, shrink and basis (measured: one
    /// number sets the basis to `0%`; a basis alone sets the factors to 1).
    fn flex(tokens: &[ComponentValue]) -> Option<Vec<Value>> {
        let p = parts(tokens);
        let text = |s: &str| Value::Text(s.to_owned());
        match p.as_slice() {
            [one] if keyword(one).as_deref() == Some("none") => {
                return Some(vec![text("0"), text("0"), text("auto")]);
            }
            [one] if keyword(one).as_deref() == Some("auto") => {
                return Some(vec![text("1"), text("1"), text("auto")]);
            }
            _ => {}
        }
        // `<grow> <shrink>? || <basis>`: a number is a factor unless both
        // factors are there (`1 1 0`: the basis `0`).
        let mut factors = Vec::new();
        let mut basis = None;
        for part in p {
            let number = matches!(part, [ComponentValue::Number(_)]);
            if number && factors.len() < 2 {
                factors.push(serialize(part, Style::PLAIN));
            } else if basis.is_none() {
                basis = Some(serialize_longhand(Ser::Length, part));
            } else {
                return None;
            }
        }
        let grow = factors.first().cloned().unwrap_or_else(|| "1".to_owned());
        let shrink = factors.get(1).cloned().unwrap_or_else(|| "1".to_owned());
        let basis = basis.unwrap_or_else(|| "0%".to_owned());
        Some(vec![
            Value::Text(grow),
            Value::Text(shrink),
            Value::Text(basis),
        ])
    }

    /// `font`: style, variant, weight and stretch in any order, the size,
    /// `/` and the line height, then the families. A system font is kept
    /// as written.
    fn font(&self, tokens: &[ComponentValue]) -> Option<Vec<Value>> {
        let p = parts(tokens);
        // Longhand indices: 0 style, 1 variant-caps, 2 feature settings,
        // 3 variation settings, 4 weight, 5 stretch, 6 size, 7 line
        // height, 8 family.
        let mut found: [Option<String>; 9] = Default::default();
        let mut i = 0;
        let mut previous: Option<usize> = None;
        while let Some(part) = p.get(i) {
            let slot = [
                (0, "font-style"),
                (1, "font-variant-caps"),
                (4, "font-weight"),
                (5, "font-stretch"),
            ]
            .into_iter()
            .find(|(k, name)| {
                found.get(*k).is_some_and(Option::is_none) && self.accepts(name, part)
            });
            if let Some((k, _)) = slot {
                *found.get_mut(k)? = Some(serialize(part, Style::PLAIN));
                previous = Some(k);
                i += 1;
                continue;
            }
            // `oblique 10deg`: the angle belongs to the style.
            if previous == Some(0)
                && let Some(style) = found.first_mut().and_then(Option::as_mut)
                && style == "oblique"
                && self.accepts(
                    "font-style",
                    &join(&[&[ComponentValue::Ident("oblique".into())], part]),
                )
            {
                style.push(' ');
                style.push_str(&serialize(part, Style::PLAIN));
                i += 1;
                previous = None;
                continue;
            }
            break;
        }
        let size = p.get(i)?;
        if !self.accepts("font-size", size) {
            return None;
        }
        *found.get_mut(6)? = Some(serialize_longhand(Ser::Length, size));
        i += 1;
        if p.get(i).is_some_and(|x| is_slash(x)) {
            let height = p.get(i + 1)?;
            *found.get_mut(7)? = Some(serialize(height, Style::PLAIN));
            i += 2;
        }
        let family = join(p.get(i..)?);
        if family.is_empty() {
            return None;
        }
        *found.get_mut(8)? = Some(serialize_longhand(Ser::FontFamily, &family));
        Some(
            found
                .into_iter()
                .map(|f| f.map_or(Value::Implicit("normal"), Value::Text))
                .collect(),
        )
    }

    /// `background` of one layer: image, position with `/` size, repeat,
    /// attachment, the boxes (origin, then clip) and the color, in any
    /// order. Several layers are kept as written.
    fn background(&self, tokens: &[ComponentValue]) -> Option<Vec<Value>> {
        let p = parts(tokens);
        if p.iter().any(|x| is_comma(x)) {
            return None;
        }
        // Longhand indices: 0 image, 1 position-x, 2 position-y, 3 size,
        // 4 repeat, 5 attachment, 6 origin, 7 clip, 8 color.
        let mut found: [Option<String>; 9] = Default::default();
        let mut i = 0;
        while let Some(part) = p.get(i) {
            let free = |k: usize| found.get(k).is_some_and(Option::is_none);
            if free(0) && self.accepts("background-image", part) {
                found[0] = Some(serialize(part, Style::PLAIN));
            } else if free(4) && self.accepts("background-repeat", part) {
                let mut end = i + 1;
                if p.get(end)
                    .is_some_and(|n| self.accepts("background-repeat", &join(&[part, n])))
                {
                    end += 1;
                }
                found[4] = Some(serialize_longhand(Ser::Repeat, &join(p.get(i..end)?)));
                i = end;
                continue;
            } else if free(5) && self.accepts("background-attachment", part) {
                found[5] = Some(serialize(part, Style::PLAIN));
            } else if free(6) && self.accepts("background-origin", part) {
                let text = serialize(part, Style::PLAIN);
                found[6] = Some(text);
            } else if free(7) && found[6].is_some() && self.accepts("background-clip", part) {
                found[7] = Some(serialize(part, Style::PLAIN));
            } else if free(1) && self.accepts("background-position", part) {
                // The longest run of parts that is a position.
                let mut end = i + 1;
                while end < p.len()
                    && end - i < 4
                    && self.accepts("background-position", &join(p.get(i..=end)?))
                {
                    end += 1;
                }
                let (x, y) = position_axes(p.get(i..end)?)?;
                found[1] = Some(x);
                found[2] = Some(y);
                i = end;
                if p.get(i).is_some_and(|x| is_slash(x)) {
                    let mut size_end = i + 2;
                    if p.get(size_end).is_some_and(|n| {
                        p.get(i + 1)
                            .is_some_and(|s| self.accepts("background-size", &join(&[s, n])))
                    }) {
                        size_end += 1;
                    }
                    let size = join(p.get(i + 1..size_end)?);
                    found[3] = Some(serialize_longhand(Ser::BackgroundSize, &size));
                    i = size_end;
                }
                continue;
            } else if free(8) && self.accepts("background-color", part) {
                found[8] = Some(serialize_longhand(Ser::Color, part));
            } else {
                return None;
            }
            i += 1;
        }
        // One box sets both the origin and the clip.
        if found[7].is_none() {
            let origin = found[6].clone();
            found[7] = origin;
        }
        Some(
            found
                .into_iter()
                .map(|f| f.map_or(Value::Implicit("initial"), Value::Text))
                .collect(),
        )
    }

    /// `background-position` and `mask-position`: x and y of each layer.
    fn position(tokens: &[ComponentValue]) -> Option<Vec<Value>> {
        let p = parts(tokens);
        let mut xs = Vec::new();
        let mut ys = Vec::new();
        for layer in p.split(|x| is_comma(x)) {
            let (x, y) = position_axes(layer)?;
            xs.push(x);
            ys.push(y);
        }
        Some(vec![Value::Text(xs.join(", ")), Value::Text(ys.join(", "))])
    }

    /// `grid-row`, `grid-column` and `grid-area`: lines between `/`; a
    /// missing line is the name before it, or `auto`.
    fn grid_lines(&self, tokens: &[ComponentValue]) -> Option<Vec<Value>> {
        let p = parts(tokens);
        let lines: Vec<String> = p
            .split(|x| is_slash(x))
            .map(|line| serialize_longhand(Ser::GridLine, &join(line)))
            .collect();
        let n = self.def.longhands.len();
        if lines.is_empty() || lines.len() > n {
            return None;
        }
        let fill = |from: Option<&String>| match from {
            Some(name) if is_custom_ident(name) => name.clone(),
            _ => "auto".to_owned(),
        };
        let mut out: Vec<String> = lines.clone();
        // grid-area: row-start, column-start, row-end, column-end; the
        // ends copy the starts, the column start the row start.
        let source = |k: usize| {
            if n == 4 {
                [0, 0, 0, 1].get(k).copied()
            } else {
                Some(0)
            }
        };
        for k in lines.len()..n {
            let from = source(k).and_then(|s| out.get(s)).cloned();
            out.push(fill(from.as_ref()));
        }
        Some(out.into_iter().map(Value::Text).collect())
    }
}

/// True if a grid line is a name alone.
fn is_custom_ident(line: &str) -> bool {
    !line.is_empty()
        && line != "auto"
        && !line.contains(' ')
        && !line.starts_with(|c: char| c.is_ascii_digit() || c == '-' && line.len() > 1)
}

/// The x and y parts of a `<bg-position>` of one layer, as Chromium writes
/// them (`center` fills a missing axis; keyword pairs in x, y order).
fn position_axes(parts: &[&[ComponentValue]]) -> Option<(String, String)> {
    let words: Vec<String> = parts.iter().map(|p| serialize(p, Style::LENGTH)).collect();
    let vertical = |s: &str| matches!(s, "top" | "bottom");
    let horizontal = |s: &str| matches!(s, "left" | "right");
    let is_keyword = |s: &str| vertical(s) || horizontal(s) || s == "center";
    match words.as_slice() {
        [one] if vertical(one) => Some(("center".to_owned(), one.clone())),
        [one] => Some((one.clone(), "center".to_owned())),
        [a, b] if vertical(a) || horizontal(b) => Some((b.clone(), a.clone())),
        [a, b] => Some((a.clone(), b.clone())),
        [_, _, _] | [_, _, _, _] => {
            // Edge keywords with offsets: each keyword takes the offset
            // after it.
            let mut groups: Vec<String> = Vec::new();
            for w in &words {
                match groups.last_mut() {
                    Some(last) if !is_keyword(w) => {
                        last.push(' ');
                        last.push_str(w);
                    }
                    _ => groups.push(w.clone()),
                }
            }
            let [a, b] = groups.as_slice() else {
                return None;
            };
            let first = a.split(' ').next().unwrap_or("");
            let second = b.split(' ').next().unwrap_or("");
            if vertical(first) || horizontal(second) {
                Some((b.clone(), a.clone()))
            } else {
                Some((a.clone(), b.clone()))
            }
        }
        _ => None,
    }
}

/// The value of shorthand `def` (index `index`) from its longhands'
/// `values`, or `""` if it cannot express them.
pub(crate) fn value_of(index: usize, def: &Shorthand, values: &[&Value]) -> String {
    let first = values.first().copied();
    if let Some(Value::Wide(k)) = first {
        return if values.iter().all(|v| *v == &Value::Wide(k)) {
            (*k).to_owned()
        } else {
            String::new()
        };
    }
    if let Some(Value::Pending(s, text)) = first {
        return if *s == index
            && values
                .iter()
                .all(|v| matches!(v, Value::Pending(t, x) if t == s && x == text))
        {
            text.to_string()
        } else {
            String::new()
        };
    }
    if values
        .iter()
        .any(|v| matches!(v, Value::Wide(_) | Value::Pending(..)))
    {
        return String::new();
    }
    let t: Vec<&str> = values.iter().map(|v| v.text()).collect();
    let implicit = |i: usize| matches!(values.get(i), Some(Value::Implicit(_)));
    let get = |i: usize| t.get(i).copied().unwrap_or("");
    match def.kind {
        Kind::Sides => sides(&t),
        Kind::Pair => {
            if get(0) == get(1) {
                get(0).to_owned()
            } else {
                format!("{} {}", get(0), get(1))
            }
        }
        Kind::Radius => {
            let split = |s: &str| {
                let mut it = s.split(' ');
                let h = it.next().unwrap_or("").to_owned();
                let v = it.next().map_or_else(|| h.clone(), str::to_owned);
                (h, v)
            };
            let (h, v): (Vec<String>, Vec<String>) = t.iter().map(|s| split(s)).unzip();
            let h = sides(&h.iter().map(String::as_str).collect::<Vec<_>>());
            let v = sides(&v.iter().map(String::as_str).collect::<Vec<_>>());
            if h == v { h } else { format!("{h} / {v}") }
        }
        Kind::BorderSide => border(def, &t),
        Kind::AnyOrder => any_order(def, &t, &implicit),
        Kind::Flex => t.join(" "),
        Kind::Font => font(&t),
        Kind::Background => background(&t, &implicit),
        Kind::Position => {
            let xs: Vec<&str> = get(0).split(", ").collect();
            let ys: Vec<&str> = get(1).split(", ").collect();
            if xs.len() != ys.len() {
                return String::new();
            }
            xs.iter()
                .zip(ys.iter())
                .map(|(x, y)| format!("{x} {y}"))
                .collect::<Vec<_>>()
                .join(", ")
        }
        Kind::GridLines => grid_lines(&t),
        Kind::Single => get(0).to_owned(),
        Kind::BorderImage => {
            if t.iter().all(|s| *s == "initial") {
                "initial".to_owned()
            } else {
                String::new()
            }
        }
        Kind::Opaque => String::new(),
    }
}

/// The shortest form of four side values.
fn sides<S: AsRef<str> + PartialEq>(t: &[S]) -> String {
    let [top, right, bottom, left] = t else {
        return String::new();
    };
    let (top, right, bottom, left) = (top.as_ref(), right.as_ref(), bottom.as_ref(), left.as_ref());
    if left != right {
        format!("{top} {right} {bottom} {left}")
    } else if bottom != top {
        format!("{top} {right} {bottom}")
    } else if right != top {
        format!("{top} {right}")
    } else {
        top.to_owned()
    }
}

/// `border` and its side shorthands: the width, style and color that are
/// not the initial ones (Chromium; all initial gives `""`). The sides must
/// be equal and `border-image` initial.
fn border(def: &Shorthand, t: &[&str]) -> String {
    let mut width = None;
    let mut style = None;
    let mut color = None;
    for (l, v) in def.longhands.iter().zip(t) {
        let slot = if l.starts_with("border-image") {
            if *v != "initial" {
                return String::new();
            }
            continue;
        } else if l.ends_with("-width") {
            &mut width
        } else if l.ends_with("-style") {
            &mut style
        } else {
            &mut color
        };
        match slot {
            Some(old) if old != v => return String::new(),
            _ => *slot = Some(*v),
        }
    }
    let parts: Vec<&str> = [
        width.filter(|w| *w != "medium"),
        style.filter(|s| *s != "none"),
        color.filter(|c| *c != "currentcolor"),
    ]
    .into_iter()
    .flatten()
    .collect();
    parts.join(" ")
}

/// The any-order shorthands: their parts in Chromium's order, without the
/// implicit ones (and for `text-decoration` without the initial ones).
fn any_order(def: &Shorthand, t: &[&str], implicit: &dyn Fn(usize) -> bool) -> String {
    let get = |i: usize| t.get(i).copied().unwrap_or("");
    match def.name {
        "flex-flow" => {
            let wrap = (get(1) != "nowrap").then(|| get(1));
            let direction = (get(0) != "row" || wrap.is_none()).then(|| get(0));
            [direction, wrap]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" ")
        }
        "columns" => {
            let parts: Vec<&str> = t.iter().copied().filter(|s| *s != "auto").collect();
            if parts.is_empty() {
                "auto".to_owned()
            } else {
                parts.join(" ")
            }
        }
        "text-decoration" => {
            let line = get(0);
            let style = (!implicit(1) && get(1) != "solid").then(|| get(1));
            let color = (!implicit(2) && get(2) != "currentcolor").then(|| get(2));
            let line = (!implicit(0) && (line != "none" || (style.is_none() && color.is_none())))
                .then_some(line);
            [line, style, color]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" ")
        }
        _ => (0..t.len())
            .filter(|&i| !implicit(i))
            .map(get)
            .collect::<Vec<_>>()
            .join(" "),
    }
}

/// `font`: style, variant, weight and stretch without `normal`, the size
/// with the line height, the families. `""` if the feature or variation
/// settings are not `normal`.
fn font(t: &[&str]) -> String {
    let get = |i: usize| t.get(i).copied().unwrap_or("");
    if get(2) != "normal" || get(3) != "normal" {
        return String::new();
    }
    if !matches!(get(1), "normal" | "small-caps") || get(5).ends_with('%') {
        return String::new();
    }
    let mut parts: Vec<String> = [0, 1, 4, 5]
        .into_iter()
        .map(get)
        .filter(|s| *s != "normal")
        .map(str::to_owned)
        .collect();
    if get(7) == "normal" {
        parts.push(get(6).to_owned());
    } else {
        parts.push(format!("{} / {}", get(6), get(7)));
    }
    parts.push(get(8).to_owned());
    parts.join(" ")
}

/// `background` of one layer: image, position and size, repeat,
/// attachment, origin, clip, color, without the implicit ones.
fn background(t: &[&str], implicit: &dyn Fn(usize) -> bool) -> String {
    // Layers (the image and the color may have commas of their own).
    if [1, 2, 3, 4, 5, 6, 7]
        .iter()
        .any(|&i| t.get(i).is_some_and(|s| s.contains(',')))
    {
        return String::new();
    }
    let get = |i: usize| t.get(i).copied().unwrap_or("");
    let mut parts: Vec<String> = Vec::new();
    if !implicit(0) {
        parts.push(get(0).to_owned());
    }
    let position = !implicit(1) || !implicit(2);
    if position || !implicit(3) {
        let x = if implicit(1) { "0%" } else { get(1) };
        let y = if implicit(2) { "0%" } else { get(2) };
        if implicit(3) {
            parts.push(format!("{x} {y}"));
        } else {
            parts.push(format!("{x} {y} / {}", get(3)));
        }
    }
    for i in [4, 5, 6, 7, 8] {
        if !implicit(i) {
            parts.push(get(i).to_owned());
        }
    }
    parts.join(" ")
}

/// `grid-row`, `grid-column` and `grid-area`: the lines without the
/// trailing ones that the shorthand would fill in.
fn grid_lines(t: &[&str]) -> String {
    let n = t.len();
    let source = |k: usize| {
        if n == 4 {
            [0, 0, 0, 1].get(k).copied().unwrap_or(0)
        } else {
            0
        }
    };
    let mut keep = n;
    while keep > 1 {
        let k = keep - 1;
        let from = t.get(source(k)).copied().unwrap_or("auto");
        let implied = if is_custom_ident(from) { from } else { "auto" };
        if t.get(k).copied() != Some(implied) {
            break;
        }
        keep -= 1;
    }
    t.get(..keep).unwrap_or(&[]).join(" / ")
}
