// Helpers of the cases of media-hostile.json (M14 feature 10a): the events of
// a media element with its states, the state, a load-then-play step.
const EVN = ['loadstart', 'progress', 'suspend', 'abort', 'error', 'emptied', 'stalled',
  'loadedmetadata', 'loadeddata', 'canplay', 'canplaythrough', 'playing', 'waiting', 'seeking',
  'seeked', 'ended', 'durationchange', 'timeupdate', 'play', 'pause', 'resize'];
const EVO = [];
const T0 = performance.now();
const r2 = x => Math.round(x * 100) / 100;
// Repeated events of one kind in a row are one entry with a `*`.
function watch(m) {
  EVN.forEach(n => m.addEventListener(n, () => {
    const l = EVO[EVO.length - 1];
    if (l && l.n === n && ['timeupdate', 'progress', 'suspend', 'stalled'].includes(n)) {
      l.k++;
      return;
    }
    EVO.push({n, k: 1, rs: m.readyState, ns: m.networkState,
      s: Math.round((performance.now() - T0) / 500) / 2});
  }));
}
const evs = () => EVO.map(o => o.n + (o.k > 1 ? '*' : '') + ' rs=' + o.rs + ' ns=' + o.ns);
// The events with their time since the page started, in half seconds.
const evt = () => EVO.map(o => o.n + (o.k > 1 ? '*' : '') + ' @' + o.s);
const S = m => ({
  rs: m.readyState, ns: m.networkState, paused: m.paused, ended: m.ended,
  ct: r2(m.currentTime), dur: m.duration,
  size: m.videoWidth === undefined ? 'n/a' : m.videoWidth + 'x' + m.videoHeight,
  err: m.error ? m.error.code + ': ' + m.error.message : null,
});
const sleep = ms => new Promise(r => setTimeout(r, ms));
const once = (m, n, ms) => new Promise(r => {
  const t = setTimeout(() => r('timeout'), ms);
  m.addEventListener(n, () => { clearTimeout(t); r(n); }, {once: true});
});
const log = () => fetch('__log').then(r => r.json())
  .then(j => j.filter(e => !e.path.endsWith('.js'))
    .map(e => e.path.split('/').pop() + ' ' + (e.headers.range || '-') + ' -> ' + e.status));
// Waits `wait` ms, plays (muted), waits for `ended` or `error` (at most
// `end` ms); the events and the state before and after the play. A `play()`
// promise that does not settle within `end` ms is `pending`.
async function loadPlay(m, wait, end) {
  await sleep(wait);
  const pre = {ev: evs(), s: S(m)};
  const n = EVO.length;
  m.muted = true;
  const play = await Promise.race([
    m.play().then(() => 'resolved', e => e.name + ': ' + e.message),
    sleep(end).then(() => 'pending'),
  ]);
  await Promise.race([once(m, 'ended', end), once(m, 'error', end)]);
  await sleep(200);
  return {pre, play, after: evs().slice(n), s: S(m)};
}
// A video made by the first query, after the load of the page (a media
// element that stalls delays the page's load, which the probe tool waits
// for), with `preload` and `src`.
function lateVideo(src, preload) {
  const m = document.createElement('video');
  m.preload = preload;
  watch(m);
  m.src = src;
  document.body.append(m);
  return m;
}
