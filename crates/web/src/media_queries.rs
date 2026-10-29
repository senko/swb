//! `matchMedia` and `MediaQueryList` (CSSOM View §4.2,
//! <https://drafts.csswg.org/cssom-view/#the-mediaquerylist-interface>;
//! M8 feature 7b2), and the "evaluate media queries and report changes"
//! step of the rendering update
//! (<https://drafts.csswg.org/cssom-view/#evaluate-media-queries-and-report-changes>).
//!
//! A `MediaQueryList` is an event target whose generic data
//! ([`crate::events::TargetData`]) holds its listeners and its
//! [`MediaQueryState`]: the parsed list (the media query parser and
//! evaluator of `swb_css`), its serialization and the matches state of the
//! last evaluation. `matches` evaluates the list against the document's
//! media environment at each read (Chromium: `matches` is current in a
//! `resize` listener, before the change events of the frame).
//!
//! Only the lists that got a listener (`addListener`, `addEventListener`
//! or `onchange`) are evaluated in the rendering update: the others cannot
//! see an event, so a page that calls `matchMedia` in a loop keeps no list
//! alive. They are kept in the order of creation, which is the order of
//! the change events (measured: the order of `matchMedia` calls, not of
//! the listeners).

use swb_js::{Gc, Generic, Object, Runtime, Tracer, VmResult};

use crate::bindings::{self as b, Cx, Platform};
use crate::events::{EventData, Extra, TargetData};
use crate::generated as g;
use crate::generated::types::{
    AddEventListenerOptionsOrBoolean, EventInit, EventListener, EventListenerOptionsOrBoolean,
};
use crate::state::{WebState, engine, state, state_mut};

/// The state of a `MediaQueryList`.
#[derive(Debug)]
pub(crate) struct MediaQueryState {
    /// The serialized media query list (`media`).
    media: String,
    list: swb_css::MediaQueryList,
    /// The order of creation in the document.
    sequence: u64,
    /// The matches state of the last evaluation (at creation, then in the
    /// rendering update).
    matches: bool,
    /// The object, once it is in the document's list of evaluated lists.
    registered: Option<Gc<Object>>,
}

impl MediaQueryState {
    pub(crate) fn trace(&self, tracer: &mut Tracer<'_>) {
        if let Some(object) = self.registered {
            tracer.object(object);
        }
    }

    pub(crate) fn heap_size(&self) -> usize {
        size_of::<MediaQueryState>() + self.media.capacity()
    }
}

/// `matchMedia(query)`: a new `MediaQueryList`.
pub(crate) fn match_media(cx: &mut Cx<'_>, query: &str) -> b::Result<Gc<Object>> {
    let list = swb_css::MediaQueryList::parse_str(query);
    let matches = list.matches(&engine(cx.rt)?.media_environment());
    let state = state_mut(cx.rt)?;
    state.media_query_count += 1;
    let media = MediaQueryState {
        media: list.to_string(),
        list,
        sequence: state.media_query_count,
        matches,
        registered: None,
    };
    let mut data = TargetData::default();
    data.media = Some(Box::new(media));
    let data = cx.rt.alloc_host_data(data)?;
    let realm = cx.realm();
    Ok(cx.rt.new_host_object(realm, g::MEDIA_QUERY_LIST, data)?)
}

/// The state of the `MediaQueryList` `this`.
fn this_state<'a>(cx: &'a Cx<'_>, this: Platform) -> b::Result<&'a MediaQueryState> {
    let handle = crate::events::event_handle(this.data)?;
    cx.rt
        .heap()
        .generic::<TargetData>(handle)
        .map_err(swb_js::VmError::from)?
        .media
        .as_deref()
        .ok_or_else(|| swb_js::VmError::type_error("Illegal invocation").into())
}

impl g::media_query_list::MediaQueryListImpl for WebState {
    fn media(cx: &mut Cx<'_>, this: Platform) -> b::Result<String> {
        Ok(this_state(cx, this)?.media.clone())
    }

    fn matches(cx: &mut Cx<'_>, this: Platform) -> b::Result<bool> {
        let env = engine(cx.rt)?.media_environment();
        Ok(this_state(cx, this)?.list.matches(&env))
    }

    /// `addListener(callback)`: `addEventListener('change', callback)`.
    fn add_listener(
        cx: &mut Cx<'_>,
        this: Platform,
        callback: Option<EventListener>,
    ) -> b::Result<()> {
        <WebState as g::event_target::EventTargetImpl>::add_event_listener(
            cx,
            this,
            "change".to_owned(),
            callback,
            AddEventListenerOptionsOrBoolean::Boolean(false),
        )
    }

    /// `removeListener(callback)`: `removeEventListener('change', callback)`.
    fn remove_listener(
        cx: &mut Cx<'_>,
        this: Platform,
        callback: Option<EventListener>,
    ) -> b::Result<()> {
        <WebState as g::event_target::EventTargetImpl>::remove_event_listener(
            cx,
            this,
            "change".to_owned(),
            callback,
            EventListenerOptionsOrBoolean::Boolean(false),
        )
    }
}

/// Puts the event target `object`, if it is a `MediaQueryList`, in the
/// document's list of evaluated lists when it gets its first listener.
pub(crate) fn register(rt: &mut Runtime, object: Gc<Object>) -> VmResult<()> {
    let Some((_, swb_js::HostData::Generic(handle))) = rt.host_object(object.into()) else {
        return Ok(());
    };
    let Ok(data) = rt.heap_mut().generic_mut::<TargetData>(handle) else {
        return Ok(());
    };
    let Some(media) = data.media.as_deref_mut() else {
        return Ok(());
    };
    if media.registered.is_some() {
        return Ok(());
    }
    media.registered = Some(object);
    let sequence = media.sequence;
    state_mut(rt)?.media_queries.insert(sequence, handle);
    Ok(())
}

/// True if a `MediaQueryList` of the document has a listener.
pub(crate) fn any_registered(rt: &Runtime) -> bool {
    state(rt).is_ok_and(|s| !s.media_queries.is_empty())
}

/// CSSOM View "evaluate media queries and report changes": fires `change`
/// at each list with listeners whose result changed since the last
/// evaluation, in the order of creation (a part of a task).
pub(crate) fn report_changes(rt: &mut Runtime) -> VmResult<()> {
    let env = engine(rt)?.media_environment();
    let lists: Vec<Gc<Generic>> = state(rt)?.media_queries.values().copied().collect();
    for handle in lists {
        let changed = {
            let Some(media) = rt
                .heap_mut()
                .generic_mut::<TargetData>(handle)?
                .media
                .as_deref_mut()
            else {
                continue;
            };
            let matches = media.list.matches(&env);
            if matches == media.matches {
                continue;
            }
            media.matches = matches;
            media
                .registered
                .map(|object| (object, media.media.clone(), matches))
        };
        let Some((target, media, matches)) = changed else {
            continue;
        };
        let init = EventInit {
            bubbles: false,
            cancelable: false,
            composed: false,
        };
        let now = crate::events::now(rt);
        let mut data = EventData::new(
            "change".to_owned(),
            &init,
            now,
            Extra::MediaQuery { media, matches },
        );
        data.is_trusted = true;
        let event = crate::events::new_event_object(rt, 0, g::MEDIA_QUERY_LIST_EVENT, data)?;
        crate::events::dispatch(rt, 0, target, event, None)?;
    }
    Ok(())
}
