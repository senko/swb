(()=>{
// The page checks of wikipedia_web_browser.rs. It installs listeners for
// errors, unhandled rejections, `hashchange` and `popstate`, and defines
// window.__s(), which returns the JSON text of the page state that the test
// compares with Chromium's values: only class names, booleans, numbers and
// short texts (rectangles as "x,y,width,height" in whole px).
// Left out on purpose (features 12b and 12c): the VisualEditor root classes
// (`ve-*`), the elements that VisualEditor and Popups add, the pixels and
// everything that `filter` and gradients change.
window.__e=[];addEventListener('error',e=>__e.push('error '+e.message));
addEventListener('unhandledrejection',e=>__e.push('unhandled '+(e.reason&&(e.reason.name||e.reason.constructor.name))+': '+(e.reason&&e.reason.message)));
window.__h=[];
addEventListener('hashchange',e=>__h.push('hashchange '+String(e.oldURL).split('#')[1]+'>'+String(e.newURL).split('#')[1]));
addEventListener('popstate',e=>__h.push('popstate '+JSON.stringify(e.state)));
const R=e=>{if(!e)return null;const b=e.getBoundingClientRect();return [b.x,b.y,b.width,b.height].map(Math.round).join(',')};
const ids=['vector-toc','vector-main-menu','vector-page-tools','vector-appearance'];
const q=(s,n)=>[...document.querySelectorAll(s)].slice(0,n);
window.__s=()=>{
 const sh=document.querySelector('.vector-sticky-header');
 // The rectangles below the references move when the references are in
 // two columns (`column-width: 30em`: multi-column layout is not
 // implemented in swb, backlog): record them only while the references fit
 // one column, as in the default layout.
 const refs=document.querySelector('.mw-references-wrap');
 const one=!refs||refs.getBoundingClientRect().width<2*(parseFloat(getComputedStyle(refs).columnWidth)||432);
 const body=document.querySelector('.mw-body');
 const b=body.getBoundingClientRect();
 return JSON.stringify({
 root:document.documentElement.className.split(' ').filter(c=>!/^ve-/.test(c)).sort().join(' '),
 cookie:document.cookie.replace(/sessionId=\w+/,'sessionId=X'),
 pin:ids.map(i=>{const e=document.getElementById(i);return e?i+' in '+e.parentNode.id+' '+R(e)+' '+getComputedStyle(e).display:i+' none'}),
 pinbtn:q('.vector-pinnable-header-toggle-button').map(e=>e.closest('.vector-pinnable-header').dataset.featureName+'/'+(e.classList.contains('vector-pinnable-header-pin-button')?'pin':'unpin')+' '+R(e)),
 dd:q('.vector-dropdown-checkbox').map(e=>e.id+' '+e.checked+' '+e.getAttribute('aria-expanded')),
 ddc:q('.vector-dropdown-content').map(e=>(e.parentNode.id||e.id)+' '+getComputedStyle(e).display+' '+R(e)),
 toc:q('.vector-toc-list-item').map(e=>e.classList.contains('vector-toc-list-item-expanded')?1:0).join(''),
 tocBtn:q('.vector-toc-toggle').map(e=>e.getAttribute('aria-expanded')),
 tocActive:q('.vector-toc-list-item-active').map(e=>e.id),
 tocSub:q('.vector-toc-list').map(e=>e.id+':'+getComputedStyle(e).display).filter(s=>/sublist/.test(s)),
 coll:q('.mw-collapsible',4).map(e=>e.className.split(' ').filter(c=>/collapsed|expanded/.test(c)).join('/')+' '+(one?R(e):'-')),
 toggles:q('.mw-collapsible-toggle',4).map(e=>e.className.replace(/mw-collapsible-toggle-?/g,'').trim()+'|'+e.getAttribute('aria-expanded')+'|'+e.textContent),
 sticky:sh?sh.className+' '+R(sh)+' '+getComputedStyle(sh).display:null,
 stickyVisible:document.documentElement.className.includes('vector-sticky-header-visible'),
 pv:q('.mwe-popups').map(e=>e.className+' '+R(e)+' '+e.innerText.slice(0,80).replace(/\s+/g,' ')),
 body:document.body.className.split(' ').sort().join(' '),
 spc:q('.vector-sticky-pinned-container').map(e=>e.id+' '+(one?R(e):'-')+' '+getComputedStyle(e).position),
 hash:location.hash,scroll:Math.round(scrollY),
 h:__h.slice(),
 ac:document.activeElement&&(document.activeElement.localName+'#'+document.activeElement.id),
 search:q('#searchInput,.cdx-typeahead-search').map(e=>e.localName+'#'+e.id+' '+e.className.split(' ').filter(c=>/typeahead|expand|active|focus/.test(c)).join('/')),
 fs:getComputedStyle(document.body).fontSize,
 bg:getComputedStyle(document.body).backgroundColor,
 fg:getComputedStyle(document.body).color,
 cw:document.documentElement.clientWidth,
 content:[b.x,b.y,b.width].map(Math.round).join(','),
 e:__e.slice()})};
return 1})()
