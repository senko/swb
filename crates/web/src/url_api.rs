//! `URL` and `URLSearchParams` (URL Standard §6,
//! <https://url.spec.whatwg.org/#api>) over the `url` crate.
//!
//! A `URL` object keeps a `url::Url` (the crate follows the WHATWG URL
//! parser and serializer) and, once a script reads `searchParams`, its
//! `URLSearchParams` object. The two are linked as the standard requires:
//! a change of the list sets the URL's query (the "update steps"), and a
//! new `search` or `href` sets the list again. The attribute setters
//! follow the standard's API setters on top of the crate's: where the
//! crate's setter differs from the state machine with a state override
//! (`host` with a port, `port` with trailing text, `protocol` with
//! trailing text), the code here prepares the input.
//!
//! `Location` (`location.rs`) uses the same getters and setters on the
//! document's URL.

use swb_js::{Gc, GenericData, HostData, Object, Runtime, Tracer};
use url::Url;

use crate::bindings::{self as b, Cx, Exception, Platform};
use crate::generated as g;
use crate::generated::types::UsvStringSequenceSequenceOrUsvStringRecordOrUsvString as Init;
use crate::state::WebState;

/// The host data of a `URL` object.
pub(crate) struct UrlData {
    pub(crate) url: Url,
    /// The object of `searchParams` once a script read it.
    search_params: Option<Gc<Object>>,
}

impl GenericData for UrlData {
    fn trace(&self, tracer: &mut Tracer<'_>) {
        if let Some(object) = self.search_params {
            tracer.object(object);
        }
    }

    fn heap_size(&self) -> usize {
        self.url.as_str().len() * 2
    }
}

/// The host data of a `URLSearchParams` object: the list and the `URL`
/// object whose query it is.
pub(crate) struct ParamsData {
    list: Vec<(String, String)>,
    url: Option<Gc<Object>>,
}

impl GenericData for ParamsData {
    fn trace(&self, tracer: &mut Tracer<'_>) {
        if let Some(object) = self.url {
            tracer.object(object);
        }
    }

    fn heap_size(&self) -> usize {
        self.list
            .iter()
            .map(|(name, value)| name.capacity() + value.capacity() + 48)
            .sum()
    }
}

fn generic(this: Platform) -> b::Result<Gc<swb_js::Generic>> {
    match this.data {
        HostData::Generic(handle) => Ok(handle),
        HostData::Word(_) => Err(Exception::type_error("Illegal invocation")),
    }
}

fn data(rt: &Runtime, this: Platform) -> b::Result<&UrlData> {
    rt.heap()
        .generic::<UrlData>(generic(this)?)
        .map_err(|_| Exception::type_error("Illegal invocation"))
}

fn data_mut(rt: &mut Runtime, this: Platform) -> b::Result<&mut UrlData> {
    rt.heap_mut()
        .generic_mut::<UrlData>(generic(this)?)
        .map_err(|_| Exception::type_error("Illegal invocation"))
}

fn params(rt: &Runtime, this: Platform) -> b::Result<&ParamsData> {
    rt.heap()
        .generic::<ParamsData>(generic(this)?)
        .map_err(|_| Exception::type_error("Illegal invocation"))
}

fn params_mut(rt: &mut Runtime, this: Platform) -> b::Result<&mut ParamsData> {
    rt.heap_mut()
        .generic_mut::<ParamsData>(generic(this)?)
        .map_err(|_| Exception::type_error("Illegal invocation"))
}

// ----- The attributes, shared with `Location` -----

/// The API getters of a URL (URL Standard §6.1).
pub(crate) mod get {
    use url::Url;

    pub(crate) fn protocol(url: &Url) -> String {
        format!("{}:", url.scheme())
    }

    pub(crate) fn host(url: &Url) -> String {
        match (url.host_str(), url.port()) {
            (Some(host), Some(port)) => format!("{host}:{port}"),
            (Some(host), None) => host.to_owned(),
            (None, _) => String::new(),
        }
    }

    pub(crate) fn hostname(url: &Url) -> String {
        url.host_str().unwrap_or_default().to_owned()
    }

    pub(crate) fn port(url: &Url) -> String {
        url.port().map(|p| p.to_string()).unwrap_or_default()
    }

    pub(crate) fn search(url: &Url) -> String {
        match url.query() {
            Some(query) if !query.is_empty() => format!("?{query}"),
            _ => String::new(),
        }
    }

    pub(crate) fn hash(url: &Url) -> String {
        match url.fragment() {
            Some(fragment) if !fragment.is_empty() => format!("#{fragment}"),
            _ => String::new(),
        }
    }

    /// The origin of a URL as `URL.origin` gives it. Chromium words the
    /// origin of a `file:` URL `file://` (the URL standard leaves it
    /// open; measured).
    pub(crate) fn origin(url: &Url) -> String {
        if url.scheme() == "file" {
            return "file://".to_owned();
        }
        url.origin().ascii_serialization()
    }
}

/// The input of the basic URL parser: ASCII tab and newline removed.
fn strip_tabs(input: &str) -> String {
    input
        .chars()
        .filter(|c| !matches!(c, '\t' | '\n' | '\r'))
        .collect()
}

/// The API setters of a URL (URL Standard §6.1): each changes `url` as
/// the standard says, or leaves it as it is when the standard's state
/// machine fails.
pub(crate) mod set {
    use url::Url;

    use super::{SpecialNonFile, strip_tabs};

    /// The scheme: the text up to the first `:` (the scheme state with a
    /// state override stops there).
    pub(crate) fn protocol(url: &mut Url, value: &str) {
        let value = strip_tabs(value);
        let scheme = value.split(':').next().unwrap_or_default();
        let _ = url.set_scheme(scheme);
    }

    pub(crate) fn username(url: &mut Url, value: &str) {
        let _ = url.set_username(value);
    }

    pub(crate) fn password(url: &mut Url, value: &str) {
        let _ = url.set_password(if value.is_empty() { None } else { Some(value) });
    }

    /// Splits the input of the host setters at the end of the host: `/`,
    /// `?`, `#` and, for special schemes, `\`.
    fn host_input(url: &Url, value: &str) -> String {
        let special = matches!(
            url.scheme(),
            "http" | "https" | "ws" | "wss" | "ftp" | "file"
        );
        let value = strip_tabs(value);
        let end = value
            .find(|c: char| matches!(c, '/' | '?' | '#') || (special && c == '\\'))
            .unwrap_or(value.len());
        value[..end].to_owned()
    }

    /// URL Standard, host state with a state override: an empty host is
    /// refused when the URL has credentials or a port.
    fn has_credentials_or_port(url: &Url) -> bool {
        !url.username().is_empty() || url.password().is_some() || url.port().is_some()
    }

    /// The position of the `:` that starts the port, outside `[...]`.
    fn port_colon(host: &str) -> Option<usize> {
        let mut inside = false;
        for (i, c) in host.char_indices() {
            match c {
                '[' => inside = true,
                ']' => inside = false,
                ':' if !inside => return Some(i),
                _ => {}
            }
        }
        None
    }

    /// `host`: the host, and a port if the input has one.
    pub(crate) fn host(url: &mut Url, value: &str) {
        if url.cannot_be_a_base() {
            return;
        }
        let input = host_input(url, value);
        let (host, port) = match port_colon(&input) {
            Some(i) => (&input[..i], Some(&input[i + 1..])),
            None => (&input[..], None),
        };
        if host.is_empty() && (url.is_special_non_file() || has_credentials_or_port(url)) {
            return;
        }
        if url.set_host(Some(host)).is_err() {
            return;
        }
        if let Some(port) = port {
            port_digits(url, port);
        }
    }

    /// `hostname`: a `:` outside brackets ends the input without a change.
    pub(crate) fn hostname(url: &mut Url, value: &str) {
        if url.cannot_be_a_base() {
            return;
        }
        let input = host_input(url, value);
        if port_colon(&input).is_some() {
            return;
        }
        if input.is_empty() && (url.is_special_non_file() || has_credentials_or_port(url)) {
            return;
        }
        let _ = url.set_host(Some(&input));
    }

    /// The port state with a state override: the leading digits, at most
    /// 65535.
    fn port_digits(url: &mut Url, text: &str) {
        let digits: String = text.chars().take_while(char::is_ascii_digit).collect();
        if digits.is_empty() {
            return;
        }
        if let Ok(port) = digits.trim_start_matches('0').parse::<u32>().or_else(|e| {
            if digits.chars().all(|c| c == '0') {
                Ok(0)
            } else {
                Err(e)
            }
        }) && let Ok(port) = u16::try_from(port)
        {
            let _ = url.set_port(Some(port));
        }
    }

    pub(crate) fn port(url: &mut Url, value: &str) {
        if url.host().is_none() || url.cannot_be_a_base() || url.scheme() == "file" {
            return;
        }
        let value = strip_tabs(value);
        if value.is_empty() {
            let _ = url.set_port(None);
            return;
        }
        port_digits(url, &value);
    }

    pub(crate) fn pathname(url: &mut Url, value: &str) {
        if url.cannot_be_a_base() {
            return;
        }
        url.set_path(&strip_tabs(value));
    }

    pub(crate) fn search(url: &mut Url, value: &str) {
        if value.is_empty() {
            url.set_query(None);
            return;
        }
        let value = strip_tabs(value);
        // Chromium (measured) removes the query for "?", where the standard
        // sets an empty one.
        match value.strip_prefix('?').unwrap_or(&value) {
            "" => url.set_query(None),
            query => url.set_query(Some(&query.replace('\'', "%27"))),
        }
    }

    /// `location.hash`: an empty value keeps an empty fragment (the URL ends
    /// with `#`), unlike `URL.hash`.
    pub(crate) fn location_hash(url: &mut Url, value: &str) {
        let value = strip_tabs(value);
        url.set_fragment(Some(value.strip_prefix('#').unwrap_or(&value)));
    }

    /// True if the scheme state accepts the start of `value`: a letter, then
    /// letters, digits, `+`, `-` and `.`, up to the first `:`.
    pub(crate) fn valid_scheme(value: &str) -> bool {
        let scheme = value.split(':').next().unwrap_or_default();
        let mut chars = scheme.chars();
        chars.next().is_some_and(|c| c.is_ascii_alphabetic())
            && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
    }

    pub(crate) fn hash(url: &mut Url, value: &str) {
        if value.is_empty() {
            url.set_fragment(None);
            return;
        }
        let value = strip_tabs(value);
        url.set_fragment(Some(value.strip_prefix('#').unwrap_or(&value)));
    }
}

/// `url::Url` has no "special but not file" test; this adds one.
trait SpecialNonFile {
    fn is_special_non_file(&self) -> bool;
}

impl SpecialNonFile for Url {
    fn is_special_non_file(&self) -> bool {
        matches!(self.scheme(), "http" | "https" | "ws" | "wss" | "ftp")
    }
}

// ----- URL -----

/// The URL parser with an optional base (URL Standard §6.1, the
/// constructor steps); `None` if either fails.
fn parse_with_base(input: &str, base: Option<&str>) -> Result<Url, ParseFailure> {
    let base = match base {
        Some(base) => Some(Url::parse(base).map_err(|_| ParseFailure::Base)?),
        None => None,
    };
    let options = Url::options().base_url(base.as_ref());
    options.parse(input).map_err(|_| ParseFailure::Url)
}

enum ParseFailure {
    Base,
    Url,
}

impl g::url::URLImpl for WebState {
    fn create_object_url(cx: &mut Cx<'_>, obj: Platform) -> b::Result<String> {
        crate::file_api::create_object_url(cx, obj)
    }

    fn revoke_object_url(cx: &mut Cx<'_>, url: String) -> b::Result<()> {
        crate::file_api::revoke_object_url(cx, &url)
    }

    fn constructor(cx: &mut Cx<'_>, url: String, base: Option<String>) -> b::Result<Gc<Object>> {
        let parsed = match parse_with_base(&url, base.as_deref()) {
            Ok(parsed) => parsed,
            Err(ParseFailure::Base) => return Err(Exception::type_error("Invalid base URL")),
            Err(ParseFailure::Url) => return Err(Exception::type_error("Invalid URL")),
        };
        make(cx, parsed)
    }

    fn parse(cx: &mut Cx<'_>, url: String, base: Option<String>) -> b::Result<Option<Gc<Object>>> {
        match parse_with_base(&url, base.as_deref()) {
            Ok(parsed) => {
                let data = cx.rt.alloc_host_data(UrlData {
                    url: parsed,
                    search_params: None,
                })?;
                Ok(Some(cx.rt.new_host_object(cx.realm(), g::URL, data)?))
            }
            Err(_) => Ok(None),
        }
    }

    fn can_parse(_cx: &mut Cx<'_>, url: String, base: Option<String>) -> b::Result<bool> {
        Ok(parse_with_base(&url, base.as_deref()).is_ok())
    }

    fn href(cx: &mut Cx<'_>, this: Platform) -> b::Result<String> {
        Ok(data(cx.rt, this)?.url.as_str().to_owned())
    }

    fn set_href(cx: &mut Cx<'_>, this: Platform, value: String) -> b::Result<()> {
        let parsed = Url::parse(&value).map_err(|_| Exception::type_error("Invalid URL"))?;
        data_mut(cx.rt, this)?.url = parsed;
        refresh_list(cx.rt, this)
    }

    fn origin(cx: &mut Cx<'_>, this: Platform) -> b::Result<String> {
        Ok(get::origin(&data(cx.rt, this)?.url))
    }

    fn protocol(cx: &mut Cx<'_>, this: Platform) -> b::Result<String> {
        Ok(get::protocol(&data(cx.rt, this)?.url))
    }

    fn set_protocol(cx: &mut Cx<'_>, this: Platform, value: String) -> b::Result<()> {
        set::protocol(&mut data_mut(cx.rt, this)?.url, &value);
        Ok(())
    }

    fn username(cx: &mut Cx<'_>, this: Platform) -> b::Result<String> {
        Ok(data(cx.rt, this)?.url.username().to_owned())
    }

    fn set_username(cx: &mut Cx<'_>, this: Platform, value: String) -> b::Result<()> {
        set::username(&mut data_mut(cx.rt, this)?.url, &value);
        Ok(())
    }

    fn password(cx: &mut Cx<'_>, this: Platform) -> b::Result<String> {
        Ok(data(cx.rt, this)?
            .url
            .password()
            .unwrap_or_default()
            .to_owned())
    }

    fn set_password(cx: &mut Cx<'_>, this: Platform, value: String) -> b::Result<()> {
        set::password(&mut data_mut(cx.rt, this)?.url, &value);
        Ok(())
    }

    fn host(cx: &mut Cx<'_>, this: Platform) -> b::Result<String> {
        Ok(get::host(&data(cx.rt, this)?.url))
    }

    fn set_host(cx: &mut Cx<'_>, this: Platform, value: String) -> b::Result<()> {
        set::host(&mut data_mut(cx.rt, this)?.url, &value);
        Ok(())
    }

    fn hostname(cx: &mut Cx<'_>, this: Platform) -> b::Result<String> {
        Ok(get::hostname(&data(cx.rt, this)?.url))
    }

    fn set_hostname(cx: &mut Cx<'_>, this: Platform, value: String) -> b::Result<()> {
        set::hostname(&mut data_mut(cx.rt, this)?.url, &value);
        Ok(())
    }

    fn port(cx: &mut Cx<'_>, this: Platform) -> b::Result<String> {
        Ok(get::port(&data(cx.rt, this)?.url))
    }

    fn set_port(cx: &mut Cx<'_>, this: Platform, value: String) -> b::Result<()> {
        set::port(&mut data_mut(cx.rt, this)?.url, &value);
        Ok(())
    }

    fn pathname(cx: &mut Cx<'_>, this: Platform) -> b::Result<String> {
        Ok(data(cx.rt, this)?.url.path().to_owned())
    }

    fn set_pathname(cx: &mut Cx<'_>, this: Platform, value: String) -> b::Result<()> {
        set::pathname(&mut data_mut(cx.rt, this)?.url, &value);
        Ok(())
    }

    fn search(cx: &mut Cx<'_>, this: Platform) -> b::Result<String> {
        Ok(get::search(&data(cx.rt, this)?.url))
    }

    fn set_search(cx: &mut Cx<'_>, this: Platform, value: String) -> b::Result<()> {
        set::search(&mut data_mut(cx.rt, this)?.url, &value);
        refresh_list(cx.rt, this)
    }

    fn search_params(cx: &mut Cx<'_>, this: Platform) -> b::Result<Gc<Object>> {
        if let Some(object) = data(cx.rt, this)?.search_params {
            return Ok(object);
        }
        let list = parse_query(data(cx.rt, this)?.url.query().unwrap_or_default());
        let handle = cx.rt.alloc_host_data(ParamsData {
            list,
            url: Some(this.object),
        })?;
        let object = cx
            .rt
            .new_host_object(cx.realm(), g::URL_SEARCH_PARAMS, handle)?;
        data_mut(cx.rt, this)?.search_params = Some(object);
        Ok(object)
    }

    fn hash(cx: &mut Cx<'_>, this: Platform) -> b::Result<String> {
        Ok(get::hash(&data(cx.rt, this)?.url))
    }

    fn set_hash(cx: &mut Cx<'_>, this: Platform, value: String) -> b::Result<()> {
        set::hash(&mut data_mut(cx.rt, this)?.url, &value);
        Ok(())
    }

    fn to_json(cx: &mut Cx<'_>, this: Platform) -> b::Result<String> {
        Ok(data(cx.rt, this)?.url.as_str().to_owned())
    }
}

/// A new `URL` object for the constructor.
fn make(cx: &mut Cx<'_>, url: Url) -> b::Result<Gc<Object>> {
    let data = cx.rt.alloc_host_data(UrlData {
        url,
        search_params: None,
    })?;
    Ok(cx.new_instance::<WebState>(data)?)
}

/// The URL's query changed: the list of its `searchParams` follows.
fn refresh_list(rt: &mut Runtime, this: Platform) -> b::Result<()> {
    let (query, object) = {
        let data = data(rt, this)?;
        (
            data.url.query().unwrap_or_default().to_owned(),
            data.search_params,
        )
    };
    if let Some(object) = object
        && let Some((_, HostData::Generic(handle))) = rt.host_object(object.into())
        && let Ok(list) = rt.heap_mut().generic_mut::<ParamsData>(handle)
    {
        list.list = parse_query(&query);
    }
    Ok(())
}

// ----- URLSearchParams -----

/// The `application/x-www-form-urlencoded` parser (URL Standard §5.1).
fn parse_query(query: &str) -> Vec<(String, String)> {
    url::form_urlencoded::parse(query.as_bytes())
        .map(|(name, value)| (name.into_owned(), value.into_owned()))
        .collect()
}

/// The serialization of the list of the `URLSearchParams` object `object`,
/// or `None` if it is another object (the body of a `fetch()`).
pub(crate) fn query_of(rt: &Runtime, object: Gc<Object>) -> Option<String> {
    let (_, HostData::Generic(handle)) = rt.host_object(object.into())? else {
        return None;
    };
    let data = rt.heap().generic::<ParamsData>(handle).ok()?;
    Some(serialize_list(&data.list))
}

/// The `application/x-www-form-urlencoded` serializer (URL Standard §5.1).
fn serialize_list(list: &[(String, String)]) -> String {
    let mut serializer = url::form_urlencoded::Serializer::new(String::new());
    for (name, value) in list {
        serializer.append_pair(name, value);
    }
    serializer.finish()
}

/// The update steps of `URLSearchParams`: the list changed, so the query
/// of the URL object follows.
fn update(rt: &mut Runtime, this: Platform) -> b::Result<()> {
    let (query, owner) = {
        let data = params(rt, this)?;
        (serialize_list(&data.list), data.url)
    };
    let Some(owner) = owner else {
        return Ok(());
    };
    if let Some((_, HostData::Generic(handle))) = rt.host_object(owner.into())
        && let Ok(data) = rt.heap_mut().generic_mut::<UrlData>(handle)
    {
        data.url
            .set_query(if query.is_empty() { None } else { Some(&query) });
    }
    Ok(())
}

/// The list of the constructor's `init` (URL Standard §6.2).
fn init_list(init: Init) -> b::Result<Vec<(String, String)>> {
    match init {
        Init::UsvStringSequenceSequence(sequences) => sequences
            .into_iter()
            .map(|pair| match <[String; 2]>::try_from(pair) {
                Ok([name, value]) => Ok((name, value)),
                // Chromium's message repeats its prefix (measured).
                Err(_) => Err(Exception::type_error(
                    "Failed to construct 'URLSearchParams': Sequence initializer must only contain pair elements",
                )),
            })
            .collect(),
        Init::UsvStringRecord(pairs) => Ok(pairs),
        Init::UsvString(text) => Ok(parse_query(text.strip_prefix('?').unwrap_or(&text))),
    }
}

/// UTF-16 code unit order (the order of `sort()`).
fn units_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    a.encode_utf16().cmp(b.encode_utf16())
}

impl g::url_search_params::URLSearchParamsImpl for WebState {
    fn constructor(cx: &mut Cx<'_>, init: Init) -> b::Result<Gc<Object>> {
        let list = init_list(init)?;
        let data = cx.rt.alloc_host_data(ParamsData { list, url: None })?;
        Ok(cx.new_instance::<WebState>(data)?)
    }

    fn size(cx: &mut Cx<'_>, this: Platform) -> b::Result<u32> {
        Ok(u32::try_from(params(cx.rt, this)?.list.len()).unwrap_or(u32::MAX))
    }

    fn append(cx: &mut Cx<'_>, this: Platform, name: String, value: String) -> b::Result<()> {
        params_mut(cx.rt, this)?.list.push((name, value));
        update(cx.rt, this)
    }

    fn delete(
        cx: &mut Cx<'_>,
        this: Platform,
        name: String,
        value: Option<String>,
    ) -> b::Result<()> {
        params_mut(cx.rt, this)?
            .list
            .retain(|(n, v)| !(*n == name && value.as_ref().is_none_or(|x| x == v)));
        update(cx.rt, this)
    }

    fn get(cx: &mut Cx<'_>, this: Platform, name: String) -> b::Result<Option<String>> {
        Ok(params(cx.rt, this)?
            .list
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, v)| v.clone()))
    }

    fn get_all(cx: &mut Cx<'_>, this: Platform, name: String) -> b::Result<Vec<String>> {
        Ok(params(cx.rt, this)?
            .list
            .iter()
            .filter(|(n, _)| *n == name)
            .map(|(_, v)| v.clone())
            .collect())
    }

    fn has(
        cx: &mut Cx<'_>,
        this: Platform,
        name: String,
        value: Option<String>,
    ) -> b::Result<bool> {
        Ok(params(cx.rt, this)?
            .list
            .iter()
            .any(|(n, v)| *n == name && value.as_ref().is_none_or(|x| x == v)))
    }

    fn set(cx: &mut Cx<'_>, this: Platform, name: String, value: String) -> b::Result<()> {
        {
            let list = &mut params_mut(cx.rt, this)?.list;
            match list.iter().position(|(n, _)| *n == name) {
                Some(first) => {
                    list[first].1 = value;
                    let mut index = 0;
                    list.retain(|(n, _)| {
                        index += 1;
                        index - 1 == first || *n != name
                    });
                }
                None => list.push((name, value)),
            }
        }
        update(cx.rt, this)
    }

    fn sort(cx: &mut Cx<'_>, this: Platform) -> b::Result<()> {
        params_mut(cx.rt, this)?
            .list
            .sort_by(|a, b| units_cmp(&a.0, &b.0));
        update(cx.rt, this)
    }

    fn stringify(cx: &mut Cx<'_>, this: Platform) -> b::Result<String> {
        Ok(serialize_list(&params(cx.rt, this)?.list))
    }

    fn pair_at(cx: &mut Cx<'_>, this: Platform, index: u32) -> b::Result<Option<(String, String)>> {
        Ok(params(cx.rt, this)?.list.get(index as usize).cloned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type Setter = fn(&mut Url, &str);

    fn u(text: &str) -> Url {
        Url::parse(text).unwrap()
    }

    #[test]
    fn getters() {
        let url = u("https://user:pw@example.com:8443/a/b?x=1#frag");
        assert_eq!(get::protocol(&url), "https:");
        assert_eq!(get::host(&url), "example.com:8443");
        assert_eq!(get::hostname(&url), "example.com");
        assert_eq!(get::port(&url), "8443");
        assert_eq!(get::search(&url), "?x=1");
        assert_eq!(get::hash(&url), "#frag");
        assert_eq!(get::origin(&url), "https://example.com:8443");
        assert_eq!(get::origin(&u("file:///a")), "file://");
        assert_eq!(get::origin(&u("foo://a/")), "null");
        assert_eq!(get::search(&u("https://a/?")), "");
    }

    #[test]
    fn setters() {
        let mut url = u("https://example.com/a?x=1#f");
        set::host(&mut url, "other.test:99/ignored");
        assert_eq!(url.as_str(), "https://other.test:99/a?x=1#f");
        set::hostname(&mut url, "a.test:5");
        assert_eq!(url.host_str(), Some("other.test"));
        set::port(&mut url, "8080abc");
        assert_eq!(url.port(), Some(8080));
        set::port(&mut url, "");
        assert_eq!(url.port(), None);
        set::port(&mut url, "70000");
        assert_eq!(url.port(), None);
        set::protocol(&mut url, "http:foo");
        assert_eq!(url.scheme(), "http");
        set::search(&mut url, "?q=a b");
        assert_eq!(url.query(), Some("q=a%20b"));
        set::search(&mut url, "");
        assert_eq!(url.query(), None);
        set::hash(&mut url, "#x y");
        assert_eq!(url.fragment(), Some("x%20y"));
        set::pathname(&mut url, "/p q");
        assert_eq!(url.path(), "/p%20q");
    }

    #[test]
    fn setters_leave_consistent_urls() {
        let bases = [
            "http://user:pw@a.com:8080/p/a?old=1#oldf",
            "https://a.com/x",
            "file:///dir/f.txt?q#h",
            "mailto:a@b.c?s=1#f",
            "foo://host:9/p?q#f",
            "data:text/plain,hello#frag",
        ];
        let values = [
            "",
            "x",
            "u:p@h",
            "h.test:81",
            "[::1]:3000",
            "a b",
            "https",
            "file",
            "foo",
            "//x",
            "\\x",
            "?q",
            "#f",
            "81",
            "99999",
            "p@q",
            ":",
            "@",
            "a@b:c",
        ];
        for base in bases {
            for value in values {
                let setters: [(&str, Setter); 9] = [
                    ("protocol", set::protocol),
                    ("username", set::username),
                    ("password", set::password),
                    ("host", set::host),
                    ("hostname", set::hostname),
                    ("port", set::port),
                    ("pathname", set::pathname),
                    ("search", set::search),
                    ("hash", set::hash),
                ];
                for (name, setter) in setters {
                    let mut url = u(base);
                    setter(&mut url, value);
                    let text = format!("{base} {name} = {value:?}");
                    // Every getter works on the result, and it parses again.
                    let _ = (url.username(), url.password(), url.host_str(), url.port());
                    let _ = (get::host(&url), get::hostname(&url), get::origin(&url));
                    assert!(Url::parse(url.as_str()).is_ok(), "{text}: {url}");
                }
            }
        }
    }

    #[test]
    fn query_lists() {
        assert_eq!(
            parse_query("a=1&b=%20x+y&a=2"),
            vec![
                ("a".to_owned(), "1".to_owned()),
                ("b".to_owned(), " x y".to_owned()),
                ("a".to_owned(), "2".to_owned())
            ]
        );
        assert_eq!(
            serialize_list(&[("a b".to_owned(), "é&".to_owned())]),
            "a+b=%C3%A9%26"
        );
        assert_eq!(units_cmp("\u{10000}", "\u{ffff}"), std::cmp::Ordering::Less);
    }
}
