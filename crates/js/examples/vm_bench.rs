//! Measurements of the compiler and the interpreter for the spike report
//! (ADR 0026 "Consequences"): the instruction size, the code size of a
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
];

/// One block of the generated program (the program of session 2).
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
        "var a = []; a.length = 4294967295; a.forEach(function () {})",
        "var a = []; a.length = 4294967295; a.join('')",
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
