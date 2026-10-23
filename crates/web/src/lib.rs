//! The Web API bindings of swb (ADR 0028 sections 1, 4 and 5).
//!
//! Feature 4b builds the parts that the generated glue needs:
//!
//! - `bindings`: the binding context ([`bindings::Cx`]), the host trait
//!   that the engine side implements ([`bindings::Host`]: node wrappers,
//!   content attributes for `[Reflect]`, the glue state), the conversions
//!   of Web IDL §3.2 in both directions, exceptions with Chromium's
//!   messages and `DOMException`, overload resolution, `[Reflect]`.
//! - `generated`: the glue that `swb-webidl` writes from the IDL files in
//!   `crates/web/idl/` (`just bindings`); checked in. Each interface has a
//!   trait with one function per member, which the hand-written behaviour
//!   implements; the glue does the brand checks, the argument conversions,
//!   the calls and the result conversions.
//!
//! The behaviour of the interfaces (`Node`, `EventTarget`, `Event`) comes
//! with features 4c and 5a; this crate has the behaviour of `DOMException`.

pub mod bindings;
#[rustfmt::skip]
pub mod generated;
