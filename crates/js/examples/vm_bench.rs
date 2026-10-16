//! Measurements of the compiler and the interpreter for ADR 0026 ("Consequences") and
//! `docs/js-spike-report.md`: the instruction size, the code size of a
//! generated program, and the time of small programs (compare with
//! `node --jitless`, V8's interpreter, on the same programs; the
//! `--print-js` option writes them as a Node.js script).
//!
//! `cargo run --release -p swb-js --example vm_bench [-- --print-js]`

use std::time::{Duration, Instant};

use swb_js::{Runtime, RuntimeConfig, Value};

/// The benchmark programs: (name, source). Each source defines `run()`;
/// locals keep the loops out of the global object.
const PROGRAMS: &[(&str, &str)] = &[
    (
        "fib(27)",
        "function fib(n) { return n < 2 ? n : fib(n - 1) + fib(n - 2); }
         function run() { return fib(27); }",
    ),
    (
        "sum 10M integers",
        "function run() { var s = 0; for (var i = 0; i < 10000000; i++) { s += i; } return s; }",
    ),
    (
        "o.x = o.x + o.y, 5M times",
        "function run() { var o = {x: 0, y: 1}; for (var i = 0; i < 5000000; i++) { o.x = o.x + o.y; } return o.x; }",
    ),
    (
        "closure counter 1M calls",
        "function make() { var n = 0; return function () { n++; return n; }; }
         function run() { var c = make(); var r = 0; for (var i = 0; i < 1000000; i++) { r = c(); } return r; }",
    ),
    (
        "1M small objects",
        "function run() { var last; for (var i = 0; i < 1000000; i++) { last = {a: i, b: i + 0.5, c: 'x'}; } return last.a; }",
    ),
    (
        "string: 20k appends of 'ab'",
        "function run() { var s = ''; for (var i = 0; i < 20000; i++) { s += 'ab'; } return s.length; }",
    ),
    (
        "sum 10M integers in a try",
        "function run() { var s = 0; for (var i = 0; i < 10000000; i++) { try { s += i; } catch (e) { s = 0; } } return s; }",
    ),
    (
        "sum 10M integers in a try-finally",
        "function run() { var s = 0; for (var i = 0; i < 10000000; i++) { try { s += i; } finally { s += 0; } } return s; }",
    ),
    (
        "throw and catch 1M times",
        "function run() { var c = 0; for (var i = 0; i < 1000000; i++) { try { throw i; } catch (e) { c += e; } } return c; }",
    ),
    (
        "throw TypeError 1M times",
        "function run() { var c = 0; for (var i = 0; i < 1000000; i++) { try { null.x; } catch (e) { c++; } } return c; }",
    ),
    (
        "throw new Error 1M times",
        "function run() { var c = 0; for (var i = 0; i < 1000000; i++) { try { throw new Error('x'); } catch (e) { c++; } } return c; }",
    ),
    (
        "throw new Error 10 frames deep, 200k times",
        "function d(n) { if (n) return d(n - 1); throw new Error('x'); }
         function run() { var c = 0; for (var i = 0; i < 200000; i++) { try { d(10); } catch (e) { c++; } } return c; }",
    ),
    (
        "new Error and read stack, 100k times",
        "function d(n) { return n ? d(n - 1) : new Error('x'); }
         function run() { var c = 0; for (var i = 0; i < 100000; i++) { c += d(5).stack.length; } return c; }",
    ),
    (
        "forEach over 1M elements",
        "var arr = []; for (var i = 0; i < 1000000; i++) arr[i] = i;
         function run() { var s = 0; arr.forEach(function (x) { s += x; }); return s; }",
    ),
    (
        "for loop over 1M elements",
        "var arr = []; for (var i = 0; i < 1000000; i++) arr[i] = i;
         function run() { var s = 0; for (var i = 0; i < arr.length; i++) { s += arr[i]; } return s; }",
    ),
    (
        "join 1M elements",
        "var arr = []; for (var i = 0; i < 1000000; i++) arr[i] = i;
         function run() { return arr.join().length; }",
    ),
    (
        "1M calls that read arguments",
        "function f(a, b) { return arguments.length + arguments[0] + b; }
         function run() { var s = 0; for (var i = 0; i < 1000000; i++) { s += f(i, 1); } return s; }",
    ),
    (
        "1M calls that write mapped arguments",
        "function f(a, b) { arguments[1] = a; return b + a; }
         function run() { var s = 0; for (var i = 0; i < 1000000; i++) { s += f(i, 1); } return s; }",
    ),
    (
        "1M calls of new Function code",
        "var add = new Function('a', 'b', 'return a + b');
         function run() { var s = 0; for (var i = 0; i < 1000000; i++) { s = add(s, 1); } return s; }",
    ),
    (
        "string: indexOf 1M calls",
        "function run() { var h = 'the quick brown fox jumps over the lazy dog, again and again'; var r = 0; for (var i = 0; i < 1000000; i++) { r += h.indexOf('lazy') + h.indexOf('x', 30); } return r; }",
    ),
    (
        "string: slice and charCodeAt 1M calls",
        "function run() { var h = 'the quick brown fox jumps over the lazy dog'; var r = 0; for (var i = 0; i < 1000000; i++) { r += h.slice(4, 9).length + h.charCodeAt(i % 40); } return r; }",
    ),
    (
        "string: split 100k times into 8 pieces",
        "function run() { var h = 'a,bb,ccc,dddd,eeeee,ffffff,ggggggg,h'; var r = 0; for (var i = 0; i < 100000; i++) { r += h.split(',').length; } return r; }",
    ),
    (
        "array: sort 100k integers (default order)",
        "var data = (function () { var d = [], x = 1; for (var i = 0; i < 100000; i++) { x = (x * 1103515245 + 12345) % 2147483648; d.push(x | 0); } return d; })();
         function run() { var s = data.slice().sort(); return s[0] + s[99999]; }",
    ),
    (
        "array: sort 100k integers with (a, b) => a - b",
        "var data = (function () { var d = [], x = 1; for (var i = 0; i < 100000; i++) { x = (x * 1103515245 + 12345) % 2147483648; d.push(x | 0); } return d; })();
         function run() { var s = data.slice().sort(function (a, b) { return a - b; }); return s[0] + s[99999]; }",
    ),
    (
        "map: set and get of 100k integer keys",
        "function run() { var m = new Map(); for (var i = 0; i < 100000; i++) m.set(i, i); var s = 0; for (var i = 0; i < 100000; i++) s += m.get(i); return s + m.size; }",
    ),
    (
        "map: set and get of 100k string keys",
        "var keys = []; for (var i = 0; i < 100000; i++) keys.push('key' + i);
         function run() { var m = new Map(); for (var i = 0; i < 100000; i++) m.set(keys[i], i); var s = 0; for (var i = 0; i < 100000; i++) s += m.get(keys[i]); return s + m.size; }",
    ),
    (
        "set: add and has of 100k integers",
        "function run() { var m = new Set(); for (var i = 0; i < 100000; i++) m.add(i); var s = 0; for (var i = 0; i < 100000; i++) if (m.has(i)) s++; return s + m.size; }",
    ),
    (
        "map: for-of over 100k entries, 5 times",
        "var m = new Map(); for (var i = 0; i < 100000; i++) m.set(i, i);
         function run() { var s = 0; for (var r = 0; r < 5; r++) for (var [k, v] of m) s += k + v; return s; }",
    ),
    (
        "set: for-of over 100k values, 5 times",
        "var m = new Set(); for (var i = 0; i < 100000; i++) m.add(i);
         function run() { var s = 0; for (var r = 0; r < 5; r++) for (var v of m) s += v; return s; }",
    ),
    (
        "map: 1M alternating inserts and deletes",
        "function run() { var m = new Map(); for (var i = 0; i < 1000000; i++) { m.set(i, i); if (i > 10) m.delete(i - 10); } return m.size; }",
    ),
    (
        "iterator: values().map().filter().toArray() of 100k, 5 times",
        "var data = []; for (var i = 0; i < 100000; i++) data.push(i);
         function run() { var n = 0; for (var r = 0; r < 5; r++) n += data.values().map(function (x) { return x + 1; }).filter(function (x) { return x % 2 === 0; }).toArray().length; return n; }",
    ),
    (
        "iterator: Iterator.from(user iterator).map().toArray() of 100k, 5 times",
        "function run() { var n = 0; for (var r = 0; r < 5; r++) { var i = 0; var it = { next: function () { return i < 100000 ? { value: i++, done: false } : { value: undefined, done: true }; } }; n += Iterator.from(it).map(function (x) { return x + 1; }).toArray().length; } return n; }",
    ),
    (
        "iterator: take, drop, flatMap and reduce over 100k, 5 times",
        "var data = []; for (var i = 0; i < 100000; i++) data.push(i);
         function run() { var n = 0; for (var r = 0; r < 5; r++) n += data.values().drop(10).take(90000).flatMap(function (x) { return [x, x]; }).reduce(function (a, b) { return a + b; }, 0) % 1000; return n; }",
    ),
    (
        "iterator: for-of over map().filter() of 100k, 5 times",
        "var data = []; for (var i = 0; i < 100000; i++) data.push(i);
         function run() { var s = 0; for (var r = 0; r < 5; r++) for (var v of data.values().map(function (x) { return x + 1; }).filter(function (x) { return x % 2 === 0; })) s += v; return s; }",
    ),
    (
        "array: 1M push, then 1M pop",
        "function run() { var a = []; for (var i = 0; i < 1000000; i++) a.push(i); var s = 0; while (a.length) s += a.pop(); return s; }",
    ),
    (
        "array: indexOf on 100k elements, 200 times",
        "var arr = []; for (var i = 0; i < 100000; i++) arr.push(i);
         function run() { var r = 0; for (var i = 0; i < 200; i++) r += arr.indexOf(99999 - (i % 10)); return r; }",
    ),
    (
        "array: filter of 100k elements, 10 times",
        "var arr = []; for (var i = 0; i < 100000; i++) arr.push(i);
         function run() { var r = 0; for (var i = 0; i < 10; i++) r += arr.filter(function (x) { return x & 1; }).length; return r; }",
    ),
    (
        "array: reduce of 100k elements, 10 times",
        "var arr = []; for (var i = 0; i < 100000; i++) arr.push(i);
         function run() { var r = 0; for (var i = 0; i < 10; i++) r += arr.reduce(function (a, b) { return a + b; }, 0); return r; }",
    ),
    (
        "JSON.parse of a 1 MB text",
        r#"var text = (function () { var items = []; for (var i = 0; i < 5000; i++) items.push({ id: i, name: 'item ' + i, tags: ['alpha', 'beta', 'gamma'], price: i * 1.25 + 0.01, active: i % 2 === 0, note: null, nested: { x: i, y: [i, i + 1, i + 2], s: 'text with "escapes" and \n a break and \u00e9' } }); return JSON.stringify(items, null, 1); })();
         function run() { return JSON.parse(text).length + text.length; }"#,
    ),
    (
        "JSON.stringify of the 1 MB result",
        r#"var value = (function () { var items = []; for (var i = 0; i < 5000; i++) items.push({ id: i, name: 'item ' + i, tags: ['alpha', 'beta', 'gamma'], price: i * 1.25 + 0.01, active: i % 2 === 0, note: null, nested: { x: i, y: [i, i + 1, i + 2], s: 'text with "escapes" and \n a break and \u00e9' } }); return JSON.parse(JSON.stringify(items, null, 1)); })();
         function run() { return JSON.stringify(value, null, 1).length; }"#,
    ),
    (
        "JSON.stringify of the result without indentation",
        r#"var value = (function () { var items = []; for (var i = 0; i < 5000; i++) items.push({ id: i, name: 'item ' + i, tags: ['alpha', 'beta', 'gamma'], price: i * 1.25 + 0.01, active: i % 2 === 0, note: null, nested: { x: i, y: [i, i + 1, i + 2], s: 'text with "escapes" and \n a break and \u00e9' } }); return items; })();
         function run() { return JSON.stringify(value).length; }"#,
    ),
    (
        "string: replace, trim, padStart 300k calls",
        "function run() { var r = 0; for (var i = 0; i < 300000; i++) { r += ' x-y-z '.trim().replace('-', '+').padStart(8, '.').length; } return r; }",
    ),
    (
        "string: toLowerCase, toUpperCase 4M calls on 10 units",
        "function run() { var a = 'Hello, Wor', b = 'hello, wor'; var r = 0; for (var i = 0; i < 1000000; i++) { r += a.toLowerCase().length + b.toLowerCase().length + a.toUpperCase().length + b.toUpperCase().length; } return r; }",
    ),
    (
        "string: normalize('NFC') 2M calls on normalized text",
        "function run() { var s = 'caf' + String.fromCharCode(233) + ' cr' + String.fromCharCode(232) + 'me'; var w = 'a' + String.fromCharCode(0x3b1, 0x3b2, 0x3b3, 0x44f); var r = 0; for (var i = 0; i < 1000000; i++) { r += s.normalize('NFC').length + w.normalize().length; } return r; }",
    ),
    (
        "string: localeCompare 1M calls",
        "function run() { var a = 'apple pie', b = 'Apple tart'; var r = 0; for (var i = 0; i < 1000000; i++) { r += a.localeCompare(b); } return r; }",
    ),
];

/// One block of the generated program (the front-end measurements use the same program).
const CHUNK: &str = r"function chunkN(p, q) {
  var total = 0, items = [1, 2.5, 'three', null, true, , { a: p, 'b': q, [p]: q, m() { return this.a; } }];
  let counter = 0;
  const limit = items.length * 2;
  function inner(x) { return x + counter++ - total * limit / 3 % 2; }
  for (let i = 0; i < limit; i++) {
    if (i & 1) { continue; } else if (i >> 3) { break; }
    total += inner(i) ? items[i % items.length] : -i;
    counter = counter >= 10 ? 0 : counter;
  }
  var add = (a, b) => a + b, twice = v => { return add(v, v); };
  while (counter > 0) { counter -= 1; total = total || twice(counter) && !counter; }
  do { total **= 1; } while (total < 0);
  if (typeof p === 'undefined' || p instanceof inner) { total = void 0; } else { counter = 0; }
  label: for (var j = 0; j < 3; ++j) { if (j == 2) break label; }
  try { if (p) throw new Error('x'); } catch (err) { total = err; } finally { counter = counter || 0; }
  try { total &&= 1; total ||= 2; total ??= 3; } catch { }
  return { total: total, counter, inner, add, gen: function* () { yield total; yield; } };
}
chunkN(1, 2);
";

fn main() {
    if std::env::args().any(|a| a == "--print-js") {
        print_js();
        return;
    }
    std::thread::Builder::new()
        .stack_size(64 << 20)
        .spawn(measure)
        .expect("a thread")
        .join()
        .expect("the measurements finish");
}

fn measure() {
    let mut rt = Runtime::new(RuntimeConfig::default()).expect("a runtime");
    // Code size.
    let mut text = String::new();
    let mut n = 0;
    while text.len() < 5 << 20 {
        text.push_str(&CHUNK.replace('N', &n.to_string()));
        n += 1;
    }
    let start = Instant::now();
    let stats = rt.compile_stats(&text).expect("the program compiles");
    let compile = start.elapsed();
    println!(
        "instruction size {} bytes; generated program {} bytes: {} functions, {} instructions ({:.2} per source byte), {} constants, {} sites, {} line entries",
        stats.instruction_size,
        text.len(),
        stats.functions,
        stats.instructions,
        stats.instructions as f64 / text.len() as f64,
        stats.constants,
        stats.sites,
        stats.lines,
    );
    println!(
        "code objects {:.1} MB = {:.2} bytes per source byte; parse + compile {:.0} ms ({:.0} MB/s)",
        stats.bytes as f64 / 1e6,
        stats.bytes as f64 / text.len() as f64,
        compile.as_secs_f64() * 1000.0,
        text.len() as f64 / compile.as_secs_f64() / 1e6,
    );
    rt.collect();
    // The generated program also runs.
    let start = Instant::now();
    let result = rt.eval(&text);
    println!(
        "running the generated program: {:?} in {:.0} ms",
        result.map(|_| ()),
        start.elapsed().as_secs_f64() * 1000.0
    );
    // The time limit: how late after the deadline endless scripts end.
    for source in [
        "for (;;) {}",
        "function f() { f(); } for (;;) { try { f(); } catch (e) {} }",
        // `find` visits every index (the other loops skip holes).
        "var a = []; a.length = 4294967295; a.find(function () {})",
        "var a = []; a.length = 4294967295; Object.setPrototypeOf(a, new String('x')); Array.prototype.join.call(a, '')",
    ] {
        let mut rt = Runtime::new(RuntimeConfig::default()).expect("a runtime");
        let limit = Duration::from_millis(100);
        let start = Instant::now();
        rt.set_deadline(Some(start + limit));
        let result = rt.eval(source);
        let late = start.elapsed().saturating_sub(limit);
        println!(
            "time limit 100 ms: {source}: {:?}, {:.2} ms after the deadline",
            result.map(|_| ()),
            late.as_secs_f64() * 1000.0
        );
    }
    // Speed.
    for (name, source) in PROGRAMS {
        let mut rt = Runtime::new(RuntimeConfig::default()).expect("a runtime");
        rt.eval(source).expect("the program compiles");
        let mut best = Duration::MAX;
        let mut gc = Duration::ZERO;
        let mut collections = 0;
        let mut result = Value::Undefined;
        for _ in 0..5 {
            let before = rt.heap().stats();
            let start = Instant::now();
            result = rt.eval("run()").expect("the program runs");
            let elapsed = start.elapsed();
            if elapsed < best {
                best = elapsed;
                let after = rt.heap().stats();
                gc = after.total_pause.saturating_sub(before.total_pause);
                collections = after.collections - before.collections;
            }
        }
        println!(
            "{name}: {:.1} ms (result {}), GC {:.1} ms in {collections} collections ({:.0} %)",
            best.as_secs_f64() * 1000.0,
            rt.display(result),
            gc.as_secs_f64() * 1000.0,
            gc.as_secs_f64() / best.as_secs_f64() * 100.0,
        );
    }
}

/// Prints the programs as a Node.js script with the same timing.
fn print_js() {
    println!("const results = [];");
    for (name, source) in PROGRAMS {
        println!(
            "(function () {{ {source}\n let best = Infinity; let r; for (let k = 0; k < 5; k++) {{ const t = process.hrtime.bigint(); r = run(); const ms = Number(process.hrtime.bigint() - t) / 1e6; if (ms < best) best = ms; }} console.log({name:?} + ': ' + best.toFixed(1) + ' ms (result ' + r + ')'); }})();"
        );
    }
}
