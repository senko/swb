//! Input steps for tools: `swb --headless --steps=JSON` runs a list of
//! steps through the same `input.*` methods that the protocol offers
//! (`docs/testing.md`, "Probe Chromium behaviour"). The probe tool sends
//! the same steps to Chromium through Playwright's mouse and keyboard and
//! compares the events that the page saw.
//!
//! A step is an object with one key (and `modifiers` for the point steps):
//! `click`, `dblclick`, `rightclick`, `middleclick` and `hover` take a CSS
//! selector (the center of the first match) or `[x, y]`; `mousedown` and
//! `mouseup` take a button name at the pointer position; `type` takes
//! text; `key` a key name with modifiers (`Shift+Tab`); `wheel` takes
//! `[dx, dy]` at the pointer position; `wait` takes milliseconds of page
//! time; `focus` takes a selector; `eval` takes an expression.

use std::time::Duration;

use serde_json::{Value, json};
use swb_engine::{JsonForm, Page};

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

/// The parameters that name a point: `[x, y]`, or the first match of a
/// selector as a node.
fn target(page: &mut Page, argument: &Value) -> Result<Value, String> {
    if let Some([x, y]) = argument.as_array().map(Vec::as_slice) {
        return Ok(json!({ "x": x, "y": y }));
    }
    let selector = argument.as_str().ok_or("a selector or [x, y]")?;
    let found = call(page, "dom.querySelector", json!({ "selector": selector }))?;
    match found.get("nodeId").and_then(Value::as_u64) {
        Some(id) => Ok(json!({ "nodeId": id })),
        None => Err(format!("no element matches {selector}")),
    }
}

fn run_step(page: &mut Page, step: &Value) -> Result<(), String> {
    let object = step.as_object().ok_or("a step is an object")?;
    let modifiers = object
        .get("modifiers")
        .cloned()
        .unwrap_or_else(|| json!([]));
    let (kind, argument) = object
        .iter()
        .find(|(key, _)| key.as_str() != "modifiers")
        .ok_or("a step needs a kind")?;
    match kind.as_str() {
        "click" | "dblclick" | "rightclick" | "middleclick" | "hover" => {
            let mut params = target(page, argument)?;
            params["modifiers"] = modifiers;
            // Playwright moves the pointer to the point first, also when
            // it is there.
            call(page, "input.mouseMove", params.clone())?;
            if kind != "hover" {
                params["button"] = json!(match kind.as_str() {
                    "rightclick" => "right",
                    "middleclick" => "middle",
                    _ => "left",
                });
                params["clickCount"] = json!(if kind == "dblclick" { 2 } else { 1 });
                call(page, "input.click", params)?;
            }
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
            call(page, "input.type", json!({ "text": text }))?;
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
            let source = format!(
                "document.querySelector({}).focus()",
                Value::String(selector.to_owned())
            );
            page.evaluate(&source, JsonForm::Strict);
        }
        "eval" => {
            let source = argument.as_str().ok_or("eval needs an expression")?;
            page.evaluate(source, JsonForm::Strict);
        }
        other => return Err(format!("unknown step {other}")),
    }
    Ok(())
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
