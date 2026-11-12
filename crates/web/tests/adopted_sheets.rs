//! `adoptedStyleSheets` and constructed style sheets in the bindings (M10
//! sub-feature 5c-2, ADR 0036 section 6), with and without the GC stress
//! mode: the list that the host gets for each scope follows the script
//! array through every trap and the setter; the sheets that an array holds
//! live through collections, and none stays when the arrays let them go or
//! the runtime ends; a root that moves to another document loses its sheets.

#![allow(clippy::unwrap_used, reason = "the setup of test runtimes")]

mod common;

use std::rc::{Rc, Weak};

use common::{TestDocument, answer, load, on_big_stack};
use swb_dom::NodeId;
use swb_web::ScriptRuntime;

const PAGE: &str = "<!DOCTYPE html><html><body><div id=h></div><script>\
    var R = h.attachShadow({mode: 'open'});\
    function S(t) { var s = new CSSStyleSheet(); s.replaceSync(t); return s; }\
    var A = S('a{}'), B = S('b{}'), C = S('c{}');\
    function texts(l) { return l.map(function (s) { return s.cssRules.length ? s.cssRules[0].cssText : ''; }).join(); }\
    </script></body></html>";

/// Runs `f` on the test document that the runtime holds.
fn with_doc<T>(rt: &mut ScriptRuntime, f: impl FnOnce(&mut TestDocument) -> T) -> T {
    let host = rt.take_back().unwrap();
    let mut doc = host.into_any().downcast::<TestDocument>().unwrap();
    let result = f(&mut doc);
    rt.lend(doc);
    result
}

/// The texts of the first rules of the host's list of `scope`, as `texts`
/// gives them for the script array.
fn host_texts(rt: &mut ScriptRuntime, scope: NodeId) -> String {
    with_doc(rt, |doc| {
        let list = doc.adopted.get(&scope).cloned().unwrap_or_default();
        list.iter()
            .map(|sheet| {
                let sheet = sheet.borrow();
                let quirks = false;
                sheet
                    .rules()
                    .first()
                    .map(|r| r.borrow_mut().css_text(quirks))
                    .unwrap_or_default()
            })
            .collect::<Vec<_>>()
            .join(",")
    })
}

/// The id of the shadow root `R`.
fn root_id(rt: &mut ScriptRuntime) -> NodeId {
    with_doc(rt, |doc| {
        let host = doc.dom.element_by_id("h").unwrap();
        doc.dom.shadow_root(host).unwrap()
    })
}

/// After each change of a root's and the document's array, the host's list
/// equals the array (also after the changes that throw part-way).
#[test]
fn host_lists_follow_the_arrays() {
    on_big_stack(|| {
        for stress in [false, true] {
            let mut rt = load(PAGE, stress);
            let root = root_id(&mut rt);
            for (scope, array) in [
                (root, "R.adoptedStyleSheets"),
                (NodeId::DOCUMENT, "document.adoptedStyleSheets"),
            ] {
                for step in [
                    "a.push(A); a.push(B)",
                    "a.unshift(C)",
                    "a.splice(1, 1)",
                    "a.length = 1",
                    "a[1] = A",
                    "a[0] = B",
                    "a.reverse()",
                    "a.sort(function (x, y) { return x === A ? -1 : 1; })",
                    "try { a.push(C, 1); } catch (e) {}",
                    "try { a[7] = A; } catch (e) {}",
                    "Object.defineProperty(a, a.length, {value: B})",
                    "delete a[a.length - 1]",
                    "a.pop()",
                    "a.push(A, B); if (delete a[0]) { throw new Error('deleted'); }",
                    "try { a.length = 9; } catch (e) {}",
                    "set([A, B, C, A])",
                    "try { set([B, document.createElement('p')]); } catch (e) {}",
                    "try { set([C, document.styleSheets[0] || {}]); } catch (e) {}",
                    "set([])",
                    "set([C, B])",
                ] {
                    let script = format!(
                        "(function () {{ var a = {array}; function set(v) {{ {array} = v; }} {step}; return texts({array}); }})()"
                    );
                    let got = answer(&mut rt, &script);
                    let host = host_texts(&mut rt, scope);
                    assert_eq!(
                        got,
                        serde_json::Value::String(host.clone()),
                        "{array}: {step} (stress {stress})"
                    );
                }
            }
        }
    });
}

/// A sheet that only an array holds lives through collections; once no
/// array holds it and the wrapper table let it go, nothing keeps it; the
/// end of the runtime leaves no sheet.
#[test]
fn sheets_live_as_long_as_an_array_holds_them() {
    on_big_stack(|| {
        for stress in [false, true] {
            let mut rt = load(PAGE, stress);
            let root = root_id(&mut rt);
            answer(
                &mut rt,
                "(function () { var s = new CSSStyleSheet(); s.replaceSync('i{color:red}'); R.adoptedStyleSheets = [s]; document.adoptedStyleSheets = [s]; var t = new CSSStyleSheet(); t.replaceSync('u{}'); R.adoptedStyleSheets.push(t); })(), 1",
            );
            rt.collect();
            rt.collect();
            assert_eq!(
                answer(
                    &mut rt,
                    "[R.adoptedStyleSheets[0] === document.adoptedStyleSheets[0], texts(R.adoptedStyleSheets)].join()"
                ),
                "true,i { color: red; },u { }",
                "stress {stress}"
            );
            let held: Vec<Weak<_>> = with_doc(&mut rt, |doc| {
                let list = &doc.adopted[&root];
                assert!(Rc::ptr_eq(&list[0], &doc.adopted[&NodeId::DOCUMENT][0]));
                list.iter().map(Rc::downgrade).collect()
            });
            assert!(held.iter().all(|w| w.upgrade().is_some()));
            if !stress {
                // Let go of them; new sheets fill the wrapper table, whose
                // sweep drops the entries that no list holds.
                answer(
                    &mut rt,
                    "R.adoptedStyleSheets = []; document.adoptedStyleSheets = []; for (var i = 0; i < 5000; i++) new CSSStyleSheet(); 1",
                );
                rt.collect();
                assert!(held.iter().all(|w| w.upgrade().is_none()));
            }
            // A sheet in an array at the end of the runtime.
            answer(&mut rt, "document.adoptedStyleSheets = [S('q{}')], 1");
            let last = with_doc(&mut rt, |doc| {
                Rc::downgrade(&doc.adopted[&NodeId::DOCUMENT][0])
            });
            drop(rt);
            assert!(last.upgrade().is_none(), "stress {stress}");
            assert!(held.iter().all(|w| w.upgrade().is_none()));
        }
    });
}

/// A root that moves to another document loses its sheets (measured in
/// Chromium 148, case `adopted-host-moves`); the array stays the same
/// object, and the host's list empties with it.
#[test]
fn a_root_that_moves_to_another_document_loses_its_sheets() {
    on_big_stack(|| {
        for stress in [false, true] {
            let mut rt = load(PAGE, stress);
            let root = root_id(&mut rt);
            answer(
                &mut rt,
                "window.arr = R.adoptedStyleSheets; R.adoptedStyleSheets = [A, B]; 1",
            );
            assert_eq!(host_texts(&mut rt, root), "a { },b { }");
            // Removed from the document: the sheets stay.
            assert_eq!(
                answer(&mut rt, "h.remove(); R.adoptedStyleSheets.length"),
                2,
                "stress {stress}"
            );
            // A script that holds the array sees it empty at once, before
            // any use of the getter (Chromium 148 gives 0, measured in the
            // review of 5c-2).
            assert_eq!(
                answer(
                    &mut rt,
                    "var d = document.implementation.createHTMLDocument(''); d.body.appendChild(R.host); document.body.appendChild(R.host); [window.arr.length, window.arr[0], R.adoptedStyleSheets.length, window.arr === R.adoptedStyleSheets].join()"
                ),
                "0,,0,true",
                "stress {stress}"
            );
            assert_eq!(host_texts(&mut rt, root), "");
            // The same with `adoptNode`, which moves no child.
            answer(&mut rt, "R.adoptedStyleSheets = [A]; h.remove(); 1");
            assert_eq!(
                answer(
                    &mut rt,
                    "document.implementation.createHTMLDocument('').adoptNode(R.host); window.arr.length"
                ),
                0,
                "stress {stress}"
            );
            assert_eq!(host_texts(&mut rt, root), "");
            answer(&mut rt, "document.body.appendChild(R.host); 1");
            // The template contents of the document keep them.
            answer(
                &mut rt,
                "R.adoptedStyleSheets = [C]; var t = document.createElement('template'); t.content.appendChild(R.host); document.body.appendChild(R.host); 1",
            );
            assert_eq!(answer(&mut rt, "R.adoptedStyleSheets.length"), 1);
            assert_eq!(host_texts(&mut rt, root), "c { }");
        }
    });
}

/// An array takes at most 65,536 entries (ADR 0036 section 12): a push or a
/// setter past it throws `NotAllowedError` and keeps the entries before it.
#[test]
fn arrays_have_a_limit() {
    on_big_stack(|| {
        let mut rt = load(PAGE, false);
        assert_eq!(
            answer(
                &mut rt,
                "(function () { var a = document.adoptedStyleSheets, e = ''; for (var i = 0; i < 65537; i++) { try { a.push(A); } catch (x) { e = x.name + ': ' + x.message; } } return [a.length, e].join(); })()"
            ),
            "65536,NotAllowedError: swb: too many adopted style sheets"
        );
        assert_eq!(
            answer(
                &mut rt,
                "(function () { var l = new Array(65537).fill(B); l[0] = C; try { R.adoptedStyleSheets = l; } catch (x) { return [R.adoptedStyleSheets.length, x.message].join(); } })()"
            ),
            "65536,Failed to set the 'adoptedStyleSheets' property on 'ShadowRoot': swb: too many adopted style sheets"
        );
        let root = root_id(&mut rt);
        assert_eq!(with_doc(&mut rt, |doc| doc.adopted[&root].len()), 65536);
    });
}
