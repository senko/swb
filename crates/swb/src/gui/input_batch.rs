//! The input of one batch of window events (ADR 0040 section 1): the last
//! pointer move, which the shell applies once, and the wheel deltas, which
//! it applies at the end of the batch. This type holds the rules for their
//! order and needs no window, so it has unit tests.
//!
//! A pointer move waits until something depends on it: an event whose meaning
//! depends on the pointer position or on the order of input (a press or a
//! release of a button, a wheel event, a key, a leave), a request of the
//! shell that reads the page's pointer state, or the end of the batch. The
//! shell's own record of the pointer position is not part of this type: it is
//! updated at once for every move.

use winit::dpi::PhysicalPosition;

/// An input event, as far as the order rules care.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Input {
    /// The pointer moved to a position (device pixels of the window).
    Move(PhysicalPosition<f64>),
    /// The pointer left the window.
    Leave,
    /// A mouse button went down or up.
    Button,
    /// A wheel event.
    Wheel,
    /// A key went down or up.
    Key,
}

/// The pending input of the current batch.
#[derive(Debug, Default)]
pub(crate) struct InputBatch {
    /// The last pointer move that the page has not seen.
    pending_move: Option<PhysicalPosition<f64>>,
    /// The wheel delta (CSS px) since the last application: the wheel
    /// events of one batch scroll once, so a scroll container's display
    /// list is built at most once per batch. The fraction of a pixel stays
    /// here (scroll offsets are whole pixels, as in Chromium).
    pending_wheel: (f32, f32),
}

impl InputBatch {
    /// Notes an input event. Returns the pending move that must be applied
    /// before the event is handled: a move is only noted (later moves
    /// replace it); every other event takes it.
    pub(crate) fn push(&mut self, input: Input) -> Option<PhysicalPosition<f64>> {
        match input {
            Input::Move(position) => {
                self.pending_move = Some(position);
                None
            }
            Input::Leave | Input::Button | Input::Wheel | Input::Key => self.take_move(),
        }
    }

    /// Takes the pending move: the end of the batch, or work of the shell
    /// that reads the page's pointer state.
    pub(crate) fn take_move(&mut self) -> Option<PhysicalPosition<f64>> {
        self.pending_move.take()
    }

    /// Adds a wheel delta (CSS px). A delta that is not finite is dropped.
    pub(crate) fn add_wheel(&mut self, dx: f32, dy: f32) {
        if dx.is_finite() && dy.is_finite() {
            self.pending_wheel.0 += dx;
            self.pending_wheel.1 += dy;
        }
    }

    /// Takes the whole pixels of the wheel delta; the fraction stays for
    /// the next call.
    pub(crate) fn take_wheel(&mut self) -> (f32, f32) {
        let (whole, rest) = split_wheel(self.pending_wheel);
        self.pending_wheel = rest;
        whole
    }
}

/// Splits a wheel delta (CSS px) into the whole pixels to scroll now and
/// the fraction to keep for later, on each axis (toward zero, so that
/// small deltas in either direction add up). A component that is not
/// finite (a sum that overflowed) counts as 0.
pub(crate) fn split_wheel((dx, dy): (f32, f32)) -> ((f32, f32), (f32, f32)) {
    let finite = |v: f32| if v.is_finite() { v } else { 0.0 };
    let (dx, dy) = (finite(dx), finite(dy));
    let whole = (dx.trunc(), dy.trunc());
    (whole, (dx - whole.0, dy - whole.1))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(x: f64, y: f64) -> Input {
        Input::Move(PhysicalPosition::new(x, y))
    }

    #[allow(clippy::unnecessary_wraps)] // Compared with `Option` results.
    fn pos(x: f64, y: f64) -> Option<PhysicalPosition<f64>> {
        Some(PhysicalPosition::new(x, y))
    }

    #[test]
    fn moves_are_coalesced_to_the_last_one() {
        let mut batch = InputBatch::default();
        for i in 0..100 {
            assert_eq!(batch.push(at(f64::from(i), 5.0)), None);
        }
        assert_eq!(batch.take_move(), pos(99.0, 5.0));
        assert_eq!(batch.take_move(), None);
    }

    #[test]
    fn a_press_after_moves_sees_the_last_move() {
        let mut batch = InputBatch::default();
        batch.push(at(1.0, 1.0));
        batch.push(at(2.0, 2.0));
        assert_eq!(batch.push(Input::Button), pos(2.0, 2.0));
        // The press has taken it: the release has nothing to apply.
        assert_eq!(batch.push(Input::Button), None);
    }

    #[test]
    fn a_drag_applies_the_moves_before_the_release() {
        let mut batch = InputBatch::default();
        assert_eq!(batch.push(Input::Button), None);
        batch.push(at(10.0, 10.0));
        batch.push(at(20.0, 30.0));
        assert_eq!(batch.push(Input::Button), pos(20.0, 30.0));
    }

    #[test]
    fn a_leave_after_a_move_sees_the_move_first() {
        let mut batch = InputBatch::default();
        batch.push(at(3.0, 4.0));
        assert_eq!(batch.push(Input::Leave), pos(3.0, 4.0));
        assert_eq!(batch.take_move(), None);
    }

    #[test]
    fn keys_and_wheel_events_take_the_pending_move() {
        let mut batch = InputBatch::default();
        batch.push(at(1.0, 2.0));
        assert_eq!(batch.push(Input::Key), pos(1.0, 2.0));
        batch.push(at(5.0, 6.0));
        assert_eq!(batch.push(Input::Wheel), pos(5.0, 6.0));
        // Moves after the wheel event wait for the end of the batch; the
        // wheel then scrolls at the shell's latest position.
        batch.push(at(7.0, 8.0));
        batch.add_wheel(0.0, 48.0);
        assert_eq!(batch.take_move(), pos(7.0, 8.0));
        assert_eq!(batch.take_wheel(), (0.0, 48.0));
    }

    #[test]
    fn wheel_deltas_add_up_and_keep_the_fraction() {
        let mut batch = InputBatch::default();
        batch.add_wheel(0.0, 20.5);
        batch.add_wheel(0.0, 20.5);
        assert_eq!(batch.take_wheel(), (0.0, 41.0));
        batch.add_wheel(0.0, -0.5);
        assert_eq!(batch.take_wheel(), (0.0, 0.0));
        batch.add_wheel(0.0, -0.5);
        assert_eq!(batch.take_wheel(), (0.0, -1.0));
        batch.add_wheel(f32::NAN, 5.0);
        assert_eq!(batch.take_wheel(), (0.0, 0.0));
    }

    #[test]
    fn wheel_deltas_scroll_whole_pixels_and_keep_the_fraction() {
        assert_eq!(split_wheel((48.0, -2.75)), ((48.0, -2.0), (0.0, -0.75)));
        // Half-pixel deltas (a touchpad at scale 2) add up in both
        // directions.
        let (whole, rest) = split_wheel((0.0, -0.5));
        assert_eq!((whole, rest), ((0.0, 0.0), (0.0, -0.5)));
        let (whole, _) = split_wheel((rest.0, rest.1 - 0.5));
        assert_eq!(whole, (0.0, -1.0));
        // An overflowed sum is dropped, not kept as NaN.
        assert_eq!(split_wheel((f32::INFINITY, 3.5)), ((0.0, 3.0), (0.0, 0.5)));
    }
}
