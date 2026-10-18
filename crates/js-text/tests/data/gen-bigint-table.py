#!/usr/bin/env python3
"""Writes bigint_reference.tsv, the reference table of js-text's BigInt
arithmetic (crates/js-text/tests/bigint_reference.rs).

The expected results come from Python's int, an arbitrary-precision
integer used as a black box, with the ECMAScript rules applied here:
truncating division with the dividend's sign for the remainder, the
shifts and bitwise operators on the infinite two's complement form (as
Python's own), asIntN and asUintN, the conversion to a double rounded to
nearest (float(int); OverflowError means +-Infinity), the exact
comparison with a double (Python compares int and float exactly), the
StringToBigInt grammar of ECMA-262 section 7.1.14 (checked by this script),
toString in the radixes 2 to 36, and the size limit of js-text
(MAX_BITS bits; a larger result is the error TooLarge).

One case per line, fields separated by tabs; the first field is the
operation:

  add|sub|mul|and|or|xor|shl|shr|cmp  A B RESULT
  divrem A B QUOTIENT REMAINDER        (divab: the same, and Algorithm D
                                        needs its add-back step)
  pow A E RESULT       not A RESULT     asint|asuint BITS A RESULT
  tof64 A F64          fromf64 F64 RESULT       cmpf A F64 ORDER
  tostr RADIX A TEXT   parse TEXT RESULT        lit TEXT RESULT

A value is hexadecimal with an optional "-", or <hex>P<k> for hex * 2^k
and M<k> for 2^k - 1 (also with "-"). F64 is the bit pattern of a double in
hexadecimal. ORDER is lt, eq, gt or nan. A RESULT can be an error name:
TooLarge, DivisionByZero, NegativeExponent, NotIntegral, InvalidSyntax.
TEXT uses \\uXXXX escapes for code units outside printable ASCII and for
the backslash.

Fixed seeds, so the output is reproducible.
Usage: python3 gen-bigint-table.py bigint_reference.tsv
"""

import math
import random
import struct
import sys

MAX_BITS = 1 << 18
LIMB = 1 << 64
rng = random.Random(20261018)


def enc(v):
    if isinstance(v, str):
        return v
    sign = "-" if v < 0 else ""
    m = abs(v)
    if m >= 1 << 200:
        if (m + 1) & m == 0:
            return f"{sign}M{m.bit_length()}"
        k = (m & -m).bit_length() - 1
        if k >= 200 and (m >> k).bit_length() <= 256:
            return f"{sign}{m >> k:x}P{k}"
    return sign + format(m, "x")


def checked(v):
    return "TooLarge" if abs(v).bit_length() > MAX_BITS else v


def f64_bits(x):
    return format(struct.unpack("<Q", struct.pack("<d", x))[0], "016x")


def bits_f64(h):
    return struct.unpack("<d", struct.pack("<Q", int(h, 16)))[0]


def esc(text):
    out = []
    for ch in text:
        cp = ord(ch)
        units = [cp] if cp < 0x10000 else [0xD800 + ((cp - 0x10000) >> 10), 0xDC00 + ((cp - 0x10000) & 0x3FF)]
        for u in units:
            if 0x20 < u < 0x7F and u != 0x5C:
                out.append(chr(u))
            else:
                out.append("\\u%04x" % u)
    return "".join(out)


# --- values -------------------------------------------------------------

SPECIAL_LIMBS = [0, 1, 2, LIMB - 1, LIMB - 2, 1 << 63, (1 << 63) - 1, (1 << 63) + 1, 0x5555555555555555]


def random_magnitude(limbs):
    """A magnitude of exactly `limbs` limbs, with patterns that stress
    carries and borrows."""
    if limbs == 0:
        return 0
    style = rng.random()
    out = 0
    for i in range(limbs):
        if style < 0.5:
            limb = rng.getrandbits(64)
        elif style < 0.8:
            limb = rng.choice(SPECIAL_LIMBS) if rng.random() < 0.5 else rng.getrandbits(64)
        else:
            limb = rng.choice(SPECIAL_LIMBS)
        out |= limb << (64 * i)
    top_bits = rng.choice([1, 2, 63, 64, rng.randint(1, 64)])
    top = out >> (64 * (limbs - 1))
    top &= (1 << top_bits) - 1
    top |= 1 << (top_bits - 1)
    return (out & ((1 << (64 * (limbs - 1))) - 1)) | (top << (64 * (limbs - 1)))


def random_size():
    r = rng.random()
    if r < 0.45:
        return rng.randint(0, 4)
    if r < 0.85:
        return rng.randint(5, 16)
    return rng.randint(17, 40)


def random_value(limbs=None):
    m = random_magnitude(random_size() if limbs is None else limbs)
    return -m if rng.random() < 0.5 else m


EDGE = sorted(
    {
        s * v
        for v in [
            0, 1, 2, 3, 7, 10, (1 << 63) - 1, 1 << 63, (1 << 63) + 1, LIMB - 1, LIMB, LIMB + 1,
            (1 << 128) - 1, 1 << 128, (1 << 128) + 1, 1 << 127, (1 << 192) - 1, 1 << 191,
            (1 << 53), (1 << 53) + 1, 10 ** 19, 10 ** 38, 3 ** 40,
        ]
        for s in (1, -1)
    }
)


# --- the operations ----------------------------------------------------

def trunc_div(a, b):
    q = abs(a) // abs(b)
    if (a < 0) != (b < 0):
        q = -q
    return q, a - q * b


def as_int_n(bits, a):
    if bits == 0:
        return 0
    if a.bit_length() < bits:
        return a
    m = a % (1 << bits)
    return m - (1 << bits) if m >= 1 << (bits - 1) else m


def to_f64(a):
    try:
        return float(a)
    except OverflowError:
        return math.inf if a > 0 else -math.inf


DIGITS = "0123456789abcdefghijklmnopqrstuvwxyz"


def to_string(a, radix):
    if a == 0:
        return "0"
    m = abs(a)
    out = []
    while m:
        m, d = divmod(m, radix)
        out.append(DIGITS[d])
    return ("-" if a < 0 else "") + "".join(reversed(out))


WHITE = [0x9, 0xB, 0xC, 0x20, 0xA0, 0xFEFF, 0x1680] + list(range(0x2000, 0x200B)) + [0x202F, 0x205F, 0x3000, 0xA, 0xD, 0x2028, 0x2029]
WHITE_CHARS = "".join(chr(c) for c in WHITE)


def string_to_bigint(text):
    """StringToBigInt (section 7.1.14): the value, or InvalidSyntax."""
    t = text.strip(WHITE_CHARS) if text else text
    # str.strip with an explicit set removes exactly these code points.
    if t == "":
        return 0
    if len(t) >= 2 and t[0] == "0" and t[1] in "xXoObB":
        radix = {"x": 16, "o": 8, "b": 2}[t[1].lower()]
        digits = t[2:]
        sign = 1
    else:
        radix = 10
        sign = -1 if t[0] == "-" else 1
        digits = t[1:] if t[0] in "+-" else t
    # ASCII first: str.lower maps some other letters (U+212A KELVIN SIGN)
    # to ASCII ones.
    if digits == "" or not digits.isascii() or any(c.lower() not in DIGITS[:radix] for c in digits):
        return "InvalidSyntax"
    return checked(sign * int(digits, radix))


def cmp_order(a, b):
    if isinstance(b, float) and math.isnan(b):
        return "nan"
    return "lt" if a < b else "gt" if a > b else "eq"


# --- Algorithm D, simulated to find the add-back cases -----------------

def limbs_of(m, n=None):
    out = []
    while m:
        out.append(m & (LIMB - 1))
        m >>= 64
    if n is not None:
        out += [0] * (n - len(out))
    return out


def add_backs(u, v):
    """The number of D6 steps of js-text's Algorithm D for |u| / |v|
    (v of at least two limbs, |u| >= |v|): the same normalization and
    the same quotient estimate with its two-limb test."""
    u, v = abs(u), abs(v)
    vl = limbs_of(v)
    n = len(vl)
    if n < 2 or u < v:
        return 0
    s = 63 - (vl[-1].bit_length() - 1)
    vn = limbs_of(v << s, n)
    un = limbs_of(u << s, len(limbs_of(u)) + 1)
    m = len(un) - 1 - n
    count = 0
    v1, v2 = vn[n - 1], vn[n - 2]
    for j in range(m, -1, -1):
        u0, u1, u2 = un[j + n], un[j + n - 1], un[j + n - 2]
        if u0 >= v1:
            q = LIMB - 1
            r = u1 + v1
        else:
            q, r = divmod(u0 * LIMB + u1, v1)
        while r < LIMB and q * v2 > r * LIMB + u2:
            q -= 1
            r += v1
        window = sum(un[j + i] << (64 * i) for i in range(n + 1))
        diff = window - q * (v << s)
        if diff < 0:
            count += 1
            diff += v << s
        for i, limb in enumerate(limbs_of(diff, n + 1)):
            un[j + i] = limb
    return count


def find_add_back_cases(wanted):
    cases = []
    # Hacker's Delight's case for 16-bit digits, with 64-bit limbs, and
    # variations of its pattern: a divisor whose second limb is 0, a
    # window whose top two limbs give an exact q-hat.
    half = 1 << 63
    seeds = [(half * LIMB ** 2 + (half - 1) * LIMB ** 3, 1 + half * LIMB ** 2)]
    tries = 0
    while len(cases) < wanted and tries < 200000:
        tries += 1
        if seeds:
            u, v = seeds.pop()
        else:
            n = rng.randint(2, 8)
            extra = rng.randint(0, 4)
            top = rng.choice([half, half + 1, LIMB - 1, rng.getrandbits(64) | half])
            low = rng.getrandbits(64 * (n - 2)) or 1
            v = (top << (64 * (n - 1))) | (rng.choice([0, 0, 1, rng.getrandbits(64)]) << (64 * (n - 2))) | low
            # A numerator whose windows start with v's top limbs.
            u = 0
            for i in range(n + extra):
                u |= rng.choice([0, LIMB - 1, top, top - 1, rng.getrandbits(64)]) << (64 * i)
            u |= rng.choice([top - 1, top]) << (64 * (n + extra))
            shift = rng.choice([0, 0, 1, 7, 63])
            u >>= shift
            v >>= shift
            if v.bit_length() <= 64 or u < v:
                continue
        if add_backs(u, v) > 0:
            sign_u = rng.choice([1, -1])
            sign_v = rng.choice([1, -1])
            cases.append((sign_u * u, sign_v * v))
    return cases


# --- the table ---------------------------------------------------------

def main(path):
    lines = []

    def emit(*fields):
        lines.append("\t".join(str(enc(f)) if not isinstance(f, str) else f for f in fields))

    def pairs(count):
        for _ in range(count):
            yield random_value(), random_value()
        for a in EDGE:
            for b in rng.sample(EDGE, 6):
                yield a, b

    # Addition, subtraction, multiplication, comparison.
    for a, b in pairs(110):
        emit("add", a, b, checked(a + b))
        emit("sub", a, b, checked(a - b))
        emit("cmp", a, b, cmp_order(a, b))
    for a, b in pairs(110):
        emit("mul", a, b, checked(a * b))
    for size in (100, 300):
        a, b = random_value(size), random_value(size)
        emit("add", a, b, a + b)
        emit("mul", a, b, a * b)

    # Division: random, one-limb divisors, divisors with a top limb of 1
    # or all ones, quotients of one limb, and the add-back cases.
    def divrem(a, b, op="divrem"):
        if b == 0:
            emit(op, a, b, "DivisionByZero", "DivisionByZero")
        else:
            q, r = trunc_div(a, b)
            emit(op, a, b, q, r)

    for a, b in pairs(110):
        divrem(a, b)
    for _ in range(120):
        a = random_value()
        b = random_value(rng.randint(1, 1))
        divrem(a, b)
    for top in (1, LIMB - 1, 1 << 63, 3):
        for _ in range(25):
            n = rng.randint(2, 12)
            b = (top << (64 * (n - 1))) | rng.getrandbits(64 * (n - 1))
            a = random_magnitude(n + rng.randint(0, 20))
            if rng.random() < 0.3:
                a = b * rng.getrandbits(64) + rng.randint(0, b - 1)
            divrem(a * rng.choice([1, -1]), b * rng.choice([1, -1]))
    for a, b in find_add_back_cases(40):
        divrem(a, b, "divab")
    a, b = random_value(300), random_value(120)
    divrem(a, b)

    # Exponentiation.
    for b in [0, 1, -1, 2, -2, 3, -3, 10, -10, 255, LIMB - 1, LIMB, LIMB + 1, (1 << 127) + 5, -(3 ** 30)]:
        for e in [0, 1, 2, 3, 5, 10, 31, 64, 65, 100, 333]:
            r = b ** e
            emit("pow", b, e, checked(r))
    for _ in range(60):
        b = random_value(rng.randint(0, 3))
        e = rng.randint(0, 40)
        emit("pow", b, e, checked(b ** e))
    for b, e in [(2, MAX_BITS - 1), (-2, MAX_BITS - 1), (2, MAX_BITS), (4, MAX_BITS // 2), (4, MAX_BITS // 2 - 1),
                 (-1 << 64, 4095), (-1 << 64, 4096), (3, 10 ** 12), (2, LIMB), (1, LIMB + 5), (-1, LIMB + 5),
                 (0, LIMB), (-1, LIMB), (5, -1), (0, -1), (1, -(LIMB ** 2))]:
        if e < 0:
            emit("pow", b, e, "NegativeExponent")
        elif abs(b) <= 1:
            emit("pow", b, e, 1 if e == 0 else b if e % 2 == 1 else b * b)
        elif e > MAX_BITS:
            emit("pow", b, e, "TooLarge")
        else:
            emit("pow", b, e, checked(b ** e))
    # 3^e at the limit: the largest exponent within it and the next one.
    e = int(MAX_BITS / math.log2(3))
    while (3 ** e).bit_length() > MAX_BITS:
        e -= 1
    while (3 ** (e + 1)).bit_length() <= MAX_BITS:
        e += 1
    emit("pow", 3, e + 1, "TooLarge")
    # The value itself is too long for the table (bigint_reference.rs
    # checks its length).

    # Shifts.
    for _ in range(150):
        a = random_value()
        k = rng.choice([0, 1, 2, 63, 64, 65, 127, 128, 129, rng.randint(0, 3000)])
        k = -k if rng.random() < 0.3 else k
        emit("shl", a, k, checked(a << k if k >= 0 else a >> -k))
        emit("shr", a, k, checked(a >> k if k >= 0 else a << -k))
    for a in [1, -1, 3, -3, LIMB, -LIMB, -(LIMB + 1)]:
        for k in [LIMB, LIMB ** 3, MAX_BITS, MAX_BITS - 1, MAX_BITS - a.bit_length(), MAX_BITS - a.bit_length() + 1]:
            emit("shl", a, k, "TooLarge" if k > MAX_BITS else checked(a << k))
            emit("shr", a, k, a >> k if k < 4 * LIMB else (-1 if a < 0 else 0))
            emit("shr", a, -k, "TooLarge" if k > MAX_BITS else checked(a << k))
            emit("shl", a, -k, a >> k if k < 4 * LIMB else (-1 if a < 0 else 0))
    emit("shl", 0, LIMB ** 2, 0)
    emit("shr", 0, -(LIMB ** 2), 0)

    # Bitwise operators.
    for a, b in pairs(100):
        emit("and", a, b, a & b)
        emit("or", a, b, a | b)
        emit("xor", a, b, a ^ b)
    for a in EDGE + [random_value() for _ in range(60)]:
        emit("not", a, checked(~a))

    # asIntN and asUintN.
    for bits in [0, 1, 2, 7, 8, 63, 64, 65, 127, 128, 129, 200, 1000, 2 ** 53 - 1]:
        for a in rng.sample(EDGE, 12) + [random_value() for _ in range(8)]:
            emit("asint", str(bits), a, as_int_n(bits, a))
            if a >= 0 and a.bit_length() <= bits:
                u = a
            elif bits > MAX_BITS + 64:
                u = "TooLarge"
            else:
                u = checked(a % (1 << bits))
            emit("asuint", str(bits), a, u)
    for bits in [MAX_BITS, MAX_BITS + 1]:
        emit("asuint", str(bits), -1, checked((1 << bits) - 1))
        emit("asint", str(bits), -1, -1)
    emit("asuint", str(1000), -(LIMB + 3), (-(LIMB + 3)) % (1 << 1000))

    # Conversion to a double.
    def tof64(a):
        emit("tof64", a, f64_bits(to_f64(a)))

    for k in [53, 54, 55, 63, 64, 65, 100, 1000, 1023]:
        p = 1 << k
        for d in [-3, -2, -1, 0, 1, 2, 3]:
            tof64(p + d)
            tof64(-(p + d))
        ulp = 1 << (k - 53)
        for mult in [1, 3, 5]:
            tof64(p + mult * ulp // 2)  # ties to even, above 2^53
            tof64(p + mult * ulp // 2 + 1)
            tof64(p + mult * ulp // 2 - 1)
    top = (1 << 1024) - (1 << 971)  # the largest finite double
    for d in [0, 1, -1, (1 << 970) - 1, 1 << 970, (1 << 970) + 1, 1 << 971]:
        tof64(top + d)
        tof64(-(top + d))
    tof64(1 << 1024)
    tof64((1 << 2000) + 12345)
    for _ in range(120):
        tof64(random_value())

    # Conversion from a double.
    floats = [0.0, -0.0, 1.0, -1.0, 0.5, -0.5, 1.5, 2.0 ** 53, 2.0 ** 53 + 2, 2.0 ** 64, -(2.0 ** 64), 1e21, 1e300,
              1.7976931348623157e308, -1.7976931348623157e308, 5e-324, math.inf, -math.inf, math.nan,
              123456789.0, 4503599627370495.5, 2.0 ** 52 + 1]
    for _ in range(60):
        floats.append(float(rng.randint(-(1 << 70), 1 << 70)) * 2.0 ** rng.randint(-30, 900))
    for f in floats:
        r = int(f) if math.isfinite(f) and f.is_integer() else "NotIntegral"
        emit("fromf64", f64_bits(f), r)

    # Comparison with doubles.
    for f in floats:
        if not math.isfinite(f):
            for a in [0, 5, -5, 1 << 2000, -(1 << 2000)]:
                emit("cmpf", a, f64_bits(f), cmp_order(a, f))
            continue
        near = int(f) if f.is_integer() else math.floor(f)
        for a in [near - 1, near, near + 1, -near, 0, (1 << 1100)]:
            emit("cmpf", a, f64_bits(f), cmp_order(a, f))
    for _ in range(100):
        a = random_value()
        f = to_f64(a)
        if math.isinf(f):
            f = 1.7976931348623157e308
        emit("cmpf", a, f64_bits(f), cmp_order(a, f))

    # toString in every radix.
    for radix in range(2, 37):
        for a in rng.sample(EDGE, 10) + [random_value() for _ in range(10)]:
            emit("tostr", str(radix), a, to_string(a, radix))
        a = random_value(60)
        emit("tostr", str(radix), a, to_string(a, radix))

    # StringToBigInt and the literals: every radix of the grammar.
    texts = ["", " ", "0", "-0", "+0", "00", "007", "-007", "0x", "0X", "0o", "0b", "0x0", "0xF", "0XfF",
             "0o17", "0O17", "0b101", "0B101", "-0x1", "+0x1", "0x-1", "1_000", "0x_1", "1n", "1.5", "1.",
             ".5", "1e3", "+", "-", "--1", "+-1", "0b2", "0o8", "0xg", "Infinity", "NaN", "1 2", "0x 1",
             "᠎1", "１", "١", "- 1", "12345678901234567890123456789",
             "-98765432109876543210987654321098765432109876543210"]
    for c in WHITE:
        texts.append(chr(c) + "42" + chr(c))
    texts.append(WHITE_CHARS + "-17" + WHITE_CHARS)
    texts.append(WHITE_CHARS)
    for _ in range(80):
        radix = rng.choice([2, 8, 10, 16])
        a = abs(random_value())
        digits = to_string(a, radix)
        if radix == 16 and rng.random() < 0.5:
            digits = "".join(c.upper() if rng.random() < 0.5 else c for c in digits)
        prefix = {2: "0b", 8: "0o", 10: "", 16: "0x"}[radix]
        if rng.random() < 0.3:
            prefix = prefix.upper()
        lead = "0" * rng.choice([0, 0, 1, 5])
        sign = rng.choice(["", "", "-", "+"]) if radix == 10 else ""
        space = "".join(rng.choice(WHITE_CHARS) for _ in range(rng.choice([0, 0, 1, 3])))
        texts.append(space + sign + prefix + lead + digits + space)
    # Many leading zeros (the texts at the limit are in
    # bigint_reference.rs).
    texts.append("0x" + "0" * 5000 + "1")
    texts.append("-" + "0" * 3000 + "12")
    for text in texts:
        emit("parse", esc(text), string_to_bigint(text))
        valid_literal = text != "" and text == text.strip(WHITE_CHARS) and text[0] not in "+-"
        if valid_literal:
            r = string_to_bigint(text)
            emit("lit", esc(text), r)
        elif text == "":
            emit("lit", "", "InvalidSyntax")

    with open(path, "w", encoding="ascii", newline="\n") as out:
        for line in lines:
            out.write(line + "\n")
    print(f"{len(lines)} cases", file=sys.stderr)


if __name__ == "__main__":
    main(sys.argv[1])
