'use strict';
// Reference values for swb's number conversions in the radixes other than
// 10, written from their definitions with exact BigInt arithmetic and a
// plain search, independent of the algorithms in js-text:
//
// - radixString(x, radix): Number::toString(x, radix) as swb defines it
//   (ADR 0026 section 15): the digit string with the fewest significant
//   digits whose exact value rounds (to nearest, ties to even) to x; of
//   those the closest to x; of two equally close the one whose digits
//   form an even integer (§6.1.6.1.20 note 2); written positionally,
//   without an exponent.
// - parseIntExact(text, radix): parseInt's value of the leading digits,
//   the exact integer rounded to the nearest double (Number of a BigInt).

const view = new DataView(new ArrayBuffer(8));

// x = F × 2^E for a positive finite x, F < 2^53.
function decompose(x) {
  view.setFloat64(0, x);
  const bits = view.getBigUint64(0);
  const fraction = bits & ((1n << 52n) - 1n);
  const biased = Number((bits >> 52n) & 0x7ffn);
  return biased === 0 ? [fraction, -1074] : [fraction | (1n << 52n), biased - 1075];
}

// A rational number [num, den] for N × 2^p.
function dyadic(n, p) {
  return p >= 0 ? [n << BigInt(p), 1n] : [n, 1n << BigInt(-p)];
}

// A rational number for N × radix^p.
function radixPower(n, radix, p) {
  const b = BigInt(radix);
  return p >= 0 ? [n * b ** BigInt(p), 1n] : [n, b ** BigInt(-p)];
}

function cmp(a, b) {
  const l = a[0] * b[1];
  const r = b[0] * a[1];
  return l < r ? -1 : l > r ? 1 : 0;
}

// The values that round to x: the half gaps to the neighbours. Below a
// power of two (not at the smallest exponent) the gap is half as large.
// The ends round to x when F is even.
function roundingRange(x) {
  const [f, e] = decompose(x);
  const unequal = f === 1n << 52n && e > -1074;
  return {
    low: unequal ? dyadic(4n * f - 1n, e - 2) : dyadic(2n * f - 1n, e - 1),
    high: dyadic(2n * f + 1n, e - 1),
    inclusive: f % 2n === 0n,
  };
}

function inRange(value, range) {
  const lo = cmp(value, range.low);
  const hi = cmp(value, range.high);
  return range.inclusive ? lo >= 0 && hi <= 0 : lo > 0 && hi < 0;
}

// t with radix^t ≤ x < radix^(t+1): an estimate from the bit lengths,
// corrected by exact comparisons.
function topPosition(xr, radix) {
  const bits = xr[0].toString(2).length - xr[1].toString(2).length;
  let t = Math.floor(bits / Math.log2(radix));
  while (cmp(radixPower(1n, radix, t), xr) > 0) t--;
  while (cmp(radixPower(1n, radix, t + 1), xr) <= 0) t++;
  return t;
}

function radixString(x, radix) {
  if (Number.isNaN(x)) return 'NaN';
  if (x === 0) return '0';
  if (x === Infinity) return 'Infinity';
  if (x === -Infinity) return '-Infinity';
  if (x < 0) return '-' + radixString(-x, radix);
  const [f, e] = decompose(x);
  const xr = dyadic(f, e);
  const range = roundingRange(x);
  const t = topPosition(xr, radix);
  // Search k = 1, 2, …: the k-digit strings closest to x are the
  // multiples of radix^(t−k+1) just below and just above x. A double
  // needs at most 54 significant digits; the bound only stops the loop.
  for (let k = 1; k <= 1200; k++) {
    const p = t - k + 1;
    const q = radixPower(1n, radix, p);
    const below = (xr[0] * q[1]) / (xr[1] * q[0]); // floor(x / q)
    const candidates = [below, below + 1n].filter((c) => inRange(radixPower(c, radix, p), range));
    if (candidates.length === 0) continue;
    let c = candidates[0];
    if (candidates.length === 2) {
      // 2x against the midpoint (2·below + 1) q.
      const order = cmp([2n * xr[0], xr[1]], radixPower(2n * below + 1n, radix, p));
      if (order < 0) c = below;
      else if (order > 0) c = below + 1n;
      else c = below % 2n === 0n ? below : below + 1n; // the even integer
    }
    let digits = c.toString(radix);
    let point = p; // value = digits × radix^point
    while (digits.length > 1 && digits.endsWith('0')) { digits = digits.slice(0, -1); point++; }
    if (point >= 0) return digits + '0'.repeat(point);
    const n = digits.length + point; // digits before the point
    if (n > 0) return digits.slice(0, n) + '.' + digits.slice(n);
    return '0.' + '0'.repeat(-n) + digits;
  }
  throw new Error('no digits for ' + x);
}

// parseInt(text, radix) for a radix from 2 to 36: the digits after white
// space and a sign (without the 0x prefix rule, which these radixes do
// not have), as an exact BigInt, then Number(), which rounds to nearest,
// ties to even.
const DIGITS = '0123456789abcdefghijklmnopqrstuvwxyz';

function parseIntExact(text, radix) {
  let i = 0;
  while (i < text.length && /\s/.test(text[i])) i++;
  let sign = 1;
  if (text[i] === '-' || text[i] === '+') { if (text[i] === '-') sign = -1; i++; }
  const b = BigInt(radix);
  let value = 0n;
  let count = 0;
  for (; i < text.length; i++) {
    const c = text[i];
    const d = DIGITS.indexOf(c >= 'A' && c <= 'Z' ? c.toLowerCase() : c);
    if (d < 0 || d >= radix) break;
    value = value * b + BigInt(d);
    count++;
  }
  if (count === 0) return NaN;
  return sign * Number(value);
}

module.exports = { radixString, parseIntExact };
