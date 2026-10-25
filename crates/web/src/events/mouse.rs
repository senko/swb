//! The mouse event interfaces: `MouseEvent` (UI Events, CSSOM View and
//! Pointer Lock), `WheelEvent`, `PointerEvent` (Pointer Events) and
//! `DragEvent`.
//! <https://www.w3.org/TR/pointerevents/#mouseevent-interface>
//!
//! Measured in Chromium 148 (`tools/probes/web-input-events.json`):
//! a `MouseEvent` floors `screenX`, `clientX` and their partners (a
//! `PointerEvent` keeps them); `pageX` is `clientX` plus the scroll
//! offset of the viewport when `view` is the window, at the time the event
//! is made; `offsetX` and `layerX` of an event that is not dispatched are
//! the rounded and the floored page coordinates. After the dispatch,
//! Chromium makes them relative to the target and to its layer; swb has no
//! layout in a script task, so they stay as they were (a departure, ADR 0028
//! "Feature 6d1 notes"). `fromElement` and `toElement` swap for
//! `mouseout` and `mouseleave`. `wheelDeltaX` and `wheelDeltaY` follow
//! `deltaX` and `deltaY` truncated unless the init dictionary gives them,
//! and `wheelDelta` is `wheelDeltaY` or, if that is 0, `wheelDeltaX`.

use swb_js::{Gc, Object, Runtime, VmResult};

use super::ui::{
    Family, construct_ui, field_getters, fields, fields_mut, init_event, modifier, ui_part, view_of,
};
use super::{EventData, Extra, base, new_event_object, now, this_data};
use crate::bindings::{self as b, Cx, NodeId, Platform};
use crate::generated as g;
use crate::generated::types::{DragEventInit, MouseEventInit, PointerEventInit, WheelEventInit};
use crate::state::{WebState, engine};

/// The members of the mouse events.
#[derive(Debug)]
pub(crate) struct Mouse {
    pub(crate) screen: (f64, f64),
    pub(crate) client: (f64, f64),
    pub(crate) page: (f64, f64),
    pub(crate) offset: (f64, f64),
    pub(crate) layer: (i32, i32),
    pub(crate) movement: (f64, f64),
    pub(crate) button: i16,
    /// True for a trusted event that has no button (`button` reads -1).
    pub(crate) no_button: bool,
    pub(crate) buttons: u16,
    pub(crate) related_target: Option<Gc<Object>>,
    pub(crate) wheel: Wheel,
    pub(crate) pointer: Pointer,
}

impl Default for Mouse {
    fn default() -> Mouse {
        Mouse {
            screen: (0.0, 0.0),
            client: (0.0, 0.0),
            page: (0.0, 0.0),
            offset: (0.0, 0.0),
            layer: (0, 0),
            movement: (0.0, 0.0),
            button: 0,
            no_button: false,
            buttons: 0,
            related_target: None,
            wheel: Wheel::default(),
            pointer: Pointer::default(),
        }
    }
}

/// The members of `WheelEvent`.
#[derive(Debug, Default)]
pub(crate) struct Wheel {
    pub(crate) delta: [f64; 3],
    pub(crate) mode: u32,
    pub(crate) legacy_x: i32,
    pub(crate) legacy_y: i32,
}

/// The members of `PointerEvent`.
#[derive(Debug)]
pub(crate) struct Pointer {
    pub(crate) id: i32,
    pub(crate) width: f64,
    pub(crate) height: f64,
    pub(crate) pressure: f32,
    pub(crate) tangential_pressure: f32,
    pub(crate) tilt: (i32, i32),
    pub(crate) twist: i32,
    pub(crate) altitude: f64,
    pub(crate) azimuth: f64,
    pub(crate) kind: String,
    pub(crate) primary: bool,
    pub(crate) persistent_device_id: i32,
    pub(crate) coalesced: Vec<Gc<Object>>,
    pub(crate) predicted: Vec<Gc<Object>>,
}

impl Default for Pointer {
    fn default() -> Pointer {
        Pointer {
            id: 0,
            width: 1.0,
            height: 1.0,
            pressure: 0.0,
            tangential_pressure: 0.0,
            tilt: (0, 0),
            twist: 0,
            altitude: std::f64::consts::FRAC_PI_2,
            azimuth: 0.0,
            kind: String::new(),
            primary: false,
            persistent_device_id: 0,
            coalesced: Vec::new(),
            predicted: Vec::new(),
        }
    }
}

impl Mouse {
    /// `button` as the getter reports it: a trusted pointer event without
    /// a button (`pointermove`, ...) reports -1, while the constructor
    /// reads a dictionary's -1 as 0 (measured).
    pub(crate) fn reported_button(&self) -> i16 {
        match self.button {
            _ if self.no_button => -1,
            -1 => 0,
            button => button,
        }
    }

    /// The button for `which`: "no button" counts as -1 (`which` 0).
    pub(crate) fn stored_button(&self) -> i16 {
        if self.no_button { -1 } else { self.button }
    }

    /// Sets the coordinates from the values that script gave and the
    /// scroll offset that the page coordinates add.
    fn place(&mut self, pointer: bool, screen: (f64, f64), client: (f64, f64), scroll: (f64, f64)) {
        let snap = |v: f64| if pointer { v } else { v.floor() };
        self.screen = (snap(screen.0), snap(screen.1));
        self.client = (snap(client.0), snap(client.1));
        self.page = (self.client.0 + scroll.0, self.client.1 + scroll.1);
        self.offset = if pointer {
            self.page
        } else {
            ((client.0 + scroll.0).round(), (client.1 + scroll.1).round())
        };
        self.layer = (self.page.0.floor() as i32, self.page.1.floor() as i32);
    }
}

/// The scroll offset of the viewport that the page coordinates of an event
/// with a `view` add.
fn scroll_of(cx: &Cx<'_>, view: Option<Gc<Object>>) -> (f64, f64) {
    match (view, engine(cx.rt)) {
        (Some(_), Ok(engine)) => engine.scroll_offset(),
        _ => (0.0, 0.0),
    }
}

/// The mouse part of the fields of a `MouseEvent`-like dictionary.
macro_rules! mouse_part {
    ($cx:expr, $family:expr, $init:expr) => {{
        let init = &$init;
        let mut f = ui_part!($family, init);
        let scroll = scroll_of($cx, f.view);
        f.mouse.place(
            $family == Family::Pointer,
            (init.screen_x, init.screen_y),
            (init.client_x, init.client_y),
            scroll,
        );
        f.mouse.movement = (init.movement_x.trunc(), init.movement_y.trunc());
        f.mouse.button = init.button;
        f.mouse.buttons = init.buttons;
        f.mouse.related_target = init.related_target.map(|p| p.object);
        f
    }};
}

/// The node of an event target object (not the window).
fn node_of_object(cx: &Cx<'_>, object: Gc<Object>) -> Option<NodeId> {
    let value = object.into();
    let (_, data) = cx.rt.host_object(value)?;
    if cx.rt.implements(value, g::NODE) {
        b::node_of(data)
    } else {
        None
    }
}

/// `fromElement` and `toElement` (legacy): the related target and the
/// target, swapped for `mouseout` and `mouseleave`.
fn from_to(cx: &Cx<'_>, this: Platform, from: bool) -> b::Result<Option<NodeId>> {
    let event = this_data(cx, this)?;
    let related = fields(cx, this)?.mouse.related_target;
    let out = matches!(event.kind.as_str(), "mouseout" | "mouseleave");
    let object = if out == from { event.target } else { related };
    Ok(object.and_then(|o| node_of_object(cx, o)))
}

impl g::mouse_event::MouseEventImpl for WebState {
    fn constructor(cx: &mut Cx<'_>, kind: String, init: MouseEventInit) -> b::Result<Gc<Object>> {
        let f = mouse_part!(cx, Family::Mouse, init);
        construct_ui(cx, kind, (init.bubbles, init.cancelable, init.composed), f)
    }

    field_getters! {
        screen_x: f64 = |f| f.mouse.screen.0;
        screen_y: f64 = |f| f.mouse.screen.1;
        client_x: f64 = |f| f.mouse.client.0;
        client_y: f64 = |f| f.mouse.client.1;
        ctrl_key: bool = |f| f.mods & modifier::CONTROL != 0;
        shift_key: bool = |f| f.mods & modifier::SHIFT != 0;
        alt_key: bool = |f| f.mods & modifier::ALT != 0;
        meta_key: bool = |f| f.mods & modifier::META != 0;
        button: i16 = |f| f.mouse.reported_button();
        buttons: u16 = |f| f.mouse.buttons;
        related_target: Option<Gc<Object>> = |f| f.mouse.related_target;
        page_x: f64 = |f| f.mouse.page.0;
        page_y: f64 = |f| f.mouse.page.1;
        x: f64 = |f| f.mouse.client.0;
        y: f64 = |f| f.mouse.client.1;
        offset_x: f64 = |f| f.mouse.offset.0;
        offset_y: f64 = |f| f.mouse.offset.1;
        movement_x: f64 = |f| f.mouse.movement.0;
        movement_y: f64 = |f| f.mouse.movement.1;
        layer_x: i32 = |f| f.mouse.layer.0;
        layer_y: i32 = |f| f.mouse.layer.1;
    }

    fn from_element(cx: &mut Cx<'_>, this: Platform) -> b::Result<Option<NodeId>> {
        from_to(cx, this, true)
    }

    fn to_element(cx: &mut Cx<'_>, this: Platform) -> b::Result<Option<NodeId>> {
        from_to(cx, this, false)
    }

    fn get_modifier_state(cx: &mut Cx<'_>, this: Platform, key: String) -> b::Result<bool> {
        Ok(fields(cx, this)?.modifier_state(&key))
    }

    /// `initMouseEvent`: integer coordinates; `buttons` is not touched.
    fn init_mouse_event(
        cx: &mut Cx<'_>,
        this: Platform,
        kind: String,
        bubbles: bool,
        cancelable: bool,
        view: Option<Platform>,
        detail: i32,
        screen_x: i32,
        screen_y: i32,
        client_x: i32,
        client_y: i32,
        ctrl: bool,
        alt: bool,
        shift: bool,
        meta: bool,
        button: i16,
        related: Option<Platform>,
    ) -> b::Result<()> {
        if !init_event(cx, this, kind, bubbles, cancelable)? {
            return Ok(());
        }
        let view = view_of(view);
        let scroll = scroll_of(cx, view);
        let f = fields_mut(cx, this)?;
        f.view = view;
        f.detail = detail;
        let pointer = f.family == Family::Pointer;
        f.mouse.place(
            pointer,
            (f64::from(screen_x), f64::from(screen_y)),
            (f64::from(client_x), f64::from(client_y)),
            scroll,
        );
        for (on, bit) in [
            (ctrl, modifier::CONTROL),
            (alt, modifier::ALT),
            (shift, modifier::SHIFT),
            (meta, modifier::META),
        ] {
            f.mods = if on { f.mods | bit } else { f.mods & !bit };
        }
        f.mouse.button = button;
        f.mouse.related_target = related.map(|p| p.object);
        Ok(())
    }
}

impl g::wheel_event::WheelEventImpl for WebState {
    fn constructor(cx: &mut Cx<'_>, kind: String, init: WheelEventInit) -> b::Result<Gc<Object>> {
        let mut f = mouse_part!(cx, Family::Wheel, init);
        let delta = [init.delta_x, init.delta_y, init.delta_z];
        let wheel_x = if init.wheel_delta_x == 0 {
            init.delta_x as i32
        } else {
            init.wheel_delta_x
        };
        let wheel_y = if init.wheel_delta_y == 0 {
            init.delta_y as i32
        } else {
            init.wheel_delta_y
        };
        f.mouse.wheel = Wheel {
            delta,
            mode: init.delta_mode,
            legacy_x: wheel_x,
            legacy_y: wheel_y,
        };
        construct_ui(cx, kind, (init.bubbles, init.cancelable, init.composed), f)
    }

    field_getters! {
        delta_x: f64 = |f| f.mouse.wheel.delta[0];
        delta_y: f64 = |f| f.mouse.wheel.delta[1];
        delta_z: f64 = |f| f.mouse.wheel.delta[2];
        delta_mode: u32 = |f| f.mouse.wheel.mode;
        wheel_delta_x: i32 = |f| f.mouse.wheel.legacy_x;
        wheel_delta_y: i32 = |f| f.mouse.wheel.legacy_y;
        wheel_delta: i32 = |f| if f.mouse.wheel.legacy_y != 0 { f.mouse.wheel.legacy_y } else { f.mouse.wheel.legacy_x };
    }
}

impl g::drag_event::DragEventImpl for WebState {
    fn constructor(cx: &mut Cx<'_>, kind: String, init: DragEventInit) -> b::Result<Gc<Object>> {
        let f = mouse_part!(cx, Family::Drag, init);
        construct_ui(cx, kind, (init.bubbles, init.cancelable, init.composed), f)
    }

    field_getters! {
        data_transfer: Option<Gc<Object>> = |_f| None;
    }
}

/// `tiltX` and `tiltY` in degrees from the altitude and azimuth angles
/// (Pointer Events, "Converting between tiltX/tiltY and
/// altitudeAngle/azimuthAngle",
/// <https://www.w3.org/TR/pointerevents/#converting-between-tiltx-tilty-and-altitudeangle-azimuthangle>).
fn tilt_from_spherical(altitude: f64, azimuth: f64) -> (i32, i32) {
    use std::f64::consts::{FRAC_PI_2, PI};
    let (x, y) = if altitude == 0.0 {
        let quarter = |a: f64| a > 0.0 && a < FRAC_PI_2;
        let x = if azimuth == 0.0 || azimuth == 2.0 * PI || quarter(azimuth) {
            FRAC_PI_2
        } else if azimuth == PI || (azimuth > FRAC_PI_2 && azimuth < 3.0 * FRAC_PI_2) {
            -FRAC_PI_2
        } else if azimuth > 3.0 * FRAC_PI_2 && azimuth < 2.0 * PI {
            FRAC_PI_2
        } else {
            0.0
        };
        let y = if azimuth == FRAC_PI_2 || (azimuth > 0.0 && azimuth < PI) {
            FRAC_PI_2
        } else if azimuth == 3.0 * FRAC_PI_2 || (azimuth > PI && azimuth < 2.0 * PI) {
            -FRAC_PI_2
        } else {
            0.0
        };
        (x, y)
    } else {
        let tan = altitude.tan();
        ((azimuth.cos() / tan).atan(), (azimuth.sin() / tan).atan())
    };
    let degrees = 180.0 / PI;
    ((x * degrees).round() as i32, (y * degrees).round() as i32)
}

/// The altitude and azimuth angles from `tiltX` and `tiltY` in degrees
/// (same section as [`tilt_from_spherical`]).
fn spherical_from_tilt(tilt_x: i32, tilt_y: i32) -> (f64, f64) {
    use std::f64::consts::{FRAC_PI_2, PI};
    let (rx, ry) = (
        f64::from(tilt_x) * PI / 180.0,
        f64::from(tilt_y) * PI / 180.0,
    );
    let azimuth = if tilt_x == 0 {
        match tilt_y.signum() {
            1 => FRAC_PI_2,
            -1 => 3.0 * FRAC_PI_2,
            _ => 0.0,
        }
    } else if tilt_y == 0 {
        if tilt_x < 0 { PI } else { 0.0 }
    } else if tilt_x.abs() == 90 || tilt_y.abs() == 90 {
        0.0
    } else {
        let a = ry.tan().atan2(rx.tan());
        if a < 0.0 { a + 2.0 * PI } else { a }
    };
    let altitude = if tilt_x.abs() == 90 || tilt_y.abs() == 90 {
        0.0
    } else if tilt_x == 0 {
        FRAC_PI_2 - ry.abs()
    } else if tilt_y == 0 {
        FRAC_PI_2 - rx.abs()
    } else {
        (1.0 / (rx.tan().powi(2) + ry.tan().powi(2)).sqrt()).atan()
    };
    (altitude, azimuth)
}

impl g::pointer_event::PointerEventImpl for WebState {
    fn constructor(cx: &mut Cx<'_>, kind: String, init: PointerEventInit) -> b::Result<Gc<Object>> {
        let mut f = mouse_part!(cx, Family::Pointer, init);
        let spherical = init.altitude_angle.is_some() || init.azimuth_angle.is_some();
        let tilted = init.tilt_x.is_some() || init.tilt_y.is_some();
        let (mut tilt, mut altitude, mut azimuth) = (
            (init.tilt_x.unwrap_or(0), init.tilt_y.unwrap_or(0)),
            init.altitude_angle.unwrap_or(std::f64::consts::FRAC_PI_2),
            init.azimuth_angle.unwrap_or(0.0),
        );
        if tilted && !spherical {
            (altitude, azimuth) = spherical_from_tilt(tilt.0, tilt.1);
        } else if spherical && !tilted {
            tilt = tilt_from_spherical(altitude, azimuth);
        }
        f.mouse.pointer = Pointer {
            id: init.pointer_id,
            width: init.width,
            height: init.height,
            pressure: init.pressure,
            tangential_pressure: init.tangential_pressure,
            tilt,
            twist: init.twist,
            altitude,
            azimuth,
            kind: init.pointer_type,
            primary: init.is_primary,
            persistent_device_id: init.persistent_device_id,
            coalesced: init.coalesced_events.iter().map(|p| p.object).collect(),
            predicted: init.predicted_events.iter().map(|p| p.object).collect(),
        };
        construct_ui(cx, kind, (init.bubbles, init.cancelable, init.composed), f)
    }

    field_getters! {
        pointer_id: i32 = |f| f.mouse.pointer.id;
        width: f64 = |f| f.mouse.pointer.width;
        height: f64 = |f| f.mouse.pointer.height;
        pressure: f32 = |f| f.mouse.pointer.pressure;
        tilt_x: i32 = |f| f.mouse.pointer.tilt.0;
        tilt_y: i32 = |f| f.mouse.pointer.tilt.1;
        azimuth_angle: f64 = |f| f.mouse.pointer.azimuth;
        altitude_angle: f64 = |f| f.mouse.pointer.altitude;
        tangential_pressure: f32 = |f| f.mouse.pointer.tangential_pressure;
        twist: i32 = |f| f.mouse.pointer.twist;
        pointer_type: String = |f| f.mouse.pointer.kind.clone();
        is_primary: bool = |f| f.mouse.pointer.primary;
        persistent_device_id: i32 = |f| f.mouse.pointer.persistent_device_id;
        get_predicted_events: Vec<Gc<Object>> = |f| f.mouse.pointer.predicted.clone();
        get_coalesced_events: Vec<Gc<Object>> = |f| f.mouse.pointer.coalesced.clone();
    }
}

/// The event of `click()` and of the label steps: Chromium makes a
/// `PointerEvent` with `pointerId` -1 that is not trusted, bubbles, can be
/// canceled and is composed; `detail` and the coordinates are 0.
pub(crate) fn new_click_event(
    rt: &mut Runtime,
    trusted: bool,
    detail: i32,
) -> VmResult<Gc<Object>> {
    let mut f = super::ui::Fields::new(Family::Pointer);
    f.view = Some(rt.global(0)?);
    f.detail = detail;
    f.mouse.pointer.id = -1;
    let mut data = EventData::new(
        "click".to_owned(),
        &base(true, true, true),
        now(rt),
        Extra::Ui(Box::new(f)),
    );
    data.is_trusted = trusted;
    new_event_object(rt, 0, g::POINTER_EVENT, data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tilt_conversions_match_chromium() {
        // Chromium 148: altitudeAngle 0.5 gives tiltX 61; tiltX 30 gives
        // altitudeAngle 1.0471975511965979.
        assert_eq!(tilt_from_spherical(0.5, 0.0), (61, 0));
        let (altitude, azimuth) = spherical_from_tilt(30, 0);
        assert!((altitude - std::f64::consts::FRAC_PI_3).abs() < 1e-12);
        assert_eq!(azimuth, 0.0);
        assert_eq!(tilt_from_spherical(0.0, 0.0), (90, 0));
        assert_eq!(spherical_from_tilt(0, 0).0, std::f64::consts::FRAC_PI_2);
    }

    #[test]
    fn mouse_events_floor_and_pointer_events_keep_fractions() {
        let mut mouse = Mouse::default();
        mouse.place(false, (1.2, 9.9), (1.7, -2.5), (0.0, 0.0));
        assert_eq!(mouse.screen, (1.0, 9.0));
        assert_eq!(mouse.client, (1.0, -3.0));
        assert_eq!(mouse.offset, (2.0, -3.0));
        assert_eq!(mouse.layer, (1, -3));
        mouse.place(true, (0.0, 0.0), (10.5, 20.0), (30.0, 70.0));
        assert_eq!(mouse.page, (40.5, 90.0));
        assert_eq!(mouse.offset, (40.5, 90.0));
    }
}
