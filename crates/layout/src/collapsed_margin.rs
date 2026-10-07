// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Collapsing margins (CSS 2.2 §8.3.1,
//! <https://www.w3.org/TR/CSS22/box.html#collapsing-margins>).
//!
//! This file is derived from `CollapsedMargin` and `CollapsedBlockMargins`
//! in Servo's `components/layout/fragment_tree/fragment.rs`
//! (<https://github.com/servo/servo>, Copyright The Servo Project
//! Developers). It is licensed under the Mozilla Public License 2.0, not
//! under swb's MIT License; it is kept in its own file so that the MPL-2.0
//! covers only this file. See `THIRD_PARTY_NOTICES.md` and ADR 0021.

/// Collapsing margins: the largest positive and the most negative margin
/// (CSS 2.2 §8.3.1).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct CollapsedMargin {
    max_positive: f32,
    min_negative: f32,
}

impl CollapsedMargin {
    pub(crate) fn new(margin: f32) -> Self {
        CollapsedMargin {
            max_positive: margin.max(0.0),
            min_negative: margin.min(0.0),
        }
    }

    pub(crate) fn adjoin(self, other: CollapsedMargin) -> Self {
        CollapsedMargin {
            max_positive: self.max_positive.max(other.max_positive),
            min_negative: self.min_negative.min(other.min_negative),
        }
    }

    pub(crate) fn solve(self) -> f32 {
        self.max_positive + self.min_negative
    }
}

/// The margins a laid-out block-level box exposes to its parent for
/// collapsing.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct BlockMargins {
    /// The top margin, collapsed with any child margins it adjoins.
    pub(crate) start: CollapsedMargin,
    /// The bottom margin, collapsed with any child margins it adjoins.
    pub(crate) end: CollapsedMargin,
    /// True if the top and bottom margins adjoin (an empty box).
    pub(crate) collapsed_through: bool,
}
