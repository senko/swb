//! `Headers` (Fetch §5.2, <https://fetch.spec.whatwg.org/#headers-class>)
//! and the header list that `Request` and `Response` share.
//!
//! The list keeps the headers in the order of insertion with the names in
//! lowercase and the values normalized (leading and trailing HTTP
//! whitespace removed). Iteration sorts and combines
//! (<https://fetch.spec.whatwg.org/#concept-header-list-sort-and-combine>):
//! the names in code unit order, the values of a name joined with `, `,
//! each `set-cookie` value on its own; the sorted list is cached until the
//! next change. The guard decides which headers a script may write
//! (<https://fetch.spec.whatwg.org/#concept-headers-guard>).

use swb_js::{Gc, GenericData, HostData, Object, Runtime, Value};

use super::generic;
use crate::bindings::{self as b, Cx, Exception, Platform};
use crate::generated as g;
use crate::state::WebState;

/// The most headers in one list (a script cannot grow one without bound).
const MAX_HEADERS: usize = 1 << 16;

/// The guard of a header list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Guard {
    /// `new Headers()`: anything goes.
    None,
    /// The headers of a `Request`: forbidden request headers are ignored.
    Request,
    /// The headers of a `no-cors` `Request`: only the no-CORS-safelisted
    /// ones stay.
    RequestNoCors,
    /// The headers of a `Response` made by a script: forbidden response
    /// headers are ignored.
    Response,
    /// The headers of a response from the network: read only.
    Immutable,
}

/// The data of a `Headers` object.
#[derive(Debug)]
pub(crate) struct HeadersData {
    /// The headers in order: lowercase name, normalized value.
    list: Vec<(String, String)>,
    guard: Guard,
    /// The sort-and-combine of `list`, until the list changes.
    sorted: Option<Vec<(String, String)>>,
}

impl GenericData for HeadersData {
    fn trace(&self, _tracer: &mut swb_js::Tracer<'_>) {}

    fn heap_size(&self) -> usize {
        let list: usize = self
            .list
            .iter()
            .map(|(n, v)| n.capacity() + v.capacity() + 64)
            .sum();
        let sorted: usize = self.sorted.as_ref().map_or(0, |s| {
            s.iter()
                .map(|(n, v)| n.capacity() + v.capacity() + 64)
                .sum()
        });
        list + sorted + 64
    }
}

/// The names that a script may not set on a request.
///
/// <https://fetch.spec.whatwg.org/#forbidden-request-header>
pub(crate) fn is_forbidden_request_header(name: &str, value: &str) -> bool {
    const NAMES: [&str; 21] = [
        "accept-charset",
        "accept-encoding",
        "access-control-request-headers",
        "access-control-request-method",
        "connection",
        "content-length",
        "cookie",
        "cookie2",
        "date",
        "dnt",
        "expect",
        "host",
        "keep-alive",
        "origin",
        "referer",
        "set-cookie",
        "te",
        "trailer",
        "transfer-encoding",
        "upgrade",
        "via",
    ];
    if NAMES.contains(&name) || name.starts_with("proxy-") || name.starts_with("sec-") {
        return true;
    }
    // The method override headers with a forbidden method.
    if matches!(
        name,
        "x-http-method" | "x-http-method-override" | "x-method-override"
    ) {
        return value
            .split(',')
            .any(|method| swb_net::Method::is_forbidden(method.trim()));
    }
    false
}

/// <https://fetch.spec.whatwg.org/#forbidden-response-header-name>
pub(crate) fn is_forbidden_response_header(name: &str) -> bool {
    matches!(name, "set-cookie" | "set-cookie2")
}

/// HTTP whitespace: tab, line feed, carriage return and space.
fn is_http_whitespace(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\r' | ' ')
}

/// A header value with the leading and trailing HTTP whitespace removed.
pub(crate) fn normalize_value(value: &str) -> &str {
    value.trim_matches(is_http_whitespace)
}

/// A header name and value as the `Headers` methods accept them, or the
/// `TypeError` message.
fn validate(name: &str, value: Option<&str>) -> Result<(), &'static str> {
    if !swb_net::is_token(name) {
        return Err("Invalid name");
    }
    if let Some(value) = value
        && value.chars().any(|c| matches!(c, '\0' | '\n' | '\r'))
    {
        return Err("Invalid value");
    }
    Ok(())
}

impl HeadersData {
    /// An empty list with `guard`.
    pub(crate) fn new(guard: Guard) -> HeadersData {
        HeadersData {
            list: Vec::new(),
            guard,
            sorted: None,
        }
    }

    /// The headers in order, with lowercase names.
    #[cfg(test)]
    fn list(&self) -> &[(String, String)] {
        &self.list
    }

    fn get(&self, name: &str) -> Option<String> {
        let name = name.to_ascii_lowercase();
        let mut values = self.list.iter().filter(|(n, _)| *n == name).peekable();
        values.peek()?;
        Some(
            values
                .map(|(_, v)| v.as_str())
                .collect::<Vec<_>>()
                .join(", "),
        )
    }

    /// Whether the guard makes the write of `(name, value)` a no-op
    /// (Fetch "append a header", step 4 to 6).
    fn ignores(&self, name: &str, value: &str) -> bool {
        match self.guard {
            Guard::Request => is_forbidden_request_header(name, value),
            Guard::RequestNoCors => {
                let mut combined = self.get(name).unwrap_or_default();
                if !combined.is_empty() {
                    combined.push_str(", ");
                }
                combined.push_str(value);
                !is_no_cors_safelisted(name, &combined)
            }
            Guard::Response => is_forbidden_response_header(name),
            Guard::None | Guard::Immutable => false,
        }
    }

    /// Fetch "append a header to a header list" with the checks of
    /// `Headers.append`. `Err` is the message of a `TypeError`.
    pub(crate) fn append(&mut self, name: &str, value: &str) -> Result<(), &'static str> {
        validate(name, Some(normalize_value(value)))?;
        if self.guard == Guard::Immutable {
            return Err("Headers are immutable");
        }
        let (name, value) = (name.to_ascii_lowercase(), normalize_value(value));
        if self.ignores(&name, value) {
            return Ok(());
        }
        if self.list.len() >= MAX_HEADERS {
            return Err("Too many headers");
        }
        self.sorted = None;
        self.list.push((name, value.to_owned()));
        Ok(())
    }

    /// `Headers.set`.
    fn set(&mut self, name: &str, value: &str) -> Result<(), &'static str> {
        validate(name, Some(normalize_value(value)))?;
        if self.guard == Guard::Immutable {
            return Err("Headers are immutable");
        }
        let (name, value) = (name.to_ascii_lowercase(), normalize_value(value).to_owned());
        // `set` replaces: the no-CORS check looks at the new value alone.
        let ignored = match self.guard {
            Guard::RequestNoCors => !is_no_cors_safelisted(&name, &value),
            _ => self.ignores(&name, &value),
        };
        if ignored {
            return Ok(());
        }
        self.sorted = None;
        if let Some(index) = self.list.iter().position(|(n, _)| *n == name) {
            self.list[index].1 = value;
            let mut position = 0;
            self.list.retain(|(n, _)| {
                let keep = position <= index || *n != name;
                position += 1;
                keep
            });
        } else {
            if self.list.len() >= MAX_HEADERS {
                return Err("Too many headers");
            }
            self.list.push((name, value));
        }
        Ok(())
    }

    /// `Headers.delete`.
    fn delete(&mut self, name: &str) -> Result<(), &'static str> {
        validate(name, None)?;
        if self.guard == Guard::Immutable {
            return Err("Headers are immutable");
        }
        let name = name.to_ascii_lowercase();
        let ignored = match self.guard {
            Guard::Request => is_forbidden_request_header(&name, ""),
            Guard::RequestNoCors => !matches!(
                name.as_str(),
                "accept" | "accept-language" | "content-language" | "content-type"
            ),
            Guard::Response => is_forbidden_response_header(&name),
            Guard::None | Guard::Immutable => false,
        };
        if !ignored {
            self.sorted = None;
            self.list.retain(|(n, _)| *n != name);
        }
        Ok(())
    }

    /// The sort and combine of the list.
    fn sorted(&mut self) -> &[(String, String)] {
        self.sorted.get_or_insert_with(|| {
            let mut names: Vec<&str> = self.list.iter().map(|(n, _)| n.as_str()).collect();
            names.sort_unstable();
            names.dedup();
            let mut out = Vec::new();
            for name in names {
                let values = self.list.iter().filter(|(n, _)| n == name).map(|(_, v)| v);
                if name == "set-cookie" {
                    out.extend(values.map(|v| (name.to_owned(), v.clone())));
                } else {
                    let joined = values.map(String::as_str).collect::<Vec<_>>().join(", ");
                    out.push((name.to_owned(), joined));
                }
            }
            out
        })
    }
}

/// <https://fetch.spec.whatwg.org/#no-cors-safelisted-request-header>
fn is_no_cors_safelisted(name: &str, value: &str) -> bool {
    matches!(
        name,
        "accept" | "accept-language" | "content-language" | "content-type"
    ) && swb_net::is_cors_safelisted_header(name, value)
}

fn data(rt: &Runtime, this: Platform) -> b::Result<&HeadersData> {
    generic::<HeadersData>(rt, this)
}

fn data_mut(rt: &mut Runtime, this: Platform) -> b::Result<&mut HeadersData> {
    super::generic_mut::<HeadersData>(rt, this)
}

/// A new `Headers` object with `list` (already normalized) and `guard`.
pub(crate) fn new_object(
    rt: &mut Runtime,
    realm: u32,
    guard: Guard,
    list: Vec<(String, String)>,
) -> b::Result<Gc<Object>> {
    let mut headers = HeadersData::new(guard);
    headers.list = list;
    let handle = rt.alloc_host_data(headers)?;
    Ok(rt.new_host_object(realm, g::HEADERS, handle)?)
}

/// The data of the `Headers` object `object`.
fn of_object(rt: &mut Runtime, object: Gc<Object>) -> b::Result<&mut HeadersData> {
    match rt.host_object(object.into()) {
        Some((_, HostData::Generic(handle))) => rt
            .heap_mut()
            .generic_mut::<HeadersData>(handle)
            .map_err(|_| Exception::type_error("Illegal invocation")),
        _ => Err(Exception::type_error("Illegal invocation")),
    }
}

/// The headers of the `Headers` object `object`, in order, for a request
/// to the network.
pub(crate) fn list_of(rt: &mut Runtime, object: Gc<Object>) -> b::Result<Vec<(String, String)>> {
    Ok(of_object(rt, object)?.list.clone())
}

/// The value of the header `name` of the `Headers` object `object` (the
/// values of a repeated header joined with `, `).
pub(crate) fn value_of(
    rt: &mut Runtime,
    object: Gc<Object>,
    name: &str,
) -> b::Result<Option<String>> {
    Ok(of_object(rt, object)?.get(name))
}

/// Appends a header to the `Headers` object `object` as `append` does.
pub(crate) fn append_to(
    rt: &mut Runtime,
    object: Gc<Object>,
    name: &str,
    value: &str,
) -> b::Result<()> {
    of_object(rt, object)?
        .append(name, value)
        .map_err(Exception::type_error)
}

/// A copy of the list of `object` in a new `Headers` object.
pub(crate) fn copy_of(
    rt: &mut Runtime,
    realm: u32,
    object: Gc<Object>,
    guard: Guard,
) -> b::Result<Gc<Object>> {
    let list = list_of(rt, object)?;
    new_object(rt, realm, guard, list)
}

/// The pairs of a `HeadersInit` value (Fetch "fill"): a sequence of
/// pairs, or a record of names and values. `Err` messages are Chromium's.
fn init_pairs(rt: &mut Runtime, init: Value) -> b::Result<Vec<(String, String)>> {
    if !matches!(init, Value::Object(_)) {
        return Err(Exception::type_error(
            "The provided value is not of type '(record<ByteString, ByteString> or sequence<sequence<ByteString>>)'.",
        ));
    }
    if let Some(method) = b::convert::iterator_method(rt, init)? {
        return b::convert::sequence_from_method(rt, init, method, |rt, item| {
            let pair = b::convert::sequence(rt, item, b::convert::byte_string)?;
            // Chromium: "Invalid value" for a pair of another length.
            match <[String; 2]>::try_from(pair) {
                Ok([name, value]) => Ok((name, value)),
                Err(_) => Err(Exception::type_error("Invalid value")),
            }
        });
    }
    b::convert::record(rt, init, b::convert::byte_string, b::convert::byte_string)
}

/// Fetch "fill a Headers object": appends the pairs of `init` to the
/// `Headers` object `object`, with its guard.
pub(crate) fn fill(rt: &mut Runtime, object: Gc<Object>, init: Value) -> b::Result<()> {
    for (name, value) in init_pairs(rt, init)? {
        append_to(rt, object, &name, &value)?;
    }
    Ok(())
}

impl g::headers::HeadersImpl for WebState {
    fn constructor(cx: &mut Cx<'_>, init: Value) -> b::Result<Gc<Object>> {
        let handle = cx.rt.alloc_host_data(HeadersData::new(Guard::None))?;
        let object = cx.new_instance::<WebState>(handle)?;
        if !init.is_undefined() {
            fill(cx.rt, object, init)?;
        }
        Ok(object)
    }

    fn append(cx: &mut Cx<'_>, this: Platform, name: String, value: String) -> b::Result<()> {
        data_mut(cx.rt, this)?
            .append(&name, &value)
            .map_err(Exception::type_error)
    }

    fn delete(cx: &mut Cx<'_>, this: Platform, name: String) -> b::Result<()> {
        data_mut(cx.rt, this)?
            .delete(&name)
            .map_err(Exception::type_error)
    }

    fn get(cx: &mut Cx<'_>, this: Platform, name: String) -> b::Result<Option<String>> {
        validate(&name, None).map_err(Exception::type_error)?;
        Ok(data(cx.rt, this)?.get(&name))
    }

    fn get_set_cookie(cx: &mut Cx<'_>, this: Platform) -> b::Result<Vec<String>> {
        Ok(data(cx.rt, this)?
            .list
            .iter()
            .filter(|(n, _)| n == "set-cookie")
            .map(|(_, v)| v.clone())
            .collect())
    }

    fn has(cx: &mut Cx<'_>, this: Platform, name: String) -> b::Result<bool> {
        validate(&name, None).map_err(Exception::type_error)?;
        Ok(data(cx.rt, this)?.get(&name).is_some())
    }

    fn set(cx: &mut Cx<'_>, this: Platform, name: String, value: String) -> b::Result<()> {
        data_mut(cx.rt, this)?
            .set(&name, &value)
            .map_err(Exception::type_error)
    }

    fn pair_at(cx: &mut Cx<'_>, this: Platform, index: u32) -> b::Result<Option<(String, String)>> {
        Ok(data_mut(cx.rt, this)?.sorted().get(index as usize).cloned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(headers: &mut HeadersData) -> Vec<(String, String)> {
        headers.sorted().to_vec()
    }

    #[test]
    fn names_are_lowercase_and_values_trimmed() {
        let mut h = HeadersData::new(Guard::None);
        h.append("Content-Type", " text/html\t").unwrap();
        assert_eq!(h.get("CONTENT-type").as_deref(), Some("text/html"));
        assert_eq!(h.append("a b", "x"), Err("Invalid name"));
        assert_eq!(h.append("a", "x\ny"), Err("Invalid value"));
        assert_eq!(h.append("", "x"), Err("Invalid name"));
    }

    #[test]
    fn sort_and_combine() {
        let mut h = HeadersData::new(Guard::None);
        for (n, v) in [
            ("B", "2"),
            ("a", "1"),
            ("b", "3"),
            ("Set-Cookie", "x=1"),
            ("set-cookie", "y=2"),
        ] {
            h.append(n, v).unwrap();
        }
        let pairs = list(&mut h);
        assert_eq!(
            pairs,
            [
                ("a".to_owned(), "1".to_owned()),
                ("b".to_owned(), "2, 3".to_owned()),
                ("set-cookie".to_owned(), "x=1".to_owned()),
                ("set-cookie".to_owned(), "y=2".to_owned()),
            ]
        );
        // `get` combines also the cookies.
        assert_eq!(h.get("set-cookie").as_deref(), Some("x=1, y=2"));
    }

    #[test]
    fn set_replaces_in_place() {
        let mut h = HeadersData::new(Guard::None);
        for (n, v) in [("a", "1"), ("b", "2"), ("a", "3")] {
            h.append(n, v).unwrap();
        }
        h.set("A", "new").unwrap();
        assert_eq!(
            h.list(),
            [
                ("a".to_owned(), "new".to_owned()),
                ("b".to_owned(), "2".to_owned())
            ]
        );
        h.delete("a").unwrap();
        assert_eq!(h.get("a"), None);
    }

    #[test]
    fn guards() {
        let mut request = HeadersData::new(Guard::Request);
        request.append("Host", "evil").unwrap();
        request.append("Sec-Fetch-Mode", "x").unwrap();
        request.append("Proxy-Authorization", "x").unwrap();
        request.append("X-HTTP-Method-Override", "TRACE").unwrap();
        request.append("X-HTTP-Method-Override", "PUT").unwrap();
        request.append("X-Ok", "1").unwrap();
        assert_eq!(
            request.list(),
            [
                ("x-http-method-override".to_owned(), "PUT".to_owned()),
                ("x-ok".to_owned(), "1".to_owned())
            ]
        );
        let mut no_cors = HeadersData::new(Guard::RequestNoCors);
        no_cors.append("X-Custom", "1").unwrap();
        no_cors.append("Accept", "text/html").unwrap();
        no_cors.append("Content-Type", "application/json").unwrap();
        no_cors.append("Content-Type", "text/plain").unwrap();
        assert_eq!(
            no_cors.list(),
            [
                ("accept".to_owned(), "text/html".to_owned()),
                ("content-type".to_owned(), "text/plain".to_owned())
            ]
        );
        let mut response = HeadersData::new(Guard::Response);
        response.append("Set-Cookie", "a=1").unwrap();
        response.append("X-A", "1").unwrap();
        assert_eq!(response.list().len(), 1);
        let mut immutable = HeadersData::new(Guard::Immutable);
        assert_eq!(immutable.append("a", "1"), Err("Headers are immutable"));
        assert_eq!(immutable.set("a", "1"), Err("Headers are immutable"));
        assert_eq!(immutable.delete("a"), Err("Headers are immutable"));
    }
}
