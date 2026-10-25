//! The events of the user's input that the engine fires (UI Events,
//! <https://www.w3.org/TR/uievents/>, and Pointer Events,
//! <https://www.w3.org/TR/pointerevents/>; ADR 0028 section 11, "Feature
//! 6d2 notes"): the pointer and mouse events, `wheel`, the keyboard events
//! and `beforeinput`/`input`.
//!
//! The engine describes an event with a [`UserEvent`] (it knows the hit
//! test, the layout and the state of the buttons); this module makes the
//! event object with the values that Chromium 148 shows for real input
//! (measured with `tools/probes/web-input-real.json`): `pageX` and `pageY`
//! add the scroll offset of the viewport to the client coordinates, a
//! `PointerEvent` has the pointer id 1 and the type `mouse`, and `wheel`
//! is cancelable only when a listener on the path is not passive.

use swb_dom::NodeId;
use swb_js::{Runtime, VmResult};

use super::target::{TargetKind, listeners_of};
use super::ui::{Family, Fields, modifier};
use crate::bindings::Host;
use crate::generated as g;
use crate::state::{WebState, dom, engine};

/// The modifier keys held during an event (bits of [`UserEvent::modifiers`]).
pub mod modifiers {
    /// `ctrlKey`.
    pub const CONTROL: u16 = super::modifier::CONTROL;
    /// `shiftKey`.
    pub const SHIFT: u16 = super::modifier::SHIFT;
    /// `altKey`.
    pub const ALT: u16 = super::modifier::ALT;
    /// `metaKey`.
    pub const META: u16 = super::modifier::META;
}

/// The interface of an event of the user's input.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum UserKind {
    /// `MouseEvent` (`mouseover`, `mousedown`, `dblclick`, ...).
    #[default]
    Mouse,
    /// `PointerEvent` (`pointerdown`, `click`, `contextmenu`, ...).
    Pointer,
    /// `WheelEvent`.
    Wheel,
    /// `KeyboardEvent`.
    Keyboard,
    /// `InputEvent` (`beforeinput`, `input`).
    Input,
}

/// The attributes of an event of the user's input. The type, the
/// `bubbles`, `cancelable` and `composed` flags are those of the
/// [`super::TrustedEvent`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UserEvent {
    /// The interface.
    pub kind: UserKind,
    /// `clientX` and `clientY` in CSS px (viewport coordinates).
    pub client: (f64, f64),
    /// `offsetX` and `offsetY`: relative to the target.
    pub offset: (f64, f64),
    /// `movementX` and `movementY`.
    pub movement: (f64, f64),
    /// `button`.
    pub button: i16,
    /// `buttons`.
    pub buttons: u16,
    /// `detail`: the click count of a mouse event.
    pub detail: i32,
    /// `relatedTarget`.
    pub related_target: Option<NodeId>,
    /// The modifier keys ([`modifiers`]).
    pub modifiers: u16,
    /// `pointerId` of a pointer event: 1 for the mouse.
    pub pointer_id: i32,
    /// `isPrimary` of a pointer event.
    pub primary: bool,
    /// `pressure` of a pointer event.
    pub pressure: f32,
    /// `deltaX` and `deltaY` of a wheel event.
    pub delta: (f64, f64),
    /// `key` of a keyboard event.
    pub key: String,
    /// `code`.
    pub code: String,
    /// `keyCode`.
    pub key_code: i32,
    /// `charCode`.
    pub char_code: i32,
    /// `location`.
    pub location: u32,
    /// `repeat`.
    pub repeat: bool,
    /// `inputType` of an input event.
    pub input_type: String,
    /// `data` of an input event.
    pub data: Option<String>,
}

/// The legacy wheel tick of Chromium: `wheelDelta` is 120 per axis, with
/// the sign opposite to the delta (measured; the size of the delta does
/// not matter).
fn wheel_ticks(delta: f64) -> i32 {
    if delta > 0.0 {
        -120
    } else if delta < 0.0 {
        120
    } else {
        0
    }
}

/// The class and the fields of a [`UserEvent`] (without the objects that
/// need the heap).
pub(crate) fn fields_of(
    rt: &mut Runtime,
    event: &UserEvent,
) -> VmResult<(swb_js::ClassId, Fields)> {
    let (class, family) = match event.kind {
        UserKind::Mouse => (g::MOUSE_EVENT, Family::Mouse),
        UserKind::Pointer => (g::POINTER_EVENT, Family::Pointer),
        UserKind::Wheel => (g::WHEEL_EVENT, Family::Wheel),
        UserKind::Keyboard => (g::KEYBOARD_EVENT, Family::Keyboard),
        UserKind::Input => (g::INPUT_EVENT, Family::Input),
    };
    let mut f = Fields::new(family);
    f.view = Some(rt.global(0)?);
    f.detail = event.detail;
    f.mods = event.modifiers;
    f.related_target = None;
    match event.kind {
        UserKind::Mouse | UserKind::Pointer | UserKind::Wheel => fill_mouse(rt, &mut f, event)?,
        UserKind::Keyboard => {
            f.key.clone_from(&event.key);
            f.code.clone_from(&event.code);
            f.key_code = event.key_code;
            f.char_code = event.char_code;
            f.location = event.location;
            f.repeat = event.repeat;
        }
        UserKind::Input => {
            f.input_type.clone_from(&event.input_type);
            f.data.clone_from(&event.data);
        }
    }
    Ok((class, f))
}

fn fill_mouse(rt: &mut Runtime, f: &mut Fields, event: &UserEvent) -> VmResult<()> {
    let (scroll_x, scroll_y) = engine(rt)?.scroll_offset();
    let pointer = event.kind == UserKind::Pointer;
    let m = &mut f.mouse;
    let snap = |v: f64| if pointer { v } else { v.floor() };
    m.client = (snap(event.client.0), snap(event.client.1));
    m.screen = m.client;
    m.page = (m.client.0 + scroll_x, m.client.1 + scroll_y);
    m.offset = event.offset;
    m.layer = (m.page.0.floor() as i32, m.page.1.floor() as i32);
    m.movement = (event.movement.0.trunc(), event.movement.1.trunc());
    m.no_button = event.button == -1;
    m.button = event.button.max(0);
    m.buttons = event.buttons;
    if let Some(node) = event.related_target {
        m.related_target = Some(WebState::wrap_node(rt, 0, node)?);
    }
    if pointer {
        let p = &mut m.pointer;
        p.id = event.pointer_id;
        "mouse".clone_into(&mut p.kind);
        p.primary = event.primary;
        p.pressure = event.pressure;
    }
    if event.kind == UserKind::Wheel {
        let w = &mut m.wheel;
        w.delta = [event.delta.0, event.delta.1, 0.0];
        w.mode = 0;
        w.legacy_x = wheel_ticks(event.delta.0);
        w.legacy_y = wheel_ticks(event.delta.1);
    }
    Ok(())
}

/// True if a listener of `kind` on the path from `target` up to the window
/// is not passive. Chromium makes a `wheel` event cancelable only then.
pub(crate) fn has_blocking_listener(
    rt: &Runtime,
    target: TargetKind,
    kind: &str,
) -> VmResult<bool> {
    let mut targets = vec![target];
    if let TargetKind::Node(node) = target {
        let document = dom(rt)?;
        targets.extend(document.ancestors(node).map(TargetKind::Node));
        if document.is_connected(node) {
            targets.push(TargetKind::Window);
        }
    }
    for t in targets {
        if listeners_of(rt, t, kind)?.iter().any(|l| !l.passive) {
            return Ok(true);
        }
    }
    Ok(false)
}
