(()=>{
// The page checks of craigslist_posting.rs. It installs listeners for
// errors, unhandled rejections, print events and mouse events, and
// defines window.__s(), which returns the JSON text of everything that
// the test compares with Chromium's values.
window.__e=[];addEventListener('error',e=>__e.push('error '+e.message));
addEventListener('unhandledrejection',e=>__e.push('unhandled '+(e.reason&&(e.reason.name||e.reason.constructor.name))+': '+(e.reason&&e.reason.message)));
window.__p=[];addEventListener('beforeprint',()=>__p.push('bp'));addEventListener('afterprint',()=>__p.push('ap'));
window.__ev=[];for(const t of ['contextmenu','dblclick','mousedown','mouseup','click'])document.addEventListener(t,e=>__ev.push(t+' '+e.defaultPrevented+' '+e.clientX+','+e.clientY+' b'+e.button+' d'+e.detail));
const attrs=e=>e.localName+' '+[...e.attributes].map(a=>a.name+'='+a.value).sort().join(' ');
window.__s=()=>{const mp=document.querySelector('.leaflet-map-pane');return JSON.stringify({
 timeago:[...document.querySelectorAll('time.timeago')].map(t=>t.textContent+'|'+t.getAttribute('title')+'|'+t.getAttribute('datetime')),
 versions:[jQuery.fn.jquery,L.version],
 orientation:screen.orientation.type,
 media:['(max-width: 767px)','print','all','(resolution: 1dppx)','(touch-enabled)'].map(q=>matchMedia(q).matches),
 imageText:(document.querySelector('.slider-info')||{}).textContent,
 slides:[...document.querySelectorAll('.slide')].map(e=>e.getAttribute('style')),
 thumbs:[...document.querySelectorAll('a.thumb')].map(e=>e.className),
 mapPane:mp?mp.style.cssText:null,
 mapClass:(document.querySelector('.leaflet-container')||{}).className,
 tiles:document.querySelectorAll('.leaflet-tile').length,
 tileLoaded:document.querySelectorAll('.leaflet-tile-loaded').length,
 layers:[...document.querySelectorAll('.leaflet-tile-container')].map(e=>e.className+'|'+(e.getAttribute('style')||'')),
 panes:[...document.querySelectorAll('.leaflet-pane')].map(e=>e.className+'|'+(e.getAttribute('style')||'')),
 svg:[...document.querySelectorAll('.leaflet-pane svg, .leaflet-pane svg *')].map(attrs),
 srcs:[...document.querySelectorAll('.leaflet-tile')].map(e=>e.getAttribute('src')).sort(),
 ev:__ev.splice(0),
 flaps:document.querySelectorAll('.cl-reply-flap').length,
 reply:[...document.querySelectorAll('.actions-combo > *')].map(e=>e.className),
 e:__e,p:__p,
 menu:document.querySelectorAll('.share-menu li').length,
 body:document.body.className,
 copied:document.body.innerText.includes('URL copied'),
 msgs:[...document.querySelectorAll('div')].filter(e=>e.children.length==0&&/copied/.test(e.textContent)).map(e=>e.className+'|'+e.textContent)})};
return 1})()
