//! Tests of the CSSOM sheet model. The texts are Chromium 148's
//! (`tools/probes/web-cssom-sheets.json`).

use super::super::memory::{BYTES_PER_ITEM, ENTRY_BYTES};
use super::*;

fn texts(sheet: &SheetRef) -> Vec<String> {
    let quirks = sheet.borrow().quirks;
    sheet
        .borrow()
        .rules()
        .iter()
        .map(|r| r.borrow_mut().css_text(quirks))
        .collect()
}

#[test]
fn rule_texts() {
    let sheet = StyleSheet::parse("", false);
    for (text, want) in [
        ("#a{}", "#a { }"),
        (
            "DIV.Foo#Bar > SPAN + i ~ b{color:RED}",
            "div.Foo#Bar > span + i ~ b { color: red; }",
        ),
        (
            "*{margin:0 auto;padding:1px 2px 3px}",
            "* { margin: 0px auto; padding: 1px 2px 3px; }",
        ),
        ("a { unknown: 1; color: blue }", "a { color: blue; }"),
        (
            "a { margin: 1px; margin-left: 2px }",
            "a { margin: 1px 1px 1px 2px; }",
        ),
        (
            "@media screen and (max-width:599px){.x{height:26px;max-width:248px}}",
            "@media screen and (max-width: 599px) {\n  .x { height: 26px; max-width: 248px; }\n}",
        ),
        (
            "@media print, (min-width: 100px) and (max-width: 200px) {}",
            "@media print, (min-width: 100px) and (max-width: 200px) {\n}",
        ),
        (
            "@media screen { @media (min-width: 1px) { a { top: 0 } } }",
            "@media screen {\n  @media (min-width: 1px) {\n  a { top: 0px; }\n}\n}",
        ),
        (
            "@supports not (display: grid) { a { top: 0 } }",
            "@supports not (display: grid) {\n  a { top: 0px; }\n}",
        ),
        (
            "@keyframes pulse{0%{opacity:1;}60%{opacity:0.3;}100%{opacity:1;}}",
            "@keyframes pulse { \n  0% { opacity: 1; }\n  60% { opacity: 0.3; }\n  100% { opacity: 1; }\n}",
        ),
        (
            "@keyframes x { from { top: 0 } to { top: 10px } }",
            "@keyframes x { \n  0% { top: 0px; }\n  100% { top: 10px; }\n}",
        ),
        (
            "@font-face { font-family: \"X Y\"; src: url(x.woff2) format(\"woff2\"); font-weight: 400 }",
            "@font-face { font-family: \"X Y\"; src: url(\"x.woff2\") format(\"woff2\"); font-weight: 400; }",
        ),
        (
            "@import url(x.css) screen;",
            "@import url(\"x.css\") screen;",
        ),
        (
            "@import 'x.css' LAYER( a.b ) supports(display:grid) print;",
            "@import url(\"x.css\") layer(a.b) supports(display:grid) print;",
        ),
        ("@import url(x.css) layer;", "@import url(\"x.css\") layer;"),
        ("@layer   a ,b.c ;", "@layer a, b.c;"),
        ("@layer x.y {}", "@layer x.y {\n}"),
        ("@layer { p { top: 0 } }", "@layer {\n  p { top: 0px; }\n}"),
        (
            "@layer d { @layer e { p { top: 1px } } span { top: 2px } @layer f, g; }",
            "@layer d {\n  @layer e {\n  p { top: 1px; }\n}\n  span { top: 2px; }\n  @layer f, g;\n}",
        ),
        (
            "@media screen { @layer h { i { top: 3px } } }",
            "@media screen {\n  @layer h {\n  i { top: 3px; }\n}\n}",
        ),
    ] {
        let length = sheet.borrow().rules().len() as u32;
        StyleSheet::insert_rule(&sheet, text, length).unwrap();
        assert_eq!(
            texts(&sheet).last().map(String::as_str),
            Some(want),
            "{text}"
        );
        StyleSheet::delete_rule(&sheet, length).unwrap();
    }
}

#[test]
fn insert_and_delete() {
    let sheet = StyleSheet::parse("#a {}", false);
    assert_eq!(
        StyleSheet::insert_rule(&sheet, "#b {}", 2),
        Err(RuleError::Index { length: 1 })
    );
    assert_eq!(
        StyleSheet::insert_rule(&sheet, "#b {};", 0),
        Err(RuleError::Syntax)
    );
    assert_eq!(
        StyleSheet::insert_rule(&sheet, "@import url(x.css);", 1),
        Err(RuleError::Hierarchy)
    );
    assert_eq!(
        StyleSheet::insert_rule(&sheet, "@import url(x.css);", 0),
        Ok(0)
    );
    assert_eq!(
        StyleSheet::insert_rule(&sheet, "#b {}", 0),
        Err(RuleError::Hierarchy)
    );
    assert_eq!(StyleSheet::insert_rule(&sheet, "#b {}", 1), Ok(1));
    assert!(sheet.borrow().is_modified());
    let first = Rc::clone(&sheet.borrow().rules()[1]);
    assert!(Rc::ptr_eq(&parent_sheet(&first).unwrap(), &sheet));
    let removed = StyleSheet::delete_rule(&sheet, 1).unwrap();
    assert!(Rc::ptr_eq(&removed, &first));
    assert!(parent_sheet(&removed).is_none());
    assert_eq!(removed.borrow_mut().css_text(false), "#b { }");
    assert_eq!(
        StyleSheet::delete_rule(&sheet, 5).unwrap_err(),
        RuleError::Index { length: 2 }
    );
}

#[test]
fn grouping_rules() {
    let sheet = StyleSheet::parse("@media (min-width: 1px) { #a { top: 1px } }", false);
    let media = Rc::clone(&sheet.borrow().rules()[0]);
    assert_eq!(insert_into_group(&media, "#c { top: 3px }", 1), Ok(1));
    assert_eq!(
        insert_into_group(&media, "@import url(x.css);", 0),
        Err(RuleError::ImportInGroup)
    );
    assert_eq!(
        insert_into_group(&media, "#c {}", 9),
        Err(RuleError::Index { length: 2 })
    );
    let inner = Rc::clone(&media.borrow().child_rules().unwrap()[0]);
    assert!(Rc::ptr_eq(&inner.borrow().parent_rule().unwrap(), &media));
    assert!(Rc::ptr_eq(&parent_sheet(&inner).unwrap(), &sheet));
    StyleSheet::delete_rule(&sheet, 0).unwrap();
    // The inner rule keeps its parent rule; the sheet is gone for both.
    assert!(Rc::ptr_eq(&inner.borrow().parent_rule().unwrap(), &media));
    assert!(parent_sheet(&inner).is_none());
    let removed = delete_from_group(&media, 0).unwrap();
    assert!(removed.borrow().parent_rule().is_none());
}

/// The rule objects of `@layer`: types, children, insertion and deletion
/// (Chromium 148, `tools/probes/cascade-layers.json`, `cssom-*` cases).
#[test]
fn layer_rules() {
    let sheet = StyleSheet::parse("@layer a, b; @layer c { p { top: 1px } } @layer { }", false);
    let rules: Vec<RuleRef> = sheet.borrow().rules().to_vec();
    assert_eq!(rules.len(), 3);
    for rule in &rules {
        assert_eq!(rule.borrow().type_number(), rule_type::NONE);
    }
    assert!(rules[0].borrow().child_rules().is_none());
    let block = &rules[1];
    assert_eq!(block.borrow().child_rules().map(<[RuleRef]>::len), Some(1));
    let name_of = |rule: &RuleRef| match &rule.borrow().kind {
        RuleKind::LayerBlock { name, .. } => name.as_ref().map(ToString::to_string),
        _ => panic!("expected a layer block"),
    };
    assert_eq!(name_of(block).as_deref(), Some("c"));
    assert_eq!(name_of(&rules[2]), None);
    // Rules go into a block, a statement or a bad name does not parse.
    assert_eq!(insert_into_group(block, "@layer z;", 0), Ok(0));
    assert_eq!(insert_into_group(block, "q { top: 2px }", 2), Ok(2));
    assert_eq!(
        insert_into_group(block, "@layer a b;", 0),
        Err(RuleError::Syntax)
    );
    assert_eq!(
        insert_into_group(block, "@import url(x.css);", 0),
        Err(RuleError::ImportInGroup)
    );
    assert_eq!(
        block.borrow_mut().css_text(false),
        "@layer c {\n  @layer z;\n  p { top: 1px; }\n  q { top: 2px; }\n}"
    );
    let inner = Rc::clone(&block.borrow().child_rules().unwrap()[1]);
    assert!(Rc::ptr_eq(&inner.borrow().parent_rule().unwrap(), block));
    assert!(Rc::ptr_eq(&parent_sheet(&inner).unwrap(), &sheet));
    delete_from_group(block, 0).unwrap();
    assert_eq!(
        StyleSheet::insert_rule(&sheet, "@layer a, ;", 0),
        Err(RuleError::Syntax)
    );
    assert_eq!(StyleSheet::insert_rule(&sheet, "@layer a, c;", 3), Ok(3));
    assert_eq!(
        texts(&sheet).last().map(String::as_str),
        Some("@layer a, c;")
    );
}

#[test]
fn script_nesting_is_bounded() {
    let sheet = StyleSheet::parse("@media screen {}", false);
    let mut group = Rc::clone(&sheet.borrow().rules()[0]);
    let mut inserted = 0;
    loop {
        match insert_into_group(&group, "@media screen {}", 0) {
            Ok(_) => inserted += 1,
            Err(e) => {
                assert_eq!(e, RuleError::TooDeep);
                break;
            }
        }
        let inner = Rc::clone(&group.borrow().child_rules().unwrap()[0]);
        group = inner;
    }
    assert_eq!(inserted, MAX_GROUP_DEPTH);
    // The serialization recurses along the nesting without trouble.
    let text = sheet.borrow().rules()[0].borrow_mut().css_text(false);
    assert_eq!(text.matches("@media").count(), MAX_GROUP_DEPTH + 1);
}

#[test]
fn style_changes_reach_the_cascade_declarations() {
    let sheet = StyleSheet::parse(".x { color: blue }", false);
    let rule = Rc::clone(&sheet.borrow().rules()[0]);
    let mut rule = rule.borrow_mut();
    let Rule { kind, memory, .. } = &mut *rule;
    let RuleKind::Style(style) = kind else {
        panic!("expected a style rule");
    };
    assert!(
        style
            .declarations_block(false)
            .set_property("width", "7px", "")
    );
    style.written(memory);
    // The cascade takes the declarations of the block at its next use.
    assert_eq!(style.declarations.len(), 0);
    let block = style.cascade_block(&ParserContext::author(None, false));
    assert_eq!(block.normal.len(), 2);
    assert_eq!(style.declarations.len(), 2);
    assert!(style.set_selectors("#a, #b", memory));
    assert!(!style.set_selectors("a::after *", memory));
    assert_eq!(style.selectors().cssom_text(), "#a, #b");
}

#[test]
fn selector_text_writes_are_bounded() {
    let memory = StyleMemory::with_limit(4096);
    let sheet = StyleSheet::parse_in(".x{}", false, &memory, false);
    let rule = Rc::clone(&sheet.borrow().rules()[0]);
    assert!(set_selector_text(&rule, "#a, #b"));
    let small = memory.used();
    assert!(small > 0);
    // The charge follows the list as it is now, not the sum of the writes.
    for _ in 0..100 {
        assert!(set_selector_text(&rule, "#c, #d"));
    }
    assert_eq!(memory.used(), small);
    // A list that does not fit changes nothing.
    let long = vec!["#e"; 1000].join(", ");
    assert!(!set_selector_text(&rule, &long));
    assert_eq!(memory.used(), small);
    let RuleKind::Style(style) = &rule.borrow().kind else {
        panic!("expected a style rule");
    };
    assert_eq!(style.selectors().cssom_text(), "#c, #d");
}

#[test]
fn quirks_apply_to_rule_blocks() {
    let sheet = StyleSheet::parse("#a { width: 10 }", true);
    assert_eq!(texts(&sheet), vec!["#a { width: 10px; }"]);
}

#[test]
fn import_rules_stay_a_prefix() {
    let sheet = StyleSheet::parse("@import url(a.css); @import url(b.css); #a {}", false);
    let import = "@import url(c.css);";
    // Between the two imports and after them: allowed.
    assert_eq!(StyleSheet::insert_rule(&sheet, import, 1), Ok(1));
    assert_eq!(StyleSheet::insert_rule(&sheet, import, 3), Ok(3));
    // After the style rule: refused. A style rule among the imports: refused.
    assert_eq!(
        StyleSheet::insert_rule(&sheet, import, 5),
        Err(RuleError::Hierarchy)
    );
    for at in 0..4 {
        assert_eq!(
            StyleSheet::insert_rule(&sheet, "#b {}", at),
            Err(RuleError::Hierarchy),
            "{at}"
        );
    }
    assert_eq!(StyleSheet::insert_rule(&sheet, "#b {}", 4), Ok(4));
    assert_eq!(StyleSheet::insert_rule(&sheet, "#b {}", 6), Ok(6));
}

/// The bytes that the parsed rule `text` is charged.
fn charge(text: &str) -> usize {
    rule_bytes(&parse_rule(text).unwrap())
}

#[test]
fn inserted_rules_count_against_the_script_limit() {
    let rule = ".a { top: 1px; left: 2px }";
    let cost = charge(rule);
    let memory = StyleMemory::with_limit(3 * cost + cost / 2);
    let sheet = StyleSheet::parse_in("#page { top: 1px }", false, &memory, false);
    // The page's own rules are free.
    assert_eq!(memory.used(), 0);
    for i in 0..3 {
        assert_eq!(StyleSheet::insert_rule(&sheet, rule, i), Ok(i));
    }
    assert_eq!(memory.used(), 3 * cost);
    assert_eq!(
        StyleSheet::insert_rule(&sheet, rule, 0),
        Err(RuleError::Memory)
    );
    assert_eq!(sheet.borrow().rules().len(), 4);
    // A removed rule gives its bytes back when nothing holds it.
    drop(StyleSheet::delete_rule(&sheet, 0).unwrap());
    assert_eq!(memory.used(), 2 * cost);
    assert_eq!(StyleSheet::insert_rule(&sheet, rule, 0), Ok(0));
    // A rule inside a group counts too, and a syntax error returns its bytes.
    let group_sheet = StyleSheet::parse_in("@media screen { }", false, &memory, false);
    let group = Rc::clone(&group_sheet.borrow().rules()[0]);
    assert_eq!(insert_into_group(&group, rule, 0), Err(RuleError::Memory));
    drop(sheet);
    assert_eq!(memory.used(), 0);
    assert_eq!(
        insert_into_group(&group, "#b {};", 0),
        Err(RuleError::Syntax)
    );
    assert_eq!(memory.used(), 0);
    assert_eq!(insert_into_group(&group, rule, 0), Ok(0));
    assert_eq!(memory.used(), cost);
}

#[test]
fn the_charge_follows_the_parsed_form() {
    // A shorthand costs the entries of its longhands, and `all` those of
    // (almost) all longhands, whatever the length of the text.
    let plain = charge(".a { top: 1px }");
    let margin = charge(".a { margin: 1px }");
    let all = charge(".a { all: initial }");
    assert!(margin > plain + 3 * ENTRY_BYTES, "{plain} {margin}");
    assert!(all > 50 * ENTRY_BYTES, "{all}");
    assert!(all > 20 * estimate_style_bytes(".a { all: initial }"));
    // Declarations that the cascade drops cost no entries.
    assert!(charge(".a { unknown: 1px }") < plain);
    // The estimate before the parse counts the text only.
    assert_eq!(estimate_style_bytes(""), 0);
    let text = ".a{--v:1;--w:2;}";
    assert_eq!(
        estimate_style_bytes(text),
        text.len() * 2 + 3 * BYTES_PER_ITEM
    );
}

#[test]
fn inserted_rule_of_many_entries_is_refused_before_the_stylist_sees_it() {
    let memory = StyleMemory::with_limit(1 << 20);
    let sheet = StyleSheet::parse_in("", false, &memory, false);
    let text = ".a { all: initial }";
    assert!(estimate_style_bytes(text) < memory.free());
    let fits = (memory.free() / charge(text)) as u32;
    for i in 0..fits {
        assert_eq!(StyleSheet::insert_rule(&sheet, text, i), Ok(i));
    }
    assert_eq!(
        StyleSheet::insert_rule(&sheet, text, fits),
        Err(RuleError::Memory)
    );
    assert!(memory.used() <= 1 << 20);
}

#[test]
fn script_sheet_text_that_does_not_fit_has_no_rules() {
    let css = "#a { top: 1px } #b { top: 2px }";
    let cost = rules_bytes(&parse_stylesheet(css).rules);
    let memory = StyleMemory::with_limit(cost + 1);
    let first = StyleSheet::parse_in(css, false, &memory, true);
    assert_eq!(first.borrow().rules().len(), 2);
    assert_eq!(memory.used(), cost);
    // The style rules hold their declarations; the sheet holds the rest.
    let held = first.borrow().held_bytes();
    assert!(held > 0 && held < cost, "{held} {cost}");
    let second = StyleSheet::parse_in(css, false, &memory, true);
    assert!(second.borrow().rules().is_empty());
    // The page's own text is not counted and is never refused.
    let own = StyleSheet::parse_in(css, false, &memory, false);
    assert_eq!(own.borrow().rules().len(), 2);
    drop(first);
    assert_eq!(memory.used(), 0);
}

/// Writes `value` to `property` of the rule as the bindings do; true if it
/// was set.
fn set(rule: &RuleRef, property: &str, value: &str) -> bool {
    let mut guard = rule.borrow_mut();
    let budget = guard.write_budget(false);
    let RuleKind::Style(style) = &mut guard.kind else {
        panic!("expected a style rule");
    };
    let changed = style
        .declarations_block(false)
        .set_property_within(property, value, "", &budget);
    if changed {
        guard.declarations_written();
    }
    changed
}

#[test]
fn declaration_writes_are_charged_as_the_block_is_after_the_write() {
    let memory = StyleMemory::with_limit(MAX_SCRIPT_STYLE_BYTES);
    let sheet = StyleSheet::parse_in("", false, &memory, false);
    StyleSheet::insert_rule(&sheet, ".x { color: blue }", 0).unwrap();
    let rule = Rc::clone(&sheet.borrow().rules()[0]);
    let first = memory.used();
    // A write of the same property again and again does not add up.
    assert!(set(&rule, "transform", "rotate(1deg)"));
    let one = memory.used();
    assert!(one > first);
    for i in 0..1000 {
        assert!(set(&rule, "transform", &format!("rotate({i}deg) ")));
    }
    assert!(memory.used() < one + 400, "{} {one}", memory.used());
    // A longer value costs more, and removing the property gives the bytes
    // back.
    assert!(set(
        &rule,
        "font-family",
        &format!("{}b", "a, ".repeat(2000))
    ));
    let long = memory.used();
    assert!(long > one + 4000, "{long} {one}");
    assert!(set(&rule, "font-family", "serif"));
    assert!(memory.used() < one + 1000);
    let mut guard = rule.borrow_mut();
    let RuleKind::Style(style) = &mut guard.kind else {
        panic!("expected a style rule");
    };
    let block = style.declarations_block(false);
    assert!(block.remove_property("transform").1);
    assert!(block.remove_property("font-family").1);
    guard.declarations_written();
    drop(guard);
    assert!(memory.used() <= first);
}

#[test]
fn a_write_that_does_not_fit_changes_nothing() {
    let memory = StyleMemory::with_limit(8 << 10);
    let sheet = StyleSheet::parse_in("", false, &memory, false);
    StyleSheet::insert_rule(&sheet, ".x { color: blue }", 0).unwrap();
    let rule = Rc::clone(&sheet.borrow().rules()[0]);
    let mut set_count = 0;
    while set(&rule, &format!("--v{set_count}"), "1") {
        set_count += 1;
        assert!(set_count < 1000, "the budget never ends");
    }
    assert!(set_count > 5);
    assert!(memory.used() <= 8 << 10);
    let used = memory.used();
    let length = |rule: &RuleRef| {
        let mut guard = rule.borrow_mut();
        let RuleKind::Style(style) = &mut guard.kind else {
            panic!("expected a style rule");
        };
        style.declarations_block(false).len()
    };
    let before = length(&rule);
    // The refused write changes nothing, also for a shorthand.
    assert!(!set(&rule, "margin", "1px"));
    assert_eq!(length(&rule), before);
    assert_eq!(memory.used(), used);
    // `cssText` that does not fit changes nothing, and one that fits
    // replaces the block.
    assert!(!set_declarations_text(
        &rule,
        &"margin: 1px; ".repeat(100),
        false
    ));
    assert_eq!(length(&rule), before);
    assert!(set_declarations_text(&rule, "top: 1px", false));
    assert_eq!(length(&rule), 1);
    assert!(memory.used() < used);
    drop(rule);
    drop(sheet);
    assert_eq!(memory.used(), 0);
}

#[test]
fn rules_of_the_pages_own_sheets_are_charged_when_written() {
    let memory = StyleMemory::with_limit(MAX_SCRIPT_STYLE_BYTES);
    let sheet = StyleSheet::parse_in(".x { color: blue; top: 1px }", false, &memory, false);
    let rule = Rc::clone(&sheet.borrow().rules()[0]);
    assert_eq!(memory.used(), 0);
    assert!(set(&rule, "left", "2px"));
    let used = memory.used();
    assert!(used > 0);
    assert!(set(&rule, "left", "3px"));
    assert_eq!(memory.used(), used);
}
