'use strict';
// The cases of bigint_bench.rs in Node.js: add, mul, div and toString of
// values of 1, 10 and 1,000 limbs (64-bit), best of 5 runs.
// Usage: node --jitless crates/js-text/examples/bigint_bench.js
function value(bits, seed) {
  let state = BigInt(seed) | 1n;
  const mask = (1n << 64n) - 1n;
  let x = 0n;
  const limbs = Math.ceil(bits / 64);
  for (let i = 0; i < limbs; i++) {
    state ^= (state << 13n) & mask;
    state ^= state >> 7n;
    state ^= (state << 17n) & mask;
    x |= state << BigInt(64 * i);
  }
  x &= (1n << BigInt(bits)) - 1n;
  return x | (1n << BigInt(bits - 1));
}
function best(runs, inner, f) {
  let best = Infinity;
  for (let r = 0; r < runs; r++) {
    const start = process.hrtime.bigint();
    for (let i = 0; i < inner; i++) f();
    const ms = Number(process.hrtime.bigint() - start) / 1e6 / inner;
    best = Math.min(best, ms);
  }
  return best;
}
function format(ms) {
  if (ms >= 1) return ms.toFixed(1) + ' ms';
  if (ms >= 0.001) return (ms * 1000).toFixed(2) + ' µs';
  return (ms * 1e6).toFixed(1) + ' ns';
}
let sink;
console.log('| Case | Time |');
console.log('|---|---|');
for (const limbs of [1, 10, 1000]) {
  const bits = limbs * 64 - 1;
  const a = value(bits, 1), b = value(bits, 2), wide = value(bits * 2, 3);
  const inner = Math.max(3, Math.floor(1e6 / (limbs * limbs)));
  const rows = [
    [`add, ${limbs} limbs`, () => (sink = a + b)],
    [`mul, ${limbs} × ${limbs} limbs`, () => (sink = a * b)],
    [`div, ${limbs * 2} by ${limbs} limbs`, () => (sink = wide / b)],
    [`toString(10), ${limbs} limbs`, () => (sink = a.toString())],
  ];
  for (const [name, f] of rows) console.log(`| ${name} | ${format(best(5, inner, f))} |`);
}
const max = 1 << 18;
const full = value(max, 5), half = value(max / 2, 6), half2 = value(max / 2, 7);
console.log(`| mul at swb's limit | ${format(best(3, 1, () => (sink = half * half2)))} |`);
console.log(`| div at swb's limit | ${format(best(3, 1, () => (sink = full / half)))} |`);
console.log(`| toString(10) at swb's limit | ${format(best(3, 1, () => (sink = full.toString())))} |`);
