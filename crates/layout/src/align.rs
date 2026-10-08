//! Self-alignment helpers shared by flex, grid and positioned layout: `auto`
//! resolution and the margin-box edge that an item aligns to.

use swb_style::Alignment;

/// A self-alignment value (`align-self` or `justify-self`) with `auto`
/// resolved to the container's default (`align-items` or
/// `justify-items`).
pub(crate) fn resolve_self_alignment(own: Alignment, container_default: Alignment) -> Alignment {
    match own {
        Alignment::Auto => container_default,
        a => a,
    }
}

/// The edge of an item's margin box that is placed at an alignment point
/// in one axis. `Baseline` is only produced by grid layout; for offsets it
/// counts as `Start`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Edge {
    Start,
    Center,
    End,
    Baseline,
}

impl Edge {
    /// `start` plus the share of `size` before this edge: nothing for
    /// `Start`, half for `Center`, all for `End`.
    pub(crate) fn along(self, start: f32, size: f32) -> f32 {
        match self {
            Edge::Start | Edge::Baseline => start,
            Edge::Center => start + size / 2.0,
            Edge::End => start + size,
        }
    }
}
