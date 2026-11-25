delete window.MediaSource; delete window.ManagedMediaSource; delete window.WebKitMediaSource;
window.__ev = [];
const t0 = performance.now();
for (const n of ['loadstart','progress','suspend','abort','error','emptied','stalled','loadedmetadata','loadeddata','canplay','canplaythrough','durationchange','resize','timeupdate','play','pause','waiting','playing','seeking','seeked','ended']) {
  document.addEventListener(n, (e) => {
    const m = e.target;
    if (!(m instanceof HTMLMediaElement)) return;
    window.__ev.push(Math.round(performance.now()-t0) + ' ' + n + ' ns=' + m.networkState + ' rs=' + m.readyState + ' err=' + (m.error ? m.error.code + ':' + m.error.message : '-') + ' src=' + (m.currentSrc||'').slice(-30));
  }, true);
}
