//! Input event types that hosts (the GUI and the automation server) pass
//! to a page.

/// Keyboard modifiers held during an event.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Modifiers {
    /// Shift.
    pub shift: bool,
    /// Control.
    pub ctrl: bool,
    /// Alt.
    pub alt: bool,
    /// Meta (the "Windows" or "Command" key).
    pub meta: bool,
}

impl Modifiers {
    /// No modifiers.
    pub const NONE: Modifiers = Modifiers {
        shift: false,
        ctrl: false,
        alt: false,
        meta: false,
    };

    /// True if no modifier is held.
    pub fn is_empty(self) -> bool {
        self == Modifiers::NONE
    }
}

/// A mouse button.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseButton {
    /// The primary (usually left) button.
    Primary,
    /// The middle button or wheel.
    Middle,
    /// The secondary (usually right) button.
    Secondary,
    /// The "back" side button.
    Back,
    /// The "forward" side button.
    Forward,
}

/// A key. The names are the DOM `KeyboardEvent.key` values
/// (<https://www.w3.org/TR/uievents-key/>).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Key {
    /// A key that produces a character (`" "` for the space bar).
    Character(String),
    /// `Tab`.
    Tab,
    /// `Enter`.
    Enter,
    /// `Escape`.
    Escape,
    /// `Backspace`.
    Backspace,
    /// `Delete`.
    Delete,
    /// `ArrowUp`.
    ArrowUp,
    /// `ArrowDown`.
    ArrowDown,
    /// `ArrowLeft`.
    ArrowLeft,
    /// `ArrowRight`.
    ArrowRight,
    /// `PageUp`.
    PageUp,
    /// `PageDown`.
    PageDown,
    /// `Home`.
    Home,
    /// `End`.
    End,
}

/// Named keys and their DOM names.
const NAMED_KEYS: [(Key, &str); 13] = [
    (Key::Tab, "Tab"),
    (Key::Enter, "Enter"),
    (Key::Escape, "Escape"),
    (Key::Backspace, "Backspace"),
    (Key::Delete, "Delete"),
    (Key::ArrowUp, "ArrowUp"),
    (Key::ArrowDown, "ArrowDown"),
    (Key::ArrowLeft, "ArrowLeft"),
    (Key::ArrowRight, "ArrowRight"),
    (Key::PageUp, "PageUp"),
    (Key::PageDown, "PageDown"),
    (Key::Home, "Home"),
    (Key::End, "End"),
];

impl Key {
    /// Parses a DOM key value: one character, or a key name such as
    /// `Enter`. `Space` is accepted for `" "`, as in Playwright.
    pub fn from_dom(value: &str) -> Option<Key> {
        if value == "Space" {
            return Some(Key::Character(" ".to_owned()));
        }
        if let Some((key, _)) = NAMED_KEYS.iter().find(|(_, name)| *name == value) {
            return Some(key.clone());
        }
        let mut chars = value.chars();
        match (chars.next(), chars.next()) {
            (Some(_), None) => Some(Key::Character(value.to_owned())),
            _ => None,
        }
    }

    /// True if the key is the given character, ignoring ASCII case.
    pub(crate) fn is_char(&self, c: char) -> bool {
        match self {
            Key::Character(s) => {
                let mut chars = s.chars();
                chars.next().is_some_and(|k| k.eq_ignore_ascii_case(&c)) && chars.next().is_none()
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dom_key_values() {
        assert_eq!(Key::from_dom("Enter"), Some(Key::Enter));
        assert_eq!(Key::from_dom("a"), Some(Key::Character("a".to_owned())));
        assert_eq!(Key::from_dom("č"), Some(Key::Character("č".to_owned())));
        assert_eq!(Key::from_dom("Space"), Some(Key::Character(" ".to_owned())));
        assert_eq!(Key::from_dom("enter"), None);
        assert_eq!(Key::from_dom(""), None);
        assert!(Key::Character("A".to_owned()).is_char('a'));
        assert!(!Key::Tab.is_char('a'));
    }
}
