//! `HTMLElement` and the elements whose members the standard describes in
//! prose: the metadata and interaction attributes of `HTMLElement`,
//! `dataset`, `tabIndex`, `title.text`, `base.href`, `a.text`, the URL
//! getters of `formAction`, and the members of `img` that need no layout.
//!
//! <https://html.spec.whatwg.org/multipage/dom.html#htmlelement>

use swb_dom::{NodeId, ns, parse_integer};

use super::{attribute, is_html, remove_attribute, set_attribute};
use crate::bindings::{self as b, Cx};
use crate::generated as g;
use crate::generated::types::BooleanOrUnrestrictedDoubleOrDomString as Hidden;
use crate::state::{Slot, WebState, dom, engine_mut, same_object};

/// The value of an enumerated attribute that has the states "true",
/// "false" and a default, in ASCII lowercase.
fn lowercase_attribute(cx: &Cx<'_>, element: NodeId, name: &str) -> b::Result<Option<String>> {
    Ok(attribute(cx, element, name)?.map(|v| v.to_ascii_lowercase()))
}

/// The nearest value of the attribute `name` on `element` and its
/// ancestors that `state` accepts.
fn inherited(
    cx: &Cx<'_>,
    element: NodeId,
    name: &str,
    state: impl Fn(&str) -> Option<bool>,
) -> b::Result<Option<bool>> {
    let dom = dom(cx.rt)?;
    for node in std::iter::once(element).chain(dom.ancestors(element)) {
        if let Some(value) = dom.element(node).and_then(|e| e.attr(name))
            && let Some(result) = state(&value.to_ascii_lowercase())
        {
            return Ok(Some(result));
        }
    }
    Ok(None)
}

impl g::html_element::HTMLElementImpl for WebState {
    fn click(cx: &mut Cx<'_>, this: NodeId) -> b::Result<()> {
        super::activation::click(cx, this)
    }

    fn focus(
        cx: &mut Cx<'_>,
        this: NodeId,
        options: crate::generated::types::FocusOptions,
    ) -> b::Result<()> {
        super::focus::focus(cx, this, &options)
    }

    fn blur(cx: &mut Cx<'_>, this: NodeId) -> b::Result<()> {
        super::focus::blur(cx, this)
    }

    /// `translate`: the nearest `translate` attribute, else true.
    /// <https://html.spec.whatwg.org/multipage/dom.html#attr-translate>
    fn translate(cx: &mut Cx<'_>, this: NodeId) -> b::Result<bool> {
        let state = |v: &str| match v {
            "yes" | "" => Some(true),
            "no" => Some(false),
            _ => None,
        };
        Ok(inherited(cx, this, "translate", state)?.unwrap_or(true))
    }

    fn set_translate(cx: &mut Cx<'_>, this: NodeId, value: bool) -> b::Result<()> {
        set_attribute(cx, this, "translate", if value { "yes" } else { "no" })
    }

    /// `hidden`: false, true, or `"until-found"`.
    /// <https://html.spec.whatwg.org/multipage/interaction.html#dom-hidden>
    fn hidden(cx: &mut Cx<'_>, this: NodeId) -> b::Result<Option<Hidden>> {
        Ok(Some(match lowercase_attribute(cx, this, "hidden")? {
            None => Hidden::Boolean(false),
            Some(v) if v == "until-found" => Hidden::DomString("until-found".to_owned()),
            Some(_) => Hidden::Boolean(true),
        }))
    }

    fn set_hidden(cx: &mut Cx<'_>, this: NodeId, value: Option<Hidden>) -> b::Result<()> {
        let hidden = match value {
            None | Some(Hidden::Boolean(false)) => false,
            Some(Hidden::Boolean(true)) => true,
            Some(Hidden::UnrestrictedDouble(n)) => n != 0.0 && !n.is_nan(),
            Some(Hidden::DomString(s)) => {
                if s.eq_ignore_ascii_case("until-found") {
                    return set_attribute(cx, this, "hidden", "until-found");
                }
                !s.is_empty()
            }
        };
        if hidden {
            set_attribute(cx, this, "hidden", "")
        } else {
            remove_attribute(cx, this, "hidden")
        }
    }

    /// `draggable`: the attribute, else true for `img` and for `a` with an
    /// `href`.
    /// <https://html.spec.whatwg.org/multipage/dnd.html#the-draggable-attribute>
    fn draggable(cx: &mut Cx<'_>, this: NodeId) -> b::Result<bool> {
        Ok(
            match lowercase_attribute(cx, this, "draggable")?.as_deref() {
                Some("true") => true,
                Some("false") => false,
                _ => {
                    is_html(cx, this, "img")?
                        || (is_html(cx, this, "a")? && attribute(cx, this, "href")?.is_some())
                }
            },
        )
    }

    fn set_draggable(cx: &mut Cx<'_>, this: NodeId, value: bool) -> b::Result<()> {
        set_attribute(cx, this, "draggable", if value { "true" } else { "false" })
    }

    /// `spellcheck`: the nearest `spellcheck` attribute, else true.
    /// <https://html.spec.whatwg.org/multipage/interaction.html#attr-spellcheck>
    fn spellcheck(cx: &mut Cx<'_>, this: NodeId) -> b::Result<bool> {
        let state = |v: &str| match v {
            "true" | "" => Some(true),
            "false" => Some(false),
            _ => None,
        };
        Ok(inherited(cx, this, "spellcheck", state)?.unwrap_or(true))
    }

    fn set_spellcheck(cx: &mut Cx<'_>, this: NodeId, value: bool) -> b::Result<()> {
        set_attribute(cx, this, "spellcheck", if value { "true" } else { "false" })
    }

    /// `dataset`: the one `DOMStringMap` of the element.
    fn dataset(cx: &mut Cx<'_>, this: NodeId) -> b::Result<swb_js::Gc<swb_js::Object>> {
        dataset(cx, this)
    }

    /// `style`: the one `CSSStyleDeclaration` of the element.
    fn style(cx: &mut Cx<'_>, this: NodeId) -> b::Result<swb_js::Gc<swb_js::Object>> {
        crate::cssom::style(cx, this)
    }

    fn tab_index(cx: &mut Cx<'_>, this: NodeId) -> b::Result<i32> {
        tab_index(cx, this)
    }

    fn offset_parent(cx: &mut Cx<'_>, this: NodeId) -> b::Result<Option<NodeId>> {
        Ok(crate::cssom_view::offset_metrics(cx.rt, this)?.parent)
    }

    fn offset_top(cx: &mut Cx<'_>, this: NodeId) -> b::Result<i32> {
        Ok(crate::cssom_view::offset_metrics(cx.rt, this)?.top)
    }

    fn offset_left(cx: &mut Cx<'_>, this: NodeId) -> b::Result<i32> {
        Ok(crate::cssom_view::offset_metrics(cx.rt, this)?.left)
    }

    fn offset_width(cx: &mut Cx<'_>, this: NodeId) -> b::Result<i32> {
        Ok(crate::cssom_view::offset_metrics(cx.rt, this)?.width)
    }

    fn offset_height(cx: &mut Cx<'_>, this: NodeId) -> b::Result<i32> {
        Ok(crate::cssom_view::offset_metrics(cx.rt, this)?.height)
    }
}

/// `dataset` of an HTML or SVG element.
pub(crate) fn dataset(cx: &mut Cx<'_>, this: NodeId) -> b::Result<swb_js::Gc<swb_js::Object>> {
    let realm = cx.realm();
    Ok(same_object(cx.rt, this, Slot::Dataset, |rt| {
        crate::dom::string_map::new_string_map(rt, realm, this)
    })?)
}

/// `tabIndex`: the parsed `tabindex` attribute, else 0 for the elements
/// that can take the focus (links, controls, `iframe`, `object`, media
/// with controls, the SVG `a`) and -1 for the others (measured in Chromium: `a` and
/// `area` without `href` are 0, `embed` and `summary` are -1, and an
/// element with `contenteditable` is -1 too; the standard says 0 for it).
/// <https://html.spec.whatwg.org/multipage/interaction.html#dom-tabindex>
pub(crate) fn tab_index(cx: &mut Cx<'_>, this: NodeId) -> b::Result<i32> {
    if let Some(value) = attribute(cx, this, "tabindex")?
        && let Some(n) = parse_integer(&value).and_then(|n| i32::try_from(n).ok())
    {
        return Ok(n);
    }
    let dom = dom(cx.rt)?;
    let Some(e) = dom.element(this) else {
        return Ok(-1);
    };
    let focusable = if e.is_html() {
        matches!(
            &**e.local_name(),
            "a" | "area"
                | "button"
                | "input"
                | "select"
                | "textarea"
                | "iframe"
                | "object"
                | "audio"
                | "video"
        )
    } else {
        // Measured in Chromium: of the SVG elements only `a` is 0, with or
        // without `href`.
        e.name.ns == ns!(svg) && &**e.local_name() == "a"
    };
    Ok(if focusable { 0 } else { -1 })
}

impl g::html_title_element::HTMLTitleElementImpl for WebState {
    /// `title.text`: the child text content.
    fn text(cx: &mut Cx<'_>, this: NodeId) -> b::Result<String> {
        child_text_content(cx, this)
    }

    fn set_text(cx: &mut Cx<'_>, this: NodeId, value: String) -> b::Result<()> {
        crate::dom::string_replace_all(cx, this, &value)
    }
}

/// The concatenation of the Text node children of `node` ("child text
/// content").
pub(crate) fn child_text_content(cx: &Cx<'_>, node: NodeId) -> b::Result<String> {
    let dom = dom(cx.rt)?;
    Ok(dom
        .children(node)
        .filter_map(|c| dom.node(c).as_text())
        .collect())
}

impl g::html_base_element::HTMLBaseElementImpl for WebState {
    /// `base.href`: the `href` attribute resolved against the fallback
    /// base URL (the document URL), else the document URL.
    /// <https://html.spec.whatwg.org/multipage/semantics.html#dom-base-href>
    fn href(cx: &mut Cx<'_>, this: NodeId) -> b::Result<String> {
        let Some(value) = attribute(cx, this, "href")? else {
            return Ok(engine_mut(cx.rt)?.url());
        };
        Ok(engine_mut(cx.rt)?
            .parse_url_against_document_url(&value)
            .unwrap_or(value))
    }
}

/// The URL of a `formaction` or `action` attribute: the attribute resolved
/// against the document base URL, else the document URL when the attribute
/// is missing or empty.
/// <https://html.spec.whatwg.org/multipage/form-control-infrastructure.html#form-submission-algorithm>
pub(crate) fn action_url(cx: &mut Cx<'_>, this: NodeId, name: &str) -> b::Result<String> {
    match attribute(cx, this, name)? {
        Some(value) if !value.is_empty() => {
            Ok(engine_mut(cx.rt)?.parse_url(&value).unwrap_or(value))
        }
        _ => Ok(engine_mut(cx.rt)?.url()),
    }
}

/// The largest rendered size of an image that Chromium 148 reports
/// (`img.width = 1e10` reads 33554428, also for 33554430).
const MAX_RENDERED_IMAGE_SIZE: f64 = 33_554_428.0;

/// `img.width` or `img.height` (HTML §4.8.4.4): the rendered size of the
/// content box of a rendered image in whole px (a layout flush), at most
/// Chromium's limit, else the content attribute.
fn image_size(cx: &mut Cx<'_>, this: NodeId, name: &str, horizontal: bool) -> b::Result<u32> {
    let size = engine_mut(cx.rt)?.content_size(this);
    cx.rt.check_time()?;
    if let Some((width, height)) = size {
        let value = if horizontal { width } else { height };
        // `as` saturates.
        return Ok((value + 0.5).floor().min(MAX_RENDERED_IMAGE_SIZE) as u32);
    }
    b::reflect::get_unsigned_long::<WebState>(
        cx,
        this,
        name,
        b::reflect::UnsignedLimit::Range(0, u32::MAX),
        None,
    )
}

impl g::html_image_element::HTMLImageElementImpl for WebState {
    fn width(cx: &mut Cx<'_>, this: NodeId) -> b::Result<u32> {
        image_size(cx, this, "width", true)
    }

    fn height(cx: &mut Cx<'_>, this: NodeId) -> b::Result<u32> {
        image_size(cx, this, "height", false)
    }

    fn natural_width(_cx: &mut Cx<'_>, _this: NodeId) -> b::Result<u32> {
        Ok(0)
    }

    fn natural_height(_cx: &mut Cx<'_>, _this: NodeId) -> b::Result<u32> {
        Ok(0)
    }

    /// `complete`: true without a `src` and a `srcset`; false otherwise
    /// (swb does not report image loads to scripts yet).
    fn complete(cx: &mut Cx<'_>, this: NodeId) -> b::Result<bool> {
        let has = |name: &str| attribute(cx, this, name).map(|v| v.is_some_and(|v| !v.is_empty()));
        Ok(!has("src")? && !has("srcset")?)
    }

    fn current_src(_cx: &mut Cx<'_>, _this: NodeId) -> b::Result<String> {
        Ok(String::new())
    }

    /// `new Image(width, height)`.
    fn legacy_factory_image(
        cx: &mut Cx<'_>,
        width: Option<u32>,
        height: Option<u32>,
    ) -> b::Result<swb_js::Gc<swb_js::Object>> {
        super::factories::image(cx, width, height)
    }
}

impl g::html_link_element::HTMLLinkElementImpl for WebState {
    fn rel_list(cx: &mut Cx<'_>, this: NodeId) -> b::Result<swb_js::Gc<swb_js::Object>> {
        super::token_list(cx, this, "rel")
    }

    fn sizes(cx: &mut Cx<'_>, this: NodeId) -> b::Result<swb_js::Gc<swb_js::Object>> {
        super::token_list(cx, this, "sizes")
    }

    fn blocking(cx: &mut Cx<'_>, this: NodeId) -> b::Result<swb_js::Gc<swb_js::Object>> {
        super::token_list(cx, this, "blocking")
    }
}

impl g::html_style_element::HTMLStyleElementImpl for WebState {
    fn blocking(cx: &mut Cx<'_>, this: NodeId) -> b::Result<swb_js::Gc<swb_js::Object>> {
        super::token_list(cx, this, "blocking")
    }
}

impl g::htmli_frame_element::HTMLIFrameElementImpl for WebState {
    fn sandbox(cx: &mut Cx<'_>, this: NodeId) -> b::Result<swb_js::Gc<swb_js::Object>> {
        super::token_list(cx, this, "sandbox")
    }
}

/// The `autocomplete` getter of `input`, `select` and `textarea`: the
/// attribute value if it is a valid autofill field name (`on`, `off`, or
/// optional section, address type and contact type tokens and a field
/// name), in ASCII lowercase; else the empty string.
/// <https://html.spec.whatwg.org/multipage/form-control-infrastructure.html#autofill-processing-model>
pub(crate) fn autocomplete(cx: &mut Cx<'_>, this: NodeId) -> b::Result<String> {
    let value = attribute(cx, this, "autocomplete")?.unwrap_or_default();
    Ok(autofill_tokens(&value))
}

fn autofill_tokens(value: &str) -> String {
    const FIELDS: &[&str] = &[
        "name",
        "honorific-prefix",
        "given-name",
        "additional-name",
        "family-name",
        "honorific-suffix",
        "nickname",
        "username",
        "new-password",
        "current-password",
        "one-time-code",
        "organization-title",
        "organization",
        "street-address",
        "address-line1",
        "address-line2",
        "address-line3",
        "address-level4",
        "address-level3",
        "address-level2",
        "address-level1",
        "country",
        "country-name",
        "postal-code",
        "cc-name",
        "cc-given-name",
        "cc-additional-name",
        "cc-family-name",
        "cc-number",
        "cc-exp",
        "cc-exp-month",
        "cc-exp-year",
        "cc-csc",
        "cc-type",
        "transaction-currency",
        "transaction-amount",
        "language",
        "bday",
        "bday-day",
        "bday-month",
        "bday-year",
        "sex",
        "url",
        "photo",
    ];
    const CONTACT_FIELDS: &[&str] = &[
        "tel",
        "tel-country-code",
        "tel-national",
        "tel-area-code",
        "tel-local",
        "tel-local-prefix",
        "tel-local-suffix",
        "tel-extension",
        "email",
        "impp",
    ];
    let lower = value.to_ascii_lowercase();
    let mut tokens: Vec<&str> = lower.split_ascii_whitespace().collect();
    if matches!(tokens.as_slice(), ["on" | "off"]) {
        return lower.trim().to_owned();
    }
    if tokens.last() == Some(&"webauthn") {
        tokens.pop();
    }
    let mut rest = tokens.as_slice();
    if let [first, tail @ ..] = rest
        && first.starts_with("section-")
    {
        rest = tail;
    }
    if let [first, tail @ ..] = rest
        && matches!(*first, "shipping" | "billing")
    {
        rest = tail;
    }
    let mut contact = false;
    if let [first, tail @ ..] = rest
        && matches!(*first, "home" | "work" | "mobile" | "fax" | "pager")
    {
        contact = true;
        rest = tail;
    }
    let valid = match rest {
        [field] => CONTACT_FIELDS.contains(field) || (!contact && FIELDS.contains(field)),
        _ => false,
    };
    if valid {
        lower.split_ascii_whitespace().collect::<Vec<_>>().join(" ")
    } else {
        String::new()
    }
}
