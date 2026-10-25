//! The UI event interfaces (UI Events,
//! <https://www.w3.org/TR/uievents/>): `UIEvent`, `FocusEvent`,
//! `InputEvent`, `KeyboardEvent`, `CompositionEvent` and `TextEvent`, and
//! the shared state [`Fields`] of every event of the family (the mouse
//! events of `mouse.rs` use it too).
//!
//! The attributes follow Chromium 148 where it differs from the
//! specification (measured with `tools/probes/web-input-events.json`):
//! `which` is computed from the interface (`button + 1` for mouse events,
//! `keyCode` for keyboard events, else 0) and not initialized; `keyCode`
//! and `charCode` are signed; the `init*` methods change only the
//! members that they name (`initKeyboardEvent` leaves `key` and `keyCode`
//! alone).

use swb_js::{Gc, Object};

use super::{EventData, Extra, base, construct, this_data, this_data_mut};
use crate::bindings::{self as b, Cx, Platform};
use crate::generated as g;
use crate::generated::types::{
    CompositionEventInit, FocusEventInit, InputEventInit, KeyboardEventInit, UIEventInit,
};
use crate::state::WebState;

/// The interface family of an event with [`Fields`]: it decides `which`
/// and `fromElement`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Family {
    #[default]
    Ui,
    Focus,
    Input,
    Composition,
    Text,
    Keyboard,
    Mouse,
    Wheel,
    Pointer,
    Drag,
}

impl Family {
    /// True for `MouseEvent` and its subclasses.
    pub(crate) fn is_mouse(self) -> bool {
        matches!(
            self,
            Family::Mouse | Family::Wheel | Family::Pointer | Family::Drag
        )
    }
}

/// The modifier keys of `EventModifierInit`, as bits of [`Fields::mods`].
pub(crate) mod modifier {
    pub(crate) const CONTROL: u16 = 1;
    pub(crate) const SHIFT: u16 = 1 << 1;
    pub(crate) const ALT: u16 = 1 << 2;
    pub(crate) const META: u16 = 1 << 3;
    pub(crate) const ALT_GRAPH: u16 = 1 << 4;
    pub(crate) const CAPS_LOCK: u16 = 1 << 5;
    pub(crate) const FN: u16 = 1 << 6;
    pub(crate) const FN_LOCK: u16 = 1 << 7;
    pub(crate) const HYPER: u16 = 1 << 8;
    pub(crate) const NUM_LOCK: u16 = 1 << 9;
    pub(crate) const SCROLL_LOCK: u16 = 1 << 10;
    pub(crate) const SUPER: u16 = 1 << 11;
    pub(crate) const SYMBOL: u16 = 1 << 12;
    pub(crate) const SYMBOL_LOCK: u16 = 1 << 13;

    /// The bit of the key name that `getModifierState` takes (UI Events
    /// §3.7.1.3, <https://www.w3.org/TR/uievents/#keys-modifier>), or 0.
    pub(crate) fn bit(name: &str) -> u16 {
        match name {
            "Control" => CONTROL,
            "Shift" => SHIFT,
            "Alt" => ALT,
            "Meta" => META,
            "AltGraph" => ALT_GRAPH,
            "CapsLock" => CAPS_LOCK,
            "Fn" => FN,
            "FnLock" => FN_LOCK,
            "Hyper" => HYPER,
            "NumLock" => NUM_LOCK,
            "ScrollLock" => SCROLL_LOCK,
            "Super" => SUPER,
            "Symbol" => SYMBOL,
            "SymbolLock" => SYMBOL_LOCK,
            _ => 0,
        }
    }
}

/// The modifier bits of a dictionary that has the members of
/// `EventModifierInit`.
macro_rules! modifiers_of {
    ($init:expr) => {{
        use $crate::events::ui::modifier as m;
        let i = &$init;
        [
            (i.ctrl_key, m::CONTROL),
            (i.shift_key, m::SHIFT),
            (i.alt_key, m::ALT),
            (i.meta_key, m::META),
            (i.modifier_alt_graph, m::ALT_GRAPH),
            (i.modifier_caps_lock, m::CAPS_LOCK),
            (i.modifier_fn, m::FN),
            (i.modifier_fn_lock, m::FN_LOCK),
            (i.modifier_hyper, m::HYPER),
            (i.modifier_num_lock, m::NUM_LOCK),
            (i.modifier_scroll_lock, m::SCROLL_LOCK),
            (i.modifier_super, m::SUPER),
            (i.modifier_symbol, m::SYMBOL),
            (i.modifier_symbol_lock, m::SYMBOL_LOCK),
        ]
        .iter()
        .fold(0_u16, |bits, (on, bit)| if *on { bits | bit } else { bits })
    }};
}
pub(crate) use modifiers_of;

/// The state of an event of the UI family: the members of all its
/// interfaces; the family decides which of them an event shows.
#[derive(Debug, Default)]
pub(crate) struct Fields {
    pub(crate) family: Family,
    /// `UIEvent.view`: the window or none.
    pub(crate) view: Option<Gc<Object>>,
    pub(crate) detail: i32,
    /// The modifier keys ([`modifier`]).
    pub(crate) mods: u16,
    /// `MouseEvent` and its subclasses.
    pub(crate) mouse: super::mouse::Mouse,
    /// `KeyboardEvent`.
    pub(crate) key: String,
    pub(crate) code: String,
    pub(crate) location: u32,
    pub(crate) repeat: bool,
    pub(crate) is_composing: bool,
    pub(crate) char_code: i32,
    pub(crate) key_code: i32,
    /// `InputEvent.data`, `CompositionEvent.data`, `TextEvent.data`.
    pub(crate) data: Option<String>,
    /// `InputEvent.inputType`.
    pub(crate) input_type: String,
    /// `FocusEvent.relatedTarget`.
    pub(crate) related_target: Option<Gc<Object>>,
}

impl Fields {
    /// Empty fields of `family`.
    pub(crate) fn new(family: Family) -> Fields {
        Fields {
            family,
            ..Fields::default()
        }
    }

    /// `UIEvent.which` of the family.
    pub(crate) fn which(&self) -> u32 {
        match self.family {
            Family::Keyboard => u32::try_from(self.key_code).unwrap_or(0),
            f if f.is_mouse() => (i32::from(self.mouse.button).wrapping_add(1)) as u32,
            _ => 0,
        }
    }

    /// `getModifierState`.
    pub(crate) fn modifier_state(&self, name: &str) -> bool {
        let bit = modifier::bit(name);
        bit != 0 && self.mods & bit != 0
    }

    /// The objects that the collector must see.
    pub(crate) fn trace(&self, tracer: &mut swb_js::Tracer<'_>) {
        for object in [self.view, self.related_target, self.mouse.related_target]
            .into_iter()
            .flatten()
        {
            tracer.object(object);
        }
        for object in self
            .mouse
            .pointer
            .coalesced
            .iter()
            .chain(&self.mouse.pointer.predicted)
        {
            tracer.object(*object);
        }
    }

    /// The bytes that the fields own.
    pub(crate) fn heap_size(&self) -> usize {
        size_of::<Fields>()
            + self.key.capacity()
            + self.code.capacity()
            + self.data.as_ref().map_or(0, String::capacity)
            + self.input_type.capacity()
    }
}

/// The fields of the event `this`, or `Illegal invocation`.
pub(crate) fn fields<'a>(cx: &'a Cx<'_>, this: Platform) -> b::Result<&'a Fields> {
    match &this_data(cx, this)?.extra {
        Extra::Ui(fields) => Ok(fields),
        _ => Err(swb_js::VmError::type_error("Illegal invocation").into()),
    }
}

/// The fields of the event `this`, for changes.
pub(crate) fn fields_mut<'a>(cx: &'a mut Cx<'_>, this: Platform) -> b::Result<&'a mut Fields> {
    match &mut this_data_mut(cx, this)?.extra {
        Extra::Ui(fields) => Ok(fields),
        _ => Err(swb_js::VmError::type_error("Illegal invocation").into()),
    }
}

/// A new event of the UI family for a constructor.
pub(crate) fn construct_ui(
    cx: &mut Cx<'_>,
    kind: String,
    flags: (bool, bool, bool),
    fields: Fields,
) -> b::Result<Gc<Object>> {
    construct(
        cx,
        kind,
        &base(flags.0, flags.1, flags.2),
        Extra::Ui(Box::new(fields)),
    )
}

/// The event data that `init*` changes: DOM "initialize" (ignored while
/// the event is dispatched) with the fields; false if the event is being
/// dispatched.
pub(crate) fn init_event(
    cx: &mut Cx<'_>,
    this: Platform,
    kind: String,
    bubbles: bool,
    cancelable: bool,
) -> b::Result<bool> {
    let event: &mut EventData = this_data_mut(cx, this)?;
    if event.dispatching {
        return Ok(false);
    }
    event.initialize(kind, bubbles, cancelable);
    Ok(true)
}

/// The window that a `view` argument or member holds.
pub(crate) fn view_of(view: Option<Platform>) -> Option<Gc<Object>> {
    view.map(|p| p.object)
}

/// The fields of the `UIEventInit` part of any dictionary of the family.
macro_rules! ui_part {
    ($family:expr, $init:expr) => {{
        let init = &$init;
        let mut f = $crate::events::ui::Fields::new($family);
        f.view = $crate::events::ui::view_of(init.view);
        f.detail = init.detail;
        f.mods = $crate::events::ui::modifiers_of!(init);
        f
    }};
}
pub(crate) use ui_part;

/// Getters of the fields: `name: type = |f| expression;`.
macro_rules! field_getters {
    ($($name:ident: $ty:ty = |$f:ident| $e:expr;)*) => {
        $(
            fn $name(cx: &mut Cx<'_>, this: Platform) -> b::Result<$ty> {
                let $f = $crate::events::ui::fields(cx, this)?;
                Ok($e)
            }
        )*
    };
}
pub(crate) use field_getters;

impl g::ui_event::UIEventImpl for WebState {
    fn constructor(cx: &mut Cx<'_>, kind: String, init: UIEventInit) -> b::Result<Gc<Object>> {
        let mut f = Fields::new(Family::Ui);
        f.view = view_of(init.view);
        f.detail = init.detail;
        construct_ui(cx, kind, (init.bubbles, init.cancelable, init.composed), f)
    }

    field_getters! {
        view: Option<Gc<Object>> = |f| f.view;
        detail: i32 = |f| f.detail;
        source_capabilities: Option<Gc<Object>> = |_f| None;
        which: u32 = |f| f.which();
    }

    fn init_ui_event(
        cx: &mut Cx<'_>,
        this: Platform,
        kind: String,
        bubbles: bool,
        cancelable: bool,
        view: Option<Platform>,
        detail: i32,
    ) -> b::Result<()> {
        if init_event(cx, this, kind, bubbles, cancelable)? {
            let f = fields_mut(cx, this)?;
            f.view = view_of(view);
            f.detail = detail;
        }
        Ok(())
    }
}

impl g::focus_event::FocusEventImpl for WebState {
    fn constructor(cx: &mut Cx<'_>, kind: String, init: FocusEventInit) -> b::Result<Gc<Object>> {
        let mut f = Fields::new(Family::Focus);
        f.view = view_of(init.view);
        f.detail = init.detail;
        f.related_target = init.related_target.map(|p| p.object);
        construct_ui(cx, kind, (init.bubbles, init.cancelable, init.composed), f)
    }

    field_getters! {
        related_target: Option<Gc<Object>> = |f| f.related_target;
    }
}

impl g::input_event::InputEventImpl for WebState {
    fn constructor(cx: &mut Cx<'_>, kind: String, init: InputEventInit) -> b::Result<Gc<Object>> {
        let mut f = Fields::new(Family::Input);
        f.view = view_of(init.view);
        f.detail = init.detail;
        f.data = init.data;
        f.is_composing = init.is_composing;
        f.input_type = init.input_type;
        construct_ui(cx, kind, (init.bubbles, init.cancelable, init.composed), f)
    }

    field_getters! {
        data: Option<String> = |f| f.data.clone();
        is_composing: bool = |f| f.is_composing;
        input_type: String = |f| f.input_type.clone();
        data_transfer: Option<Gc<Object>> = |_f| None;
        get_target_ranges: Vec<Gc<Object>> = |_f| Vec::new();
    }
}

impl g::composition_event::CompositionEventImpl for WebState {
    fn constructor(
        cx: &mut Cx<'_>,
        kind: String,
        init: CompositionEventInit,
    ) -> b::Result<Gc<Object>> {
        let mut f = Fields::new(Family::Composition);
        f.view = view_of(init.view);
        f.detail = init.detail;
        f.data = Some(init.data);
        construct_ui(cx, kind, (init.bubbles, init.cancelable, init.composed), f)
    }

    field_getters! {
        data: String = |f| f.data.clone().unwrap_or_default();
    }

    fn init_composition_event(
        cx: &mut Cx<'_>,
        this: Platform,
        kind: String,
        bubbles: bool,
        cancelable: bool,
        view: Option<Platform>,
        data: String,
    ) -> b::Result<()> {
        if init_event(cx, this, kind, bubbles, cancelable)? {
            let f = fields_mut(cx, this)?;
            f.view = view_of(view);
            f.data = Some(data);
        }
        Ok(())
    }
}

impl g::text_event::TextEventImpl for WebState {
    field_getters! {
        data: String = |f| f.data.clone().unwrap_or_default();
    }

    fn init_text_event(
        cx: &mut Cx<'_>,
        this: Platform,
        kind: String,
        bubbles: bool,
        cancelable: bool,
        view: Option<Platform>,
        data: String,
    ) -> b::Result<()> {
        if init_event(cx, this, kind, bubbles, cancelable)? {
            let f = fields_mut(cx, this)?;
            f.view = view_of(view);
            f.data = Some(data);
        }
        Ok(())
    }
}

impl g::keyboard_event::KeyboardEventImpl for WebState {
    fn constructor(
        cx: &mut Cx<'_>,
        kind: String,
        init: KeyboardEventInit,
    ) -> b::Result<Gc<Object>> {
        let mut f = ui_part!(Family::Keyboard, init);
        f.key = init.key;
        f.code = init.code;
        f.location = init.location;
        f.repeat = init.repeat;
        f.is_composing = init.is_composing;
        f.char_code = init.char_code;
        f.key_code = init.key_code;
        construct_ui(cx, kind, (init.bubbles, init.cancelable, init.composed), f)
    }

    field_getters! {
        key: String = |f| f.key.clone();
        code: String = |f| f.code.clone();
        location: u32 = |f| f.location;
        ctrl_key: bool = |f| f.mods & modifier::CONTROL != 0;
        shift_key: bool = |f| f.mods & modifier::SHIFT != 0;
        alt_key: bool = |f| f.mods & modifier::ALT != 0;
        meta_key: bool = |f| f.mods & modifier::META != 0;
        repeat: bool = |f| f.repeat;
        is_composing: bool = |f| f.is_composing;
        char_code: i32 = |f| f.char_code;
        key_code: i32 = |f| f.key_code;
    }

    fn get_modifier_state(cx: &mut Cx<'_>, this: Platform, key: String) -> b::Result<bool> {
        Ok(fields(cx, this)?.modifier_state(&key))
    }

    /// `initKeyboardEvent`: the fifth argument is Chromium's old
    /// `keyIdentifier` and sets nothing; `key`, `code`, `keyCode` and
    /// `charCode` stay as they are.
    fn init_keyboard_event(
        cx: &mut Cx<'_>,
        this: Platform,
        kind: String,
        bubbles: bool,
        cancelable: bool,
        view: Option<Platform>,
        _key: String,
        location: u32,
        ctrl: bool,
        alt: bool,
        shift: bool,
        meta: bool,
    ) -> b::Result<()> {
        if init_event(cx, this, kind, bubbles, cancelable)? {
            let f = fields_mut(cx, this)?;
            f.view = view_of(view);
            f.location = location;
            for (on, bit) in [
                (ctrl, modifier::CONTROL),
                (alt, modifier::ALT),
                (shift, modifier::SHIFT),
                (meta, modifier::META),
            ] {
                f.mods = if on { f.mods | bit } else { f.mods & !bit };
            }
        }
        Ok(())
    }
}

/// An uninitialized event of the UI family for `createEvent`.
pub(crate) fn blank(family: Family) -> Extra {
    Extra::Ui(Box::new(Fields::new(family)))
}
