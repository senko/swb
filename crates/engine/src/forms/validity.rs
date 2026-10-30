//! Constraint validation (HTML §4.10.20.3, "Constraint validation"): the
//! validity states of a control (`ValidityState`), whether it is a
//! candidate for validation (`willValidate`), and the English texts of
//! `validationMessage`.
//!
//! <https://html.spec.whatwg.org/multipage/form-control-infrastructure.html#constraints>
//!
//! Validity is computed on demand from the state of a control and its
//! attributes ([`validity`]); nothing is cached except the compiled
//! `pattern` regular expressions and the results of their matches
//! ([`ValidityCx`]). The texts are Chromium's, measured (headless
//! Chromium, `en-US`): the order of the states in a message, the texts of
//! the e-mail address check, the way numbers and dates show in them.
//!
//! Deviations, each measured against Chromium: the date and time types
//! are text fields in swb, so a text that does not parse is `badInput`
//! (Chromium: an incomplete field); `range` and `color` are always valid
//! (their values are sanitized); the validity of a `url` follows the URL
//! parser of swb (Chromium accepts `http://a b`).

use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::HashMap;

use swb_dom::{Document, ElementData, NodeId, is_html_whitespace, is_valid_float};
use swb_js_regexp::{Flags, Matcher, Outcome, Regex};
use swb_js_text::Str16;

use super::values::{format_native, native_value};
use super::{ControlState, ControlType, DateType, InputType, state_value};

/// The bytes of custom validity messages in one document (a row of the
/// limits table in ADR 0028 section 16).
pub(crate) const MAX_CUSTOM_BYTES: usize = 8 << 20;

/// The steps that the `pattern` matches of one pass (a style update, one
/// script call) may use in all.
const PASS_STEPS: u64 = 2_000_000;

/// The steps of one `pattern` match: one control cannot use the whole
/// budget of a pass.
const MATCH_STEPS: u64 = 200_000;

/// The memory that one `pattern` match may use for backtracking.
const MATCH_MEMORY: usize = 1 << 20;

/// The matches kept for later passes, the longest value kept, and the
/// bytes of the kept patterns and values.
const MEMO_ENTRIES: usize = 4096;
const MEMO_VALUE_BYTES: usize = 1024;
const MEMO_BYTES: usize = 4 << 20;

/// The compiled patterns kept, and their bytes (text and program).
const REGEX_ENTRIES: usize = 256;
const REGEX_BYTES: usize = 8 << 20;

/// The longest pattern that is compiled; a longer one is ignored.
const MAX_PATTERN_BYTES: usize = 64 << 10;

/// The pattern text that one pass may compile (a compile can take about
/// 3 µs per byte: 300 different patterns of 44 KB took 28 s in one style
/// update before this bound), and the cost of one compile beyond its
/// text.
const PASS_COMPILE_BYTES: usize = 128 << 10;
const COMPILE_OVERHEAD_BYTES: usize = 64;

/// The validity states of a control.
/// <https://html.spec.whatwg.org/multipage/form-control-infrastructure.html#validity-states>
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(clippy::struct_field_names)]
pub(crate) struct Validity {
    pub(crate) value_missing: bool,
    pub(crate) type_mismatch: bool,
    pub(crate) pattern_mismatch: bool,
    pub(crate) too_long: bool,
    pub(crate) too_short: bool,
    pub(crate) range_underflow: bool,
    pub(crate) range_overflow: bool,
    pub(crate) step_mismatch: bool,
    pub(crate) bad_input: bool,
    pub(crate) custom_error: bool,
}

impl Validity {
    /// True if no state is set.
    pub(crate) fn is_valid(self) -> bool {
        self == Validity::default()
    }
}

/// The compiled patterns and the matches, behind a `RefCell`: a style
/// update reads validity through `&`.
#[derive(Debug)]
struct Patterns {
    regexes: Regexes,
    /// The results of matches by pattern, then value: true if the value
    /// matches or the match used all of [`MATCH_STEPS`].
    memo: HashMap<String, HashMap<String, bool>>,
    /// The results in `memo`, and the bytes of their patterns and values.
    memo_entries: usize,
    memo_bytes: usize,
    /// What is left of [`PASS_STEPS`] and [`PASS_COMPILE_BYTES`].
    steps_left: u64,
    compile_left: usize,
}

impl Default for Patterns {
    fn default() -> Patterns {
        Patterns {
            regexes: Regexes::default(),
            memo: HashMap::new(),
            memo_entries: 0,
            memo_bytes: 0,
            steps_left: PASS_STEPS,
            compile_left: PASS_COMPILE_BYTES,
        }
    }
}

impl Patterns {
    /// The remembered result of `value` against `pattern`.
    fn known(&self, pattern: &str, value: &str) -> Option<bool> {
        self.memo.get(pattern)?.get(value).copied()
    }

    /// Remembers the result of a new match, within [`MEMO_ENTRIES`] and
    /// [`MEMO_BYTES`] (when they are full, the old results go).
    fn remember(&mut self, pattern: &str, value: &str, result: bool) {
        if value.len() > MEMO_VALUE_BYTES {
            return;
        }
        let bytes = pattern.len() + value.len();
        if self.memo_entries >= MEMO_ENTRIES || self.memo_bytes + bytes > MEMO_BYTES {
            self.memo.clear();
            self.memo_entries = 0;
            self.memo_bytes = 0;
        }
        self.memo_entries += 1;
        self.memo_bytes += bytes;
        if let Some(values) = self.memo.get_mut(pattern) {
            values.insert(value.to_owned(), result);
        } else {
            let values = HashMap::from([(value.to_owned(), result)]);
            self.memo.insert(pattern.to_owned(), values);
        }
    }
}

/// The compiled regular expressions by pattern text, within
/// [`REGEX_ENTRIES`] and [`REGEX_BYTES`]; `None` for a pattern that does
/// not compile.
#[derive(Debug, Default)]
struct Regexes {
    map: HashMap<String, Option<Regex>>,
    bytes: usize,
}

/// What [`Regexes::get`] finds.
enum Lookup<'a> {
    Regex(Cow<'a, Regex>),
    /// The pattern does not compile (it is ignored, as the standard says).
    Invalid,
    /// The pass may not compile more: a later pass compiles it.
    Later,
}

impl Regexes {
    /// The regular expression of `pattern`, compiled now if it is not
    /// kept and `compile_left` allows. When the cache is full, the old
    /// ones go; a regular expression larger than the whole cache is not
    /// kept.
    fn get(&mut self, pattern: &str, compile_left: &mut usize) -> Lookup<'_> {
        if !self.map.contains_key(pattern) {
            if pattern.len() <= MAX_PATTERN_BYTES {
                let cost = pattern.len() + COMPILE_OVERHEAD_BYTES;
                if cost > *compile_left {
                    return Lookup::Later;
                }
                *compile_left -= cost;
            }
            let regex = compile(pattern);
            let bytes = pattern.len() + regex.as_ref().map_or(0, Regex::heap_bytes);
            if bytes > REGEX_BYTES {
                return regex.map_or(Lookup::Invalid, |r| Lookup::Regex(Cow::Owned(r)));
            }
            if self.map.len() >= REGEX_ENTRIES || self.bytes + bytes > REGEX_BYTES {
                self.map.clear();
                self.bytes = 0;
            }
            self.bytes += bytes;
            self.map.insert(pattern.to_owned(), regex);
        }
        match self.map.get(pattern) {
            Some(Some(regex)) => Lookup::Regex(Cow::Borrowed(regex)),
            _ => Lookup::Invalid,
        }
    }
}

/// What validity needs beyond the control states: the custom validity
/// messages that scripts set, and the cache of `pattern` matches.
#[derive(Debug, Default)]
pub(crate) struct ValidityCx {
    custom: HashMap<NodeId, String>,
    custom_bytes: usize,
    patterns: RefCell<Patterns>,
}

impl ValidityCx {
    /// Starts a pass (a style update, a script call): gives the matches a
    /// new budget of steps and of compiles.
    pub(crate) fn begin_pass(&self) {
        let mut patterns = self.patterns.borrow_mut();
        patterns.steps_left = PASS_STEPS;
        patterns.compile_left = PASS_COMPILE_BYTES;
    }

    /// The custom validity message of `node` (`setCustomValidity`), if it
    /// is not empty.
    pub(crate) fn custom(&self, node: NodeId) -> Option<&str> {
        self.custom.get(&node).map(String::as_str)
    }

    /// The bytes of the custom messages.
    #[cfg(test)]
    pub(crate) fn custom_bytes(&self) -> usize {
        self.custom_bytes
    }

    /// Sets the custom message of `node`; an empty one clears it. False
    /// (and nothing changes) if the messages would pass
    /// [`MAX_CUSTOM_BYTES`].
    pub(crate) fn set_custom(&mut self, node: NodeId, message: &str) -> bool {
        let old = self.custom.get(&node).map_or(0, String::len);
        let total = self.custom_bytes.saturating_sub(old) + message.len();
        if total > MAX_CUSTOM_BYTES {
            return false;
        }
        self.custom_bytes = total;
        if message.is_empty() {
            self.custom.remove(&node);
        } else {
            self.custom.insert(node, message.to_owned());
        }
        true
    }

    /// True if the whole `value` matches the `pattern` attribute `pattern`
    /// (HTML: the regular expression `^(?:pattern)$`, with the `v` flag).
    /// A pattern that does not compile matches everything, and so does a
    /// match that runs out of steps: of its own [`MATCH_STEPS`], or of the
    /// steps of the pass (Chromium does not limit them; swb must not hang
    /// a style update). A pattern that the pass may not compile any more
    /// matches for this pass. Only results that do not depend on the
    /// other matches of the pass are remembered.
    fn pattern_matches(&self, pattern: &str, value: &str) -> bool {
        let mut guard = self.patterns.borrow_mut();
        let patterns = &mut *guard;
        if let Some(known) = patterns.known(pattern, value) {
            return known;
        }
        if patterns.steps_left == 0 {
            return true;
        }
        let regex = match patterns.regexes.get(pattern, &mut patterns.compile_left) {
            Lookup::Regex(regex) => regex,
            Lookup::Invalid | Lookup::Later => return true,
        };
        let budget = patterns.steps_left.min(MATCH_STEPS);
        let units: Vec<u16> = value.encode_utf16().collect();
        let mut matcher = Matcher::new(&regex, 0);
        let outcome = matcher.run(&regex, Str16::Wide(&units), budget as u32, MATCH_MEMORY);
        drop(regex);
        patterns.steps_left -= matcher.steps().min(budget);
        let known = match outcome {
            // A match, or a failure of the engine (the memory limit): the
            // value passes.
            Ok(Outcome::Match(_)) | Err(_) => Some(true),
            Ok(Outcome::NoMatch) => Some(false),
            // Out of the match's own steps: the same in every pass. Out of
            // the steps of the pass: a later pass tries again.
            Ok(Outcome::Pending) => (budget == MATCH_STEPS).then_some(true),
        };
        match known {
            Some(known) => {
                patterns.remember(pattern, value, known);
                known
            }
            None => true,
        }
    }
}

/// Compiles `^(?:pattern)$` with the `v` flag.
fn compile(pattern: &str) -> Option<Regex> {
    if pattern.len() > MAX_PATTERN_BYTES {
        return None;
    }
    let source: Vec<u16> = format!("^(?:{pattern})$").encode_utf16().collect();
    Regex::new(Str16::Wide(&source), Flags::UNICODE_SETS).ok()
}

/// What validity reads of a control.
pub(crate) struct Subject<'a> {
    pub(crate) doc: &'a Document,
    pub(crate) element: &'a ElementData,
    pub(crate) node: NodeId,
    pub(crate) state: &'a ControlState,
    /// True if the control is a `required` radio button of a group
    /// without a checked button (the caller knows the group).
    pub(crate) radio_missing: bool,
}

impl Subject<'_> {
    /// The value of the control as the `value` IDL attribute gives it.
    fn value(&self) -> String {
        state_value(self.element, self.doc, self.state)
    }

    /// True if a script or the user can change the value: not disabled,
    /// not read-only.
    fn is_mutable(&self) -> bool {
        self.state.is_editable(self.element) || !self.state.ty.is_text() && !self.state.disabled
    }

    fn input_type(&self) -> Option<InputType> {
        match self.state.ty {
            ControlType::Input(t) => Some(t),
            _ => None,
        }
    }

    /// True for the text fields and text areas to which `maxlength` and
    /// `minlength` apply.
    fn has_length(&self) -> bool {
        match self.state.ty {
            ControlType::Input(t) => t.has_size(),
            ControlType::TextArea => true,
            _ => false,
        }
    }
}

/// The validity states of a control (without the custom error, which
/// [`validity`] adds).
pub(crate) fn validity(s: &Subject<'_>, cx: &ValidityCx) -> Validity {
    let mut v = Validity {
        value_missing: value_missing(s),
        custom_error: cx.custom(s.node).is_some(),
        ..Validity::default()
    };
    let value = s.value();
    if let Some(ty) = s.input_type() {
        v.type_mismatch = type_mismatch(ty, s.element, &value);
        if ty.has_size() {
            v.pattern_mismatch = !value.is_empty()
                && s.element
                    .attr("pattern")
                    .is_some_and(|p| !pattern_ok(cx, p, &value, ty, s.element));
        }
        if has_range(ty) {
            let r = range(s.element, ty, &value);
            v.range_underflow = r.underflow;
            v.range_overflow = r.overflow;
            v.step_mismatch = r.step_mismatch;
        }
        v.bad_input = bad_input(ty, s.state.edit.text(), &value);
    }
    if s.state.user_edited && s.has_length() {
        let length = utf16_len(&value);
        let max = s
            .element
            .attr("maxlength")
            .and_then(swb_dom::parse_non_negative_u32);
        let min = s
            .element
            .attr("minlength")
            .and_then(swb_dom::parse_non_negative_u32);
        v.too_long = max.is_some_and(|m| length > m as usize);
        v.too_short = length > 0 && min.is_some_and(|m| length < m as usize);
    }
    v
}

/// True if the value matches the pattern. An e-mail field with `multiple`
/// must match for each address (measured).
fn pattern_ok(cx: &ValidityCx, pattern: &str, value: &str, ty: InputType, e: &ElementData) -> bool {
    if ty == InputType::Email && e.has_attr("multiple") {
        return value
            .split(',')
            .map(|a| a.trim_matches(is_html_whitespace))
            .all(|a| a.is_empty() || cx.pattern_matches(pattern, a));
    }
    cx.pattern_matches(pattern, value)
}

fn utf16_len(s: &str) -> usize {
    s.encode_utf16().count()
}

/// True if a `required` control has no value.
/// <https://html.spec.whatwg.org/multipage/form-control-infrastructure.html#suffering-from-being-missing>
fn value_missing(s: &Subject<'_>) -> bool {
    let ty = s.state.ty;
    if ty == ControlType::Input(InputType::Radio) {
        return s.radio_missing;
    }
    if !s.element.has_attr("required") || !ty.has_required() {
        return false;
    }
    match ty {
        ControlType::Input(InputType::Checkbox) => !s.state.checked,
        // swb has no file chooser: a file input never has a file.
        ControlType::Input(InputType::File) => true,
        ControlType::Select => super::select::select_is_missing(s.doc, s.node, s.state),
        t if t.is_text() => s.is_mutable() && s.value().is_empty(),
        _ => false,
    }
}

/// `typeMismatch`: an e-mail address or URL that is not one.
fn type_mismatch(ty: InputType, e: &ElementData, value: &str) -> bool {
    if value.is_empty() {
        return false;
    }
    match ty {
        InputType::Email => email_error(value, e.has_attr("multiple")).is_some(),
        InputType::Url => swb_net::Url::parse(value).is_err(),
        _ => false,
    }
}

/// `badInput`: the user typed a text that is no number, or no date or
/// time. A script cannot cause it: the value setter sanitizes.
fn bad_input(ty: InputType, text: &str, value: &str) -> bool {
    match ty {
        InputType::Number | InputType::Date(_) => !text.is_empty() && value.is_empty(),
        _ => false,
    }
}

/// True for the types with `min`, `max` and `step`.
fn has_range(ty: InputType) -> bool {
    matches!(ty, InputType::Number | InputType::Date(_))
}

// ----- Numbers -----

/// A number of the HTML syntax for floating-point numbers.
fn parse_number(s: &str) -> Option<f64> {
    if !is_valid_float(s) {
        return None;
    }
    s.parse::<f64>().ok().filter(|v| v.is_finite())
}

/// The value of an attribute or value string of the type, as a number in
/// the unit of `step`.
fn numeric(ty: InputType, s: &str) -> Option<f64> {
    match ty {
        InputType::Number => parse_number(s),
        InputType::Date(kind) => native_value(kind, s),
        _ => None,
    }
}

/// The default `step` in the unit of `step`: 1, and 60 seconds for times.
fn default_step(ty: InputType) -> f64 {
    match ty {
        InputType::Date(DateType::Time | DateType::DateTimeLocal) => 60.0,
        _ => 1.0,
    }
}

/// The range of a control: parsed `min`, `max` and `step`.
struct Bounds {
    min: Option<f64>,
    max: Option<f64>,
    /// `None` for `step=any`.
    step: Option<f64>,
    /// The step base: `min`, else the `value` attribute, else 0.
    base: f64,
}

fn bounds(e: &ElementData, ty: InputType) -> Bounds {
    let attr = |name: &str| e.attr(name).and_then(|v| numeric(ty, v));
    let min = attr("min");
    let max = attr("max");
    let step = match e.attr("step") {
        Some(s) if s.eq_ignore_ascii_case("any") => None,
        Some(s) => Some(
            parse_number(s)
                .filter(|&v| v > 0.0)
                .unwrap_or(default_step(ty)),
        ),
        None => Some(default_step(ty)),
    };
    let base = min.or_else(|| attr("value")).unwrap_or(0.0);
    Bounds {
        min,
        max,
        step,
        base,
    }
}

/// The range states of a value.
#[derive(Clone, Copy, Debug, Default)]
struct RangeStates {
    underflow: bool,
    overflow: bool,
    step_mismatch: bool,
}

/// True if `q`, a distance from the step base in steps, is a whole number
/// (a tolerance for the rounding of decimal fractions; Chromium computes
/// in decimal).
fn is_whole(q: f64) -> bool {
    q.abs() > 9e15 || (q - q.round()).abs() <= 1e-13 * q.abs() + 1e-9
}

/// `rangeUnderflow`, `rangeOverflow` and `stepMismatch` of a value.
/// <https://html.spec.whatwg.org/multipage/input.html#the-min-and-max-attributes>
fn range(e: &ElementData, ty: InputType, value: &str) -> RangeStates {
    let Some(v) = numeric(ty, value) else {
        return RangeStates::default();
    };
    let b = bounds(e, ty);
    let mut r = RangeStates::default();
    let reversed_time = ty == InputType::Date(DateType::Time)
        && matches!((b.min, b.max), (Some(min), Some(max)) if min > max);
    if reversed_time {
        // The allowed values wrap around midnight.
        let outside = b.min.is_some_and(|m| v < m) && b.max.is_some_and(|m| v > m);
        r.underflow = outside;
        r.overflow = outside;
    } else {
        r.underflow = b.min.is_some_and(|m| v < m);
        r.overflow = b.max.is_some_and(|m| v > m);
    }
    r.step_mismatch = b.step.is_some_and(|step| !is_whole((v - b.base) / step));
    r
}

// ----- E-mail addresses -----

/// True for the characters of the local part of an address.
fn is_local_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || ".!#$%&'*+/=?^_`{|}~-".contains(c)
}

/// The message for an e-mail address that is not valid, or `None` if it
/// is. `value` is the sanitized value; with `multiple` it is a list of
/// addresses separated by commas.
/// <https://html.spec.whatwg.org/multipage/input.html#email-state-(type=email)>
pub(super) fn email_error(value: &str, multiple: bool) -> Option<String> {
    if !multiple {
        return address_error(value);
    }
    for address in value.split(',') {
        let address = address.trim_matches(is_html_whitespace);
        if address.is_empty() {
            return Some("Please enter a non-empty email address.".to_owned());
        }
        if let Some(message) = address_error(address) {
            return Some(message);
        }
    }
    None
}

/// The error of one address: the first problem, as Chromium words it.
fn address_error(address: &str) -> Option<String> {
    let Some(at) = address.find('@') else {
        return Some(format!(
            "Please include an '@' in the email address. '{address}' is missing an '@'."
        ));
    };
    let (local, domain) = (&address[..at], &address[at + 1..]);
    if local.is_empty() {
        return Some(format!(
            "Please enter a part followed by '@'. '{address}' is incomplete."
        ));
    }
    if domain.is_empty() {
        return Some(format!(
            "Please enter a part following '@'. '{address}' is incomplete."
        ));
    }
    if let Some(c) = local.chars().find(|&c| !is_local_char(c)) {
        return Some(format!(
            "A part followed by '@' should not contain the symbol '{c}'."
        ));
    }
    if let Some(c) = domain
        .chars()
        .find(|&c| !(c.is_ascii_alphanumeric() || c == '-' || c == '.'))
    {
        return Some(format!(
            "A part following '@' should not contain the symbol '{c}'."
        ));
    }
    if domain.starts_with('.') || domain.ends_with('.') || domain.contains("..") {
        return Some(format!("'.' is used at a wrong position in '{domain}'."));
    }
    let label_ok = |l: &str| l.len() <= 63 && !l.starts_with('-') && !l.ends_with('-');
    if !domain.split('.').all(label_ok) {
        return Some("Please enter an email address.".to_owned());
    }
    None
}

// ----- Messages -----

/// `validationMessage`: the message of the first state that is set, as
/// Chromium words it in English. Empty if the control is not a candidate
/// for validation or has no state set.
pub(crate) fn message(s: &Subject<'_>, cx: &ValidityCx) -> String {
    if s.state.barred {
        return String::new();
    }
    if let Some(custom) = cx.custom(s.node) {
        return custom.to_owned();
    }
    let v = validity(s, cx);
    let value = s.value();
    let ty = s.input_type();
    if v.bad_input {
        return match ty {
            Some(InputType::Date(_)) => {
                "Please enter a valid value. The field is incomplete or has an invalid date."
            }
            _ => "Please enter a number.",
        }
        .to_owned();
    }
    if v.value_missing {
        return missing_message(s).to_owned();
    }
    if v.type_mismatch {
        return match ty {
            Some(InputType::Email) => email_error(&value, s.element.has_attr("multiple"))
                .unwrap_or_else(|| "Please enter an email address.".to_owned()),
            _ => "Please enter a URL.".to_owned(),
        };
    }
    if v.pattern_mismatch {
        return "Please match the requested format.".to_owned();
    }
    if v.too_long || v.too_short {
        return length_message(s, &value, v.too_long);
    }
    if let Some(ty) = ty.filter(|&t| has_range(t))
        && let Some(m) = range_message(s.element, ty, &value, v)
    {
        return m;
    }
    String::new()
}

fn missing_message(s: &Subject<'_>) -> &'static str {
    match s.state.ty {
        ControlType::Input(InputType::Checkbox) => "Please check this box if you want to proceed.",
        ControlType::Input(InputType::Radio) => "Please select one of these options.",
        ControlType::Input(InputType::File) if s.element.has_attr("multiple") => {
            "Please select one or more files."
        }
        ControlType::Input(InputType::File) => "Please select a file.",
        ControlType::Select => "Please select an item in the list.",
        _ => "Please fill out this field.",
    }
}

fn length_message(s: &Subject<'_>, value: &str, too_long: bool) -> String {
    let length = utf16_len(value);
    let attr = |name: &str| {
        s.element
            .attr(name)
            .and_then(swb_dom::parse_non_negative_u32)
            .unwrap_or(0)
    };
    let (verb, limit, direction) = if too_long {
        ("shorten", attr("maxlength"), "less")
    } else {
        ("lengthen", attr("minlength"), "more")
    };
    format!(
        "Please {verb} this text to {limit} characters or {direction} (you are currently using {length} {}).",
        characters(length)
    )
}

fn characters(n: usize) -> &'static str {
    if n == 1 { "character" } else { "characters" }
}

/// How the numbers of a type show in messages.
struct Display {
    ty: InputType,
    /// True if times show their seconds (the step is not whole minutes).
    seconds: bool,
}

impl Display {
    fn new(ty: InputType, b: &Bounds) -> Display {
        let fine = matches!(
            ty,
            InputType::Date(DateType::Time | DateType::DateTimeLocal)
        ) && b
            .step
            .is_some_and(|s| (s / 60.0 - (s / 60.0).round()).abs() > 1e-9);
        Display { ty, seconds: fine }
    }

    /// A value (the text of an attribute, or a computed number).
    fn show(&self, x: f64) -> String {
        match self.ty {
            InputType::Date(kind) => format_native(kind, x, self.seconds),
            _ => decimal(x),
        }
    }

    /// An attribute value that parsed: numbers echo the attribute (the
    /// way Chromium's decimal type prints it).
    fn show_attr(&self, text: &str, x: f64) -> String {
        match self.ty {
            InputType::Number => decimal_text(text),
            _ => self.show(x),
        }
    }
}

/// The message of the range states, or `None` if none is set. The special
/// case that Chromium has: for a reversed time range the message of a
/// valid value still says the limit that the value is past.
fn range_message(
    element: &ElementData,
    ty: InputType,
    value: &str,
    states: Validity,
) -> Option<String> {
    let range = bounds(element, ty);
    let display = Display::new(ty, &range);
    let text = |name: &str| {
        element
            .attr(name)
            .and_then(|t| numeric(ty, t).map(|n| display.show_attr(t, n)))
    };
    let is_time = ty == InputType::Date(DateType::Time);
    let (min, max) = (text("min"), text("max"));
    if states.range_underflow || states.range_overflow {
        // A time has the minimum 00:00 when `min` is missing.
        let equal = match (range.min, range.max) {
            (Some(lo), Some(hi)) => lo == hi,
            (None, Some(hi)) => is_time && hi == 0.0,
            _ => false,
        };
        let reversed = range.min.zip(range.max).is_some_and(|(lo, hi)| lo > hi);
        let (low, high) = (
            min.unwrap_or_else(|| display.show(0.0)),
            max.unwrap_or_default(),
        );
        return Some(if equal {
            format!("Value must be {high}.")
        } else if reversed {
            match ty {
                InputType::Number => {
                    format!("Minimum value ({low}) must be less than the maximum value ({high}).")
                }
                InputType::Date(DateType::Time) => {
                    format!("Value must be between {low} and {high}.")
                }
                _ => format!("Minimum date ({low}) must come before Maximum date ({high})."),
            }
        } else if ty == InputType::Number && states.range_underflow {
            format!("Value must be greater than or equal to {low}.")
        } else if ty == InputType::Number {
            format!("Value must be less than or equal to {high}.")
        } else if states.range_underflow {
            format!("Value must be {low} or later.")
        } else {
            format!("Value must be {high} or earlier.")
        });
    }
    if states.step_mismatch {
        return Some(step_message(&range, &display, numeric(ty, value)?));
    }
    // Chromium: a time in a reversed range is valid, but its message
    // still compares plainly with the two limits.
    if is_time
        && let (Some(lo), Some(hi), Some(at)) = (range.min, range.max, numeric(ty, value))
        && lo > hi
    {
        if at > hi {
            return Some(format!("Value must be {} or earlier.", display.show(hi)));
        }
        if at < lo {
            return Some(format!("Value must be {} or later.", display.show(lo)));
        }
    }
    None
}

/// The text of a step mismatch: the nearest valid values within the range.
fn step_message(range: &Bounds, display: &Display, value: f64) -> String {
    let Some(step) = range.step else {
        return "Please enter a valid value.".to_owned();
    };
    let steps = ((value - range.base) / step).floor();
    let below = range.base + steps * step;
    let above = range.base + (steps + 1.0) * step;
    // A time of day stays within one day.
    let day = matches!(display.ty, InputType::Date(DateType::Time));
    let min = range.min.or(day.then_some(0.0));
    let max = range.max.or(day.then_some(86_399.999));
    let low = Some(below).filter(|&n| min.is_none_or(|m| n >= m - 1e-9));
    let high = Some(above).filter(|&n| max.is_none_or(|m| n <= m + 1e-9));
    match (low, high) {
        (Some(first), Some(second)) => format!(
            "Please enter a valid value. The two nearest valid values are {} and {}.",
            display.show(first),
            display.show(second)
        ),
        (Some(only), None) | (None, Some(only)) => format!(
            "Please enter a valid value. The nearest valid value is {}.",
            display.show(only)
        ),
        (None, None) => "Please enter a valid value.".to_owned(),
    }
}

// ----- Decimal numbers in messages -----

/// Writes a decimal number `digits` x 10^`exp` (`digits` without leading
/// zeros) the way Chromium's decimal type prints it: plain when the
/// exponent is 0, scientific (`1.5e+3`) for a positive exponent and for
/// numbers below 1e-6, else a plain fraction.
fn layout(negative: bool, mut digits: String, mut exp: i32) -> String {
    while exp < 0 && digits.len() > 1 && digits.ends_with('0') {
        digits.pop();
        exp += 1;
    }
    if digits == "0" {
        return "0".to_owned();
    }
    let sign = if negative { "-" } else { "" };
    let len = digits.len() as i32;
    let leading = exp + len - 1;
    if exp == 0 {
        return format!("{sign}{digits}");
    }
    if exp > 0 || leading < -6 {
        let mantissa = digits.trim_end_matches('0');
        let (first, rest) = mantissa.split_at(1);
        let point = if rest.is_empty() {
            String::new()
        } else {
            format!(".{rest}")
        };
        let plus = if leading >= 0 { "+" } else { "" };
        return format!("{sign}{first}{point}e{plus}{leading}");
    }
    if len > -exp {
        let (whole, fraction) = digits.split_at((len + exp) as usize);
        format!("{sign}{whole}.{fraction}")
    } else {
        format!("{sign}0.{}{digits}", "0".repeat((-exp - len) as usize))
    }
}

/// A number that an attribute gave, as Chromium shows it in a message
/// (`1.50` is `1.5`, `1e3` is `1e+3`, 18 digits at most).
fn decimal_text(text: &str) -> String {
    let (negative, rest) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text),
    };
    let (mantissa, exp) = match rest.split_once(['e', 'E']) {
        Some((m, e)) => (m, e.parse::<i32>().unwrap_or(0).clamp(-400, 400)),
        None => (rest, 0),
    };
    let (int, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    let all = format!("{int}{fraction}");
    let mut digits = all.trim_start_matches('0').to_owned();
    if digits.is_empty() {
        return "0".to_owned();
    }
    let mut exp = exp - fraction.len() as i32;
    let truncated = digits.len() > 18;
    if truncated {
        exp += digits.len() as i32 - 18;
        digits.truncate(18);
    }
    if exp < 0 && (truncated || digits.len() > 15) {
        // Chromium shows a long fraction with 15 significant digits.
        return decimal(text.parse().unwrap_or(0.0));
    }
    layout(negative, digits, exp)
}

/// A computed number in a message (15 significant digits).
fn decimal(x: f64) -> String {
    if x == x.trunc() && x.abs() < 1e18 {
        return format!("{}", x as i64);
    }
    let s = format!("{:.14e}", x.abs());
    let Some((mantissa, exp)) = s.split_once('e') else {
        return s;
    };
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    let exp: i32 = exp.parse().unwrap_or(0);
    let trimmed = digits.trim_end_matches('0');
    let trimmed = if trimmed.is_empty() { "0" } else { trimmed };
    // value = 0.d1d2... x 10^(exp+1) = digits x 10^(exp - (len-1)).
    layout(
        x < 0.0,
        trimmed.to_owned(),
        exp - (trimmed.len() as i32 - 1),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    use swb_css::ElementState;
    use swb_dom::parse_html;

    use super::super::Forms;

    /// The validity states, the candidate flag and the message of the
    /// control `id` in `html`.
    fn check(html: &str, id: &str) -> (Validity, bool, String) {
        let doc = parse_html(html);
        let forms = Forms::new(&doc);
        let node = doc.element_by_id(id).expect("the control exists");
        let cx = ValidityCx::default();
        let (v, candidate) = forms.validity_of(&doc, node, &cx).expect("a control");
        let message = forms
            .validation_message(&doc, node, &cx)
            .expect("a control");
        (v, candidate, message)
    }

    #[test]
    fn required_controls() {
        let missing = |html: &str| check(html, "x").0.value_missing;
        assert!(missing("<input id=x required>"));
        assert!(!missing("<input id=x required value=a>"));
        assert!(missing("<input id=x type=checkbox required>"));
        assert!(missing("<select id=x required></select>"));
        assert!(missing("<input id=x type=file required>"));
        // The types without `required`.
        assert!(!missing("<input id=x type=range required>"));
        assert!(!missing("<input id=x type=hidden required>"));
        // Not mutable: no value is needed (and the control is no candidate).
        assert!(!missing("<input id=x required readonly>"));
        assert!(!missing("<input id=x required disabled>"));
        // A checkbox is missing also when read-only (measured), but is
        // not a candidate.
        let (v, candidate, message) = check("<input id=x type=checkbox required readonly>", "x");
        assert!(v.value_missing && !candidate && message.is_empty());
        // A radio button without a name is in no group (measured).
        assert!(!missing("<input id=x type=radio required>"));
        assert!(missing("<input id=x type=radio name=g required>"));
        assert!(!missing("<input id=x type=radio name=g required checked>"));
        // A number field with text that is no number is empty and bad.
        let doc = parse_html("<input id=x type=number required>");
        let mut forms = Forms::new(&doc);
        let node = doc.element_by_id("x").expect("x");
        forms.get_mut(node).expect("x").edit.insert("1e");
        let cx = ValidityCx::default();
        let (v, _) = forms.validity_of(&doc, node, &cx).expect("a control");
        assert!(v.value_missing && v.bad_input);
        assert_eq!(
            forms.validation_message(&doc, node, &cx).as_deref(),
            Some("Please enter a number.")
        );
    }

    #[test]
    fn type_and_pattern() {
        let (v, _, m) = check("<input id=x type=email value=abc>", "x");
        assert!(v.type_mismatch);
        assert_eq!(
            m,
            "Please include an '@' in the email address. 'abc' is missing an '@'."
        );
        assert!(
            !check("<input id=x type=email multiple value='a@b, c@d'>", "x")
                .0
                .type_mismatch
        );
        let (v, _, m) = check("<input id=x type=url value=abc>", "x");
        assert!(v.type_mismatch && m == "Please enter a URL.");
        assert!(
            !check("<input id=x type=url value='mailto:a@b'>", "x")
                .0
                .type_mismatch
        );
        let (v, _, m) = check("<input id=x pattern='[a-z]+' value=abC title=letters>", "x");
        assert!(v.pattern_mismatch && m == "Please match the requested format.");
        assert!(
            !check("<input id=x pattern='[a-z]+' value=''>", "x")
                .0
                .pattern_mismatch
        );
        // A pattern that does not compile is ignored; `pattern` does not
        // apply to numbers.
        assert!(
            !check("<input id=x pattern='[' value=abc>", "x")
                .0
                .pattern_mismatch
        );
        assert!(
            !check("<input id=x type=number pattern=x value=5>", "x")
                .0
                .pattern_mismatch
        );
        assert!(
            check("<textarea id=x pattern=a>b</textarea>", "x")
                .0
                .is_valid()
        );
    }

    #[test]
    fn range_and_step() {
        let state = |html: &str| {
            let (v, _, m) = check(html, "x");
            (v.range_underflow, v.range_overflow, v.step_mismatch, m)
        };
        let (under, over, step, m) = state("<input id=x type=number min=5 value=3>");
        assert!(under && !over && !step);
        assert_eq!(m, "Value must be greater than or equal to 5.");
        let (_, over, _, m) = state("<input id=x type=number max=1e3 value=2000>");
        assert!(over);
        assert_eq!(m, "Value must be less than or equal to 1e+3.");
        // The step base is the `value` attribute without `min`.
        assert!(!state("<input id=x type=number step=2 value=3>").2);
        let (_, _, step, m) = state("<input id=x type=number step=2 min=1 value=4>");
        assert!(step);
        assert_eq!(
            m,
            "Please enter a valid value. The two nearest valid values are 3 and 5."
        );
        let (_, _, step, m) = state("<input id=x type=number min=0 max=10 step=3 value=10>");
        assert!(step);
        assert_eq!(
            m,
            "Please enter a valid value. The nearest valid value is 9."
        );
        assert!(!state("<input id=x type=number step=0.1 min=0 value=0.3>").2);
        assert!(!state("<input id=x type=number step=any value=0.35 min=0>").2);
        let (under, over, _, m) = state("<input id=x type=number min=5 max=1 value=3>");
        assert!(under && over);
        assert_eq!(
            m,
            "Minimum value (5) must be less than the maximum value (1)."
        );
        // Dates and times.
        let (under, _, _, m) = state("<input id=x type=date min=2021-01-01 value=2020-05-05>");
        assert!(under);
        assert_eq!(m, "Value must be 01/01/2021 or later.");
        let (_, _, step, m) = state("<input id=x type=time step=900 min=10:00 value=10:20>");
        assert!(step);
        assert_eq!(
            m,
            "Please enter a valid value. The two nearest valid values are 10:15\u{202f}AM and 10:30\u{202f}AM."
        );
        // A reversed time range wraps around midnight.
        assert_eq!(
            state("<input id=x type=time min=22:00 max=02:00 value=23:00>"),
            (
                false,
                false,
                false,
                "Value must be 2:00\u{202f}AM or earlier.".to_owned()
            )
        );
        let (under, over, _, m) = state("<input id=x type=time min=22:00 max=02:00 value=12:00>");
        assert!(under && over);
        assert_eq!(
            m,
            "Value must be between 10:00\u{202f}PM and 2:00\u{202f}AM."
        );
        // Range and color are always valid.
        assert!(
            check("<input id=x type=range min=5 max=1 step=3>", "x")
                .0
                .is_valid()
        );
    }

    #[test]
    fn length_only_after_a_user_edit() {
        let doc = parse_html("<input id=x maxlength=3 minlength=2 value=abcde>");
        let mut forms = Forms::new(&doc);
        let node = doc.element_by_id("x").expect("x");
        let cx = ValidityCx::default();
        assert!(forms.validity_of(&doc, node, &cx).expect("x").0.is_valid());
        let state = forms.get_mut(node).expect("x");
        state.user_edited = true;
        let (v, _) = forms.validity_of(&doc, node, &cx).expect("x");
        assert!(v.too_long && !v.too_short);
        assert_eq!(
            forms.validation_message(&doc, node, &cx).as_deref(),
            Some(
                "Please shorten this text to 3 characters or less (you are currently using 5 characters)."
            )
        );
        let state = forms.get_mut(node).expect("x");
        state.edit.select_all();
        state.edit.insert("a");
        let (v, _) = forms.validity_of(&doc, node, &cx).expect("x");
        assert!(v.too_short);
        assert_eq!(
            forms.validation_message(&doc, node, &cx).as_deref(),
            Some(
                "Please lengthen this text to 2 characters or more (you are currently using 1 character)."
            )
        );
    }

    #[test]
    fn custom_validity_wins_the_message() {
        let doc =
            parse_html("<input id=x required><fieldset id=f><input id=y required></fieldset>");
        let forms = Forms::new(&doc);
        let node = doc.element_by_id("x").expect("x");
        let mut cx = ValidityCx::default();
        assert!(cx.set_custom(node, "bad"));
        let (v, candidate) = forms.validity_of(&doc, node, &cx).expect("x");
        assert!(v.custom_error && v.value_missing && candidate);
        assert_eq!(
            forms.validation_message(&doc, node, &cx).as_deref(),
            Some("bad")
        );
        let states = forms.element_states(&doc, &std::collections::HashSet::new(), &cx);
        assert!(states[&node].contains(ElementState::INVALID));
    }

    #[test]
    fn forms_and_fieldsets_follow_their_controls() {
        let doc = parse_html(
            "<form id=f><fieldset id=fs><input id=a required></fieldset><input id=b></form>\
             <form id=g><input id=c></form><fieldset id=d disabled><input required></fieldset>\
             <input id=e form=g required>",
        );
        let forms = Forms::new(&doc);
        let cx = ValidityCx::default();
        let states = forms.element_states(&doc, &std::collections::HashSet::new(), &cx);
        let has = |id: &str, flag| states[&doc.element_by_id(id).expect("exists")].contains(flag);
        assert!(has("f", ElementState::INVALID) && !has("f", ElementState::VALID));
        assert!(has("fs", ElementState::INVALID));
        // A control of another tree position, with the `form` attribute.
        assert!(has("g", ElementState::INVALID));
        // A disabled fieldset: its controls are no candidates.
        assert!(has("d", ElementState::VALID));
        assert!(has("c", ElementState::VALID));
    }

    #[test]
    fn numbers_in_messages() {
        let t = decimal_text;
        assert_eq!(t("1.50"), "1.5");
        assert_eq!(t("1e3"), "1e+3");
        assert_eq!(t("15e2"), "1.5e+3");
        assert_eq!(t("1000000"), "1000000");
        assert_eq!(t("123e-2"), "1.23");
        assert_eq!(t("0.0000001"), "1e-7");
        assert_eq!(t("0.00001"), "0.00001");
        assert_eq!(t("-1e3"), "-1e+3");
        assert_eq!(t("-0"), "0");
        assert_eq!(t("00012"), "12");
        assert_eq!(t("-.5"), "-0.5");
        assert_eq!(t("100000000000000000000000"), "1e+23");
        assert_eq!(t("1234567890123456789012"), "1.23456789012345678e+21");
        assert_eq!(decimal(0.300_000_000_000_000_04), "0.3");
        assert_eq!(decimal(1e-7), "1e-7");
        assert_eq!(decimal(3000.0), "3000");
        assert_eq!(decimal(-0.25), "-0.25");
        assert_eq!(decimal(1.5e20), "1.5e+20");
    }

    #[test]
    fn email_messages() {
        let e = |v| address_error(v);
        assert_eq!(e("a@b.c"), None);
        assert_eq!(
            e("abc").as_deref(),
            Some("Please include an '@' in the email address. 'abc' is missing an '@'.")
        );
        assert_eq!(
            e("@b").as_deref(),
            Some("Please enter a part followed by '@'. '@b' is incomplete.")
        );
        assert_eq!(
            e("a@").as_deref(),
            Some("Please enter a part following '@'. 'a@' is incomplete.")
        );
        assert_eq!(
            e("a b@c").as_deref(),
            Some("A part followed by '@' should not contain the symbol ' '.")
        );
        assert_eq!(
            e("a@b@c").as_deref(),
            Some("A part following '@' should not contain the symbol '@'.")
        );
        assert_eq!(
            e("a@b..c").as_deref(),
            Some("'.' is used at a wrong position in 'b..c'.")
        );
        assert_eq!(e("a@-b").as_deref(), Some("Please enter an email address."));
        assert_eq!(email_error("a@b, c@d", true), None);
        assert_eq!(
            email_error("a@b,,c@d", true).as_deref(),
            Some("Please enter a non-empty email address.")
        );
    }

    #[test]
    fn patterns_match_the_whole_value() {
        let cx = ValidityCx::default();
        cx.begin_pass();
        assert!(cx.pattern_matches("[a-z]+", "abc"));
        assert!(!cx.pattern_matches("[a-z]+", "abC"));
        assert!(!cx.pattern_matches("a|b", "ab"));
        assert!(cx.pattern_matches("a|b", "a"));
        assert!(!cx.pattern_matches("", "abc"));
        // A pattern that does not compile, and one that needs the `v`
        // flag's syntax rules, are ignored or follow it.
        assert!(cx.pattern_matches("[", "abc"));
        assert!(cx.pattern_matches("(?i)a", "A"));
        assert!(cx.pattern_matches("\\d+", "12"));
    }

    #[test]
    fn a_catastrophic_pattern_ends() {
        let cx = ValidityCx::default();
        cx.begin_pass();
        let value = format!("{}!", "a".repeat(60));
        let start = std::time::Instant::now();
        // Out of steps counts as a match.
        assert!(cx.pattern_matches("(a+)+$", &value));
        assert!(start.elapsed().as_secs() < 5);
        // The result is remembered: the next pass costs nothing.
        cx.begin_pass();
        assert!(cx.pattern_matches("(a+)+$", &value));
    }

    #[test]
    fn a_used_up_pass_does_not_decide_later_passes() {
        let cx = ValidityCx::default();
        cx.begin_pass();
        // Each catastrophic match uses its own steps, so the next match of
        // the pass still has steps.
        let catastrophic = |n: usize| format!("{}!{n}", "a".repeat(40));
        assert!(cx.pattern_matches("(a+)+$", &catastrophic(0)));
        assert!(!cx.pattern_matches("[0-9]+", "abc"));
        for n in 1..=(PASS_STEPS / MATCH_STEPS) as usize {
            assert!(cx.pattern_matches("(a+)+$", &catastrophic(n)));
        }
        // The pass has no steps left: the value passes for now, and the
        // next pass decides.
        assert!(cx.pattern_matches("[0-9]+", "xyz"));
        cx.begin_pass();
        assert!(!cx.pattern_matches("[0-9]+", "xyz"));
    }

    #[test]
    fn a_pass_is_charged_the_steps_that_matches_use() {
        let cx = ValidityCx::default();
        cx.begin_pass();
        for n in 0..3000 {
            assert!(!cx.pattern_matches("[0-9]+", &format!("x{n}")));
        }
    }

    #[test]
    fn a_pass_compiles_a_bounded_amount_of_pattern_text() {
        let cx = ValidityCx::default();
        let pattern = |n: usize| format!("{}|y{n}", "x".repeat(60_000));
        cx.begin_pass();
        assert!(!cx.pattern_matches(&pattern(0), "z"));
        assert!(!cx.pattern_matches(&pattern(1), "z"));
        // Out of the compile budget: the value passes for now.
        assert!(cx.pattern_matches(&pattern(2), "z"));
        cx.begin_pass();
        assert!(!cx.pattern_matches(&pattern(2), "z"));
    }

    #[test]
    fn the_caches_are_bounded() {
        let cx = ValidityCx::default();
        let check = || {
            let patterns = cx.patterns.borrow();
            assert!(patterns.regexes.map.len() <= REGEX_ENTRIES);
            assert!(patterns.regexes.bytes <= REGEX_BYTES);
            assert!(patterns.memo_entries <= MEMO_ENTRIES);
            assert!(patterns.memo_bytes <= MEMO_BYTES);
        };
        // Many patterns (a script that writes `pattern` in a loop).
        let long = "[a-c]".repeat(2000);
        for n in 0..300 {
            cx.begin_pass();
            assert!(!cx.pattern_matches(&format!("{long}|x{n}"), "d"));
            check();
        }
        // Many values of one long pattern.
        let pattern = format!("{}|y", "x".repeat(60_000));
        for n in 0..100 {
            cx.begin_pass();
            assert!(!cx.pattern_matches(&pattern, &format!("z{n}")));
            check();
        }
    }

    #[test]
    fn custom_messages_are_bounded() {
        let mut cx = ValidityCx::default();
        let node = NodeId::DOCUMENT;
        assert!(cx.set_custom(node, "bad"));
        assert_eq!(cx.custom(node), Some("bad"));
        assert!(cx.set_custom(node, ""));
        assert_eq!(cx.custom(node), None);
        assert_eq!(cx.custom_bytes(), 0);
        let big = "x".repeat(MAX_CUSTOM_BYTES / 2 + 1);
        assert!(cx.set_custom(node, &big));
        let other = NodeId::from_index(1).expect("a node id");
        assert!(!cx.set_custom(other, &big));
        assert_eq!(cx.custom(other), None);
    }
}
