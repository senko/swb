//! The realm API, the global this value and `WindowProxy` objects (ADR 0031
//! section 5, HTML §7.2.3, ECMA-262 §9.3.1): two realms, a proxy, a retarget,
//! and the paths that look at the kind of an object.

#![allow(clippy::unwrap_used, reason = "the setup of test runtimes")]

use swb_js::{
    Gc, Object, Property, PropertyDescriptor, PropertyKey, Runtime, RuntimeConfig, Value, VmError,
    VmResult, WindowProxyHooks,
};

/// A runtime whose realm 0 has a `WindowProxy` as its global this value and
/// the global `window`.
fn with_proxy(config: RuntimeConfig) -> (Runtime, Gc<Object>) {
    let mut rt = Runtime::new(config).unwrap();
    let proxy = install_proxy(&mut rt, 0, 1);
    (rt, proxy)
}

/// Makes a proxy the global this value of `realm`.
fn install_proxy(rt: &mut Runtime, realm: u32, frame: u32) -> Gc<Object> {
    let global = rt.global(realm).unwrap();
    let proxy = rt.new_window_proxy(frame, Some(global)).unwrap();
    rt.set_global_this(realm, proxy).unwrap();
    rt.eval_named_at(realm, "var window = this;", "", (0, 0))
        .unwrap();
    proxy
}

fn text(rt: &mut Runtime, realm: u32, source: &str) -> String {
    let value = rt.eval_named_at(realm, source, "", (0, 0)).unwrap();
    rt.to_rust_string(value).unwrap()
}

fn eval(rt: &mut Runtime, source: &str) -> String {
    text(rt, 0, source)
}

#[test]
fn the_shell_keeps_this_equal_to_the_global_object() {
    let mut rt = Runtime::new(RuntimeConfig::default()).unwrap();
    let global = rt.global(0).unwrap();
    assert_eq!(rt.global_this(0).unwrap(), global);
    assert_eq!(rt.eval("this").unwrap(), Value::Object(global));
    assert_eq!(rt.eval("globalThis").unwrap(), Value::Object(global));
}

#[test]
fn every_form_of_the_global_this_gives_the_proxy() {
    let (mut rt, proxy) = with_proxy(RuntimeConfig::default());
    for source in [
        "this",
        "globalThis",
        "window",
        "eval('this')",
        "(0, eval)('this')",
        "Function('return this')()",
        "(function () { return this; })()",
        "(function () { return this; }).call(null)",
        "(function () { return this; }).call(undefined)",
        "[1].map(function () { return this; })[0]",
        "(() => this)()",
    ] {
        assert_eq!(rt.eval(source).unwrap(), Value::Object(proxy), "{source}");
    }
    assert_eq!(
        rt.eval("(function () { 'use strict'; return this; })()")
            .unwrap(),
        Value::Undefined
    );
    // A getter or a setter of the global object, by name and through the
    // proxy.
    assert_eq!(
        eval(
            &mut rt,
            "Object.defineProperty(window, 'g', {
               get() { return this; },
               set(v) { this.seen = this === window; },
               configurable: true,
             });
             g = 1;
             var a = seen; seen = 0; window.g = 1;
             [g === window, window.g === window, a, seen].join()"
        ),
        "true,true,true,true"
    );
}

#[test]
fn a_var_and_a_property_are_the_same_binding() {
    let (mut rt, _) = with_proxy(RuntimeConfig::default());
    assert_eq!(
        eval(
            &mut rt,
            "var v = 1; window.w = 2; var read = [window.v, w];
             v = 3; window.w = 4; read.push(window.v, w);
             let l = 5; read.push(window.l, 'l' in window);
             read.join()"
        ),
        "1,2,3,4,,false"
    );
    // Reads and writes through a site that has cached them.
    assert_eq!(
        eval(
            &mut rt,
            "function read() { return window.v; }
             function write(x) { window.v = x; }
             var values = [];
             for (var i = 0; i < 3; i++) { write(i); values.push(read(), v); }
             values.join()"
        ),
        "0,0,1,1,2,2"
    );
}

/// Runs each function of a site three times, so that the site has its cache
/// entry, then the change, and reads the site again.
#[test]
fn a_cached_site_follows_every_change_of_a_property() {
    let (mut rt, _) = with_proxy(RuntimeConfig::default());
    assert_eq!(
        eval(
            &mut rt,
            "var log = [];
             window.v = 1;
             function read() { return window.v; }
             function write(x) { window.v = x; }
             function strictWrite(x) { 'use strict'; window.v = x; }
             for (var i = 0; i < 3; i++) { write(i); strictWrite(i); read(); }
             Object.defineProperty(window, 'v', {writable: false});
             write(10);
             log.push(read());
             try { strictWrite(11); log.push('no error'); } catch (e) { log.push(e.name); }
             Object.defineProperty(window, 'v', {
               get() { return this === window ? 'getter' : 'other'; },
               set(x) { log.push(this === window, x); },
               configurable: true,
             });
             log.push(read());
             write(12);
             delete window.v;
             log.push(String(read()));
             write(13);
             log.push(read());
             log.join()"
        ),
        "2,TypeError,getter,true,12,undefined,13"
    );
}

#[test]
fn a_cached_site_follows_every_change_of_the_chain() {
    let (mut rt, _) = with_proxy(RuntimeConfig::default());
    assert_eq!(
        eval(
            &mut rt,
            "var log = [];
             Object.prototype.k = 'proto';
             function read() { return window.k; }
             for (var i = 0; i < 3; i++) read();
             log.push(read());
             Object.prototype.k = 'changed';
             log.push(read());
             window.k = 'own';
             log.push(read());
             delete window.k;
             log.push(read());
             delete Object.prototype.k;
             log.push(String(read()));
             Object.defineProperty(Object.prototype, 's', {
               set(x) { log.push(this === window, x); },
               configurable: true,
             });
             function write(x) { window.s = x; }
             for (i = 0; i < 3; i++) write(i);
             delete Object.prototype.s;
             write(3);
             log.push(window.s);
             log.join()"
        ),
        "proto,changed,own,changed,undefined,true,0,true,1,true,2,3"
    );
}

#[test]
fn a_cached_site_tells_the_proxy_from_an_object_on_top_of_it() {
    let (mut rt, _) = with_proxy(RuntimeConfig::default());
    assert_eq!(
        eval(
            &mut rt,
            "window.w = 'seen';
             var child = Object.create(window);
             function read(o) { return o.w; }
             function write(o, x) { o.w = x; }
             var log = [read(window), read(window), read(child), read(window)];
             write(window, 'a');
             write(window, 'b');
             write(child, 'own');
             log.push(window.w, child.w, read(child), read(window));
             delete child.w;
             log.push(read(child));
             log.join()"
        ),
        "seen,seen,seen,seen,b,own,own,b,b"
    );
}

#[test]
fn the_internal_methods_forward_to_the_target() {
    let (mut rt, _) = with_proxy(RuntimeConfig::default());
    assert_eq!(
        eval(
            &mut rt,
            "var v = 1; window.e = 2;
             Object.defineProperty(window, 'fixed', {value: 3});
             var d = Object.getOwnPropertyDescriptor(window, 'fixed');
             [
               Object.getPrototypeOf(window) === Object.prototype,
               'v' in window, 'nope' in window,
               Object.keys(window).includes('e'),
               Reflect.ownKeys(window).includes('fixed'),
               d.value, d.writable, d.configurable,
               delete window.e, 'e' in window, delete window.v, delete window.fixed,
               typeof window, Array.isArray(window),
               Object.prototype.toString.call(window),
               window instanceof Object,
               Reflect.get(window, 'v'), Reflect.set(window, 'v', 9), v,
               Reflect.has(window, 'v'),
             ].join()"
        ),
        "true,true,false,true,true,3,false,false,true,false,false,false,object,false,\
         [object Object],true,1,true,9,true"
    );
    assert_eq!(
        eval(
            &mut rt,
            "var seen = []; for (var k in window) { if (k === 'v') seen.push(k); } seen.join()"
        ),
        "v"
    );
}

#[test]
fn extensions_and_the_prototype_are_refused_with_chromiums_errors() {
    let (mut rt, _) = with_proxy(RuntimeConfig::default());
    assert_eq!(
        eval(
            &mut rt,
            "function E(g) { try { return String(g()); } catch (e) { return e.name + ': ' + e.message; } }
             [
               E(() => Object.preventExtensions(window)),
               Reflect.preventExtensions(window),
               Object.isExtensible(window),
               E(() => Object.freeze(window)),
               E(() => Object.seal(window)),
               Object.isFrozen(window),
               E(() => Object.setPrototypeOf(window, Object.prototype) === window),
               Reflect.setPrototypeOf(window, {}),
               E(() => Object.setPrototypeOf(window, {})).startsWith('TypeError: Immutable prototype object'),
             ].join('|')"
        ),
        "TypeError: Cannot prevent extensions|false|true|TypeError: Cannot freeze|\
         TypeError: Cannot seal|false|true|false|true"
    );
    // Index keys: no property (Chromium's wording).
    assert_eq!(
        eval(
            &mut rt,
            "window[5] = 1;
             var strict = E(() => (function () { 'use strict'; window[6] = 1; })());
             var define = E(() => Object.defineProperty(window, '7', {value: 1}));
             [String(window[5]), strict, define, Reflect.defineProperty(window, '7', {value: 1}), '7' in window].join('|')"
        ),
        "undefined|TypeError: Failed to set an indexed property [6] on 'Window': \
         Indexed property setter is not supported.|TypeError: Failed to set an indexed \
         property [7] on 'Window': Index property setter is not supported.|true|false"
    );
}

#[test]
fn a_proxy_works_as_a_prototype_and_as_a_proxy_target() {
    let (mut rt, _) = with_proxy(RuntimeConfig::default());
    assert_eq!(
        eval(
            &mut rt,
            "var v = 1;
             Object.defineProperty(window, 'me', {get() { return this; }, configurable: true});
             var o = Object.setPrototypeOf({}, window);
             var c = Object.create(window);
             o.y = 2;
             o.v = 3;
             var p = new Proxy(window, {});
             var seen = [];
             var t = new Proxy(window, {get(target, k, r) { seen.push(target === window, r === t); return Reflect.get(target, k, r); }});
             [
               o.v, window.v, 'y' in window, o.y, c.v, c.me === c, o.nope,
               Object.getPrototypeOf(o) === window, o instanceof Object,
               p.v, p.me === p, 'v' in p, Object.getPrototypeOf(p) === Object.prototype,
               Reflect.preventExtensions(p), Object.isExtensible(p),
               t.v, seen.join(' '),
             ].join()"
        ),
        "3,1,false,2,1,true,,true,true,1,true,true,true,false,true,1,true true"
    );
    // Identity in collections.
    assert_eq!(
        eval(
            &mut rt,
            "var m = new WeakMap([[window, 'in map']]);
             var s = new WeakSet([this]);
             var r = new WeakRef(globalThis);
             [m.get(this), s.has(window), r.deref() === window, new Set([window, this, globalThis]).size].join()"
        ),
        "in map,true,true,1"
    );
}

#[test]
fn json_and_with_see_the_target() {
    let (mut rt, _) = with_proxy(RuntimeConfig::default());
    assert_eq!(
        eval(
            &mut rt,
            "var plain = 1;
             var names = [];
             with (window) { names.push(plain); plain = 2; }
             names.push(plain);
             var cycle = '';
             try { JSON.stringify({a: window}); } catch (e) { cycle = e.name; }
             names.push(cycle);
             names.join()"
        ),
        "1,2,TypeError"
    );
}

#[test]
fn a_retarget_keeps_the_identity_and_drops_the_expando() {
    let (mut rt, proxy) = with_proxy(RuntimeConfig::default());
    let second = rt.create_realm().unwrap();
    let new_window = rt.global(second).unwrap();
    assert_eq!(
        eval(
            &mut rt,
            "window.expando = 1; var kept = window;
             function read() { return window.expando; }
             function readThroughKept(w) { return w.expando; }
             function write(w, x) { w.other = x; }
             window.other = 0;
             var values = [];
             for (var i = 0; i < 3; i++) { write(window, i); values.push(read(), readThroughKept(kept), window.other); }
             var ids = new WeakMap([[kept, 'id']]);
             values.join()"
        ),
        "1,1,0,1,1,1,1,1,2"
    );
    rt.set_window_proxy_target(proxy, Some(new_window)).unwrap();
    assert_eq!(rt.window_proxy_target(proxy.into()), Some(new_window));
    assert_eq!(rt.window_proxy_frame(proxy.into()), Some(1));
    // The running realm is still 0; its `window` variable holds the proxy,
    // whose target is the other realm's global object now.
    assert_eq!(
        eval(
            &mut rt,
            "var after = [String(read()), String(readThroughKept(kept)), kept === window, ids.get(kept)];
             write(kept, 'new');
             kept.Array === Array ? after.push('same') : after.push('other realm');
             after.join()"
        ),
        "undefined,undefined,true,id,other realm"
    );
    assert_eq!(
        text(&mut rt, second, "String(other)"),
        "new",
        "the write went to the new target"
    );
    // The old `Window` kept its properties.
    assert_eq!(text(&mut rt, second, "String(typeof expando)"), "undefined");
    // And back: the old properties are visible again, through the same sites.
    let first = rt.global(0).unwrap();
    rt.set_window_proxy_target(proxy, Some(first)).unwrap();
    assert_eq!(eval(&mut rt, "String(read())"), "1");
}

#[test]
fn a_retarget_survives_the_collector() {
    let mut config = RuntimeConfig::default();
    config.heap.stress = true;
    let (mut rt, proxy) = with_proxy(config);
    let second = rt.create_realm().unwrap();
    let new_window = rt.global(second).unwrap();
    assert_eq!(
        eval(
            &mut rt,
            "window.a = {x: 1}; var objects = []; for (var i = 0; i < 50; i++) objects.push({i}); window.a.x"
        )
        .as_str(),
        "1"
    );
    rt.set_window_proxy_target(proxy, Some(new_window)).unwrap();
    rt.collect();
    assert_eq!(
        eval(&mut rt, "window.b = {y: 2}; window.b.y + typeof window.a"),
        "2undefined"
    );
    assert_eq!(text(&mut rt, second, "b.y"), "2");
}

#[test]
fn the_realms_stay_apart() {
    let (mut rt, first) = with_proxy(RuntimeConfig::default());
    let realm = rt.create_realm().unwrap();
    let second = install_proxy(&mut rt, realm, 2);
    assert_ne!(first, second);
    // The same text in two realms, with sites of each code object.
    assert_eq!(eval(&mut rt, "var name = 'first'; window.name"), "first");
    assert_eq!(
        text(&mut rt, realm, "var name = 'second'; window.name"),
        "second"
    );
    assert_eq!(eval(&mut rt, "name"), "first");
    // A function of one realm sees its own globals and global this value.
    let f = rt
        .eval_named_at(
            realm,
            "(function () { return [this === window, name].join(); })",
            "",
            (0, 0),
        )
        .unwrap();
    let global = rt.global(0).unwrap();
    let key = rt.key_from_str("f").unwrap();
    let desc = PropertyDescriptor::data(f, true, true, true);
    assert!(rt.define_property(global, key, desc).is_ok());
    assert_eq!(eval(&mut rt, "f()"), "true,second");
    assert_eq!(rt.global_this(realm).unwrap(), second);
    // One site that reads through the proxies of two realms.
    let key = rt.key_from_str("other").unwrap();
    let desc = PropertyDescriptor::data(second.into(), true, true, true);
    rt.define_property(global, key, desc).unwrap();
    assert_eq!(
        eval(
            &mut rt,
            "function read(w) { return w.name; }
             [read(window), read(other), read(window), read(other)].join()"
        ),
        "first,second,first,second"
    );
    assert_eq!(eval(&mut rt, "other.Array === Array"), "false");
    assert_eq!(
        rt.global(realm).unwrap(),
        rt.window_proxy_target(second.into()).unwrap()
    );
    assert_eq!(
        eval(&mut rt, "Function.prototype.toString.name"),
        "toString"
    );
}

#[test]
fn a_native_runs_in_its_own_realm() {
    let (mut rt, _) = with_proxy(RuntimeConfig::default());
    let realm = rt.create_realm().unwrap();
    let second = install_proxy(&mut rt, realm, 2);
    let global = rt.global(0).unwrap();
    let key = rt.key_from_str("other").unwrap();
    let desc = PropertyDescriptor::data(second.into(), true, false, true);
    rt.define_property(global, key, desc).unwrap();
    assert_eq!(
        eval(
            &mut rt,
            "var cross = other.eval('[1, 2]');
             function realmOf(o, C) { return o instanceof C; }
             var thrown = '';
             try { other.Array.prototype.map.call(null); } catch (e) {
               thrown = [e instanceof other.TypeError, e instanceof TypeError].join();
             }
             var [a, b] = cross;
             [
               // Objects that a native of the other realm makes.
               realmOf(other.Array(3), other.Array), realmOf(other.Array(3), Array),
               realmOf(other.JSON.parse('[]'), other.Array),
               realmOf(other.Object(), other.Object), realmOf(new other.Object(), other.Object),
               realmOf(other.Object.keys({}), other.Array),
               realmOf(other.Object.getOwnPropertyDescriptor({x: 1}, 'x'), other.Object),
               realmOf(other.Function('return 1'), other.Function), realmOf(new other.Function(), Function),
               realmOf(other.Promise.resolve(1), other.Promise),
               // A primitive reads through the running realm's String.
               realmOf(other.String('a,b').split(','), other.Array),
               realmOf(new other.Map(), other.Map), realmOf(new other.Error('x'), other.Error),
               // The array methods and ArraySpeciesCreate (§23.1.3): a constructor
               // of another realm's %Array% gives way to the running realm's.
               Array.isArray(cross), realmOf(cross, Array), realmOf(cross, other.Array),
               realmOf(cross.map(x => x), other.Array),
               realmOf(Array.prototype.map.call(cross, x => x), Array),
               realmOf(Array.prototype.concat.call(cross), Array),
               a + b, [].concat(cross).length, JSON.stringify(cross),
               // A function of this realm keeps it.
               realmOf([], Array), realmOf({}, Object), realmOf(() => 1, Function), thrown,
             ].join()"
        ),
        "true,false,true,true,true,true,true,true,false,true,false,true,true,\
         true,false,true,true,true,true,3,2,[1,2],true,true,true,true,false"
    );
}

/// The hooks of a proxy without a target: every method throws; `get` answers
/// for the name `open`.
fn refuse() -> VmError {
    VmError::type_error("blocked")
}

fn hooks() -> WindowProxyHooks {
    WindowProxyHooks {
        get_prototype_of: |_, _| Ok(None),
        set_prototype_of: |_, _, proto| Ok(proto.is_none()),
        prevent_extensions: |_, _| Err(refuse()),
        get_own_property: |_, _, _| Err(refuse()),
        define_own_property: |_, _, _, _| Err(refuse()),
        has: |_, _, _| Err(refuse()),
        get: get_hook,
        set: |_, _, _, _, _| Err(refuse()),
        delete: |_, _, _| Err(refuse()),
        own_keys: |_, _| Err(refuse()),
    }
}

fn get_hook(rt: &mut Runtime, _: Gc<Object>, key: PropertyKey, _: Value) -> VmResult<Value> {
    if rt.key_to_string(key).as_deref() == Some("open") {
        return Ok(Value::Int(1));
    }
    Err(refuse())
}

#[test]
fn a_proxy_without_a_target_calls_the_hooks() {
    let mut rt = Runtime::new(RuntimeConfig::default()).unwrap();
    rt.set_window_proxy_hooks(hooks());
    let remote = rt.new_window_proxy(3, None).unwrap();
    assert_eq!(rt.window_proxy_target(remote.into()), None);
    assert_eq!(rt.window_proxy_frame(remote.into()), Some(3));
    let global = rt.global(0).unwrap();
    let key = rt.key_from_str("remote").unwrap();
    rt.define_property(
        global,
        key,
        PropertyDescriptor::data(remote.into(), true, true, true),
    )
    .unwrap();
    assert_eq!(
        eval(
            &mut rt,
            "function E(g) { try { return String(g()); } catch (e) { return e.message; } }
             function read(w) { return w.open; }
             [
               read(remote), read(remote),
               E(() => remote.x), E(() => { remote.x = 1; }), E(() => 'x' in remote),
               E(() => delete remote.x), E(() => Object.keys(remote)),
               E(() => Object.defineProperty(remote, 'x', {value: 1})),
               E(() => Object.getOwnPropertyDescriptor(remote, 'x')),
               E(() => Object.preventExtensions(remote)),
               Object.isExtensible(remote),
               String(Object.getPrototypeOf(remote)),
               Reflect.setPrototypeOf(remote, null), Reflect.setPrototypeOf(remote, {}),
               typeof remote, E(() => remote instanceof Object),
             ].join('|')"
        ),
        "1|1|blocked|blocked|blocked|blocked|blocked|blocked|blocked|blocked|true|null|true|false|\
         object|false"
    );
    assert_eq!(rt.host_object(remote.into()), None);
    // A target comes back: the same proxy forwards.
    rt.set_window_proxy_target(remote, Some(global)).unwrap();
    assert_eq!(eval(&mut rt, "var v = 5; remote.v"), "5");
}

#[test]
fn the_realms_of_a_runtime_have_a_limit() {
    let mut rt = Runtime::new(RuntimeConfig::default()).unwrap();
    let mut realms = 1;
    while rt.create_realm().is_ok() {
        realms += 1;
        assert!(realms <= 2048, "a realm beyond the limit of host objects");
    }
    assert_eq!(realms, 2048);
    assert_eq!(eval(&mut rt, "String(1 + 1)"), "2");
}

#[test]
fn only_the_global_object_of_a_realm_is_a_target() {
    let mut rt = Runtime::new(RuntimeConfig::default()).unwrap();
    let object = rt.eval("({})").unwrap();
    let Value::Object(object) = object else {
        panic!("an object");
    };
    assert!(rt.new_window_proxy(0, Some(object)).is_err());
    assert!(rt.new_window_proxy(1 << 31, None).is_err());
    assert!(rt.set_global_this(0, object).is_err());
    assert!(rt.eval_named_at(5, "1", "", (0, 0)).is_err());
    assert!(rt.eval_module_at(5, "1", "", (0, 0)).is_err());
    let global = rt.global(0).unwrap();
    rt.set_global_this(0, global).unwrap();
    let key = rt.key_from_str("globalThis").unwrap();
    let property = rt.own_property_of(global, key).unwrap();
    assert!(matches!(property, Some(Property::Data { .. })));
}
