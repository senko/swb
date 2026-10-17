// The cases of bench.rs in Node.js: `node --jitless bench.js` uses V8's
// regular expression interpreter (no native code), the fair comparison
// for swb's matcher. Keep in step with bench.rs.

const words = 'The quick brown fox jumps over the lazy dog, again and again. '.repeat(2000);
const query = 'a=1&bb=22&ccc=333&dddd=4444&'.repeat(2000);
const html = '<div class="story"><a href="/news/123">Headline</a></div>\n'.repeat(2000);
const spaces = ' '.repeat(1000) + 'x'.repeat(100000) + ' '.repeat(1000);
const EMOJI = ('Text \u{1F600} and \u{1F44D}\u{1F3FD} and \u{1F1ED}\u{1F1F7} and ' +
  '\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467} 1\u{FE0F}\u{20E3} end. ').repeat(2000);

const cases = [
  ['trim', '^\\s+|\\s+$', 'g', spaces, 20],
  ['query', '([^&=]+)=([^&]*)', 'g', query, 20],
  ['word', '\\b(?:fox|dog)\\b', 'gi', words, 20],
  ['class', '[A-Z]', 'g', words, 20],
  ['tag', '<a href="([^"]*)">(.*?)</a>', 'g', html, 20],
  ['literal', 'Headline', 'g', html, 20],
  ['alternation', '(?:quick|lazy|again)\\s(\\w+)', 'g', words, 20],
  ['dot-star', 'o.*z', 'g', words, 20],
  ['nested', '(a+)+b', '', 'a'.repeat(22), 1],
  ['redos', '^(\\w+\\s?)*$', '', 'aaaa '.repeat(4) + '!', 50],
  ['backref', '(\\w+)\\s\\1', 'g', 'hello hello world world '.repeat(5000), 20],
  ['lookbehind', '(?<=\\$)\\d+', 'g', 'cost $12, then $345, and 678 '.repeat(5000), 20],
  ['emoji', '\\p{RGI_Emoji}', 'gv', EMOJI, 5],
  ['emoji-iv', '\\p{RGI_Emoji}', 'giv', EMOJI, 5],
];

console.log('case'.padEnd(12), 'matches'.padStart(10), 'ms'.padStart(10));
for (const [name, pattern, flags, input, runs] of cases) {
  const re = new RegExp(pattern, flags);
  const global = flags.includes('g');
  let matches = 0;
  const start = process.hrtime.bigint();
  for (let r = 0; r < runs; r++) {
    matches = 0;
    re.lastIndex = 0;
    for (;;) {
      const m = re.exec(input);
      if (m === null) break;
      matches++;
      if (!global) break;
      if (m[0].length === 0) re.lastIndex++;
    }
  }
  const ms = Number(process.hrtime.bigint() - start) / 1e6 / runs;
  console.log(name.padEnd(12), String(matches).padStart(10), ms.toFixed(3).padStart(10));
}
