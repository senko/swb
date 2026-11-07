//! Style sheets and rules for scripts (CSSOM §6.1 to §6.4,
//! <https://drafts.csswg.org/cssom/>, ADR 0028 section 10):
//! `document.styleSheets`, the `sheet` of `style` and `link` elements,
//! `CSSStyleSheet`, `CSSRuleList`, `CSSRule` and its subclasses, and
//! `MediaList`.
//!
//! The objects are views of the shared model of `swb_style::cssom::sheet`,
//! which is also what the engine builds the stylist from: a change by a
//! script changes the rules that the next style flush uses, and the binding
//! tells the engine ([`crate::EngineHost::style_sheets_changed`]) when the
//! sheet belongs to the document.
//!
//! Identity: one wrapper per sheet and per rule while scripts can reach it.
//! The wrappers are kept in maps keyed by the address of the shared data;
//! each wrapper's host data holds one strong reference to its sheet or rule,
//! and the objects that belong to a wrapper (`cssRules`, `media`, `style`)
//! refer to the wrapper object, not to the data. So a sheet or rule whose
//! strong count is one is held by its wrapper alone: no list, sheet or
//! engine slot can give it out again, and its map entry can go. When the
//! maps reach their limit, such entries go (the wrappers stay alive while
//! scripts hold them); the next limit is twice the entries that stay.

use std::collections::HashMap;
use std::rc::Rc;

use swb_css::MediaQueryList;
use swb_dom::NodeId;
use swb_js::{Gc, GenericData, HostData, Object, Runtime, Tracer, Value, VmError};
use swb_style::cssom::sheet::{self as model, RuleError, RuleKind, RuleRef, SheetRef, StyleSheet};

use crate::bindings::{self as b, Cx, Platform};
use crate::generated as g;
use crate::state::{Slot, WebState, engine_mut, same_object, state, state_mut};

/// The number of wrappers kept before those that no list can give out
/// again go.
const MIN_OBJECTS: usize = 4096;

/// The wrappers of the sheets and rules that scripts have seen.
#[derive(Default)]
pub(crate) struct CssomObjects {
    sheets: HashMap<usize, Gc<Object>>,
    rules: HashMap<usize, Gc<Object>>,
    /// The size at which the next new wrapper sweeps the maps (at least
    /// [`MIN_OBJECTS`]).
    limit: usize,
}

impl CssomObjects {
    /// Reports the wrappers to the collector.
    pub(crate) fn trace(&self, tracer: &mut Tracer<'_>) {
        for object in self.sheets.values().chain(self.rules.values()) {
            tracer.object(*object);
        }
    }
}

/// The host data of a `CSSStyleSheet`: the sheet and its `[SameObject]`
/// members.
struct SheetData {
    /// The key of the document of the sheet.
    document: u32,
    sheet: SheetRef,
    rules: Option<Gc<Object>>,
    media: Option<Gc<Object>>,
}

impl GenericData for SheetData {
    /// The ids of the object mean the arena of its document.
    fn host_word(&self) -> Option<u64> {
        Some(b::word_of(self.document, NodeId::DOCUMENT))
    }

    fn trace(&self, tracer: &mut Tracer<'_>) {
        for object in [self.rules, self.media].into_iter().flatten() {
            tracer.object(object);
        }
    }

    fn heap_size(&self) -> usize {
        0
    }
}

/// The host data of a `CSSRule` (of each subclass): the rule and its
/// `[SameObject]` members.
struct RuleData {
    rule: RuleRef,
    rules: Option<Gc<Object>>,
    media: Option<Gc<Object>>,
    style: Option<Gc<Object>>,
}

impl GenericData for RuleData {
    fn trace(&self, tracer: &mut Tracer<'_>) {
        for object in [self.rules, self.media, self.style].into_iter().flatten() {
            tracer.object(object);
        }
    }

    fn heap_size(&self) -> usize {
        0
    }
}

/// The host data of a `CSSRuleList` or a `MediaList`: the wrapper of the
/// sheet or rule that it belongs to.
struct OwnerData {
    owner: Gc<Object>,
}

impl GenericData for OwnerData {
    fn trace(&self, tracer: &mut Tracer<'_>) {
        tracer.object(self.owner);
    }

    fn heap_size(&self) -> usize {
        0
    }
}

/// What a wrapper or a list stands for.
enum Owner {
    Sheet(SheetRef),
    Rule(RuleRef),
}

fn generic_handle(data: HostData) -> b::Result<Gc<swb_js::Generic>> {
    match data {
        HostData::Generic(handle) => Ok(handle),
        HostData::Word(_) => Err(VmError::type_error("Illegal invocation").into()),
    }
}

/// The sheet or rule of a wrapper object.
fn owner_of_object(rt: &Runtime, object: Gc<Object>) -> b::Result<Owner> {
    let (_, data) = rt
        .host_object(Value::Object(object))
        .ok_or(VmError::type_error("Illegal invocation"))?;
    let handle = generic_handle(data)?;
    if let Ok(data) = rt.heap().generic::<SheetData>(handle) {
        return Ok(Owner::Sheet(Rc::clone(&data.sheet)));
    }
    let data = rt
        .heap()
        .generic::<RuleData>(handle)
        .map_err(VmError::from)?;
    Ok(Owner::Rule(Rc::clone(&data.rule)))
}

fn sheet_of(rt: &Runtime, this: Platform) -> b::Result<SheetRef> {
    let handle = generic_handle(this.data)?;
    let data = rt
        .heap()
        .generic::<SheetData>(handle)
        .map_err(VmError::from)?;
    Ok(Rc::clone(&data.sheet))
}

fn rule_of(rt: &Runtime, this: Platform) -> b::Result<RuleRef> {
    let handle = generic_handle(this.data)?;
    let data = rt
        .heap()
        .generic::<RuleData>(handle)
        .map_err(VmError::from)?;
    Ok(Rc::clone(&data.rule))
}

/// The rule of the wrapper `object` of a `CSSRule` (for `rule.style`).
pub(crate) fn rule_of_object(rt: &Runtime, object: Gc<Object>) -> b::Result<RuleRef> {
    match owner_of_object(rt, object)? {
        Owner::Rule(rule) => Ok(rule),
        Owner::Sheet(_) => Err(VmError::type_error("Illegal invocation").into()),
    }
}

/// The owner of a `CSSRuleList` or a `MediaList`.
fn owner_of_list(rt: &Runtime, this: Platform) -> b::Result<(Gc<Object>, Owner)> {
    let handle = generic_handle(this.data)?;
    let owner = rt
        .heap()
        .generic::<OwnerData>(handle)
        .map_err(VmError::from)?
        .owner;
    Ok((owner, owner_of_object(rt, owner)?))
}

/// Drops the map entries of sheets and rules that only their wrapper
/// holds, when the maps reached their limit.
fn sweep(rt: &mut Runtime) -> b::Result<()> {
    let objects = &state(rt)?.doc.cssom;
    if objects.sheets.len() + objects.rules.len() < objects.limit.max(MIN_OBJECTS) {
        return Ok(());
    }
    // The wrapper holds one reference, and `owner_of_object` a second one
    // while the closure runs.
    let held_alone = |rt: &Runtime, object: Gc<Object>| -> bool {
        match owner_of_object(rt, object) {
            Ok(Owner::Sheet(sheet)) => Rc::strong_count(&sheet) <= 2,
            Ok(Owner::Rule(rule)) => Rc::strong_count(&rule) <= 2,
            Err(_) => true,
        }
    };
    let dead_sheets: Vec<usize> = objects
        .sheets
        .iter()
        .filter(|(_, o)| held_alone(rt, **o))
        .map(|(k, _)| *k)
        .collect();
    let dead_rules: Vec<usize> = objects
        .rules
        .iter()
        .filter(|(_, o)| held_alone(rt, **o))
        .map(|(k, _)| *k)
        .collect();
    let objects = &mut state_mut(rt)?.doc.cssom;
    for key in dead_sheets {
        objects.sheets.remove(&key);
    }
    for key in dead_rules {
        objects.rules.remove(&key);
    }
    objects.limit = (objects.sheets.len() + objects.rules.len())
        .saturating_mul(2)
        .max(MIN_OBJECTS);
    Ok(())
}

/// The one wrapper of `sheet`.
pub(crate) fn wrap_sheet(cx: &mut Cx<'_>, sheet: SheetRef) -> b::Result<Gc<Object>> {
    let key = Rc::as_ptr(&sheet) as usize;
    if let Some(object) = state(cx.rt)?.doc.cssom.sheets.get(&key) {
        return Ok(*object);
    }
    sweep(cx.rt)?;
    let realm = cx.realm();
    let document = crate::state::document_key(cx.rt)?;
    let data = cx.rt.alloc_host_data(SheetData {
        document,
        sheet,
        rules: None,
        media: None,
    })?;
    let object = cx.rt.new_host_object(realm, g::CSS_STYLE_SHEET, data)?;
    state_mut(cx.rt)?.doc.cssom.sheets.insert(key, object);
    Ok(object)
}

/// The one wrapper of `rule`, of the interface of its kind.
pub(crate) fn wrap_rule(cx: &mut Cx<'_>, rule: RuleRef) -> b::Result<Gc<Object>> {
    let key = Rc::as_ptr(&rule) as usize;
    if let Some(object) = state(cx.rt)?.doc.cssom.rules.get(&key) {
        return Ok(*object);
    }
    sweep(cx.rt)?;
    let class = match rule.borrow().kind {
        RuleKind::Style(_) => g::CSS_STYLE_RULE,
        RuleKind::Media { .. } => g::CSS_MEDIA_RULE,
        RuleKind::Supports { .. } => g::CSS_SUPPORTS_RULE,
        RuleKind::Import(_) => g::CSS_IMPORT_RULE,
        RuleKind::FontFace(_) => g::CSS_FONT_FACE_RULE,
        RuleKind::Keyframes(_) => g::CSS_KEYFRAMES_RULE,
    };
    let realm = cx.realm();
    let data = cx.rt.alloc_host_data(RuleData {
        rule,
        rules: None,
        media: None,
        style: None,
    })?;
    let object = cx.rt.new_host_object(realm, class, data)?;
    state_mut(cx.rt)?.doc.cssom.rules.insert(key, object);
    Ok(object)
}

/// A `[SameObject]` member of a sheet or rule wrapper: the object in the
/// field that `field` selects, made by `make` at the first access.
fn same_member<T: GenericData>(
    cx: &mut Cx<'_>,
    this: Platform,
    field: fn(&mut T) -> &mut Option<Gc<Object>>,
    make: impl FnOnce(&mut Cx<'_>) -> b::Result<Gc<Object>>,
) -> b::Result<Gc<Object>> {
    let handle = generic_handle(this.data)?;
    if let Some(object) = *field(
        cx.rt
            .heap_mut()
            .generic_mut::<T>(handle)
            .map_err(VmError::from)?,
    ) {
        return Ok(object);
    }
    let object = make(cx)?;
    *field(
        cx.rt
            .heap_mut()
            .generic_mut::<T>(handle)
            .map_err(VmError::from)?,
    ) = Some(object);
    Ok(object)
}

/// A new object of `class` that lists or describes `owner`.
fn owned_object(
    cx: &mut Cx<'_>,
    class: swb_js::ClassId,
    owner: Gc<Object>,
) -> b::Result<Gc<Object>> {
    let realm = cx.realm();
    let data = cx.rt.alloc_host_data(OwnerData { owner })?;
    Ok(cx.rt.new_host_object(realm, class, data)?)
}

/// Tells the engine that `sheet` changed if it belongs to the document.
fn sheet_changed(rt: &mut Runtime, sheet: &SheetRef) -> b::Result<()> {
    if sheet.borrow().owner.is_some() {
        engine_mut(rt)?.style_sheets_changed();
    }
    Ok(())
}

/// Records a change of `rule` and tells the engine if its sheet belongs to
/// the document.
pub(crate) fn rule_changed(rt: &mut Runtime, rule: &RuleRef) -> b::Result<()> {
    model::mark_modified(rule);
    match model::parent_sheet(rule) {
        Some(sheet) => sheet_changed(rt, &sheet),
        None => Ok(()),
    }
}

/// `SecurityError` for the rules of a sheet of another origin.
fn check_clean(sheet: &SheetRef, message: &'static str) -> b::Result<()> {
    if sheet.borrow().origin_clean {
        Ok(())
    } else {
        Err(b::Exception::security(message))
    }
}

/// The exception of a failed change of a sheet's rule list, with
/// Chromium's messages.
fn sheet_error(error: RuleError, index: u32, text: &str, delete: bool) -> b::Exception {
    match error {
        RuleError::Index { length: 0 } if delete => {
            b::Exception::dom("IndexSizeError", "Style sheet is empty (length 0).")
        }
        RuleError::Index { length } => {
            let max = if delete {
                i64::try_from(length).unwrap_or(i64::MAX) - 1
            } else {
                i64::try_from(length).unwrap_or(i64::MAX)
            };
            b::Exception::dom(
                "IndexSizeError",
                format!("The index provided ({index}) is larger than the maximum index ({max})."),
            )
        }
        RuleError::Syntax => {
            b::Exception::dom("SyntaxError", format!("Failed to parse the rule '{text}'."))
        }
        RuleError::Hierarchy | RuleError::ImportInGroup => {
            b::Exception::dom("HierarchyRequestError", "Failed to insert the rule.")
        }
        RuleError::Memory => b::Exception::dom(
            "QuotaExceededError",
            "The style data that scripts made reaches the limit of the document.",
        ),
        RuleError::TooDeep => b::Exception::dom(
            "HierarchyRequestError",
            format!(
                "Grouping rules nest at most {} levels deep.",
                model::MAX_GROUP_DEPTH
            ),
        ),
    }
}

/// The exception of a failed change of a grouping rule's list, with
/// Chromium's messages (`greated` is Chromium's spelling).
fn group_error(error: RuleError, index: u32, text: &str, delete: bool) -> b::Exception {
    match error {
        RuleError::Index { .. } if delete => b::Exception::dom(
            "IndexSizeError",
            format!("the index {index} is greated than the length of the rule list."),
        ),
        RuleError::Index { .. } => b::Exception::dom(
            "IndexSizeError",
            format!("the index {index} must be less than or equal to the length of the rule list."),
        ),
        RuleError::ImportInGroup => b::Exception::dom(
            "HierarchyRequestError",
            "'@import' rules cannot be inserted inside a group rule.",
        ),
        other => sheet_error(other, index, text, delete),
    }
}

/// Inserts `text` at `index` into the list of `sheet` (`insertRule`,
/// `addRule`).
fn insert_into_sheet(rt: &mut Runtime, sheet: &SheetRef, text: &str, index: u32) -> b::Result<u32> {
    check_clean(sheet, "Cannot access StyleSheet to insertRule")?;
    let at = StyleSheet::insert_rule(sheet, text, index)
        .map_err(|e| sheet_error(e, index, text, false))?;
    sheet_changed(rt, sheet)?;
    Ok(at)
}

/// Removes the rule at `index` of `sheet` (`deleteRule`, `removeRule`).
/// The removed rule keeps its wrapper in the map: the rules inside it still
/// give it out as their `parentRule`.
fn delete_from_sheet(rt: &mut Runtime, sheet: &SheetRef, index: u32) -> b::Result<()> {
    check_clean(sheet, "Cannot access StyleSheet to deleteRule")?;
    StyleSheet::delete_rule(sheet, index).map_err(|e| sheet_error(e, index, "", true))?;
    sheet_changed(rt, sheet)
}

// ----- document.styleSheets and LinkStyle -----

/// `document.styleSheets`: the one `StyleSheetList` of the document.
pub(crate) fn style_sheets(cx: &mut Cx<'_>, document: NodeId) -> b::Result<Gc<Object>> {
    let realm = cx.realm();
    Ok(same_object(cx.rt, document, Slot::StyleSheets, |rt| {
        let data = crate::state::node_data_here(rt, document)?;
        rt.new_host_object(realm, g::STYLE_SHEET_LIST, data)
    })?)
}

/// `sheet` of a `style` or `link` element (`LinkStyle`).
pub(crate) fn sheet(cx: &mut Cx<'_>, element: NodeId) -> b::Result<Option<Gc<Object>>> {
    match engine_mut(cx.rt)?.style_sheet_of(element) {
        Some(sheet) => Ok(Some(wrap_sheet(cx, sheet)?)),
        None => Ok(None),
    }
}

/// `disabled` of a `style` element: the disabled flag of its sheet (false
/// without one; HTML §4.2.6).
pub(crate) fn style_disabled(cx: &mut Cx<'_>, element: NodeId) -> b::Result<bool> {
    Ok(engine_mut(cx.rt)?
        .style_sheet_of(element)
        .is_some_and(|s| s.borrow().disabled))
}

/// Sets `disabled` of a `style` element: the disabled flag of its sheet
/// (nothing without one).
pub(crate) fn set_style_disabled(cx: &mut Cx<'_>, element: NodeId, value: bool) -> b::Result<()> {
    let Some(sheet) = engine_mut(cx.rt)?.style_sheet_of(element) else {
        return Ok(());
    };
    if sheet.borrow().disabled != value {
        sheet.borrow_mut().disabled = value;
        sheet_changed(cx.rt, &sheet)?;
    }
    Ok(())
}

/// True if the list is the window's document's (another document of the
/// arena has no sheets in swb).
fn is_window_document(this: Platform) -> bool {
    b::node_of(this.data) == Some(NodeId::DOCUMENT)
}

impl g::style_sheet_list::StyleSheetListImpl for WebState {
    fn length(cx: &mut Cx<'_>, this: Platform) -> b::Result<u32> {
        if !is_window_document(this) {
            return Ok(0);
        }
        let count = engine_mut(cx.rt)?.style_sheet_count();
        Ok(u32::try_from(count).unwrap_or(u32::MAX))
    }

    fn item(cx: &mut Cx<'_>, this: Platform, index: u32) -> b::Result<Option<Gc<Object>>> {
        if !is_window_document(this) {
            return Ok(None);
        }
        let sheet = usize::try_from(index)
            .ok()
            .and_then(|i| engine_mut(cx.rt).ok()?.style_sheet(i));
        match sheet {
            Some(sheet) => Ok(Some(wrap_sheet(cx, sheet)?)),
            None => Ok(None),
        }
    }

    fn index_count(cx: &mut Cx<'_>, this: Platform) -> b::Result<u32> {
        Self::length(cx, this)
    }
}

// ----- StyleSheet and CSSStyleSheet -----

impl g::style_sheet::StyleSheetImpl for WebState {
    fn type_(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<String> {
        Ok("text/css".to_owned())
    }

    fn href(cx: &mut Cx<'_>, this: Platform) -> b::Result<Option<String>> {
        Ok(sheet_of(cx.rt, this)?.borrow().href.clone())
    }

    /// The DOM changes are processed first: a sheet whose `style` element
    /// was removed or rewritten has no owner (Chromium measured).
    fn owner_node(cx: &mut Cx<'_>, this: Platform) -> b::Result<Option<NodeId>> {
        engine_mut(cx.rt)?.settle_style_sheets();
        Ok(sheet_of(cx.rt, this)?.borrow().owner)
    }

    /// Always null: swb does not load the sheets of `@import`.
    fn parent_style_sheet(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<Option<Gc<Object>>> {
        Ok(None)
    }

    fn title(cx: &mut Cx<'_>, this: Platform) -> b::Result<Option<String>> {
        Ok(sheet_of(cx.rt, this)?.borrow().title.clone())
    }

    fn media(cx: &mut Cx<'_>, this: Platform) -> b::Result<Gc<Object>> {
        let owner = this.object;
        same_member::<SheetData>(
            cx,
            this,
            |d| &mut d.media,
            |cx| owned_object(cx, g::MEDIA_LIST, owner),
        )
    }

    fn disabled(cx: &mut Cx<'_>, this: Platform) -> b::Result<bool> {
        Ok(sheet_of(cx.rt, this)?.borrow().disabled)
    }

    fn set_disabled(cx: &mut Cx<'_>, this: Platform, value: bool) -> b::Result<()> {
        let sheet = sheet_of(cx.rt, this)?;
        if sheet.borrow().disabled == value {
            return Ok(());
        }
        sheet.borrow_mut().disabled = value;
        sheet_changed(cx.rt, &sheet)
    }
}

impl g::css_style_sheet::CSSStyleSheetImpl for WebState {
    /// Always null: swb does not load the sheets of `@import`.
    fn owner_rule(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<Option<Gc<Object>>> {
        Ok(None)
    }

    fn css_rules(cx: &mut Cx<'_>, this: Platform) -> b::Result<Gc<Object>> {
        check_clean(&sheet_of(cx.rt, this)?, "Cannot access rules")?;
        let owner = this.object;
        same_member::<SheetData>(
            cx,
            this,
            |d| &mut d.rules,
            |cx| owned_object(cx, g::CSS_RULE_LIST, owner),
        )
    }

    /// The same list as `cssRules` (measured: `rules === cssRules`).
    fn rules(cx: &mut Cx<'_>, this: Platform) -> b::Result<Gc<Object>> {
        Self::css_rules(cx, this)
    }

    fn insert_rule(cx: &mut Cx<'_>, this: Platform, rule: String, index: u32) -> b::Result<u32> {
        let sheet = sheet_of(cx.rt, this)?;
        insert_into_sheet(cx.rt, &sheet, &rule, index)
    }

    fn delete_rule(cx: &mut Cx<'_>, this: Platform, index: u32) -> b::Result<()> {
        let sheet = sheet_of(cx.rt, this)?;
        delete_from_sheet(cx.rt, &sheet, index)
    }

    /// The legacy `addRule` (CSSOM §6.1.3): inserts `selector { style }` at
    /// `index` or at the end, and returns -1.
    fn add_rule(
        cx: &mut Cx<'_>,
        this: Platform,
        selector: String,
        style: String,
        index: Option<u32>,
    ) -> b::Result<i32> {
        let sheet = sheet_of(cx.rt, this)?;
        let mut text = selector;
        text.push_str(" { ");
        if !style.is_empty() {
            text.push_str(&style);
            text.push(' ');
        }
        text.push('}');
        let length = u32::try_from(sheet.borrow().rules().len()).unwrap_or(u32::MAX);
        insert_into_sheet(cx.rt, &sheet, &text, index.unwrap_or(length))?;
        Ok(-1)
    }

    fn remove_rule(cx: &mut Cx<'_>, this: Platform, index: u32) -> b::Result<()> {
        Self::delete_rule(cx, this, index)
    }
}

// ----- CSSRuleList -----

/// The rule at `index` of a `CSSRuleList`: of its sheet or of its grouping
/// rule.
fn listed_rule(rt: &Runtime, this: Platform, index: u32) -> b::Result<Option<RuleRef>> {
    let index = usize::try_from(index).unwrap_or(usize::MAX);
    Ok(match owner_of_list(rt, this)?.1 {
        Owner::Sheet(sheet) => sheet.borrow().rules().get(index).cloned(),
        Owner::Rule(rule) => rule
            .borrow()
            .child_rules()
            .and_then(|r| r.get(index))
            .cloned(),
    })
}

impl g::css_rule_list::CSSRuleListImpl for WebState {
    fn length(cx: &mut Cx<'_>, this: Platform) -> b::Result<u32> {
        let length = match owner_of_list(cx.rt, this)?.1 {
            Owner::Sheet(sheet) => sheet.borrow().rules().len(),
            Owner::Rule(rule) => rule.borrow().child_rules().map_or(0, <[RuleRef]>::len),
        };
        Ok(u32::try_from(length).unwrap_or(u32::MAX))
    }

    fn item(cx: &mut Cx<'_>, this: Platform, index: u32) -> b::Result<Option<Gc<Object>>> {
        match listed_rule(cx.rt, this, index)? {
            Some(rule) => Ok(Some(wrap_rule(cx, rule)?)),
            None => Ok(None),
        }
    }

    fn index_count(cx: &mut Cx<'_>, this: Platform) -> b::Result<u32> {
        Self::length(cx, this)
    }
}

// ----- CSSRule and its subclasses -----

impl g::css_rule::CSSRuleImpl for WebState {
    fn type_(cx: &mut Cx<'_>, this: Platform) -> b::Result<u16> {
        Ok(rule_of(cx.rt, this)?.borrow().type_number())
    }

    fn css_text(cx: &mut Cx<'_>, this: Platform) -> b::Result<String> {
        let rule = rule_of(cx.rt, this)?;
        let quirks = model::rule_quirks(&rule);
        let text = rule.borrow_mut().css_text(quirks);
        Ok(text)
    }

    /// Does nothing (CSSOM: "on setting the `cssText` attribute must do
    /// nothing").
    fn set_css_text(_cx: &mut Cx<'_>, _this: Platform, _value: String) -> b::Result<()> {
        Ok(())
    }

    fn parent_rule(cx: &mut Cx<'_>, this: Platform) -> b::Result<Option<Gc<Object>>> {
        let parent = rule_of(cx.rt, this)?.borrow().parent_rule();
        match parent {
            Some(parent) => Ok(Some(wrap_rule(cx, parent)?)),
            None => Ok(None),
        }
    }

    fn parent_style_sheet(cx: &mut Cx<'_>, this: Platform) -> b::Result<Option<Gc<Object>>> {
        match model::parent_sheet(&rule_of(cx.rt, this)?) {
            Some(sheet) => Ok(Some(wrap_sheet(cx, sheet)?)),
            None => Ok(None),
        }
    }
}

impl g::css_style_rule::CSSStyleRuleImpl for WebState {
    fn selector_text(cx: &mut Cx<'_>, this: Platform) -> b::Result<String> {
        let rule = rule_of(cx.rt, this)?;
        let rule = rule.borrow();
        Ok(match &rule.kind {
            RuleKind::Style(style) => style.selectors().cssom_text(),
            _ => String::new(),
        })
    }

    /// An invalid selector list changes nothing (CSSOM).
    fn set_selector_text(cx: &mut Cx<'_>, this: Platform, value: String) -> b::Result<()> {
        let rule = rule_of(cx.rt, this)?;
        if model::set_selector_text(&rule, &value) {
            rule_changed(cx.rt, &rule)?;
        }
        Ok(())
    }

    fn style(cx: &mut Cx<'_>, this: Platform) -> b::Result<Gc<Object>> {
        let owner = this.object;
        same_member::<RuleData>(
            cx,
            this,
            |d| &mut d.style,
            |cx| crate::cssom::rule_style(cx, owner),
        )
    }
}

impl g::css_import_rule::CSSImportRuleImpl for WebState {
    /// The URL as written (Chromium: `x.css`, not resolved).
    fn href(cx: &mut Cx<'_>, this: Platform) -> b::Result<String> {
        let rule = rule_of(cx.rt, this)?;
        let rule = rule.borrow();
        Ok(match &rule.kind {
            RuleKind::Import(import) => import.url.clone(),
            _ => String::new(),
        })
    }

    fn media(cx: &mut Cx<'_>, this: Platform) -> b::Result<Gc<Object>> {
        rule_media(cx, this)
    }

    /// Always null: swb does not load imported sheets (departure; Chromium
    /// gives the sheet once it loaded).
    fn style_sheet(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<Option<Gc<Object>>> {
        Ok(None)
    }

    /// Always null: swb ignores `layer()` of `@import`.
    fn layer_name(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<Option<String>> {
        Ok(None)
    }

    /// Always null (departure: swb keeps the condition, not its text).
    fn supports_text(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<Option<String>> {
        Ok(None)
    }
}

/// The `MediaList` of a rule wrapper (`@media`, `@import`).
fn rule_media(cx: &mut Cx<'_>, this: Platform) -> b::Result<Gc<Object>> {
    let owner = this.object;
    same_member::<RuleData>(
        cx,
        this,
        |d| &mut d.media,
        |cx| owned_object(cx, g::MEDIA_LIST, owner),
    )
}

impl g::css_grouping_rule::CSSGroupingRuleImpl for WebState {
    fn css_rules(cx: &mut Cx<'_>, this: Platform) -> b::Result<Gc<Object>> {
        let owner = this.object;
        same_member::<RuleData>(
            cx,
            this,
            |d| &mut d.rules,
            |cx| owned_object(cx, g::CSS_RULE_LIST, owner),
        )
    }

    fn insert_rule(cx: &mut Cx<'_>, this: Platform, rule: String, index: u32) -> b::Result<u32> {
        let group = rule_of(cx.rt, this)?;
        let at = model::insert_into_group(&group, &rule, index)
            .map_err(|e| group_error(e, index, &rule, false))?;
        rule_changed(cx.rt, &group)?;
        Ok(at)
    }

    fn delete_rule(cx: &mut Cx<'_>, this: Platform, index: u32) -> b::Result<()> {
        let group = rule_of(cx.rt, this)?;
        model::delete_from_group(&group, index).map_err(|e| group_error(e, index, "", true))?;
        rule_changed(cx.rt, &group)
    }
}

impl g::css_condition_rule::CSSConditionRuleImpl for WebState {
    fn condition_text(cx: &mut Cx<'_>, this: Platform) -> b::Result<String> {
        let rule = rule_of(cx.rt, this)?;
        let rule = rule.borrow();
        Ok(match &rule.kind {
            RuleKind::Media { media, .. } => media.to_string(),
            RuleKind::Supports { text, .. } => text.clone(),
            _ => String::new(),
        })
    }
}

impl g::css_media_rule::CSSMediaRuleImpl for WebState {
    fn media(cx: &mut Cx<'_>, this: Platform) -> b::Result<Gc<Object>> {
        rule_media(cx, this)
    }
}

impl g::css_keyframes_rule::CSSKeyframesRuleImpl for WebState {
    fn name(cx: &mut Cx<'_>, this: Platform) -> b::Result<String> {
        let rule = rule_of(cx.rt, this)?;
        let rule = rule.borrow();
        Ok(match &rule.kind {
            RuleKind::Keyframes(keyframes) => keyframes.name.clone(),
            _ => String::new(),
        })
    }

    /// Renames the rule (no effect on rendering: swb runs no animations).
    fn set_name(cx: &mut Cx<'_>, this: Platform, value: String) -> b::Result<()> {
        let rule = rule_of(cx.rt, this)?;
        if let RuleKind::Keyframes(keyframes) = &mut rule.borrow_mut().kind {
            keyframes.name = value;
        }
        model::mark_modified(&rule);
        Ok(())
    }
}

// ----- MediaList -----

/// Runs `f` on the media query list of a `MediaList` (that of its sheet,
/// `@media` rule or `@import` rule); returns its result and whether the
/// list changed, and tells the engine about a change that can change the
/// rendering.
fn with_media<R>(
    rt: &mut Runtime,
    this: Platform,
    f: impl FnOnce(&mut MediaQueryList) -> (R, bool),
) -> b::Result<R> {
    let (_, owner) = owner_of_list(rt, this)?;
    match owner {
        Owner::Sheet(sheet) => {
            let (result, changed) = f(&mut sheet.borrow_mut().media);
            if changed {
                sheet_changed(rt, &sheet)?;
            }
            Ok(result)
        }
        Owner::Rule(rule) => {
            let (result, changed) = match &mut rule.borrow_mut().kind {
                RuleKind::Media { media, .. } => f(media),
                RuleKind::Import(import) => f(&mut import.media),
                _ => f(&mut MediaQueryList::default()),
            };
            if changed {
                rule_changed(rt, &rule)?;
            }
            Ok(result)
        }
    }
}

impl g::media_list::MediaListImpl for WebState {
    fn length(cx: &mut Cx<'_>, this: Platform) -> b::Result<u32> {
        with_media(cx.rt, this, |m| {
            (u32::try_from(m.len()).unwrap_or(u32::MAX), false)
        })
    }

    fn media_text(cx: &mut Cx<'_>, this: Platform) -> b::Result<String> {
        with_media(cx.rt, this, |m| (m.to_string(), false))
    }

    fn set_media_text(cx: &mut Cx<'_>, this: Platform, value: String) -> b::Result<()> {
        with_media(cx.rt, this, |m| {
            *m = MediaQueryList::parse_str(&value);
            ((), true)
        })
    }

    fn item(cx: &mut Cx<'_>, this: Platform, index: u32) -> b::Result<Option<String>> {
        let index = usize::try_from(index).unwrap_or(usize::MAX);
        with_media(cx.rt, this, |m| (m.item(index), false))
    }

    /// CSSOM §4.1 "appendMedium": a query that the list has already is not
    /// added again.
    fn append_medium(cx: &mut Cx<'_>, this: Platform, medium: String) -> b::Result<()> {
        let medium = MediaQueryList::parse_str(&medium);
        with_media(cx.rt, this, |m| ((), m.append(&medium)))
    }

    /// CSSOM §4.1 "deleteMedium": `NotFoundError` if the list does not have
    /// the query.
    fn delete_medium(cx: &mut Cx<'_>, this: Platform, medium: String) -> b::Result<()> {
        let query = MediaQueryList::parse_str(&medium);
        if query.len() != 1 {
            return Ok(());
        }
        let removed = with_media(cx.rt, this, |m| {
            let removed = m.remove(&query);
            (removed, removed)
        })?;
        if removed {
            Ok(())
        } else {
            Err(b::Exception::dom(
                "NotFoundError",
                format!("Failed to delete '{medium}'."),
            ))
        }
    }

    fn index_count(cx: &mut Cx<'_>, this: Platform) -> b::Result<u32> {
        Self::length(cx, this)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages() {
        let text = |e: b::Exception| match e {
            b::Exception::Dom { name, message } => format!("{name}: {message}"),
            _ => String::new(),
        };
        assert_eq!(
            text(sheet_error(RuleError::Index { length: 1 }, 2, "", false)),
            "IndexSizeError: The index provided (2) is larger than the maximum index (1)."
        );
        assert_eq!(
            text(sheet_error(RuleError::Index { length: 2 }, 2, "", true)),
            "IndexSizeError: The index provided (2) is larger than the maximum index (1)."
        );
        assert_eq!(
            text(sheet_error(RuleError::Syntax, 0, "#b {};", false)),
            "SyntaxError: Failed to parse the rule '#b {};'."
        );
        assert_eq!(
            text(group_error(RuleError::Index { length: 1 }, 5, "", true)),
            "IndexSizeError: the index 5 is greated than the length of the rule list."
        );
    }
}
