//! Style sheets and rules for scripts (M9 feature 6, ADR 0028 section 10):
//! every case of the Chromium probe `tools/probes/web-cssom-sheets.json`
//! (`document.styleSheets`, `sheet`, `CSSStyleSheet`, `insertRule` and
//! `deleteRule` with their errors, `CSSRuleList`, the rules and their
//! `cssText`, `selectorText` and `style` with their effect on the computed
//! style, grouping rules, `MediaList`, `disabled`, the cascade order of
//! inserted rules, the interface objects, the sheets of `link` elements of
//! the page's and another origin), with the pages and resources served as
//! the probe's server does; a test of the CSS-in-JS pattern (rules inserted
//! into a `style` element's sheet before and after the elements exist);
//! and the flush counters of rule changes.

// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

#[path = "cssom_sheets/cases.rs"]
mod cases;
mod common;

use std::sync::Arc;
use std::time::Duration;

use swb_engine::{Page, Size, Url};
use swb_net::{Fetcher, Headers, NetError, Request, Response};

use common::{answer, new_page, on_page_thread};

/// A probe case with Chromium's values.
pub(crate) struct Case {
    pub(crate) name: &'static str,
    pub(crate) html: &'static str,
    /// The resources next to the page: path, content type, text.
    pub(crate) resources: &'static [(&'static str, &'static str, &'static str)],
    /// The queries and Chromium's values.
    pub(crate) queries: &'static [(&'static str, &'static str)],
}

/// Where swb departs from Chromium on purpose: (case, query index, swb's
/// value, why).
const SWB_VALUES: &[(&str, usize, &str, &str)] = &[
    (
        "insert-rule",
        2,
        r#"["DOMException HierarchyRequestError: Failed to execute 'insertRule' on 'CSSStyleSheet': Failed to insert the rule.", 0, "DOMException SyntaxError: Failed to execute 'insertRule' on 'CSSStyleSheet': Failed to parse the rule '@charset \"utf-8\";'.", "DOMException SyntaxError: Failed to execute 'insertRule' on 'CSSStyleSheet': Failed to parse the rule '@namespace svg url(http://www.w3.org/2000/svg);'.", "DOMException SyntaxError: Failed to execute 'insertRule' on 'CSSStyleSheet': Failed to parse the rule '@unknown {}'.", "DOMException SyntaxError: Failed to execute 'insertRule' on 'CSSStyleSheet': Failed to parse the rule '@unknown x;'.", 1, 3]"#,
        "swb's model has no `@namespace` rule, so it is a syntax error (Chromium: `InvalidStateError` after other rules)",
    ),
    (
        "insert-rule",
        4,
        r#"[[0, 1, 2, 3, "DOMException SyntaxError: Failed to execute 'insertRule' on 'CSSStyleSheet': Failed to parse the rule '@page { margin: 1cm }'.", "DOMException SyntaxError: Failed to execute 'insertRule' on 'CSSStyleSheet': Failed to parse the rule '@layer base;'.", "DOMException SyntaxError: Failed to execute 'insertRule' on 'CSSStyleSheet': Failed to parse the rule '@layer base { #a { color: red } }'.", "DOMException SyntaxError: Failed to execute 'insertRule' on 'CSSStyleSheet': Failed to parse the rule '@container (min-width: 10px) { #a { color: red } }'.", "DOMException SyntaxError: Failed to execute 'insertRule' on 'CSSStyleSheet': Failed to parse the rule '@counter-style x { system: cyclic; symbols: a }'.", "DOMException SyntaxError: Failed to execute 'insertRule' on 'CSSStyleSheet': Failed to parse the rule '::-webkit-scrollbar { width: 0 }'.", 4, "DOMException SyntaxError: Failed to execute 'insertRule' on 'CSSStyleSheet': Failed to parse the rule 'a:-moz-focusring {}'.", 5, "DOMException SyntaxError: Failed to execute 'insertRule' on 'CSSStyleSheet': Failed to parse the rule '.a::after .b {}'.", 6, "DOMException SyntaxError: Failed to execute 'insertRule' on 'CSSStyleSheet': Failed to parse the rule 'svg|a {}'.", 7, "DOMException SyntaxError: Failed to execute 'insertRule' on 'CSSStyleSheet': Failed to parse the rule 'a:unknown-pseudo {}'."], "CSSMediaRule 4,CSSSupportsRule 12,CSSKeyframesRule 7,CSSFontFaceRule 5,CSSStyleRule 1,CSSStyleRule 1,CSSStyleRule 1,CSSStyleRule 1"]"#,
        "the rules that swb's parser drops (`@page`, `@layer`, `@container`, `@counter-style`, `::-webkit-scrollbar`) are syntax errors",
    ),
    (
        "rule-objects",
        1,
        r##"["#a { }", "div.Foo#Bar > span + i ~ b { color: red; }", "a:hover, a:not(.x, .y)::before { content: \"x\"; }", "[data-x=\"1\"], [title~=\"a\" i] { top: 0px; }", "* { margin: 0px auto; padding: 1px 2px 3px; }", "a { --x: { a }; width: calc(3px); color: red !important; }", "a { background: url(\"x.png\") no-repeat red; }", "a { font: bold 12px / 1.5 Arial, sans-serif; }", "a { color: blue; }", ".a:nth-child(2n+1):first-child::after { }", ":is(a, b) :where(.c) { }", "a:has(> img) { }", "a { color: blue; }", "a { margin: 1px 1px 1px 2px; }", ".bXzSUE #inner, .bXzSUE #outer { fill: rgb(224, 0, 0); }", ".x { display: flex; justify-content: center; align-items: center; border-radius: 50%; box-sizing: border-box; }", "a::before, a:hover, a:first-child { }", "a > b c + d { }", "a:nth-child(-n+3), a:nth-of-type(2n+1), a:nth-last-child(2n) { }", "a[href^=\"http\"], a[lang|=\"en\" s] { }"]"##,
        "swb's selector parser accepts the `s` flag of attribute selectors (Chromium 148 does not)",
    ),
    (
        "rule-objects",
        2,
        r#"["@media screen and (max-width: 599px) {\n  .x { height: 26px; max-width: 248px; }\n}", "@media (hover: hover) {\n  .x { color: red; }\n  .y { color: blue; }\n}", "@media print, (min-width: 100px) and (max-width: 200px) {\n}", "@media (foo) {\n  a { }\n}", "@media screen {\n  @media (min-width: 1px) {\n  a { top: 0px; }\n}\n}", "@media screen and (min-width: 1px) {\n}", "@supports (text-size-adjust:none) {\n  body { }\n}", "@supports not (display: grid) {\n  a { top: 0px; }\n}", "@supports (display: grid) and (not (display: inline-grid)) {\n}", "@keyframes pulse { \n  0% { opacity: 1; }\n  60% { opacity: 0.3; }\n  100% { opacity: 1; }\n}", "@keyframes x { \n  0% { top: 0px; }\n  100% { top: 10px; }\n}", "@font-face { font-family: \"X Y\"; src: url(\"x.woff2\") format(\"woff2\"); font-weight: 400; }"]"#,
        "swb does not parse `text-size-adjust`, so the declaration is not in the block",
    ),
    (
        "grouping-rules",
        4,
        r#"[7, "CSSKeyframesRule", "k", "@keyframes k { \n  0% { top: 0px; }\n  50% { top: 5px; }\n  100% { top: 10px; }\n}", "none", "none"]"#,
        "swb's `CSSKeyframesRule` has only `name` (no `cssRules`, no `CSSKeyframeRule`)",
    ),
    (
        "grouping-rules",
        5,
        r#"[5, "CSSFontFaceRule", "@font-face { font-family: X; src: url(\"x.woff2\"); font-weight: 700; }", "none"]"#,
        "swb's `CSSFontFaceRule` has no `style`",
    ),
    (
        "grouping-rules",
        7,
        r#""CSSStyleRule,CSSStyleRule""#,
        "swb's parser flattens `@layer` blocks and drops `@layer` statements, `@page` and `@namespace`",
    ),
    (
        "interfaces",
        0,
        r#""StyleSheet:function CSSStyleSheet:function StyleSheetList:function CSSRuleList:function CSSRule:function CSSStyleRule:function CSSGroupingRule:function CSSConditionRule:function CSSMediaRule:function CSSSupportsRule:function CSSImportRule:function CSSFontFaceRule:function CSSKeyframesRule:function CSSKeyframeRule:undefined MediaList:function CSSPageRule:undefined CSSNamespaceRule:undefined CSSLayerBlockRule:undefined""#,
        "swb has no rule objects for the rules that its parser drops",
    ),
    (
        "interfaces",
        1,
        r#"[true, true, true, true, true, true, true, "TypeError: Failed to construct 'CSSRule': Illegal constructor", "TypeError: Failed to construct 'StyleSheet': Illegal constructor", "TypeError: Failed to construct 'CSSRuleList': Illegal constructor", "TypeError: Failed to construct 'StyleSheetList': Illegal constructor", "TypeError: Failed to construct 'MediaList': Illegal constructor", "TypeError: Failed to construct 'CSSStyleRule': Illegal constructor", "TypeError: Failed to construct 'CSSStyleSheet': Illegal constructor"]"#,
        "no constructable style sheets (backlog)",
    ),
    (
        "interfaces",
        2,
        r#""StyleSheet: type,href,ownerNode,parentStyleSheet,title,media,disabled,constructor | CSSStyleSheet: ownerRule,cssRules,rules,addRule,deleteRule,insertRule,removeRule,constructor | StyleSheetList: length,item,constructor | CSSRuleList: length,item,constructor | CSSRule: type,cssText,parentRule,parentStyleSheet,STYLE_RULE,CHARSET_RULE,IMPORT_RULE,MEDIA_RULE,FONT_FACE_RULE,PAGE_RULE,MARGIN_RULE,NAMESPACE_RULE,KEYFRAMES_RULE,KEYFRAME_RULE,COUNTER_STYLE_RULE,FONT_FEATURE_VALUES_RULE,SUPPORTS_RULE,constructor | CSSStyleRule: selectorText,style,constructor | CSSGroupingRule: cssRules,deleteRule,insertRule,constructor | CSSConditionRule: conditionText,constructor | CSSMediaRule: media,constructor | CSSSupportsRule: constructor | CSSImportRule: href,media,styleSheet,layerName,supportsText,constructor | CSSFontFaceRule: constructor | CSSKeyframesRule: name,constructor | MediaList: length,mediaText,appendMedium,deleteMedium,item,toString,constructor""#,
        "the members that swb leaves out: `replace`, `replaceSync`, `styleMap` and the nesting members of `CSSStyleRule`, `CSSFontFaceRule.style`, the keyframe members",
    ),
    (
        "interfaces",
        3,
        r#""styleSheets:true:true adoptedStyleSheets:false:false HTMLStyleElement.sheet:true,HTMLLinkElement.sheet:true,SVGStyleElement.sheet:true""#,
        "no `adoptedStyleSheets` (backlog)",
    ),
    (
        "link-sheets",
        0,
        r#"["l1 O/case-11/a.css T screen false text/css", "l3 O/case-11/missing.css   false text/css", "l4 X/case-11/c.css   false text/css", "l5 O/case-11/b.css   false text/css"]"#,
        "swb does not load alternate style sheets (`rel=\"alternate stylesheet\"`)",
    ),
    (
        "link-sheets",
        1,
        r##"[1, "#a { color: rgb(0, 0, 9); }", true, {}, null, "rgb(0, 0, 9)", "auto", "4px", "rgb(0, 0, 7)"]"##,
        "swb does not load alternate style sheets",
    ),
    (
        "link-sheets",
        3,
        r#"["CSSImportRule", "i.css", true, "@import url(\"i.css\");", "auto"]"#,
        "swb does not load the sheets of `@import`",
    ),
    (
        "link-sheets",
        4,
        r#"[[null, 4], true, 1, 5, "O/case-11/slow.css"]"#,
        "swb does not load alternate style sheets (one sheet fewer in the list)",
    ),
    (
        "link-sheets",
        5,
        r"[false, null, null]",
        "a `link` whose `href` changed has no sheet until the new one loads (Chromium keeps the old one)",
    ),
    (
        "hidden-and-quirks",
        1,
        r"[null, null, 0]",
        "swb gives style sheets only to the elements of the window's document",
    ),
];

/// The origin of the case pages and of the probe's second server.
const ORIGIN: &str = "http://127.0.0.1:8000";
const OTHER: &str = "http://127.0.0.1:8001";

/// Serves the page and the resources of the case `number` (1-based) on
/// both origins, as the probe's server does; other URLs get a 404.
struct CaseServer {
    folder: String,
    page: String,
    resources: Vec<(String, &'static str, String)>,
}

impl CaseServer {
    fn new(number: usize, case: &Case) -> CaseServer {
        let folder = format!("case-{number}");
        let replace = |text: &str| {
            text.replace("$ORIGIN", ORIGIN)
                .replace("$OTHER", OTHER)
                .replace("$FOLDER", &folder)
        };
        CaseServer {
            page: replace(case.html),
            resources: case
                .resources
                .iter()
                .map(|(path, ty, text)| ((*path).to_owned(), *ty, replace(text)))
                .collect(),
            folder: folder.clone(),
        }
    }

    fn page_url(&self) -> Url {
        Url::parse(&format!("{ORIGIN}/{0}/{0}.html", self.folder)).unwrap()
    }
}

impl Fetcher for CaseServer {
    fn fetch(&self, request: &Request) -> Result<Response, NetError> {
        let path = request.url.path().trim_start_matches('/');
        let file = path
            .strip_prefix(&format!("{}/", self.folder))
            .unwrap_or("");
        let (status, ty, body) = if request.url == self.page_url() {
            (200, "text/html", self.page.as_bytes().to_vec())
        } else if let Some((_, ty, text)) = self.resources.iter().find(|(p, _, _)| p == file) {
            (200, *ty, text.as_bytes().to_vec())
        } else {
            (404, "text/plain", b"not found".to_vec())
        };
        let headers: Headers = [("content-type", ty)].into_iter().collect();
        Ok(Response {
            url: request.url.clone(),
            status,
            headers,
            body,
            redirected: false,
        })
    }
}

/// A page with JavaScript on that shows `case`, loaded in the probe's
/// viewport.
fn open(number: usize, case: &Case) -> Page {
    let server = CaseServer::new(number, case);
    let url = server.page_url();
    let mut page = new_page(Arc::new(server), 2, Size::new(400.0, 200.0));
    page.set_javascript(true);
    page.navigate(url);
    assert!(page.wait_until_loaded(Duration::from_secs(20)));
    page
}

/// Runs a case: loads it and compares each answer with Chromium's.
fn run_case(number: usize, case: &'static Case) -> Vec<String> {
    let mut page = open(number, case);
    let mut failures = Vec::new();
    for (index, &(query, chromium)) in case.queries.iter().enumerate() {
        let want = SWB_VALUES
            .iter()
            .find(|v| v.0 == case.name && v.1 == index)
            .map_or(chromium, |v| v.2);
        let want: serde_json::Value = serde_json::from_str(want).unwrap();
        let got = answer(&mut page, query);
        if got != want {
            failures.push(format!(
                "{} #{index}: {query}\n  expected: {want}\n  actual:   {got}",
                case.name
            ));
        }
    }
    failures
}

#[test]
fn probe_cases() {
    let runs: Vec<_> = cases::CASES
        .iter()
        .enumerate()
        .map(|(i, case)| {
            std::thread::Builder::new()
                .stack_size(16 << 20)
                .spawn(move || run_case(i + 1, case))
                .unwrap()
        })
        .collect();
    let failures: Vec<String> = runs
        .into_iter()
        .flat_map(|run| run.join().unwrap())
        .collect();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A page that styles itself as CSS-in-JS libraries do: an empty `style`
/// element whose sheet gets rules by `insertRule` at computed indexes,
/// before the elements exist and while they change, and `deleteRule` of a
/// group. The computed styles follow the rule order (the script is swb's
/// own; the expected values follow from the cascade).
#[test]
fn css_in_js_pattern() {
    on_page_thread(|| {
        let case = Case {
            name: "css-in-js",
            html: "<!DOCTYPE html><html><head><script>
const tag = document.createElement('style');
document.head.appendChild(tag);
tag.appendChild(document.createTextNode(''));
const sheet = tag.sheet;
// One group of rules per component; a group's rules sit after those of
// the groups before it.
const sizes = [];
function insert(group, rules) {
  let at = 0;
  for (let g = 0; g <= group; g++) at += sizes[g] || 0;
  for (const r of rules) { sheet.insertRule(r, at++); }
  sizes[group] = (sizes[group] || 0) + rules.length;
}
function clear(group) {
  let at = 0;
  for (let g = 0; g < group; g++) at += sizes[g] || 0;
  for (let i = 0; i < (sizes[group] || 0); i++) sheet.deleteRule(at);
  sizes[group] = 0;
}
insert(1, ['.b1 { color: rgb(0, 0, 1) }', '@media screen { .b1 { width: 10px } }']);
insert(0, ['.a1 { color: rgb(1, 0, 0) }']);
</script></head><body><div id=x class='a1 b1'>x</div><script>
const early = [getComputedStyle(x).color, getComputedStyle(x).width];
insert(0, ['.a2 { color: rgb(2, 0, 0) }']);
</script></body></html>",
            resources: &[],
            queries: &[],
        };
        let mut page = open(1, &case);
        let q = |page: &mut Page, s: &str| answer(page, s).to_string();
        assert_eq!(q(&mut page, "early"), r#"["rgb(0, 0, 1)","10px"]"#);
        // The rules of group 1 come after those of group 0.
        assert_eq!(
            q(
                &mut page,
                "x.className = 'a2 b1'; getComputedStyle(x).color"
            ),
            r#""rgb(0, 0, 1)""#
        );
        assert_eq!(
            q(
                &mut page,
                "clear(1); [getComputedStyle(x).color, getComputedStyle(x).width, sheet.cssRules.length]"
            ),
            r#"["rgb(2, 0, 0)","384px",2]"#
        );
        assert_eq!(
            q(
                &mut page,
                "insert(2, ['#x { margin-left: 3px }']); [x.offsetLeft, Array.from(sheet.cssRules).map(r => r.cssText).join(' | ')]"
            ),
            r#"[11,".a1 { color: rgb(1, 0, 0); } | .a2 { color: rgb(2, 0, 0); } | #x { margin-left: 3px; }"]"#
        );
        // The rules are not in the element's text.
        assert_eq!(q(&mut page, "tag.textContent"), r#""""#);
    });
}

/// Rule changes make the stylist dirty: a style query after each change
/// builds the rule index once, without a parse of the other rules; a
/// query after no change builds nothing.
#[test]
fn rule_changes_rebuild_the_index_once_per_flush() {
    on_page_thread(|| {
        let case = Case {
            name: "counts",
            html: "<!DOCTYPE html><style id=s>p { color: red }</style><p id=p>x</p>",
            resources: &[],
            queries: &[],
        };
        let mut page = open(1, &case);
        page.update_layout();
        let before = page.flush_counters();
        let script = "const sh = s.sheet; for (let i = 0; i < 50; i++) sh.insertRule('.c' + i + ' { top: ' + i + 'px }', sh.cssRules.length); sh.cssRules[0].style.color = 'rgb(1, 2, 3)'; getComputedStyle(p).color";
        assert_eq!(answer(&mut page, script), "rgb(1, 2, 3)");
        assert_eq!(
            answer(&mut page, "getComputedStyle(p).color"),
            "rgb(1, 2, 3)"
        );
        let after = page.flush_counters();
        assert_eq!(after.sheet_changes - before.sheet_changes, 51);
        assert_eq!(after.stylist_runs - before.stylist_runs, 1);
    });
}

/// The style data that scripts make is bounded (ADR 0028 section 16): rules
/// of `insertRule`, the text of script-made `style` elements and the text of
/// `rule.style.cssText` count; the sheets of the page do not, and the bytes
/// return when the rules are freed.
#[test]
fn script_style_data_is_bounded() {
    on_page_thread(|| {
        let case = Case {
            name: "bound",
            html: "<!DOCTYPE html><style id=s>p { color: red }</style><p id=p>x</p>",
            resources: &[],
            queries: &[],
        };
        let mut page = open(1, &case);
        let q = |page: &mut Page, s: &str| answer(page, s).to_string();
        // About 90 MB estimated per insert: one fits in 128 MiB, the second
        // is refused.
        let setup = "var sh = s.sheet; var big = '.a{' + '--v:1;'.repeat(200000) + '}'; \
                     var names = []; \
                     function ins() { try { sh.insertRule(big, 0); return 'ok'; } \
                     catch (e) { return e.name; } }";
        q(&mut page, setup);
        assert_eq!(
            q(&mut page, "[ins(), ins(), ins(), sh.cssRules.length]"),
            r#"["ok","QuotaExceededError","QuotaExceededError",2]"#
        );
        // A new style element with the same text has no rules while the rule
        // is held, and the text stays in the DOM.
        let make = "var e = document.createElement('style'); e.textContent = big; \
                    document.head.appendChild(e); \
                    [e.sheet.cssRules.length, e.textContent.length > 1000000]";
        assert_eq!(q(&mut page, make), "[0,true]");
        // The bytes return with the rule (nothing holds the deleted one),
        // and the same text then makes a sheet of one rule.
        assert_eq!(q(&mut page, "sh.deleteRule(0); sh.cssRules.length"), "1");
        assert_eq!(q(&mut page, make), "[1,true]");
        // The page's own sheet and rule are never refused; `cssText` of a
        // rule that does not fit changes nothing.
        let set = "var r = sh.cssRules[0]; r.style.cssText = 'color: blue'; \
                   var a = r.style.cssText; ins(); \
                   r.style.cssText = '--w:1;'.repeat(200000); \
                   [a, r.style.cssText, getComputedStyle(p).color]";
        assert_eq!(
            q(&mut page, set),
            r#"["color: blue;","color: blue;","rgb(0, 0, 255)"]"#
        );
    });
}

/// Every write of a rule's declarations counts, as the block is after the
/// write (ADR 0028 section 16): `setProperty`, the property setters,
/// `cssFloat`, `removeProperty` and `cssText`. A loop of new properties
/// reaches the limit, and a loop that writes one property again and again
/// does not.
#[test]
fn rule_style_writes_are_bounded() {
    on_page_thread(|| {
        let case = Case {
            name: "style-writes",
            html: "<!DOCTYPE html><style id=s>p { color: red }</style><p id=p>x</p>",
            resources: &[],
            queries: &[],
        };
        let mut page = open(1, &case);
        let q = |page: &mut Page, s: &str| answer(page, s).to_string();
        // Values of 100 KB: about 300 KB estimated per entry, so a few
        // hundred entries fill 128 MiB.
        let setup = "var sh = s.sheet; sh.insertRule('.a { }', 0); sh.insertRule('.b { }', 1); \
                     var a = sh.cssRules[0].style, b = sh.cssRules[1].style; \
                     var v = 'x'.repeat(100000), fam = 'a, '.repeat(30000) + 'b'; \
                     for (var i = 0; i < 2000; i++) a.setProperty('--v' + i, v); \
                     var n = a.length; \
                     a.setProperty('--big', v); a.fontFamily = fam; \
                     [n > 100, n < 1000, a.getPropertyValue('--big'), a.fontFamily, a.length === n]";
        assert_eq!(q(&mut page, setup), r#"[true,true,"","",true]"#);
        // The refused writes changed nothing, and small writes still fit;
        // `removeProperty` gives the bytes back, and then the big ones fit.
        let free = "a.cssFloat = 'left'; a.color = 'blue'; \
                    for (var i = 0; i < 100; i++) a.removeProperty('--v' + i); \
                    a.setProperty('--big', v); a.fontFamily = fam; \
                    [a.cssFloat, a.color, a.getPropertyValue('--big') === v, \
                     a.fontFamily.length > 90000]";
        assert_eq!(q(&mut page, free), r#"["left","blue",true,true]"#);
        // One property written many times does not add up.
        let again = "var last; for (var i = 0; i < 20000; i++) { last = 'rotate(' + i + 'deg)'; \
                     b.transform = last; } \
                     [b.transform, b.length]";
        assert_eq!(q(&mut page, again), r#"["rotate(19999deg)",1]"#);
    });
}

/// `style.textContent = <the same text>` keeps the sheet of the element
/// (as Chromium does) unless a mutation observer is installed: then the
/// text node is replaced, as the DOM standard says and Chromium does for
/// the node, and the element gets a new sheet (departure: Chromium keeps
/// the sheet and queues the record; inserted rules are lost in swb).
#[test]
fn text_content_of_the_same_text_with_an_observer() {
    on_page_thread(|| {
        let case = Case {
            name: "text-content",
            html: "<!DOCTYPE html><style id=s>p { color: red }</style><p id=p>x</p>",
            resources: &[],
            queries: &[],
        };
        let mut page = open(1, &case);
        let q = |page: &mut Page, s: &str| answer(page, s).to_string();
        let plain = "var sheet = s.sheet; sheet.insertRule('.z { top: 1px }', 1); \
                     s.textContent = s.textContent; \
                     [s.sheet === sheet, s.sheet.cssRules.length]";
        assert_eq!(q(&mut page, plain), "[true,2]");
        let observed = "var seen = 0; new MutationObserver(function (r) { seen += r.length; }) \
                        .observe(s, { childList: true }); \
                        var before = s.sheet; s.textContent = s.textContent; \
                        [s.sheet === before, s.sheet.cssRules.length]";
        assert_eq!(q(&mut page, observed), "[false,1]");
    });
}
