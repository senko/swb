//! Input steps for tools: `swb --headless --steps=JSON` runs a list of
//! steps through the same `input.*` methods that the protocol offers
//! (`docs/testing.md`, "Probe Chromium behaviour"). The probe tool sends
//! the same steps to Chromium through Playwright's mouse and keyboard and
//! compares the events that the page saw.
//!
//! A step is an object with one key (and `modifiers` for the point steps):
//! `click`, `dblclick`, `rightclick`, `middleclick` and `hover` take a CSS
//! selector (the center of the first match; see [`split_selector`] for the
//! Playwright forms `:visible` and `>> nth=N`) or `[x, y]`; `mousedown` and
//! `mouseup` take a button name at the pointer position; `type` takes
//! text; `key` a key name with modifiers (`Shift+Tab`); `wheel` takes
//! `[dx, dy]` at the pointer position; `wait` takes milliseconds of page
//! time; `focus` takes a selector (the forms of the pointer steps); `eval`
//! takes an expression; `record` takes an expression and prints its answer
//! line to standard output, as `--eval` does; `screenshot` takes a file
//! name and writes a PNG of the viewport (and logs `screenshot FILE` at
//! level `info`, so that a log shows which messages came before it);
//! `viewport` takes `[width, height]`
//! (`page.setViewport`); `colorScheme` takes `"light"` or `"dark"`
//! (`prefers-color-scheme`); `back` and `forward` take `true`
//! (`page.back`, `page.forward`: the history of the tab). A point step may
//! have `frame`, the path of a frame (`#a >> #b`: CSS selectors of `iframe`
//! elements from the top-level document, see [`resolve_frame`]): its
//! selector is looked up in that frame, and a point `[x, y]` is in the
//! frame's viewport.

use std::io::Write;
use std::time::Duration;

use serde_json::{Value, json};
use swb_dom::NodeId;
use swb_engine::{Evaluation, FrameId, JsonForm, Page};

use crate::methods::{Outcome, execute};

/// Runs the steps in order. Returns the first error with its step number.
/// The tasks that are due run after each step, as they do in Chromium while
/// Playwright waits for its next call (a `javascript:` URL of a link, a
/// `setTimeout(0)` of a listener).
pub fn run_steps(page: &mut Page, steps: &[Value]) -> Result<(), String> {
    for (number, step) in steps.iter().enumerate() {
        run_step(page, step).map_err(|error| format!("step {}: {error}", number + 1))?;
        page.run_event_loop(Duration::from_micros(1), Duration::from_secs(5));
    }
    Ok(())
}

/// Calls a method and returns its result.
fn call(page: &mut Page, method: &str, params: Value) -> Result<Value, String> {
    match execute(page, method, params) {
        Outcome::Done(Ok(value)) => Ok(value),
        Outcome::Done(Err(error)) => Err(format!("{method}: {}", error.message)),
        Outcome::Wait(..) | Outcome::Close => Err(format!("{method}: not a step")),
    }
}

/// The frame of a frame path: CSS selectors of `iframe` elements, joined by
/// ` >> `, from the top-level document (Playwright's form of
/// `frame_locator`). The first element that a selector matches must be an
/// `iframe` that has a frame.
pub fn resolve_frame(page: &mut Page, path: &str) -> Result<FrameId, String> {
    let mut frame = FrameId::TOP;
    for selector in path.split(">>").map(str::trim) {
        if selector.is_empty() {
            return Err(format!("the frame path {path:?} has an empty selector"));
        }
        let found = page
            .with_frame(frame, |p| {
                let first = p.query_selector_all(selector).map(|n| n.first().copied());
                let is_iframe = |n| {
                    p.document()
                        .is_some_and(|d| d.is_html_element(n, &swb_dom::local_name!("iframe")))
                };
                first.map(|n| n.map(|n| (n, is_iframe(n))))
            })
            .ok_or_else(|| format!("no frame for {selector:?} in {path:?}"))?;
        let node = match found {
            None => return Err(format!("{selector:?} is not a valid selector")),
            Some(None) => return Err(format!("no iframe matches {selector:?} in {path:?}")),
            Some(Some((_, false))) => {
                return Err(format!("the first match of {selector:?} is not an iframe"));
            }
            Some(Some((node, true))) => node,
        };
        frame = page
            .frame_of(frame, node)
            .ok_or_else(|| format!("the iframe {selector:?} of {path:?} has no frame"))?;
    }
    Ok(frame)
}

/// Which match of a selector a step takes.
#[derive(Debug, PartialEq, Clone, Copy)]
enum Nth {
    /// The N-th match, counted from 0 (`>> nth=N`).
    First(usize),
    /// The last match (`>> nth=-1`).
    Last,
}

/// A step selector split into its parts: the CSS selector, whether
/// Playwright's `:visible` ended it, and the match of `>> nth=N`.
#[derive(Debug, PartialEq)]
struct SelectorForm<'a> {
    css: &'a str,
    visible: bool,
    nth: Option<Nth>,
}

/// The byte position of the last `needle` in `text` outside quoted strings
/// (`'..'` and `".."`, with a backslash escape).
fn rfind_unquoted(text: &str, needle: &str) -> Option<usize> {
    let mut found = None;
    let mut quote = None;
    let mut escaped = false;
    for (at, ch) in text.char_indices() {
        match quote {
            Some(open) => {
                if escaped {
                    escaped = false;
                } else if ch == '\\' {
                    escaped = true;
                } else if ch == open {
                    quote = None;
                }
            }
            None if ch == '\'' || ch == '"' => quote = Some(ch),
            None if text[at..].starts_with(needle) => found = Some(at),
            None => {}
        }
    }
    found
}

/// The error for a selector form that the steps do not take.
fn unsupported(selector: &str, form: &str) -> String {
    format!(
        "the selector {selector:?} uses {form}, which is not supported (a step takes a CSS \
         selector, a trailing `:visible` and a trailing `>> nth=N` or `>> nth=-1`)"
    )
}

/// Splits `S`, `S:visible`, `S >> nth=N` and `S:visible >> nth=N`
/// (Playwright's selector engines: `:visible` keeps the elements with a
/// non-empty bounding box and `visibility` other than `hidden`; `nth=N`
/// picks the N-th match, counted from 0, of what comes before it, and
/// `nth=-1` the last). Other uses of `>>` and `:visible` (a chain, a
/// `:visible` inside the selector) are an error, not a CSS selector.
fn split_selector(selector: &str) -> Result<SelectorForm<'_>, String> {
    let mut css = selector.trim();
    let mut nth = None;
    if let Some(at) = rfind_unquoted(css, ">>")
        && let Some(n) = css[at + 2..].trim().strip_prefix("nth=")
    {
        nth = Some(match n {
            "-1" => Nth::Last,
            _ => Nth::First(
                n.parse()
                    .ok()
                    .filter(|_| n.bytes().all(|b| b.is_ascii_digit()))
                    .ok_or_else(|| unsupported(selector, &format!("`nth={n}`")))?,
            ),
        });
        css = css[..at].trim_end();
    }
    let visible = css.strip_suffix(":visible");
    let css = visible.unwrap_or(css);
    if rfind_unquoted(css, ">>").is_some() {
        return Err(unsupported(selector, "`>>` (a chain of selector engines)"));
    }
    if rfind_unquoted(css, ":visible").is_some() {
        return Err(unsupported(selector, "`:visible` before the end"));
    }
    Ok(SelectorForm {
        css,
        visible: visible.is_some(),
        nth,
    })
}

/// The match that a step selector names: its position among all matches of
/// the CSS part (in document order) and its node index (`None` if
/// nothing matches). The plain selector takes the first match; the
/// `:visible` and `>> nth=N` forms pick as Playwright does (`nth` counts
/// the visible matches if `:visible` is there).
fn find(
    page: &mut Page,
    selector: &str,
    frame: Option<u64>,
) -> Result<Option<(usize, u64)>, String> {
    let form = split_selector(selector)?;
    let mut query = json!({ "selector": form.css });
    if let Some(id) = frame {
        query["frameId"] = json!(id);
    }
    let found = call(page, "dom.querySelectorAll", query)?;
    let ids = found
        .get("nodeIds")
        .and_then(Value::as_array)
        .map(|ids| ids.iter().filter_map(Value::as_u64))
        .into_iter()
        .flatten()
        .enumerate()
        .collect::<Vec<_>>();
    let frame_id = frame.map_or(FrameId::TOP, FrameId::from_u64);
    let matches: Vec<(usize, u64)> = ids
        .into_iter()
        .filter(|&(_, id)| {
            !form.visible || {
                let node = usize::try_from(id).ok().and_then(NodeId::from_index);
                node.and_then(|n| page.with_frame(frame_id, |p| p.is_visible(n)))
                    .unwrap_or(false)
            }
        })
        .collect();
    Ok(match form.nth {
        None => matches.first().copied(),
        Some(Nth::First(n)) => matches.get(n).copied(),
        Some(Nth::Last) => matches.last().copied(),
    })
}

/// The parameters that name a point: `[x, y]`, or the first match of a
/// selector as a node; in the frame of `frame` (a frame path) if given.
fn target(page: &mut Page, argument: &Value, frame: Option<&str>) -> Result<Value, String> {
    let frame_id = match frame {
        Some(path) => Some(resolve_frame(page, path)?.to_u64()),
        None => None,
    };
    let mut params = if let Some([x, y]) = argument.as_array().map(Vec::as_slice) {
        json!({ "x": x, "y": y })
    } else {
        let selector = argument.as_str().ok_or("a selector or [x, y]")?;
        match find(page, selector, frame_id)? {
            Some((_, id)) => json!({ "nodeId": id }),
            None => return Err(format!("no element matches {selector}")),
        }
    };
    if let Some(id) = frame_id {
        params["frameId"] = json!(id);
    }
    Ok(params)
}

/// A step with a point (`params` names it): the pointer moves there, and
/// the kind of step then clicks or not.
fn point_step(page: &mut Page, kind: &str, mut params: Value) -> Result<(), String> {
    // Playwright moves the pointer to the point first, also when it is
    // there.
    call(page, "input.mouseMove", params.clone())?;
    if kind != "hover" {
        params["button"] = json!(match kind {
            "rightclick" => "right",
            "middleclick" => "middle",
            _ => "left",
        });
        params["clickCount"] = json!(if kind == "dblclick" { 2 } else { 1 });
        call(page, "input.click", params)?;
    }
    Ok(())
}

fn run_step(page: &mut Page, step: &Value) -> Result<(), String> {
    let object = step.as_object().ok_or("a step is an object")?;
    let modifiers = object
        .get("modifiers")
        .cloned()
        .unwrap_or_else(|| json!([]));
    let (kind, argument) = object
        .iter()
        .find(|(key, _)| !matches!(key.as_str(), "modifiers" | "frame"))
        .ok_or("a step needs a kind")?;
    let frame = match object.get("frame") {
        Some(path) => Some(path.as_str().ok_or("frame is a frame path (a string)")?),
        None => None,
    };
    if frame.is_some()
        && !matches!(
            kind.as_str(),
            "click" | "dblclick" | "rightclick" | "middleclick" | "hover"
        )
    {
        return Err(format!("the step {kind} does not take a frame"));
    }
    match kind.as_str() {
        "click" | "dblclick" | "rightclick" | "middleclick" | "hover" => {
            let mut params = target(page, argument, frame)?;
            params["modifiers"] = modifiers;
            point_step(page, kind, params)?;
        }
        "mousedown" | "mouseup" => {
            let at = page.pointer_position().unwrap_or_default();
            let params = json!({ "x": at.x, "y": at.y, "button": argument });
            let method = if kind == "mousedown" {
                "input.mouseDown"
            } else {
                "input.mouseUp"
            };
            call(page, method, params)?;
        }
        "type" => {
            let text = argument.as_str().ok_or("type needs text")?;
            // One character at a time, with the due tasks between: Playwright
            // awaits each key, so a task queued by one character (a
            // `selectionchange`) runs before the next.
            for ch in text.chars() {
                call(page, "input.type", json!({ "text": ch.to_string() }))?;
                page.run_event_loop(Duration::from_micros(1), Duration::from_secs(5));
            }
        }
        "key" => {
            let spec = argument.as_str().ok_or("key needs a key name")?;
            let (modifiers, key) = split_key(spec);
            call(
                page,
                "input.key",
                json!({ "key": key, "modifiers": modifiers }),
            )?;
        }
        "wheel" => {
            let delta = argument.as_array().ok_or("wheel needs [dx, dy]")?;
            let at = page.pointer_position().unwrap_or_default();
            let params = json!({ "x": at.x, "y": at.y, "dx": delta.first(), "dy": delta.get(1) });
            call(page, "input.wheel", params)?;
        }
        "wait" => {
            let ms = argument.as_u64().ok_or("wait needs milliseconds")?;
            page.run_event_loop(
                Duration::from_millis(ms),
                Duration::from_millis(ms.saturating_add(5000)),
            );
        }
        "focus" => {
            let selector = argument.as_str().ok_or("focus needs a selector")?;
            focus(page, selector)?;
        }
        "eval" => {
            let source = argument.as_str().ok_or("eval needs an expression")?;
            page.evaluate(source, JsonForm::Strict);
        }
        "record" => {
            let source = argument.as_str().ok_or("record needs an expression")?;
            record(page, source)?;
        }
        "screenshot" => screenshot(page, argument)?,
        "viewport" => {
            let size = argument
                .as_array()
                .ok_or("viewport needs [width, height]")?;
            let params = json!({ "width": size.first(), "height": size.get(1) });
            call(page, "page.setViewport", params)?;
        }
        "colorScheme" => match argument.as_str() {
            Some("dark") => page.set_color_scheme(true),
            Some("light") => page.set_color_scheme(false),
            _ => return Err("colorScheme needs \"light\" or \"dark\"".to_owned()),
        },
        "back" | "forward" => history_step(page, kind, argument)?,
        other => return Err(format!("unknown step {other}")),
    }
    Ok(())
}

/// The `focus` step: calls `focus()` on the element that the selector names
/// (the forms of the pointer steps).
fn focus(page: &mut Page, selector: &str) -> Result<(), String> {
    let css = split_selector(selector)?.css;
    let Some((position, _)) = find(page, selector, None)? else {
        return Err(format!("no element matches {selector}"));
    };
    let source = format!(
        "document.querySelectorAll({})[{position}].focus()",
        Value::String(css.to_owned())
    );
    page.evaluate(&source, JsonForm::Strict);
    Ok(())
}

/// The `screenshot` step: writes the viewport as a PNG file.
fn screenshot(page: &mut Page, argument: &Value) -> Result<(), String> {
    let path = argument.as_str().ok_or("screenshot needs a file name")?;
    let pixmap = page
        .screenshot(false)
        .map_err(|e| format!("screenshot: {e}"))?;
    pixmap
        .save_png(path)
        .map_err(|e| format!("screenshot: cannot write {path}: {e}"))?;
    log::info!("screenshot {path}");
    Ok(())
}

/// How long a `record` step waits for the promise that its expression
/// returns.
const RECORD_TIMEOUT: Duration = Duration::from_secs(5);

/// The `record` step: evaluates `source` as `--eval` does and prints its
/// answer line (`{"value": V}` or `{"error": "..."}`, see [`answer_line`])
/// to standard output, so that a tool can read the state of the page
/// between steps.
fn record(page: &mut Page, source: &str) -> Result<(), String> {
    let evaluation = page.evaluate_with_timeout(source, JsonForm::Probe, RECORD_TIMEOUT);
    let mut out = std::io::stdout().lock();
    writeln!(out, "{}", answer_line(evaluation)).map_err(|e| format!("record: {e}"))?;
    out.flush().map_err(|e| format!("record: {e}"))
}

/// The line of one answer of `--eval` or of a `record` step (docs/testing.md,
/// "The `--eval` contract"): `{"value": V}` with V in the probe's JSON form
/// (`NaN`, `Infinity`, `-0.0` allowed), or `{"error": "Name: message"}`.
/// `None` is a page without scripts.
pub fn answer_line(evaluation: Option<Evaluation>) -> String {
    match evaluation {
        Some(Evaluation::Value(text)) => format!("{{\"value\": {text}}}"),
        Some(Evaluation::Error(message)) => {
            format!("{{\"error\": {}}}", Value::String(message))
        }
        None => "{\"error\": \"JavaScript is off (use --js)\"}".to_owned(),
    }
}

/// The `back` and `forward` steps: the argument is `true`.
fn history_step(page: &mut Page, kind: &str, argument: &Value) -> Result<(), String> {
    if argument != &json!(true) {
        return Err(format!("{kind} needs true"));
    }
    let method = if kind == "back" {
        "page.back"
    } else {
        "page.forward"
    };
    call(page, method, Value::Null).map(drop)
}

/// Splits `Shift+Tab` into the modifiers and the key. A `+` at the end is
/// the key itself.
fn split_key(spec: &str) -> (Vec<&str>, &str) {
    let mut modifiers = Vec::new();
    let mut rest = spec;
    while let Some((head, tail)) = rest.split_once('+') {
        if tail.is_empty() || !matches!(head, "Shift" | "Control" | "Alt" | "Meta") {
            break;
        }
        modifiers.push(head);
        rest = tail;
    }
    (modifiers, rest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selectors_split_into_css_visible_and_nth() {
        let form = |css, visible, nth| Ok(SelectorForm { css, visible, nth });
        assert_eq!(split_selector("a.b"), form("a.b", false, None));
        assert_eq!(split_selector("a:visible"), form("a", true, None));
        assert_eq!(
            split_selector("a >> nth=2"),
            form("a", false, Some(Nth::First(2)))
        );
        assert_eq!(
            split_selector("li a.x:visible >> nth=0"),
            form("li a.x", true, Some(Nth::First(0)))
        );
        assert_eq!(
            split_selector("a >> nth=-1"),
            form("a", false, Some(Nth::Last))
        );
        // `>>` and `:visible` inside a quoted attribute value are CSS.
        assert_eq!(
            split_selector("a[title='x >> nth=1']"),
            form("a[title='x >> nth=1']", false, None)
        );
        assert_eq!(
            split_selector("a[title=\"y:visible\"]:visible"),
            form("a[title=\"y:visible\"]", true, None)
        );
    }

    #[test]
    fn unsupported_forms_say_so() {
        for selector in [
            "a >> b",
            "a >> nth=1 >> nth=0",
            "a:visible b",
            "a:visible, b",
            "a >> nth=-2",
            "a >> nth=+1",
            "a >> nth= 1",
            "a >> nth=x",
        ] {
            let error = split_selector(selector).unwrap_err();
            assert!(error.contains("not supported"), "{selector}: {error}");
        }
    }

    #[test]
    fn keys_split_into_modifiers_and_key() {
        assert_eq!(split_key("Enter"), (vec![], "Enter"));
        assert_eq!(split_key("Shift+Tab"), (vec!["Shift"], "Tab"));
        assert_eq!(
            split_key("Control+Shift+a"),
            (vec!["Control", "Shift"], "a")
        );
        assert_eq!(split_key("+"), (vec![], "+"));
        assert_eq!(split_key("Control++"), (vec!["Control"], "+"));
    }
}
