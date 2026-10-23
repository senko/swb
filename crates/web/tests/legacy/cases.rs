//! The cases of the legacy platform objects and iterable declarations
//! (`super`), each run in normal and GC stress mode. Comments name the
//! places where Web IDL and Chromium differ; the cases follow Web IDL
//! there.

/// Script helpers: `el(name, attributes)` makes an element.
const HELPERS: &str = "function el(n, a) { const e = makeElement(n); for (const k in a) setAttr(e, k, a[k]); return e; }\n";

/// `crate::check` with the helpers before each case.
fn check(cases: &[(&str, &str)]) {
    let sources: Vec<String> = cases.iter().map(|(s, _)| format!("{HELPERS}{s}")).collect();
    let cases: Vec<(&str, &str)> = sources
        .iter()
        .zip(cases)
        .map(|(s, (_, e))| (s.as_str(), *e))
        .collect();
    crate::check(&cases);
}

const LIST: &str = "const a = makeElement('a'); const l = makeList(a, makeElement('b')); ";

#[test]
fn an_indexed_list_shows_its_items() {
    let cases: Vec<(String, &str)> = vec![
        (
            "[Object.getOwnPropertyNames(l), Reflect.ownKeys(l).map(String), Object.keys(l)].join(' | ')",
            "0,1 | 0,1 | 0,1",
        ),
        (
            "const d = Object.getOwnPropertyDescriptor(l, 0); [d.value === a, d.writable, d.enumerable, d.configurable, Object.getOwnPropertyDescriptor(l, 2) === undefined, Object.getOwnPropertyDescriptor(l, 'length') === undefined].join()",
            "true,false,true,true,true,true",
        ),
        (
            "[0 in l, 2 in l, l.hasOwnProperty(0), l.hasOwnProperty('0'), l.hasOwnProperty(2), 'length' in l, l.hasOwnProperty('length'), '-0' in l, '01' in l, l['01'], l['1.0'], l[-1]].join()",
            "true,false,true,true,false,true,false,false,false,,,",
        ),
        (
            "[JSON.stringify(l), Object.keys(Object.assign({}, l)).join(), l.item(0) === l[0], l.item(9), TestList.prototype.item.length].join(' | ')",
            "{\"0\":{},\"1\":{}} | 0,1 | true |  | 1",
        ),
        (
            "const r = []; for (const k in l) r.push(k); r.join()",
            "0,1,entries,keys,values,forEach,length,item",
        ),
        (
            "Object.setPrototypeOf(l, null); [l[0] === a, l.length, Object.getOwnPropertyNames(l)].join(' | ')",
            "true |  | 0,1",
        ),
        (
            "l.x = 1; Object.defineProperty(l, 'y', { value: 2, enumerable: false }); l[Symbol.toPrimitive] = 3; [Reflect.ownKeys(l).map(String), Object.keys(l)].join(' | ')",
            "0,1,x,y,Symbol(Symbol.toPrimitive) | 0,1,x",
        ),
        // The list changes under the script.
        (
            "const m = makeList(); const r = []; for (let i = 0; i < 3; i++) { listPush(m, makeElement('e' + i)); r.push(m.length + ':' + m[m.length - 1].nodeName); } r.join()",
            "1:e0,2:e1,3:e2",
        ),
        (
            "[Object.prototype.toString.call(l), String(l), Object.getPrototypeOf(l) === TestList.prototype].join()",
            "[object TestList],[object TestList],true",
        ),
    ]
    .into_iter()
    .map(|(s, e)| (format!("{LIST}{s}"), e))
    .collect();
    check(
        &cases
            .iter()
            .map(|(s, e)| (s.as_str(), *e))
            .collect::<Vec<_>>(),
    );
}

#[test]
fn an_indexed_list_refuses_writes_with_chromium_messages() {
    let cases: Vec<(String, &str)> = vec![
        // Sloppy writes are ignored; expandos with other keys are stored.
        (
            "l[0] = 1; l[5] = 2; l.foo = 3; l['-1'] = 4; l[4294967295] = 5; [l[0] === a, l[5], l.foo, l['-1'], l[4294967295], Object.keys(l), Reflect.ownKeys(l)].join(' | ')",
            "true |  | 3 | 4 | 5 | 0,1,foo,-1,4294967295 | 0,1,foo,-1,4294967295",
        ),
        (
            "t(() => { 'use strict'; l[0] = 1; })",
            "TypeError: Failed to set an indexed property [0] on 'TestList': Indexed property setter is not supported.",
        ),
        (
            "t(() => { 'use strict'; l[5] = 1; })",
            "TypeError: Failed to set an indexed property [5] on 'TestList': Indexed property setter is not supported.",
        ),
        (
            "t(() => { 'use strict'; l[4294967294] = 1; })",
            "TypeError: Failed to set an indexed property [4294967294] on 'TestList': Indexed property setter is not supported.",
        ),
        (
            "t(() => { 'use strict'; l.length = 1; })",
            "TypeError: Cannot set property length of #<TestList> which has only a getter",
        ),
        ("t(() => { 'use strict'; l.foo = 1; return l.foo; })", "1"),
        (
            "[delete l[0], delete l[5], delete l.length, l.length, delete l.foo].join()",
            "false,true,true,2,true",
        ),
        (
            "t(() => { 'use strict'; delete l[0]; })",
            "TypeError: Failed to delete an indexed property [0] from 'TestList': Index property deleter is not supported.",
        ),
        ("t(() => { 'use strict'; return delete l[5]; })", "true"),
        (
            "t(() => Object.defineProperty(l, 0, { value: 1 }))",
            "TypeError: Failed to set an indexed property [0] on 'TestList': Index property setter is not supported.",
        ),
        (
            "t(() => Object.defineProperty(l, 5, { value: 1 }))",
            "TypeError: Failed to set an indexed property [5] on 'TestList': Index property setter is not supported.",
        ),
        // Chromium's `Reflect.defineProperty` answers true for the refused
        // indices (the property stays as it was); Web IDL says false.
        (
            "Object.defineProperty(l, 'foo', { value: 1 }); [l.foo, Reflect.defineProperty(l, 0, { value: 1 }), Reflect.defineProperty(l, 5, { value: 1 }), Reflect.defineProperty(l, 'bar', { get() { return 2; } }), l.bar].join()",
            "1,false,false,true,2",
        ),
        // Through the prototype chain: the list's own index is read-only.
        // (Chromium writes `o[0]`; Web IDL refuses.)
        (
            "const o = Object.create(l); o[0] = 5; [o[0] === a, Object.keys(o).length].join()",
            "true,0",
        ),
        (
            "t(() => { 'use strict'; Object.create(l)[1] = 5; })",
            "TypeError: Failed to set an indexed property [1] on 'TestList': Indexed property setter is not supported.",
        ),
    ]
    .into_iter()
    .map(|(s, e)| (format!("{LIST}{s}"), e))
    .collect();
    check(
        &cases
            .iter()
            .map(|(s, e)| (s.as_str(), *e))
            .collect::<Vec<_>>(),
    );
}

#[test]
fn legacy_platform_objects_stay_extensible() {
    check(&[
        (
            "t(() => Object.freeze(makeList(makeElement('a'))))",
            "TypeError: Cannot freeze",
        ),
        (
            "t(() => Object.freeze(makeList()))",
            "TypeError: Cannot freeze",
        ),
        ("t(() => Object.seal(makeList()))", "TypeError: Cannot seal"),
        (
            "t(() => Object.preventExtensions(makeCollection()))",
            "TypeError: Cannot prevent extensions",
        ),
        (
            "const m = makeStringMap(); [Reflect.preventExtensions(m), Object.isExtensible(m), Object.isFrozen(m), Object.isSealed(makeList())].join()",
            "false,true,false,false",
        ),
        (
            "t(() => Object.freeze(makeStorage()))",
            "TypeError: Cannot freeze",
        ),
        // An ordinary prototype change works.
        (
            "const l = makeList(); Object.setPrototypeOf(l, Array.prototype); [Object.getPrototypeOf(l) === Array.prototype, l.length].join()",
            "true,0",
        ),
    ]);
}

#[test]
fn a_value_iterator_uses_the_array_functions() {
    let cases: Vec<(String, &str)> = vec![
        (
            "[[...l].map(e => e.nodeName).join(), Array.from(l.entries()).map(([k, v]) => k + v.nodeName).join(), Array.from(l.keys()).join(), Array.from(l.values()).map(e => e.nodeName).join()].join(' | ')",
            "a,b | 0a,1b | 0,1 | a,b",
        ),
        (
            "const p = TestList.prototype; [p[Symbol.iterator] === Array.prototype.values, p.forEach === Array.prototype.forEach, p.entries === Array.prototype.entries, p.keys === Array.prototype.keys, p.values === Array.prototype.values].join()",
            "true,true,true,true,true",
        ),
        (
            "[Object.prototype.toString.call(l.entries()), Object.getPrototypeOf(l.entries()) === Object.getPrototypeOf([].entries()), JSON.stringify(l.entries().next()), String(l.keys())].join(' | ')",
            "[object Array Iterator] | true | {\"value\":[0,{}],\"done\":false} | [object Array Iterator]",
        ),
        (
            "[Object.getOwnPropertyNames(TestList.prototype), Reflect.ownKeys(TestList.prototype).map(String)].join(' | ')",
            "entries,keys,values,forEach,length,item,constructor | entries,keys,values,forEach,length,item,constructor,Symbol(Symbol.toStringTag),Symbol(Symbol.iterator)",
        ),
        (
            "JSON.stringify(['entries', 'forEach', 'keys', 'values', Symbol.iterator, 'item', 'length'].map(k => { const d = Object.getOwnPropertyDescriptor(TestList.prototype, k); return [d.writable, d.enumerable, d.configurable]; }))",
            "[[true,true,true],[true,true,true],[true,true,true],[true,true,true],[true,false,true],[true,true,true],[null,true,true]]",
        ),
        (
            "const r = []; l.forEach(function (v, i, o) { r.push(i, v.nodeName, o === l, this.x); }, { x: 1 }); r.join()",
            "0,a,true,1,1,b,true,1",
        ),
        ("t(() => l.forEach(1))", "TypeError: number 1 is not a function"),
        // An interface with an indexed getter and no iterable declaration
        // gets `@@iterator` only.
        (
            "const p = TestCollection.prototype; [p[Symbol.iterator] === Array.prototype.values, 'forEach' in p, 'entries' in p, [...makeCollection(a)].length].join()",
            "true,false,false,1",
        ),
    ]
    .into_iter()
    .map(|(s, e)| (format!("{LIST}{s}"), e))
    .collect();
    check(
        &cases
            .iter()
            .map(|(s, e)| (s.as_str(), *e))
            .collect::<Vec<_>>(),
    );
}

const COLLECTION: &str = "const c = makeCollection(el('f', { name: 'f1', id: 'g1' }), el('f', { id: 'f2' }), el('f', { name: 'length' }), el('f', { name: 'item' }), el('f', { id: '7' }), el('f', { name: 'f1' })); ";

#[test]
fn a_collection_has_unenumerable_named_properties() {
    let cases: Vec<(String, &str)> = vec![
        // The visible names only: `length` and `item` are hidden by the
        // prototype (Chromium lists every supported name). A numeric name
        // stays among the names.
        (
            "[Object.getOwnPropertyNames(c), Reflect.ownKeys(c).map(String), Object.keys(c)].join(' | ')",
            "0,1,2,3,4,5,g1,f1,f2,7 | 0,1,2,3,4,5,g1,f1,f2,7 | 0,1,2,3,4,5",
        ),
        (
            "JSON.stringify(['0', 'g1', 'f1', 'f2', '7', 'length', 'item'].map(k => { const d = Object.getOwnPropertyDescriptor(c, k); return d === undefined ? 'none' : [d.value === c[k], d.writable, d.enumerable, d.configurable]; }))",
            "[[true,false,true,true],[true,false,false,true],[true,false,false,true],[true,false,false,true],\"none\",\"none\",\"none\"]",
        ),
        (
            "['f1' in c, c.hasOwnProperty('f1'), c.f1 === c[0], c.g1 === c[0], typeof c.length, typeof c.item, c.namedItem('length') === c[2], c[7], c['7'], c.nope].join()",
            "true,true,true,true,number,function,true,,,",
        ),
        (
            "const r = []; for (const k in c) r.push(k); r.join()",
            "0,1,2,3,4,5,length,item,namedItem",
        ),
        (
            "[Object.getOwnPropertyNames(TestCollection.prototype), Reflect.ownKeys(TestCollection.prototype).map(String)].join(' | ')",
            "length,item,namedItem,constructor | length,item,namedItem,constructor,Symbol(Symbol.toStringTag),Symbol(Symbol.iterator)",
        ),
        // An expando hides a later name; Web IDL lists the own keys after
        // the names (Chromium before them).
        (
            "c.extra = 1; listPush(c, el('f', { id: 'extra' })); [c.extra, Reflect.ownKeys(c).map(String)].join(' | ')",
            "1 | 0,1,2,3,4,5,6,g1,f1,f2,7,extra",
        ),
        // A name that is also an index is listed once.
        (
            "listPush(c, el('f', { id: '0' })); Reflect.ownKeys(c).map(String).join()",
            "0,1,2,3,4,5,6,g1,f1,f2,7",
        ),
    ]
    .into_iter()
    .map(|(s, e)| (format!("{COLLECTION}{s}"), e))
    .collect();
    check(
        &cases
            .iter()
            .map(|(s, e)| (s.as_str(), *e))
            .collect::<Vec<_>>(),
    );
}

#[test]
fn a_collection_refuses_writes_with_chromium_messages() {
    let cases: Vec<(String, &str)> = vec![
        (
            "const first = c[0]; c.f1 = 1; c.zz = 2; c[0] = 3; c[9] = 4; [c.f1 === first, c.zz, c[0] === first, c[9]].join()",
            "true,2,true,",
        ),
        (
            "t(() => { 'use strict'; c.f1 = 1; })",
            "TypeError: Failed to set a named property 'f1' on 'TestCollection': Named property setter is not supported.",
        ),
        (
            "t(() => { 'use strict'; c[0] = 1; })",
            "TypeError: Failed to set an indexed property [0] on 'TestCollection': Indexed property setter is not supported.",
        ),
        (
            "t(() => { 'use strict'; c[9] = 1; })",
            "TypeError: Failed to set an indexed property [9] on 'TestCollection': Indexed property setter is not supported.",
        ),
        (
            "t(() => { 'use strict'; c.length = 1; })",
            "TypeError: Cannot set property length of #<TestCollection> which has only a getter",
        ),
        (
            "[delete c.f1, delete c[0], delete c[9], delete c.nope, c.f1 === c[0]].join()",
            "false,false,true,true,true",
        ),
        (
            "t(() => { 'use strict'; delete c.f1; })",
            "TypeError: Failed to delete a named property 'f1' from 'TestCollection': Named property deleter is not supported.",
        ),
        (
            "t(() => Object.defineProperty(c, 'f1', { value: 1 }))",
            "TypeError: Failed to set a named property 'f1' on 'TestCollection': Named property setter is not supported.",
        ),
        (
            "t(() => Object.defineProperty(c, 0, { value: 1 }))",
            "TypeError: Failed to set an indexed property [0] on 'TestCollection': Index property setter is not supported.",
        ),
        // (Chromium: `true` for the refused name.)
        (
            "Object.defineProperty(c, 'zz', { value: 1 }); [c.zz, Reflect.defineProperty(c, 'f1', { value: 1 })].join()",
            "1,false",
        ),
        // Through the prototype chain the named properties are ignored and
        // the write defines on the receiver (Chromium refuses it).
        (
            "const o = Object.create(c); o.f1 = 5; [o.f1, Object.keys(o)].join(' | ')",
            "5 | f1",
        ),
    ]
    .into_iter()
    .map(|(s, e)| (format!("{COLLECTION}{s}"), e))
    .collect();
    check(
        &cases
            .iter()
            .map(|(s, e)| (s.as_str(), *e))
            .collect::<Vec<_>>(),
    );
}

#[test]
fn an_indexed_setter_converts_its_value() {
    let setup = "const ol = makeOptionList(el('o', { id: 'a' }), el('o', { id: 'b' })); ";
    let cases: Vec<(String, &str)> = vec![
        (
            "ol[0] = el('o', { id: 'c' }); ol[3] = el('o', { id: 'd' }); [ol.length, [...ol].map(e => getAttr(e, 'id')).join()].join(' | ')",
            "4 | c,b,,d",
        ),
        (
            "t(() => { ol[0] = 5; })",
            "TypeError: Failed to set an indexed property [0] on 'TestOptionList': parameter 2 is not of type 'TestElement'.",
        ),
        (
            "t(() => { 'use strict'; ol[0] = 5; })",
            "TypeError: Failed to set an indexed property [0] on 'TestOptionList': parameter 2 is not of type 'TestElement'.",
        ),
        (
            "t(() => Object.defineProperty(ol, 0, { get() { return 1; } }))",
            "TypeError: Failed to set an indexed property [0] on 'TestOptionList': Accessor properties are not allowed.",
        ),
        (
            "Object.defineProperty(ol, 0, { value: el('o', { id: 'x' }) }); [...ol].map(e => getAttr(e, 'id')).join()",
            "x,b",
        ),
        (
            "ol[0] = null; [ol.length, Object.getOwnPropertyDescriptor(ol, 0).writable, Reflect.ownKeys(ol).join()].join(' | ')",
            "1 | true | 0,b",
        ),
        ("[delete ol[0], ol.length].join()", "false,2"),
        (
            "t(() => { 'use strict'; delete ol[0]; })",
            "TypeError: Failed to delete an indexed property [0] from 'TestOptionList': Index property deleter is not supported.",
        ),
        // The named properties come from the parent interface.
        (
            "[ol.a === ol[0], Reflect.ownKeys(ol).join(), ol instanceof TestCollection].join(' | ')",
            "true | 0,1,a,b | true",
        ),
    ]
    .into_iter()
    .map(|(s, e)| (format!("{setup}{s}"), e))
    .collect();
    check(
        &cases
            .iter()
            .map(|(s, e)| (s.as_str(), *e))
            .collect::<Vec<_>>(),
    );
}

const STRING_MAP: &str = "const m = makeStringMap(); m.foo = '1'; m.barBaz = '2'; m.toString = 'x'; m.hasOwnProperty = 'y'; ";

#[test]
fn a_string_map_overrides_built_ins() {
    let cases: Vec<(String, &str)> = vec![
        (
            "[Object.getOwnPropertyNames(m), Object.keys(m), Reflect.ownKeys(m).map(String)].join(' | ')",
            "foo,barBaz,toString,hasOwnProperty | foo,barBaz,toString,hasOwnProperty | foo,barBaz,toString,hasOwnProperty",
        ),
        (
            "const d = Object.getOwnPropertyDescriptor(m, 'foo'); [d.value, d.writable, d.enumerable, d.configurable, Object.getOwnPropertyDescriptor(m, 'nope')].join()",
            "1,true,true,true,",
        ),
        (
            "[m.toString, m.hasOwnProperty, 'foo' in m, Object.prototype.hasOwnProperty.call(m, 'foo'), typeof m.valueOf, m.constructor === TestStringMap].join()",
            "x,y,true,true,function,true",
        ),
        ("m.newKey = 5; [m.newKey, typeof m.newKey].join()", "5,string"),
        // The setter's exception, in sloppy and strict code.
        (
            "t(() => { m['a-b'] = 1; })",
            "SyntaxError: Failed to set a named property 'a-b' on 'TestStringMap': 'a-b' is not a valid property name.",
        ),
        (
            "t(() => { 'use strict'; m['a-b'] = 1; })",
            "SyntaxError: Failed to set a named property 'a-b' on 'TestStringMap': 'a-b' is not a valid property name.",
        ),
        (
            "t(() => { m.foo = { toString() { throw new Error('boom'); } }; })",
            "Error: boom",
        ),
        (
            "[delete m.foo, 'foo' in m, delete m.nothing, delete m.toString, m.toString === Object.prototype.toString].join()",
            "true,false,true,true,true",
        ),
        (
            "t(() => { 'use strict'; return [delete m.barBaz, delete m.nope].join(); })",
            "true,true",
        ),
        (
            "Object.defineProperty(m, 'q', { value: 5 }); [m.q, Object.getOwnPropertyDescriptor(m, 'q').writable].join()",
            "5,true",
        ),
        (
            "t(() => Object.defineProperty(m, 'g', { get() { return 1; } }))",
            "TypeError: Failed to set a named property 'g' on 'TestStringMap': Accessor properties are not allowed.",
        ),
        (
            "t(() => Object.defineProperty(m, 'foo', { get() { return 1; } }))",
            "TypeError: Failed to set a named property 'foo' on 'TestStringMap': Accessor properties are not allowed.",
        ),
        // (Chromium: `true` for the refused accessor.)
        (
            "[Reflect.defineProperty(m, 'g', { get() { return 1; } }), Reflect.defineProperty(m, 'h', { value: 'v' }), m.h].join()",
            "false,true,v",
        ),
        // A generic descriptor calls the setter with undefined (Chromium;
        // Web IDL refuses it).
        (
            "Object.defineProperty(m, 'z', { enumerable: true }); [m.z, Object.keys(m).join()].join(' | ')",
            "undefined | foo,barBaz,toString,hasOwnProperty,z",
        ),
        ("t(() => Object.freeze(m))", "TypeError: Cannot freeze"),
        (
            "t(() => Object.preventExtensions(m))",
            "TypeError: Cannot prevent extensions",
        ),
        // The names in their order, numeric ones too, then the symbols
        // (Chromium lists the symbols first).
        (
            "const n = makeStringMap(); n.b = '1'; n[0] = '2'; n.a = '3'; n[Symbol.iterator] = 4; [Reflect.ownKeys(n).map(String), typeof n[Symbol.iterator]].join(' | ')",
            "b,0,a,Symbol(Symbol.iterator) | number",
        ),
        (
            "[Object.getOwnPropertyNames(TestStringMap.prototype), Reflect.ownKeys(TestStringMap.prototype).map(String), TestStringMap.prototype[Symbol.iterator]].join(' | ')",
            "constructor | constructor,Symbol(Symbol.toStringTag) | ",
        ),
        // Another receiver: the write defines on it.
        (
            "const o = Object.create(m); o.foo = 'v'; [o.foo, m.foo, Object.keys(o)].join()",
            "v,1,foo",
        ),
    ]
    .into_iter()
    .map(|(s, e)| (format!("{STRING_MAP}{s}"), e))
    .collect();
    check(
        &cases
            .iter()
            .map(|(s, e)| (s.as_str(), *e))
            .collect::<Vec<_>>(),
    );
}

#[test]
fn a_storage_does_not_override_built_ins() {
    let setup = "const s = makeStorage(); ";
    let cases: Vec<(String, &str)> = vec![
        (
            "s.b = '1'; s.setItem('a', '2'); s[0] = '3'; [Object.getOwnPropertyNames(s), Object.keys(s), s.length].join(' | ')",
            "b,a,0 | b,a,0 | 3",
        ),
        (
            "s.b = '1'; const d = Object.getOwnPropertyDescriptor(s, 'b'); [d.value, d.writable, d.enumerable, d.configurable].join()",
            "1,true,true,true",
        ),
        // The prototype's members hide items of the same name; a write
        // with the object as the receiver goes to the named setter (Web
        // IDL §3.9.2; Chromium makes an own property instead).
        (
            "s.setItem('length', 'x'); s.getItem = 'y'; [typeof s.length, typeof s.getItem, s.getItem('getItem'), Object.keys(s).length, Object.getOwnPropertyDescriptor(s, 'getItem'), 'getItem' in s].join()",
            "number,function,y,0,,true",
        ),
        (
            "s.b = '1'; [delete s.b, s.getItem('b'), delete s.nope, delete s.clear, typeof s.clear].join()",
            "true,,true,true,function",
        ),
        (
            "Object.getOwnPropertyNames(TestStorage.prototype).join() + ' ' + TestStorage.prototype[Symbol.iterator]",
            "length,clear,getItem,key,removeItem,setItem,constructor undefined",
        ),
    ]
    .into_iter()
    .map(|(s, e)| (format!("{setup}{s}"), e))
    .collect();
    check(
        &cases
            .iter()
            .map(|(s, e)| (s.as_str(), *e))
            .collect::<Vec<_>>(),
    );
}

/// A deadline that passes in script that a special operation runs (the
/// conversion of the value of a named setter, through `[[Set]]` and
/// `[[DefineOwnProperty]]`) ends the run: the exception prefix of the
/// operation does not make the termination a catchable error.
#[test]
fn a_termination_in_a_special_operation_ends_the_run() {
    let sources = [
        "m.x = { toString() { for (;;) {} } }",
        "Object.defineProperty(m, 'x', { value: { toString() { for (;;) {} } } })",
    ];
    std::thread::Builder::new()
        .stack_size(64 << 20)
        .spawn(move || {
            for source in sources {
                for stress in [false, true] {
                    let mut rt = crate::runtime(stress);
                    rt.eval("var m = makeStringMap();").unwrap();
                    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(50);
                    rt.set_deadline(Some(deadline));
                    let result = rt.eval(&format!("try {{ {source} }} catch (e) {{ 'caught' }}"));
                    assert!(
                        matches!(result, Err(swb_js::ScriptError::Terminated(_))),
                        "{source} (stress {stress}): {result:?}"
                    );
                    rt.set_deadline(None);
                    assert_eq!(crate::run(&mut rt, "m.y = 'z'; m.y"), "z", "{source}");
                }
            }
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn a_form_is_a_node_with_named_properties() {
    let setup = "const f = makeForm(); formAdd(f, el('input', { name: 'title' })); formAdd(f, el('input', { name: 'x' })); ";
    let cases: Vec<(String, &str)> = vec![
        // The write ignores the named properties and reaches the
        // prototype's setter; the read sees them ([LegacyOverrideBuiltIns]).
        (
            "f.title = 'T'; [getAttr(f, 'title'), f.title === f[0], f.x === f[1], f instanceof TestElement].join()",
            "T,true,true,true",
        ),
        (
            "t(() => { 'use strict'; f.x = 1; })",
            "TypeError: Failed to set a named property 'x' on 'TestForm': Named property setter is not supported.",
        ),
        // Unenumerable names (Chromium lists none of a form's names).
        (
            "[Reflect.ownKeys(f).map(String), Object.keys(f)].join(' | ')",
            "0,1,title,x | 0,1",
        ),
        ("t(() => Object.freeze(f))", "TypeError: Cannot freeze"),
    ]
    .into_iter()
    .map(|(s, e)| (format!("{setup}{s}"), e))
    .collect();
    check(
        &cases
            .iter()
            .map(|(s, e)| (s.as_str(), *e))
            .collect::<Vec<_>>(),
    );
}

const HEADERS: &str = "const h = new TestHeaders(); h.append('b', '2'); h.append('a', '1'); ";

#[test]
fn a_pair_iterator_has_its_own_iterator_objects() {
    let cases: Vec<(String, &str)> = vec![
        (
            "[[...h].join(';'), JSON.stringify(h.entries().next()), Array.from(h.keys()).join(), Array.from(h.values()).join()].join(' | ')",
            "a,1;b,2 | {\"done\":false,\"value\":[\"a\",\"1\"]} | a,b | 1,2",
        ),
        (
            "const i = h.entries(); [Object.prototype.toString.call(i), String(i), Object.getOwnPropertyNames(i).length, Reflect.ownKeys(Object.getPrototypeOf(i)).map(String)].join(' | ')",
            "[object TestHeaders Iterator] | [object TestHeaders Iterator] | 0 | next,Symbol(Symbol.toStringTag)",
        ),
        (
            "const p = Object.getPrototypeOf(h.entries()); const d = Object.getOwnPropertyDescriptor(p, 'next'); const g = Object.getOwnPropertyDescriptor(p, Symbol.toStringTag); [d.writable, d.enumerable, d.configurable, p.next.length, p.next.name, g.value, g.writable, g.enumerable, g.configurable].join()",
            "true,true,true,0,next,TestHeaders Iterator,false,false,true",
        ),
        (
            "const p = Object.getPrototypeOf(h.entries()); [Object.getPrototypeOf(p) === Iterator.prototype, Object.getPrototypeOf(h.keys()) === p, Object.getPrototypeOf(new TestHeaders().values()) === p, typeof p[Symbol.iterator]].join()",
            "true,true,true,function",
        ),
        (
            "const p = TestHeaders.prototype; [p[Symbol.iterator] === p.entries, p.entries.name, p.entries.length, p.keys.name, p.values.name, p.forEach.name, p.forEach.length, p[Symbol.iterator].name].join()",
            "true,entries,0,keys,values,forEach,1,entries",
        ),
        (
            "[Object.getOwnPropertyNames(TestHeaders.prototype), Reflect.ownKeys(TestHeaders.prototype).map(String)].join(' | ')",
            "append,delete,entries,forEach,keys,values,constructor | append,delete,entries,forEach,keys,values,constructor,Symbol(Symbol.toStringTag),Symbol(Symbol.iterator)",
        ),
        (
            "JSON.stringify(['entries', 'forEach', 'keys', 'values', Symbol.iterator].map(k => { const d = Object.getOwnPropertyDescriptor(TestHeaders.prototype, k); return [d.writable, d.enumerable, d.configurable]; }))",
            "[[true,true,true],[true,true,true],[true,true,true],[true,true,true],[true,false,true]]",
        ),
        (
            "const r = []; h.forEach(function (v, k, o) { r.push(v, k, o === h, this.x); }, { x: 1 }); r.join()",
            "1,a,true,1,2,b,true,1",
        ),
        (
            "t(() => h.forEach(1))",
            "TypeError: Failed to execute 'forEach' on 'TestHeaders': parameter 1 is not of type 'Function'.",
        ),
        (
            "t(() => h.forEach())",
            "TypeError: Failed to execute 'forEach' on 'TestHeaders': 1 argument required, but only 0 present.",
        ),
        ("t(() => h.entries().next.call({}))", "TypeError: Illegal invocation"),
        (
            "t(() => { h.entries().next.call(new TestHeaders().keys()); return 'ok'; })",
            "ok",
        ),
        ("t(() => TestHeaders.prototype.entries.call({}))", "TypeError: Illegal invocation"),
        (
            "t(() => TestHeaders.prototype.forEach.call({}, () => {}))",
            "TypeError: Illegal invocation",
        ),
        // The iterator reads the current pairs at each step.
        (
            "const i = h.keys(); i.next(); h.append('c', '3'); h.delete('a'); [...i].join()",
            "c",
        ),
        (
            "const i = h.keys(); [JSON.stringify(i.next()), JSON.stringify(i.next()), JSON.stringify(i.next()), JSON.stringify(i.next())].join(' ')",
            "{\"done\":false,\"value\":\"a\"} {\"done\":false,\"value\":\"b\"} {\"done\":true} {\"done\":true}",
        ),
        (
            "const r = []; h.forEach((v, k) => { r.push(k); if (k === 'a') h.append('z', '9'); }); r.join()",
            "a,b,z",
        ),
        (
            "const i = h.entries(); [i[Symbol.iterator]() === i, typeof i.map, typeof i.return].join()",
            "true,function,undefined",
        ),
        // Iterators survive collections with their target.
        (
            "const its = []; for (let n = 0; n < 50; n++) { const x = new TestHeaders(); x.append('k' + n, 'v'); its.push(x.entries()); } gc(); its.map(i => i.next().value[0]).slice(47).join()",
            "k47,k48,k49",
        ),
    ]
    .into_iter()
    .map(|(s, e)| (format!("{HEADERS}{s}"), e))
    .collect();
    check(
        &cases
            .iter()
            .map(|(s, e)| (s.as_str(), *e))
            .collect::<Vec<_>>(),
    );
}

#[test]
fn the_global_object_and_its_prototypes_are_immutable() {
    check(&[
        (
            "t(() => Object.setPrototypeOf(globalThis, {}))",
            "TypeError: Immutable prototype object '#<Window>' cannot have their prototype set",
        ),
        (
            "[Reflect.setPrototypeOf(globalThis, {}), Reflect.setPrototypeOf(globalThis, Object.getPrototypeOf(globalThis)), Object.setPrototypeOf(globalThis, Window.prototype) === globalThis, Reflect.setPrototypeOf(globalThis, null)].join()",
            "false,true,true,false",
        ),
        (
            "t(() => { globalThis.__proto__ = {}; })",
            "TypeError: Immutable prototype object '#<Window>' cannot have their prototype set",
        ),
        (
            "t(() => { 'use strict'; globalThis.__proto__ = {}; })",
            "TypeError: Immutable prototype object '#<Window>' cannot have their prototype set",
        ),
        (
            "[Reflect.setPrototypeOf(Window.prototype, {}), Object.setPrototypeOf(Window.prototype, Object.prototype) === Window.prototype, Reflect.setPrototypeOf(TestList.prototype, null)].join()",
            "false,true,true",
        ),
    ]);
}
