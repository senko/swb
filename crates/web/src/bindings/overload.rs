//! The selection step of the overload resolution algorithm (Web IDL §3.6,
//! <https://webidl.spec.whatwg.org/#es-overloads>, step 12): with more
//! than one overload of the same argument count, the value at the
//! distinguishing index picks the overload. The generator computes the
//! argument count and the distinguishing index; the glue describes the
//! type of each candidate at that index ([`Candidate`]) and calls
//! [`select`]. Buffer source and `bigint` types are not in the subset.

use swb_js::{ClassId, Runtime, Value};

use super::Result;
use super::convert::{is_platform_object, iterator_method};
use super::exception::Exception;

/// The type of a candidate overload at the distinguishing index, as the
/// tests of the algorithm see it (a union contributes its flattened
/// member types; a nullable type sets `nullable_or_dictionary`).
#[derive(Clone, Copy, Debug, Default)]
pub struct Candidate {
    /// The argument is optional.
    pub optional: bool,
    /// A nullable type or a dictionary type.
    pub nullable_or_dictionary: bool,
    /// The interface types.
    pub interfaces: &'static [ClassId],
    /// `object`.
    pub object: bool,
    /// A callback function type.
    pub callback_function: bool,
    /// A sequence type.
    pub sequence: bool,
    /// A callback interface, dictionary or record type.
    pub object_like: bool,
    /// `boolean`.
    pub boolean: bool,
    /// A numeric type.
    pub numeric: bool,
    /// A string type or an enumeration.
    pub string: bool,
    /// `any`.
    pub any: bool,
}

/// The index of the candidate that `value` selects, or Chromium's
/// `TypeError` ("Overload resolution failed.").
pub fn select(rt: &mut Runtime, value: Value, candidates: &[Candidate]) -> Result<usize> {
    let find = |test: &dyn Fn(&Candidate) -> bool| candidates.iter().position(test);
    if value.is_undefined()
        && let Some(i) = find(&|c| c.optional)
    {
        return Ok(i);
    }
    if value.is_nullish()
        && let Some(i) = find(&|c| c.nullable_or_dictionary)
    {
        return Ok(i);
    }
    if is_platform_object(rt, value)
        && let Some(i) = find(&|c| {
            c.object
                || c.interfaces
                    .iter()
                    .any(|&class| rt.implements(value, class))
        })
    {
        return Ok(i);
    }
    let is_object = matches!(value, Value::Object(_));
    if is_object
        && rt.callable(value)?
        && let Some(i) = find(&|c| c.callback_function || c.object)
    {
        return Ok(i);
    }
    if is_object
        && let Some(i) = find(&|c| c.sequence)
        && iterator_method(rt, value)?.is_some()
    {
        return Ok(i);
    }
    if is_object && let Some(i) = find(&|c| c.object_like || c.object) {
        return Ok(i);
    }
    if matches!(value, Value::Bool(_))
        && let Some(i) = find(&|c| c.boolean)
    {
        return Ok(i);
    }
    if matches!(value, Value::Int(_) | Value::Double(_))
        && let Some(i) = find(&|c| c.numeric)
    {
        return Ok(i);
    }
    for test in [
        &(|c: &Candidate| c.string) as &dyn Fn(&Candidate) -> bool,
        &|c| c.numeric,
        &|c| c.boolean,
        &|c| c.any,
    ] {
        if let Some(i) = find(test) {
            return Ok(i);
        }
    }
    Err(Exception::type_error("Overload resolution failed."))
}
