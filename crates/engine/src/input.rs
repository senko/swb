//! Input event types that hosts (the GUI and the automation server) pass
//! to a page, and the attributes that keyboard events show for a key.

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

    /// True if the key is a character that a US keyboard has (a key press
    /// with key events), or a named key.
    pub fn is_on_us_layout(&self) -> bool {
        self.info().on_layout
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

/// The attributes of the keyboard events of a key (UI Events: `key`, `code`
/// and the legacy `keyCode` and `charCode`), for a US keyboard
/// layout as Chromium 148 shows them for Playwright's keys (measured with
/// `tools/probes/web-input-real.json`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct KeyInfo {
    /// `key`.
    pub(crate) key: String,
    /// `code` (empty for a character that is not on the US layout).
    pub(crate) code: String,
    /// `keyCode` of `keydown` and `keyup`.
    pub(crate) key_code: i32,
    /// `charCode` and `keyCode` of `keypress`.
    pub(crate) char_code: i32,
    /// True if the key makes a `keypress` event: printable characters and
    /// Enter.
    pub(crate) keypress: bool,
    /// True if the character is on the US layout. Playwright types other
    /// characters as text without key events.
    pub(crate) on_layout: bool,
}

/// Keys of the US layout that are not letters or digits: the characters
/// of the key (unshifted first), its `code` and its `keyCode`.
const SYMBOL_KEYS: [(&str, &str, i32); 11] = [
    (" ", "Space", 32),
    (";:", "Semicolon", 186),
    ("=+", "Equal", 187),
    (",<", "Comma", 188),
    ("-_", "Minus", 189),
    (".>", "Period", 190),
    ("/?", "Slash", 191),
    ("`~", "Backquote", 192),
    ("[{", "BracketLeft", 219),
    ("]}", "BracketRight", 221),
    ("'\"", "Quote", 222),
];

/// The digit keys and the symbols above them.
const SHIFTED_DIGITS: &str = ")!@#$%^&*(";

/// The `code` and `keyCode` of the character `c` on a US keyboard (the
/// shifted symbols have the code of their key), or `None`.
fn us_layout(c: char) -> Option<(String, i32)> {
    let letter = c.to_ascii_uppercase();
    if letter.is_ascii_uppercase() {
        return Some((format!("Key{letter}"), letter as i32));
    }
    if c.is_ascii_digit() {
        return Some((format!("Digit{c}"), c as i32));
    }
    if let Some(digit) = SHIFTED_DIGITS.find(c) {
        return Some((format!("Digit{digit}"), 48 + digit as i32));
    }
    if c == '\\' || c == '|' {
        return Some(("Backslash".to_owned(), 220));
    }
    SYMBOL_KEYS
        .iter()
        .find(|(chars, _, _)| chars.contains(c))
        .map(|(_, code, key_code)| ((*code).to_owned(), *key_code))
}

impl Key {
    /// The attributes of the keyboard events of this key.
    pub(crate) fn info(&self) -> KeyInfo {
        let named = |key: &str, key_code: i32, keypress: bool| KeyInfo {
            key: key.to_owned(),
            code: key.to_owned(),
            key_code,
            char_code: if keypress { key_code } else { 0 },
            keypress,
            on_layout: true,
        };
        match self {
            Key::Tab => named("Tab", 9, false),
            Key::Enter => named("Enter", 13, true),
            Key::Escape => named("Escape", 27, false),
            Key::Backspace => named("Backspace", 8, false),
            Key::Delete => named("Delete", 46, false),
            Key::ArrowUp => named("ArrowUp", 38, false),
            Key::ArrowDown => named("ArrowDown", 40, false),
            Key::ArrowLeft => named("ArrowLeft", 37, false),
            Key::ArrowRight => named("ArrowRight", 39, false),
            Key::PageUp => named("PageUp", 33, false),
            Key::PageDown => named("PageDown", 34, false),
            Key::Home => named("Home", 36, false),
            Key::End => named("End", 35, false),
            Key::Character(text) => {
                let c = text.chars().next().unwrap_or(' ');
                let layout = us_layout(c);
                KeyInfo {
                    key: text.clone(),
                    code: layout.as_ref().map(|l| l.0.clone()).unwrap_or_default(),
                    // 229 is the code of a key that an input method handled.
                    key_code: layout.as_ref().map_or(229, |l| l.1),
                    char_code: c as i32,
                    keypress: !c.is_control(),
                    on_layout: layout.is_some(),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_info_matches_chromium() {
        // Measured: keydown of "a" has keyCode 65 and code KeyA, keypress
        // has keyCode and charCode 97; "B" gives 66 and 66; "!" is on the
        // key Digit1 (keyCode 49) with charCode 33.
        let a = Key::Character("a".to_owned()).info();
        assert_eq!((a.code.as_str(), a.key_code, a.char_code), ("KeyA", 65, 97));
        let b = Key::Character("B".to_owned()).info();
        assert_eq!((b.code.as_str(), b.key_code, b.char_code), ("KeyB", 66, 66));
        let bang = Key::Character("!".to_owned()).info();
        assert_eq!(
            (bang.code.as_str(), bang.key_code, bang.char_code),
            ("Digit1", 49, 33)
        );
        let zero = Key::Character(")".to_owned()).info();
        assert_eq!((zero.code.as_str(), zero.key_code), ("Digit0", 48));
        let space = Key::Character(" ".to_owned()).info();
        assert_eq!((space.code.as_str(), space.key_code), ("Space", 32));
        let enter = Key::Enter.info();
        assert_eq!(
            (enter.key_code, enter.char_code, enter.keypress),
            (13, 13, true)
        );
        assert!(!Key::Tab.info().keypress);
        assert!(!Key::Character("\u{e9}".to_owned()).info().on_layout);
    }

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
