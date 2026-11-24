//! The model of CSS animations (ADR 0044, M13 feature 3a): the
//! `animation-*` properties, the easing functions, the timing model of Web
//! Animations, the resolved `@keyframes` sets and the interpolation of
//! computed values.
//!
//! Feature 3c adds the events (`events.rs`), `cancel()`, `finish()` and
//! `startTime` (ADR 0044 sections 6 and 7), and the reach of an animation
//! for the visibility test of the frame loop (`visibility.rs`, section 8).
//!
//! Feature 3b runs the animations: [`DocumentAnimations`] holds the
//! records and the timeline of a document (`records.rs`), and the cascade
//! takes the animated values from them (`cascade.rs`, "the animation
//! input"; ADR 0044 sections 1 to 3). The API of the model:
//!
//! - [`AnimationStyle`]: the computed `animation-*` lists of an element
//!   ([`AnimationStyle::of`]).
//! - [`Timing`] and [`Timing::compute`]: the phase, the current iteration
//!   and the iteration progress of an effect at a local time.
//! - [`KeyframeSet`]: the keyframes of one `@keyframes` name, per property
//!   (`Stylist::keyframes`), and [`KeyframeSet::interval`], the two
//!   keyframes around an iteration progress and the eased distance between
//!   them.
//! - [`interpolate`]: the value of one longhand between two computed
//!   styles (the two keyframes, computed with the element's context), with
//!   the computed-value rules of the property applied.
//!
//! Specifications: [CSS Animations 1](https://drafts.csswg.org/css-animations-1/),
//! [CSS Animations 2](https://drafts.csswg.org/css-animations-2/),
//! [Web Animations 1](https://drafts.csswg.org/web-animations-1/),
//! [CSS Easing 2](https://drafts.csswg.org/css-easing-2/).

pub mod easing;
pub mod events;
pub mod interpolate;
pub mod keyframes;
mod list_support;
pub mod properties;
pub mod records;
pub mod time;
pub mod timing;
pub mod transition_properties;
pub mod visibility;

pub use easing::{Easing, LinearPoint, MAX_STEPS, StepPosition, parse_easing};
pub use events::{AnimationEventKind, PendingAnimationEvent};
pub use interpolate::{interpolate, interpolate_custom};
pub use keyframes::{Interval, Keyframe, KeyframeSet, PropertyKeyframes};
pub use properties::{AnimationEntry, AnimationName, AnimationStyle, Composition, PlayState};
pub use records::{
    AnimationId, AnimationOwner, AnimationPlayState, AnimationRecord, DocumentAnimations,
    MAX_RECORDS, RecordRoom, round_noise,
};
pub use time::{Number, Time};
pub use timing::{ComputedTiming, FillMode, Phase, PlaybackDirection, Timing};
pub use visibility::Reach;
