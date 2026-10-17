//! The legacy static properties of `RegExp` (`RegExp.$1` to `$9`,
//! `input` and `$_`, `lastMatch` and `$&`, `lastParen` and `$+`,
//! `leftContext` and `` $` ``, `rightContext` and `$'`), as V8 has them
//! (measured in Chromium 148; the legacy features proposal,
//! <https://github.com/tc39/proposal-regexp-legacy-features>, describes
//! a stricter form).
//!
//! V8's behaviour: every property is an accessor with a getter and a
//! setter, not enumerable, configurable, each with its own functions
//! (`get $1`, `set $1`, `get $_`). The getters accept any receiver.
//! The values come from the last successful match in the realm (of any
//! `RegExp` object, also of subclasses); before the first match they are
//! empty strings. Only `input` and `$_` can be set (to `ToString` of the
//! value); the other setters do nothing. `input` is also replaced by
//! every match; the other values keep the subject of the match, also
//! after `input` was set.

use crate::heap::Gc;
use crate::object::{LastMatch, Object, PropertyDescriptor};
use crate::runtime::Runtime;
use crate::value::Value;
use crate::vm::{NativeCall, NativeFn, NativeReturn, VmResult};

/// A static property: its names and its getter (the setter of `input`
/// and `$_` sets the input; the others do nothing).
struct Static {
    names: &'static [&'static str],
    get: NativeFn,
    settable: bool,
}

/// The properties in V8's key order.
const STATICS: [Static; 14] = [
    Static {
        names: &["input", "$_"],
        get: get_input,
        settable: true,
    },
    Static {
        names: &["lastMatch", "$&"],
        get: get_last_match,
        settable: false,
    },
    Static {
        names: &["lastParen", "$+"],
        get: get_last_paren,
        settable: false,
    },
    Static {
        names: &["leftContext", "$`"],
        get: get_left_context,
        settable: false,
    },
    Static {
        names: &["rightContext", "$'"],
        get: get_right_context,
        settable: false,
    },
    Static {
        names: &["$1"],
        get: get_1,
        settable: false,
    },
    Static {
        names: &["$2"],
        get: get_2,
        settable: false,
    },
    Static {
        names: &["$3"],
        get: get_3,
        settable: false,
    },
    Static {
        names: &["$4"],
        get: get_4,
        settable: false,
    },
    Static {
        names: &["$5"],
        get: get_5,
        settable: false,
    },
    Static {
        names: &["$6"],
        get: get_6,
        settable: false,
    },
    Static {
        names: &["$7"],
        get: get_7,
        settable: false,
    },
    Static {
        names: &["$8"],
        get: get_8,
        settable: false,
    },
    Static {
        names: &["$9"],
        get: get_9,
        settable: false,
    },
];

pub(super) fn install(rt: &mut Runtime, realm: u32, ctor: Gc<Object>) -> VmResult<()> {
    for property in &STATICS {
        for name in property.names {
            let get = rt.new_builtin(realm, &format!("get {name}"), 0, property.get, false)?;
            let setter: NativeFn = if property.settable {
                set_input
            } else {
                set_nothing
            };
            let set = rt.new_builtin(realm, &format!("set {name}"), 1, setter, false)?;
            let key = rt.heap.key_from_str(name)?;
            let desc = PropertyDescriptor::accessor(get.into(), set.into(), false, true);
            rt.define_own_property(ctor, key, desc)?;
        }
    }
    Ok(())
}

impl Runtime {
    /// The last match of the current realm.
    fn last_match(&self) -> Option<LastMatch> {
        self.vm
            .realms
            .get(self.current_realm() as usize)?
            .regexp
            .last_match
    }

    /// A part of the subject of the last match, or the empty string.
    fn last_match_part(
        &mut self,
        part: impl FnOnce(&LastMatch, usize) -> Option<(u32, u32)>,
    ) -> VmResult<NativeReturn> {
        let Some(last) = self.last_match() else {
            return Ok(NativeReturn::Value(self.vm.atoms.empty.into()));
        };
        let length = self.string_len(last.subject)?;
        let Some((start, end)) = part(&last, length) else {
            return Ok(NativeReturn::Value(self.vm.atoms.empty.into()));
        };
        let piece = self.substring(last.subject, start as usize, end as usize)?;
        Ok(NativeReturn::Value(piece.into()))
    }
}

#[allow(
    clippy::unnecessary_wraps,
    reason = "the signature of native functions"
)]
fn get_input(rt: &mut Runtime, _: &NativeCall) -> VmResult<NativeReturn> {
    let input = rt
        .vm
        .realms
        .get(rt.current_realm() as usize)
        .and_then(|realm| realm.regexp.input)
        .unwrap_or(rt.vm.atoms.empty);
    Ok(NativeReturn::Value(input.into()))
}

fn set_input(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let value = rt.arg(call, 0);
    let text = rt.to_string(value)?;
    let realm = rt.current_realm();
    if let Some(realm) = rt.vm.realms.get_mut(realm as usize) {
        realm.regexp.input = Some(text);
    }
    Ok(NativeReturn::Value(Value::Undefined))
}

#[allow(
    clippy::unnecessary_wraps,
    reason = "the signature of native functions"
)]
fn set_nothing(_: &mut Runtime, _: &NativeCall) -> VmResult<NativeReturn> {
    Ok(NativeReturn::Value(Value::Undefined))
}

fn get_last_match(rt: &mut Runtime, _: &NativeCall) -> VmResult<NativeReturn> {
    rt.last_match_part(|last, _| Some(last.whole))
}

fn get_last_paren(rt: &mut Runtime, _: &NativeCall) -> VmResult<NativeReturn> {
    rt.last_match_part(|last, _| last.last)
}

fn get_left_context(rt: &mut Runtime, _: &NativeCall) -> VmResult<NativeReturn> {
    rt.last_match_part(|last, _| Some((0, last.whole.0)))
}

fn get_right_context(rt: &mut Runtime, _: &NativeCall) -> VmResult<NativeReturn> {
    rt.last_match_part(|last, length| Some((last.whole.1, length as u32)))
}

/// `RegExp.$n`.
fn group(rt: &mut Runtime, n: usize) -> VmResult<NativeReturn> {
    rt.last_match_part(|last, _| last.groups.get(n - 1).copied().flatten())
}

fn get_1(rt: &mut Runtime, _: &NativeCall) -> VmResult<NativeReturn> {
    group(rt, 1)
}

fn get_2(rt: &mut Runtime, _: &NativeCall) -> VmResult<NativeReturn> {
    group(rt, 2)
}

fn get_3(rt: &mut Runtime, _: &NativeCall) -> VmResult<NativeReturn> {
    group(rt, 3)
}

fn get_4(rt: &mut Runtime, _: &NativeCall) -> VmResult<NativeReturn> {
    group(rt, 4)
}

fn get_5(rt: &mut Runtime, _: &NativeCall) -> VmResult<NativeReturn> {
    group(rt, 5)
}

fn get_6(rt: &mut Runtime, _: &NativeCall) -> VmResult<NativeReturn> {
    group(rt, 6)
}

fn get_7(rt: &mut Runtime, _: &NativeCall) -> VmResult<NativeReturn> {
    group(rt, 7)
}

fn get_8(rt: &mut Runtime, _: &NativeCall) -> VmResult<NativeReturn> {
    group(rt, 8)
}

fn get_9(rt: &mut Runtime, _: &NativeCall) -> VmResult<NativeReturn> {
    group(rt, 9)
}
