//! The visibility test of the frame loop (ADR 0044 section 8; M13 feature
//! 3c): which painted area an animation can change, by the properties of
//! its keyframes ([`Reach`]). The engine finds the bounds of that area
//! after each display list and tells [`DocumentAnimations`] which owners
//! are visible ([`DocumentAnimations::set_visible`]); an animation of an
//! owner that is not visible needs no painted frames, only its event
//! times.
//!
//! The rule never misses a frame of an element that becomes visible during
//! an animation:
//!
//! - an animation of properties that paint inside the box and change no
//!   geometry (opacity, colors, the position and size of backgrounds,
//!   `visibility`) can change only the pixels of its element's subtree
//!   ([`Reach::Subtree`]): the painted bounds of the boxes of the subtree,
//!   with transforms, shadows, outlines and filter outsets;
//! - an animation of properties that move or grow the painted area without
//!   a change of layout (the transforms, filters, shadows, `clip-path`,
//!   outlines) can paint anywhere inside the nearest ancestor that clips
//!   it ([`Reach::Clip`]; without one it counts as visible);
//! - an animation of any other property (sizes, margins, offsets, fonts,
//!   custom properties, which can feed any of them) can move other boxes,
//!   so it counts as visible ([`Reach::Everywhere`]).
//!
//! [`DocumentAnimations`]: super::DocumentAnimations
//! [`DocumentAnimations::set_visible`]: super::DocumentAnimations::set_visible

use super::keyframes::KeyframeSet;

/// The painted area that an animation can change.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Reach {
    /// The painted bounds of the owning element's subtree.
    #[default]
    Subtree,
    /// The area of the nearest ancestor that clips the owning element.
    Clip,
    /// Anywhere: the animation counts as visible.
    Everywhere,
}

/// Properties that paint inside the boxes and change no geometry.
const SUBTREE: &[&str] = &[
    "opacity",
    "color",
    "background-color",
    "border-top-color",
    "border-right-color",
    "border-bottom-color",
    "border-left-color",
    "text-decoration-color",
    "fill",
    "stroke",
    "-webkit-text-stroke-color",
    "-webkit-text-fill-color",
    "visibility",
    "background-position-x",
    "background-position-y",
    "background-size",
];

/// Properties that move or grow the painted area without a layout change.
const CLIP: &[&str] = &[
    "transform",
    "translate",
    "rotate",
    "scale",
    "transform-origin",
    "filter",
    "backdrop-filter",
    "box-shadow",
    "text-shadow",
    "clip-path",
    "outline-color",
    "outline-width",
    "outline-offset",
];

/// The reach of a property (by its name).
fn reach_of_property(name: &str) -> Reach {
    if SUBTREE.contains(&name) {
        Reach::Subtree
    } else if CLIP.contains(&name) {
        Reach::Clip
    } else {
        Reach::Everywhere
    }
}

/// The reach of an animation with the keyframes `set`: the widest of its
/// properties.
pub(crate) fn reach_of(set: &KeyframeSet) -> Reach {
    set.properties
        .iter()
        .map(|p| reach_of_property(&p.name))
        .max()
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_reach_of_properties() {
        assert_eq!(reach_of_property("opacity"), Reach::Subtree);
        assert_eq!(reach_of_property("background-position-x"), Reach::Subtree);
        assert_eq!(reach_of_property("transform"), Reach::Clip);
        assert_eq!(reach_of_property("translate"), Reach::Clip);
        assert_eq!(reach_of_property("filter"), Reach::Clip);
        assert_eq!(reach_of_property("width"), Reach::Everywhere);
        assert_eq!(reach_of_property("left"), Reach::Everywhere);
        assert_eq!(reach_of_property("--x"), Reach::Everywhere);
        assert_eq!(reach_of_property("display"), Reach::Everywhere);
        assert_eq!(reach_of(&KeyframeSet::default()), Reach::Subtree);
    }
}
