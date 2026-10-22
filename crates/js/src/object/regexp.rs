//! The data of `RegExp` objects and `RegExp` String Iterators (ECMA-262
//! §22.2.3.1 `RegExpAlloc`, §22.2.9; the built-ins are in
//! `builtins/regexp*.rs`).
//!
//! The compiled program (`swb_js_regexp::Regex`) is shared through an
//! `Rc`: all objects that one literal makes, and the literal's constant,
//! hold the same program. Each holder counts its share of the program's
//! bytes in the heap ([`RegExpData::heap_size`]), so the sum over the
//! holders is the size of the program.

use std::rc::Rc;

use swb_js_regexp::Regex;

use crate::error::{Error, Result};
use crate::heap::{Arena, Gc, Heap, Tracer};
use crate::object::{Elements, Object, ObjectKind, Shape, ShapeKind};
use crate::string::JsString;
use crate::value::Value;

/// The internal slots of a `RegExp` object: `[[OriginalSource]]` and
/// `[[RegExpMatcher]]` (the program, which also knows `[[OriginalFlags]]`
/// as flags and `[[RegExpRecord]]`). The slots `[[Realm]]` and
/// `[[LegacyFeaturesEnabled]]` of the legacy features proposal are left
/// out: V8 checks neither (measured), so every match updates the static
/// properties and every object accepts `compile`.
#[derive(Debug)]
pub struct RegExpData {
    /// `[[OriginalSource]]`: the pattern text.
    pub(crate) source: Gc<JsString>,
    /// The compiled pattern. `[[OriginalFlags]]` are its flags: they are
    /// only read through their letters, so their order in the source text
    /// cannot be observed.
    pub(crate) regex: Rc<Regex>,
}

impl RegExpData {
    pub(crate) fn trace(&self, tracer: &mut Tracer<'_>) {
        tracer.string(self.source);
    }

    /// The bytes that the object owns outside its record: the slots and
    /// its share of the program.
    pub(crate) fn heap_size(&self) -> usize {
        size_of::<RegExpData>() + program_share(&self.regex)
    }
}

/// The share of a holder of `regex` in the bytes of the program.
pub(crate) fn program_share(regex: &Rc<Regex>) -> usize {
    regex.heap_bytes() / Rc::strong_count(regex).max(1)
}

/// The state of a `RegExp` String Iterator (§22.2.9.1
/// `CreateRegExpStringIterator`): the closure's variables.
#[derive(Debug)]
pub struct RegExpStringIterator {
    /// The regular expression (`R`); `None` once the iterator is done.
    pub(crate) matcher: Option<Gc<Object>>,
    /// The string (`S`).
    pub(crate) string: Gc<JsString>,
    /// Whether the flags contained `g`.
    pub(crate) global: bool,
    /// Whether the flags contained `u` or `v`.
    pub(crate) full_unicode: bool,
}

impl RegExpStringIterator {
    pub(crate) fn trace(&self, tracer: &mut Tracer<'_>) {
        if let Some(matcher) = self.matcher {
            tracer.object(matcher);
        }
        tracer.string(self.string);
    }
}

impl Heap {
    /// The shape of `object` if it is shared (the shape of objects that
    /// were made with the same keys in the same order).
    pub(crate) fn shared_shape(&self, object: Gc<Object>) -> Result<Option<Gc<Shape>>> {
        let shape = self.object(object)?.shape;
        Ok(match self.arenas.shapes.get(shape)?.kind {
            ShapeKind::Shared(_) => Some(shape),
            ShapeKind::Dictionary(_) => None,
        })
    }

    /// A new object with a shared `shape` and the values of its named
    /// properties (`slots`, in the shape's order) and of its elements
    /// (dense, without holes), for objects whose shape the `RegExp`
    /// built-ins know: a new `RegExp` object and a match array. The
    /// shape's prototype must be alive. Never collects.
    pub(crate) fn new_object_with_shape(
        &mut self,
        shape: Gc<Shape>,
        slots: Vec<Value>,
        elements: Vec<Value>,
        kind: ObjectKind,
    ) -> Result<Gc<Object>> {
        let slot_count = match &self.arenas.shapes.get(shape)?.kind {
            ShapeKind::Shared(shared) => shared.slot_count as usize,
            ShapeKind::Dictionary(_) => usize::MAX,
        };
        if slot_count != slots.len() {
            return Err(Error::invariant("slots that do not fit the shape"));
        }
        let count = u32::try_from(elements.len())
            .map_err(|_| Error::invariant("too many elements for a new object"))?;
        let object = Object {
            shape,
            slots,
            elements: Elements::Dense {
                values: elements,
                count,
            },
            kind,
            extensible: true,
            watched: false,
        };
        self.charge(Arena::<Object>::slot_size() + object.heap_size() + object.kind.heap_size());
        self.arenas.objects.insert(object)
    }
}

/// The spans of the last successful match of a realm, for the legacy
/// static properties (`RegExp.$1` and the others). V8 keeps the subject
/// apart from `RegExp.input`, which a script can set.
#[derive(Clone, Copy, Debug)]
pub(crate) struct LastMatch {
    /// The matched string.
    pub(crate) subject: Gc<JsString>,
    /// The whole match.
    pub(crate) whole: (u32, u32),
    /// Groups 1 to 9.
    pub(crate) groups: [Option<(u32, u32)>; 9],
    /// The last group (`lastParen`); `None` without groups or when it did
    /// not take part.
    pub(crate) last: Option<(u32, u32)>,
}

/// The shapes and values that the fast paths of the `RegExp` methods
/// check (`builtins/regexp_fast.rs`): the shape of a new `RegExp` object,
/// and the shapes and method slots of `%RegExp.prototype%` and `%RegExp%`
/// as the realm made them.
#[derive(Debug)]
pub(crate) struct RegExpShapes {
    /// The shape of a `RegExp` object with only its `lastIndex` (whose
    /// value is slot 0).
    pub(crate) instance: Gc<Shape>,
    /// The shape of `%RegExp.prototype%`.
    pub(crate) prototype: Gc<Shape>,
    /// The slots of the methods and getters of `%RegExp.prototype%` that
    /// the algorithms read, with the built-in functions (`exec` first).
    pub(crate) prototype_slots: Box<[(u32, Gc<Object>)]>,
    /// The shape of `%RegExp%`.
    pub(crate) constructor: Gc<Shape>,
    /// The slot of the `@@species` getter of `%RegExp%`, with the built-in
    /// getter.
    pub(crate) species_slot: (u32, Gc<Object>),
}

/// The state of the `RegExp` built-ins in a realm.
#[derive(Debug, Default)]
pub(crate) struct RegExpRealm {
    /// The last successful match (none before the first).
    pub(crate) last_match: Option<LastMatch>,
    /// The value of `RegExp.input` (`None`: the empty string).
    pub(crate) input: Option<Gc<JsString>>,
    /// The checks of the fast paths, once the built-ins are installed.
    pub(crate) shapes: Option<RegExpShapes>,
    /// The shapes of a match array without and with `indices`, and of
    /// the `indices` array, once the first one was made.
    pub(crate) result_shapes: [Option<Gc<Shape>>; 3],
}

impl RegExpRealm {
    pub(crate) fn trace(&self, tracer: &mut Tracer<'_>) {
        if let Some(last) = &self.last_match {
            tracer.string(last.subject);
        }
        if let Some(input) = self.input {
            tracer.string(input);
        }
        if let Some(shapes) = &self.shapes {
            tracer.shape(shapes.instance);
            tracer.shape(shapes.prototype);
            tracer.shape(shapes.constructor);
            for &(_, function) in &shapes.prototype_slots {
                tracer.object(function);
            }
            tracer.object(shapes.species_slot.1);
        }
        for shape in self.result_shapes.iter().flatten() {
            tracer.shape(*shape);
        }
    }
}
