(()=>{
// The page checks of wikipedia_web_browser.rs. It installs listeners for
// errors, unhandled rejections, `hashchange` and `popstate`, and defines
// window.__s(), which returns the JSON text of the page state that the test
// compares with Chromium's values: only class names, booleans, numbers and
// short texts (rectangles as "x,y,width,height" in whole px).
// Left out on purpose (feature 12c): the pixels and everything that `filter`
// and gradients change.
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
 root:document.documentElement.className.split(' ').sort().join(' '),
 veHidden:q('body > a.oo-ui-element-hidden').map(e=>e.getAttribute('accesskey')+' '+e.className+' '+e.getAttribute('href')),
 popupsSvg:!!document.getElementById('mwe-popups-svg'),
 headCss:[...document.head.querySelectorAll('style')].reduce((n,e)=>n+e.textContent.length,0),
 pvClip:q('.mwe-popups-container').map(e=>getComputedStyle(e).clipPath),
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
 pvr:q('.mwe-popups').flatMap(p=>['.mwe-popups-container','.mwe-popups-discreet','.mwe-popups-thumbnail','.mwe-popups-extract','.mwe-popups-extract p','footer','.mwe-popups-settings-button','.mwe-popups-extract b'].map(s=>s+' '+R(p.querySelector(s)))),
 pvt:q('.mwe-popups').map(p=>{const i=p.querySelector('img');return [p.getAttribute('aria-hidden'),p.querySelector('.mwe-popups-extract').innerText,p.querySelector('.mwe-popups-settings-button').title,i?[i.complete,i.naturalWidth,i.naturalHeight,i.currentSrc.replace(/^.*\//,'')].join(' '):'no image'].join(' | ')}),
 tar:(()=>{const t=document.querySelector('.cdx-typeahead-search');if(!t)return null;const m=t.querySelector('.cdx-menu');return [t,t.querySelector('input.cdx-text-input__input'),m,...(m?m.querySelectorAll('li, li .cdx-thumbnail, li .cdx-menu-item__text__label, li .cdx-menu-item__text__description, .cdx-typeahead-search__search-footer__text'):[])].filter(Boolean).map(e=>e.localName+'.'+e.className.split(' ')[0]+' '+R(e))})(),
 tat:(()=>{const t=document.querySelector('.cdx-typeahead-search');if(!t)return null;const i=t.querySelector('input.cdx-text-input__input');const m=t.querySelector('.cdx-menu');
  const items=m?[...m.querySelectorAll('li')].map(l=>[l.id,l.classList.contains('cdx-menu-item--highlighted')?'H':'-',l.getAttribute('aria-selected'),l.title,(l.querySelector('.cdx-menu-item__text__label')||{}).textContent,(l.querySelector('.cdx-menu-item__text__description')||{}).textContent,l.querySelector('.cdx-thumbnail__placeholder')?'placeholder':l.querySelector('.cdx-thumbnail__image')?'image '+getComputedStyle(l.querySelector('.cdx-thumbnail__image')).backgroundImage.replace(/^.*\/|["')]+$/g,''):'none',l.querySelector('a').getAttribute('href')].join(' | ')):null;
  return {cls:t.className.split(' ').sort().join(' '),input:[...i.attributes].filter(a=>/^(aria-|role|class$|value$)/.test(a.name)).map(a=>a.name+'='+a.value).sort().join(' ')+' value='+i.value+' sel='+i.selectionStart+','+i.selectionEnd,
   menu:m?m.className+' '+getComputedStyle(m).display+' '+(m.querySelector('ul')||{}).id+' '+[...m.querySelector('ul').attributes].map(a=>a.name+'='+a.value).sort().join(' '):null,items}})(),
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
