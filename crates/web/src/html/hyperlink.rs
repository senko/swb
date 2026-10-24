//! The URL decomposition members of `a` and `area` (HTML
//! `HTMLHyperlinkElementUtils`,
//! <https://html.spec.whatwg.org/multipage/links.html#htmlhyperlinkelementutils>)
//! and the `href` getter.
//!
//! The getters read the URL that the `href` attribute resolves to against
//! the document base URL; the setters change that URL and write it back
//! to `href` ("update href"). The `url` crate does the parsing with the
//! WHATWG URL algorithms; its setters follow the same specification, but
//! they take the pieces apart where the standard has one state machine
//! (`host` with a port, for example), so `host` and `port` split the value
//! here.

use swb_dom::NodeId;
use url::Url;

use super::{attribute, set_attribute};
use crate::bindings::{self as b, Cx};
use crate::generated as g;
use crate::state::{WebState, engine_mut};

/// The URL of the element's `href`, or none (no attribute, or it does
/// not parse).
fn current(cx: &mut Cx<'_>, this: NodeId) -> b::Result<Option<Url>> {
    let Some(href) = attribute(cx, this, "href")? else {
        return Ok(None);
    };
    Ok(engine_mut(cx.rt)?
        .parse_url(&href)
        .and_then(|serialized| Url::parse(&serialized).ok()))
}

/// "Update href": the serialized URL becomes the attribute.
fn update(cx: &mut Cx<'_>, this: NodeId, url: &Url) -> b::Result<()> {
    set_attribute(cx, this, "href", url.as_str())
}

/// The `href` getter: the resolved URL, the attribute value if it does
/// not parse, and the empty string without the attribute.
pub(crate) fn href(cx: &mut Cx<'_>, this: NodeId) -> b::Result<String> {
    b::reflect::get_url::<WebState>(cx, this, "href")
}

fn origin(cx: &mut Cx<'_>, this: NodeId) -> b::Result<String> {
    Ok(current(cx, this)?.map_or_else(String::new, |u| {
        // Chromium (measured) gives "file://" for the origin of a file URL.
        if u.scheme() == "file" {
            "file://".to_owned()
        } else {
            u.origin().ascii_serialization()
        }
    }))
}

fn protocol(cx: &mut Cx<'_>, this: NodeId) -> b::Result<String> {
    Ok(current(cx, this)?.map_or_else(|| ":".to_owned(), |u| format!("{}:", u.scheme())))
}

fn set_protocol(cx: &mut Cx<'_>, this: NodeId, value: &str) -> b::Result<()> {
    if let Some(mut url) = current(cx, this)? {
        let scheme = value.split(':').next().unwrap_or_default();
        if url.set_scheme(scheme).is_ok() {
            update(cx, this, &url)?;
        }
    }
    Ok(())
}

fn username(cx: &mut Cx<'_>, this: NodeId) -> b::Result<String> {
    Ok(current(cx, this)?.map_or_else(String::new, |u| u.username().to_owned()))
}

fn set_username(cx: &mut Cx<'_>, this: NodeId, value: &str) -> b::Result<()> {
    if let Some(mut url) = current(cx, this)?
        && url.set_username(value).is_ok()
    {
        update(cx, this, &url)?;
    }
    Ok(())
}

fn password(cx: &mut Cx<'_>, this: NodeId) -> b::Result<String> {
    Ok(current(cx, this)?.map_or_else(String::new, |u| u.password().unwrap_or("").to_owned()))
}

fn set_password(cx: &mut Cx<'_>, this: NodeId, value: &str) -> b::Result<()> {
    if let Some(mut url) = current(cx, this)?
        && url
            .set_password(if value.is_empty() { None } else { Some(value) })
            .is_ok()
    {
        update(cx, this, &url)?;
    }
    Ok(())
}

fn host(cx: &mut Cx<'_>, this: NodeId) -> b::Result<String> {
    let Some(url) = current(cx, this)? else {
        return Ok(String::new());
    };
    let Some(host) = url.host_str() else {
        return Ok(String::new());
    };
    Ok(match url.port() {
        Some(port) => format!("{host}:{port}"),
        None => host.to_owned(),
    })
}

/// Cuts `value` before the first `/`, `\`, `?` or `#`: the host state of
/// the URL parser ends there, and the rest of the value is ignored.
fn host_part(value: &str) -> &str {
    value.split(['/', '\\', '?', '#']).next().unwrap_or("")
}

/// Splits `host:port` at the colon that is not inside brackets.
fn split_host_port(value: &str) -> (&str, Option<&str>) {
    let start = if value.starts_with('[') {
        value.find(']').unwrap_or(0)
    } else {
        0
    };
    match value[start..].find(':') {
        Some(i) => (&value[..start + i], Some(&value[start + i + 1..])),
        None => (value, None),
    }
}

/// The digits at the start of `text` as a port.
fn leading_port(text: &str) -> Option<u16> {
    let digits: String = text.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

fn set_host(cx: &mut Cx<'_>, this: NodeId, value: &str) -> b::Result<()> {
    let Some(mut url) = current(cx, this)? else {
        return Ok(());
    };
    if url.cannot_be_a_base() || value.is_empty() && url.scheme() != "file" {
        return Ok(());
    }
    let (name, port) = split_host_port(host_part(value));
    if url.set_host(Some(name)).is_ok() {
        if let Some(port) = port.and_then(leading_port) {
            let _ = url.set_port(Some(port));
        }
        update(cx, this, &url)?;
    }
    Ok(())
}

fn hostname(cx: &mut Cx<'_>, this: NodeId) -> b::Result<String> {
    Ok(current(cx, this)?
        .and_then(|u| u.host_str().map(str::to_owned))
        .unwrap_or_default())
}

fn set_hostname(cx: &mut Cx<'_>, this: NodeId, value: &str) -> b::Result<()> {
    let Some(mut url) = current(cx, this)? else {
        return Ok(());
    };
    if url.cannot_be_a_base() || value.is_empty() && url.scheme() != "file" {
        return Ok(());
    }
    // A port in the value makes the setter do nothing (measured in
    // Chromium; the standard's hostname state returns at the colon).
    let (name, port) = split_host_port(host_part(value));
    if port.is_none() && url.set_host(Some(name)).is_ok() {
        update(cx, this, &url)?;
    }
    Ok(())
}

fn port(cx: &mut Cx<'_>, this: NodeId) -> b::Result<String> {
    Ok(current(cx, this)?
        .and_then(|u| u.port())
        .map_or_else(String::new, |p| p.to_string()))
}

fn set_port(cx: &mut Cx<'_>, this: NodeId, value: &str) -> b::Result<()> {
    let Some(mut url) = current(cx, this)? else {
        return Ok(());
    };
    if url.host_str().is_none_or(str::is_empty) || url.scheme() == "file" {
        return Ok(());
    }
    let changed = if value.is_empty() {
        url.set_port(None).is_ok()
    } else {
        leading_port(value).is_some_and(|p| url.set_port(Some(p)).is_ok())
    };
    if changed {
        update(cx, this, &url)?;
    }
    Ok(())
}

fn pathname(cx: &mut Cx<'_>, this: NodeId) -> b::Result<String> {
    Ok(current(cx, this)?.map_or_else(String::new, |u| u.path().to_owned()))
}

fn set_pathname(cx: &mut Cx<'_>, this: NodeId, value: &str) -> b::Result<()> {
    if let Some(mut url) = current(cx, this)?
        && !url.cannot_be_a_base()
    {
        url.set_path(value);
        update(cx, this, &url)?;
    }
    Ok(())
}

fn search(cx: &mut Cx<'_>, this: NodeId) -> b::Result<String> {
    Ok(current(cx, this)?
        .and_then(|u| u.query().filter(|q| !q.is_empty()).map(|q| format!("?{q}")))
        .unwrap_or_default())
}

fn set_search(cx: &mut Cx<'_>, this: NodeId, value: &str) -> b::Result<()> {
    if let Some(mut url) = current(cx, this)? {
        let query = value.strip_prefix('?').unwrap_or(value);
        url.set_query(if value.is_empty() { None } else { Some(query) });
        update(cx, this, &url)?;
    }
    Ok(())
}

fn hash(cx: &mut Cx<'_>, this: NodeId) -> b::Result<String> {
    Ok(current(cx, this)?
        .and_then(|u| {
            u.fragment()
                .filter(|f| !f.is_empty())
                .map(|f| format!("#{f}"))
        })
        .unwrap_or_default())
}

fn set_hash(cx: &mut Cx<'_>, this: NodeId, value: &str) -> b::Result<()> {
    if let Some(mut url) = current(cx, this)? {
        let fragment = value.strip_prefix('#').unwrap_or(value);
        url.set_fragment(if value.is_empty() {
            None
        } else {
            Some(fragment)
        });
        update(cx, this, &url)?;
    }
    Ok(())
}

/// The members of `HyperlinkElementUtils` and `HTMLHyperlinkElementUtils`
/// for the trait of `a` and of `area`.
macro_rules! hyperlink_members {
    () => {
        fn href(cx: &mut Cx<'_>, this: NodeId) -> b::Result<String> {
            href(cx, this)
        }
        fn origin(cx: &mut Cx<'_>, this: NodeId) -> b::Result<String> {
            origin(cx, this)
        }
        fn protocol(cx: &mut Cx<'_>, this: NodeId) -> b::Result<String> {
            protocol(cx, this)
        }
        fn set_protocol(cx: &mut Cx<'_>, this: NodeId, value: String) -> b::Result<()> {
            set_protocol(cx, this, &value)
        }
        fn username(cx: &mut Cx<'_>, this: NodeId) -> b::Result<String> {
            username(cx, this)
        }
        fn set_username(cx: &mut Cx<'_>, this: NodeId, value: String) -> b::Result<()> {
            set_username(cx, this, &value)
        }
        fn password(cx: &mut Cx<'_>, this: NodeId) -> b::Result<String> {
            password(cx, this)
        }
        fn set_password(cx: &mut Cx<'_>, this: NodeId, value: String) -> b::Result<()> {
            set_password(cx, this, &value)
        }
        fn host(cx: &mut Cx<'_>, this: NodeId) -> b::Result<String> {
            host(cx, this)
        }
        fn set_host(cx: &mut Cx<'_>, this: NodeId, value: String) -> b::Result<()> {
            set_host(cx, this, &value)
        }
        fn hostname(cx: &mut Cx<'_>, this: NodeId) -> b::Result<String> {
            hostname(cx, this)
        }
        fn set_hostname(cx: &mut Cx<'_>, this: NodeId, value: String) -> b::Result<()> {
            set_hostname(cx, this, &value)
        }
        fn port(cx: &mut Cx<'_>, this: NodeId) -> b::Result<String> {
            port(cx, this)
        }
        fn set_port(cx: &mut Cx<'_>, this: NodeId, value: String) -> b::Result<()> {
            set_port(cx, this, &value)
        }
        fn pathname(cx: &mut Cx<'_>, this: NodeId) -> b::Result<String> {
            pathname(cx, this)
        }
        fn set_pathname(cx: &mut Cx<'_>, this: NodeId, value: String) -> b::Result<()> {
            set_pathname(cx, this, &value)
        }
        fn search(cx: &mut Cx<'_>, this: NodeId) -> b::Result<String> {
            search(cx, this)
        }
        fn set_search(cx: &mut Cx<'_>, this: NodeId, value: String) -> b::Result<()> {
            set_search(cx, this, &value)
        }
        fn hash(cx: &mut Cx<'_>, this: NodeId) -> b::Result<String> {
            hash(cx, this)
        }
        fn set_hash(cx: &mut Cx<'_>, this: NodeId, value: String) -> b::Result<()> {
            set_hash(cx, this, &value)
        }
    };
}

impl g::html_area_element::HTMLAreaElementImpl for WebState {
    hyperlink_members!();

    fn rel_list(cx: &mut Cx<'_>, this: NodeId) -> b::Result<swb_js::Gc<swb_js::Object>> {
        super::token_lists::token_list(cx, this, "rel")
    }
}

/// The anchor members that are not URL members: they live in the same
/// trait.
impl g::html_anchor_element::HTMLAnchorElementImpl for WebState {
    hyperlink_members!();

    fn rel_list(cx: &mut Cx<'_>, this: NodeId) -> b::Result<swb_js::Gc<swb_js::Object>> {
        super::token_lists::token_list(cx, this, "rel")
    }

    /// `a.text`: the text content.
    fn text(cx: &mut Cx<'_>, this: NodeId) -> b::Result<String> {
        Ok(crate::state::dom(cx.rt)?.text_content(this))
    }

    fn set_text(cx: &mut Cx<'_>, this: NodeId, value: String) -> b::Result<()> {
        crate::dom::string_replace_all(cx, this, &value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_and_port_split() {
        assert_eq!(split_host_port("a.b:81"), ("a.b", Some("81")));
        assert_eq!(split_host_port("a.b"), ("a.b", None));
        assert_eq!(split_host_port("[::1]:80"), ("[::1]", Some("80")));
        assert_eq!(split_host_port("[::1]"), ("[::1]", None));
        assert_eq!(leading_port("8080x"), Some(8080));
        assert_eq!(leading_port("x"), None);
    }
}
