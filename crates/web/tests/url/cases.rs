// The cases of tools/probes/web-url.json with the values of Chromium 148.
// Generated from `just probe --json` (three runs, all equal); do not edit.

use crate::common::Case;

pub(crate) const CASES: &[Case] = &[
    Case {
        name: "structure",
        html: r"<!DOCTYPE html><html><head></head><body><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : v; } catch (e) { return (e instanceof DOMException ? 'DOMException ' + e.name + ' [' + e.code + ']' : e.constructor.name) + ': ' + e.message; } }
function P(u) { return [u.href, u.origin, u.protocol, u.username, u.password, u.host, u.hostname, u.port, u.pathname, u.search, u.hash]; }
function S(init) { return [...new URLSearchParams(init)].map((p) => p.join('=')).join('&'); }
</script></body></html>",
        queries: &[
            (
                r"[typeof URL, URL.length, URL.name, typeof URLSearchParams, URLSearchParams.length, URLSearchParams.name]",
                r#"["function", 1, "URL", "function", 0, "URLSearchParams"]"#,
            ),
            (
                r"[Object.getOwnPropertyNames(URL.prototype).sort().join(), Object.getOwnPropertyNames(URL).filter((n) => !/ObjectURL/.test(n)).sort().join(), Object.getOwnPropertyNames(URLSearchParams.prototype).join(), Object.getOwnPropertyNames(URLSearchParams).join()]",
                r#"["constructor,hash,host,hostname,href,origin,password,pathname,port,protocol,search,searchParams,toJSON,toString,username", "canParse,length,name,parse,prototype", "size,append,delete,get,getAll,has,set,sort,toString,entries,forEach,keys,values,constructor", "length,name,prototype"]"#,
            ),
            (
                r"(() => { const d = Object.getOwnPropertyDescriptor(URL.prototype, 'href'); const o = Object.getOwnPropertyDescriptor(URL.prototype, 'origin'); const s = Object.getOwnPropertyDescriptor(URL.prototype, 'searchParams'); return [typeof d.get, typeof d.set, d.enumerable, d.configurable, typeof o.get, typeof o.set, typeof s.get, typeof s.set, d.get.name, d.set.name]; })()",
                r#"["function", "function", true, true, "function", "undefined", "function", "undefined", "get href", "set href"]"#,
            ),
            (
                r"[Object.prototype.toString.call(new URL('http://a/')), Object.prototype.toString.call(new URLSearchParams()), URL.prototype[Symbol.toStringTag], URLSearchParams.prototype[Symbol.toStringTag], String(Object.getOwnPropertyDescriptor(URLSearchParams.prototype, Symbol.iterator).value === URLSearchParams.prototype.entries)]",
                r#"["[object URL]", "[object URLSearchParams]", "URL", "URLSearchParams", "true"]"#,
            ),
            (
                r"[URL.canParse.length, URL.parse.length, URL.prototype.toJSON.length, URLSearchParams.prototype.append.length, URLSearchParams.prototype.delete.length, URLSearchParams.prototype.get.length, URLSearchParams.prototype.has.length, URLSearchParams.prototype.set.length, URLSearchParams.prototype.sort.length, URLSearchParams.prototype.forEach.length, URLSearchParams.prototype.entries.length]",
                r"[1, 1, 0, 2, 1, 1, 1, 2, 0, 1, 0]",
            ),
            (
                r"E(() => URL('http://a/'))",
                r#""TypeError: Failed to construct 'URL': Please use the 'new' operator, this DOM object constructor cannot be called as a function.""#,
            ),
            (
                r"E(() => URLSearchParams())",
                r#""TypeError: Failed to construct 'URLSearchParams': Please use the 'new' operator, this DOM object constructor cannot be called as a function.""#,
            ),
            (
                r"E(() => new URL())",
                r#""TypeError: Failed to construct 'URL': 1 argument required, but only 0 present.""#,
            ),
            (
                r"E(() => new URL('http://a/', undefined).href)",
                r#""http://a/""#,
            ),
            (
                r"E(() => new URL.prototype.constructor('http://a/').href)",
                r#""http://a/""#,
            ),
            (
                r"E(() => URL.prototype.href)",
                r#""TypeError: Illegal invocation""#,
            ),
            (
                r"E(() => Object.getOwnPropertyDescriptor(URL.prototype, 'href').get.call({}))",
                r#""TypeError: Illegal invocation""#,
            ),
            (
                r"E(() => URLSearchParams.prototype.get.call({}, 'a'))",
                r#""TypeError: Illegal invocation""#,
            ),
            (
                r"E(() => { class U extends URL {} const u = new U('http://a/'); return [u instanceof U, u instanceof URL, u.href, Object.getPrototypeOf(u) === U.prototype]; })",
                r#"[true, true, "http://a/", true]"#,
            ),
            (
                r"E(() => { class U extends URLSearchParams {} const u = new U('a=1'); return [u instanceof U, u.get('a')]; })",
                r#"[true, "1"]"#,
            ),
            (
                r"(() => { const u = new URL('http://a/?x=1'); u.foo = 1; return [Object.keys(u).join(), JSON.stringify(u), String(u), u + '', `${u}`, u.toString === Object.prototype.toString]; })()",
                r#"["foo", "\"http://a/?x=1\"", "http://a/?x=1", "http://a/?x=1", "http://a/?x=1", false]"#,
            ),
            (
                r"(() => { const u = new URL('http://a/?x=1'); return [JSON.stringify({u}), u.toJSON(), JSON.stringify(new URLSearchParams('a=1')), String(new URLSearchParams('a=1&b=2'))]; })()",
                r#"["{\"u\":\"http://a/?x=1\"}", "http://a/?x=1", "{}", "a=1&b=2"]"#,
            ),
            (
                r"(() => { const u = new URL('http://a/'); return [u.searchParams === u.searchParams, u.searchParams instanceof URLSearchParams]; })()",
                r"[true, true]",
            ),
            (
                r"(() => { const u = new URL('http://a/'); return Object.getOwnPropertyNames(u).length; })()",
                r"0",
            ),
        ],
    },
    Case {
        name: "parse-errors",
        html: r"<!DOCTYPE html><html><head></head><body><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : v; } catch (e) { return (e instanceof DOMException ? 'DOMException ' + e.name + ' [' + e.code + ']' : e.constructor.name) + ': ' + e.message; } }
function P(u) { return [u.href, u.origin, u.protocol, u.username, u.password, u.host, u.hostname, u.port, u.pathname, u.search, u.hash]; }
function S(init) { return [...new URLSearchParams(init)].map((p) => p.join('=')).join('&'); }
</script></body></html>",
        queries: &[
            (
                r"E(() => new URL('foo'))",
                r#""TypeError: Failed to construct 'URL': Invalid URL""#,
            ),
            (
                r"E(() => new URL(''))",
                r#""TypeError: Failed to construct 'URL': Invalid URL""#,
            ),
            (
                r"E(() => new URL('http://'))",
                r#""TypeError: Failed to construct 'URL': Invalid URL""#,
            ),
            (r"E(() => new URL('http://a b/'))", r"error: Invalid URL"),
            (
                r"E(() => new URL('http://[1::2'))",
                r#""TypeError: Failed to construct 'URL': Invalid URL""#,
            ),
            (
                r"E(() => new URL('/rel', 'foo'))",
                r#""TypeError: Failed to construct 'URL': Invalid base URL""#,
            ),
            (
                r"E(() => new URL('rel', 'http://[bad'))",
                r#""TypeError: Failed to construct 'URL': Invalid base URL""#,
            ),
            (
                r"E(() => new URL('rel', 'x'))",
                r#""TypeError: Failed to construct 'URL': Invalid base URL""#,
            ),
            (
                r"E(() => new URL('http://a:99999/'))",
                r#""TypeError: Failed to construct 'URL': Invalid URL""#,
            ),
            (
                r"E(() => new URL('http://a/', 'not a url').href)",
                r#""TypeError: Failed to construct 'URL': Invalid base URL""#,
            ),
            (
                r"E(() => new URL('http://a/', null).href)",
                r#""TypeError: Failed to construct 'URL': Invalid base URL""#,
            ),
            (
                r"E(() => new URL('http://a/', {toString() { return 'http://b/'; }}).href)",
                r#""http://a/""#,
            ),
            (
                r"E(() => new URL({toString() { return '/x'; }}, 'http://b/').href)",
                r#""http://b/x""#,
            ),
            (
                r"E(() => new URL(Symbol()))",
                r#""TypeError: Failed to construct 'URL': Cannot convert a Symbol value to a string""#,
            ),
            (
                r"E(() => new URL(undefined))",
                r#""TypeError: Failed to construct 'URL': Invalid URL""#,
            ),
            (
                r"E(() => new URL(null).href)",
                r#""TypeError: Failed to construct 'URL': Invalid URL""#,
            ),
            (
                r"E(() => new URL('http://\ud800/'))",
                r#""TypeError: Failed to construct 'URL': Invalid URL""#,
            ),
            (
                r"[URL.canParse('http://a/'), URL.canParse('foo'), URL.canParse('/x', 'http://a/'), URL.canParse('/x', 'bad'), URL.canParse('http://a/', 'bad'), URL.canParse('', 'http://a/')]",
                r"[true, false, true, false, false, true]",
            ),
            (
                r"E(() => URL.canParse())",
                r#""TypeError: Failed to execute 'canParse' on 'URL': 1 argument required, but only 0 present.""#,
            ),
            (r"E(() => URL.canParse('x', undefined))", r"false"),
            (
                r"[URL.parse('http://a/').href, URL.parse('foo'), URL.parse('/x', 'http://a/').href, URL.parse('/x', 'bad'), URL.parse('x', undefined)]",
                r#"["http://a/", null, "http://a/x", null, null]"#,
            ),
            (
                r"E(() => URL.parse())",
                r#""TypeError: Failed to execute 'parse' on 'URL': 1 argument required, but only 0 present.""#,
            ),
            (
                r"E(() => URL.parse.call(null, 'http://a/').href)",
                r#""http://a/""#,
            ),
            (
                r"E(() => new URL('http://a/').constructor === URL)",
                r"true",
            ),
        ],
    },
    Case {
        name: "parse-special",
        html: r"<!DOCTYPE html><html><head></head><body><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : v; } catch (e) { return (e instanceof DOMException ? 'DOMException ' + e.name + ' [' + e.code + ']' : e.constructor.name) + ': ' + e.message; } }
function P(u) { return [u.href, u.origin, u.protocol, u.username, u.password, u.host, u.hostname, u.port, u.pathname, u.search, u.hash]; }
function S(init) { return [...new URLSearchParams(init)].map((p) => p.join('=')).join('&'); }
</script></body></html>",
        queries: &[
            (
                r#"E(() => P(new URL("http://example.com/")))"#,
                r#"["http://example.com/", "http://example.com", "http:", "", "", "example.com", "example.com", "", "/", "", ""]"#,
            ),
            (
                r#"E(() => P(new URL("HTTP://EXAMPLE.COM:80/a/./b/../c?q=1#f")))"#,
                r##"["http://example.com/a/c?q=1#f", "http://example.com", "http:", "", "", "example.com", "example.com", "", "/a/c", "?q=1", "#f"]"##,
            ),
            (
                r#"E(() => P(new URL("https://user:pass@Example.COM:8443/p?x#y")))"#,
                r##"["https://user:pass@example.com:8443/p?x#y", "https://example.com:8443", "https:", "user", "pass", "example.com:8443", "example.com", "8443", "/p", "?x", "#y"]"##,
            ),
            (
                r#"E(() => P(new URL("https://user@example.com")))"#,
                r#"["https://user@example.com/", "https://example.com", "https:", "user", "", "example.com", "example.com", "", "/", "", ""]"#,
            ),
            (
                r#"E(() => P(new URL("https://:pw@example.com")))"#,
                r#"["https://:pw@example.com/", "https://example.com", "https:", "", "pw", "example.com", "example.com", "", "/", "", ""]"#,
            ),
            (
                r#"E(() => P(new URL("http://example.com:0/")))"#,
                r#"["http://example.com:0/", "http://example.com:0", "http:", "", "", "example.com:0", "example.com", "0", "/", "", ""]"#,
            ),
            (
                r#"E(() => P(new URL("http://example.com:65535/")))"#,
                r#"["http://example.com:65535/", "http://example.com:65535", "http:", "", "", "example.com:65535", "example.com", "65535", "/", "", ""]"#,
            ),
            (
                r#"E(() => P(new URL("https://example.com:443/")))"#,
                r#"["https://example.com/", "https://example.com", "https:", "", "", "example.com", "example.com", "", "/", "", ""]"#,
            ),
            (
                r#"E(() => P(new URL("ws://a:80/")))"#,
                r#"["ws://a/", "ws://a", "ws:", "", "", "a", "a", "", "/", "", ""]"#,
            ),
            (
                r#"E(() => P(new URL("wss://a:443/x")))"#,
                r#"["wss://a/x", "wss://a", "wss:", "", "", "a", "a", "", "/x", "", ""]"#,
            ),
            (
                r#"E(() => P(new URL("ftp://a:21/x")))"#,
                r#"["ftp://a/x", "ftp://a", "ftp:", "", "", "a", "a", "", "/x", "", ""]"#,
            ),
            (
                r#"E(() => P(new URL("http://a.com/a b/c%20d/\u00e9?q=a b&r=\u00e9#f g \u00e9")))"#,
                r##"["http://a.com/a%20b/c%20d/%C3%A9?q=a%20b&r=%C3%A9#f%20g%20%C3%A9", "http://a.com", "http:", "", "", "a.com", "a.com", "", "/a%20b/c%20d/%C3%A9", "?q=a%20b&r=%C3%A9", "#f%20g%20%C3%A9"]"##,
            ),
            (
                r#"E(() => P(new URL("http://a.com/?q='\"<>`")))"#,
                r#"["http://a.com/?q=%27%22%3C%3E`", "http://a.com", "http:", "", "", "a.com", "a.com", "", "/", "?q=%27%22%3C%3E`", ""]"#,
            ),
            (
                r#"E(() => P(new URL("http://a.com/#'\"<>` ")))"#,
                r##"["http://a.com/#'%22%3C%3E%60", "http://a.com", "http:", "", "", "a.com", "a.com", "", "/", "", "#'%22%3C%3E%60"]"##,
            ),
            (
                r#"E(() => P(new URL("http://a.com/\\\\x\\\\y")))"#,
                r#"["http://a.com///x//y", "http://a.com", "http:", "", "", "a.com", "a.com", "", "///x//y", "", ""]"#,
            ),
            (
                r#"E(() => P(new URL("http:\\\\\\\\a.com\\\\x")))"#,
                r#"["http://a.com//x", "http://a.com", "http:", "", "", "a.com", "a.com", "", "//x", "", ""]"#,
            ),
            (
                r#"E(() => P(new URL("http:a.com/x")))"#,
                r#"["http://a.com/x", "http://a.com", "http:", "", "", "a.com", "a.com", "", "/x", "", ""]"#,
            ),
            (
                r#"E(() => P(new URL("http:/a.com/x")))"#,
                r#"["http://a.com/x", "http://a.com", "http:", "", "", "a.com", "a.com", "", "/x", "", ""]"#,
            ),
            (
                r#"E(() => P(new URL("http:///a.com/x")))"#,
                r#"["http://a.com/x", "http://a.com", "http:", "", "", "a.com", "a.com", "", "/x", "", ""]"#,
            ),
            (
                r#"E(() => P(new URL("  http://a.com/x \n")))"#,
                r#"["http://a.com/x", "http://a.com", "http:", "", "", "a.com", "a.com", "", "/x", "", ""]"#,
            ),
            (
                r#"E(() => P(new URL("htt\tp://a.\ncom/\rx")))"#,
                r#"["http://a.com/x", "http://a.com", "http:", "", "", "a.com", "a.com", "", "/x", "", ""]"#,
            ),
            (
                r#"E(() => P(new URL("http://a.com/%zz/%41/%2e/%2E%2e/x")))"#,
                r#"["http://a.com/%zz/x", "http://a.com", "http:", "", "", "a.com", "a.com", "", "/%zz/x", "", ""]"#,
            ),
            (
                r#"E(() => P(new URL("http://a.com/a//b///c")))"#,
                r#"["http://a.com/a//b///c", "http://a.com", "http:", "", "", "a.com", "a.com", "", "/a//b///c", "", ""]"#,
            ),
            (
                r#"E(() => P(new URL("http://a.com/..")))"#,
                r#"["http://a.com/", "http://a.com", "http:", "", "", "a.com", "a.com", "", "/", "", ""]"#,
            ),
            (
                r#"E(() => P(new URL("http://a.com/a/../../..")))"#,
                r#"["http://a.com/", "http://a.com", "http:", "", "", "a.com", "a.com", "", "/", "", ""]"#,
            ),
            (
                r#"E(() => P(new URL("http://a.com")))"#,
                r#"["http://a.com/", "http://a.com", "http:", "", "", "a.com", "a.com", "", "/", "", ""]"#,
            ),
            (
                r#"E(() => P(new URL("http://a.com?x")))"#,
                r#"["http://a.com/?x", "http://a.com", "http:", "", "", "a.com", "a.com", "", "/", "?x", ""]"#,
            ),
            (
                r#"E(() => P(new URL("http://a.com#x")))"#,
                r##"["http://a.com/#x", "http://a.com", "http:", "", "", "a.com", "a.com", "", "/", "", "#x"]"##,
            ),
            (
                r#"E(() => P(new URL("http://m\u00fcnchen.de/")))"#,
                r#"["http://xn--mnchen-3ya.de/", "http://xn--mnchen-3ya.de", "http:", "", "", "xn--mnchen-3ya.de", "xn--mnchen-3ya.de", "", "/", "", ""]"#,
            ),
            (
                r#"E(() => P(new URL("http://xn--mnchen-3ya.de/")))"#,
                r#"["http://xn--mnchen-3ya.de/", "http://xn--mnchen-3ya.de", "http:", "", "", "xn--mnchen-3ya.de", "xn--mnchen-3ya.de", "", "/", "", ""]"#,
            ),
            (
                r#"E(() => P(new URL("http://EXAMPLE.com./")))"#,
                r#"["http://example.com./", "http://example.com.", "http:", "", "", "example.com.", "example.com.", "", "/", "", ""]"#,
            ),
            (
                r#"E(() => P(new URL("http://a%2eb.com/")))"#,
                r#"["http://a.b.com/", "http://a.b.com", "http:", "", "", "a.b.com", "a.b.com", "", "/", "", ""]"#,
            ),
            (
                r#"E(() => P(new URL("http://127.0.0.1/")))"#,
                r#"["http://127.0.0.1/", "http://127.0.0.1", "http:", "", "", "127.0.0.1", "127.0.0.1", "", "/", "", ""]"#,
            ),
            (
                r#"E(() => P(new URL("http://0x7f.1/")))"#,
                r#"["http://127.0.0.1/", "http://127.0.0.1", "http:", "", "", "127.0.0.1", "127.0.0.1", "", "/", "", ""]"#,
            ),
            (
                r#"E(() => P(new URL("http://017700000001/")))"#,
                r#"["http://127.0.0.1/", "http://127.0.0.1", "http:", "", "", "127.0.0.1", "127.0.0.1", "", "/", "", ""]"#,
            ),
            (
                r#"E(() => P(new URL("http://2130706433/")))"#,
                r#"["http://127.0.0.1/", "http://127.0.0.1", "http:", "", "", "127.0.0.1", "127.0.0.1", "", "/", "", ""]"#,
            ),
            (
                r#"E(() => P(new URL("http://1.2.3/")))"#,
                r#"["http://1.2.0.3/", "http://1.2.0.3", "http:", "", "", "1.2.0.3", "1.2.0.3", "", "/", "", ""]"#,
            ),
            (
                r#"E(() => P(new URL("http://[::1]/")))"#,
                r#"["http://[::1]/", "http://[::1]", "http:", "", "", "[::1]", "[::1]", "", "/", "", ""]"#,
            ),
            (
                r#"E(() => P(new URL("http://[2001:DB8::0:1]:8080/")))"#,
                r#"["http://[2001:db8::1]:8080/", "http://[2001:db8::1]:8080", "http:", "", "", "[2001:db8::1]:8080", "[2001:db8::1]", "8080", "/", "", ""]"#,
            ),
            (
                r#"E(() => P(new URL("http://[::ffff:1.2.3.4]/")))"#,
                r#"["http://[::ffff:102:304]/", "http://[::ffff:102:304]", "http:", "", "", "[::ffff:102:304]", "[::ffff:102:304]", "", "/", "", ""]"#,
            ),
            (
                r#"E(() => P(new URL("http://a.com:/x")))"#,
                r#"["http://a.com/x", "http://a.com", "http:", "", "", "a.com", "a.com", "", "/x", "", ""]"#,
            ),
            (
                r#"E(() => P(new URL("http://a.com:080/x")))"#,
                r#"["http://a.com/x", "http://a.com", "http:", "", "", "a.com", "a.com", "", "/x", "", ""]"#,
            ),
            (
                r#"E(() => P(new URL("http://a_b.com/")))"#,
                r#"["http://a_b.com/", "http://a_b.com", "http:", "", "", "a_b.com", "a_b.com", "", "/", "", ""]"#,
            ),
            (
                r#"E(() => P(new URL("http://a..b/")))"#,
                r#"["http://a..b/", "http://a..b", "http:", "", "", "a..b", "a..b", "", "/", "", ""]"#,
            ),
            (
                r#"E(() => P(new URL("http://-a.com/")))"#,
                r#"["http://-a.com/", "http://-a.com", "http:", "", "", "-a.com", "-a.com", "", "/", "", ""]"#,
            ),
            (
                r#"E(() => P(new URL("http://a.com/\ud83d\ude00")))"#,
                r#"["http://a.com/%F0%9F%98%80", "http://a.com", "http:", "", "", "a.com", "a.com", "", "/%F0%9F%98%80", "", ""]"#,
            ),
            (
                r#"E(() => P(new URL("http://\ud83d\ude00.com/")))"#,
                r#"["http://xn--e28h.com/", "http://xn--e28h.com", "http:", "", "", "xn--e28h.com", "xn--e28h.com", "", "/", "", ""]"#,
            ),
            (
                r#"E(() => P(new URL("http://\uff21\uff22\uff23.com/")))"#,
                r#"["http://abc.com/", "http://abc.com", "http:", "", "", "abc.com", "abc.com", "", "/", "", ""]"#,
            ),
            (
                r#"E(() => P(new URL("http://a.com/?\ud83d\ude00#\ud83d\ude00")))"#,
                r##"["http://a.com/?%F0%9F%98%80#%F0%9F%98%80", "http://a.com", "http:", "", "", "a.com", "a.com", "", "/", "?%F0%9F%98%80", "#%F0%9F%98%80"]"##,
            ),
        ],
    },
    Case {
        name: "parse-other",
        html: r"<!DOCTYPE html><html><head></head><body><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : v; } catch (e) { return (e instanceof DOMException ? 'DOMException ' + e.name + ' [' + e.code + ']' : e.constructor.name) + ': ' + e.message; } }
function P(u) { return [u.href, u.origin, u.protocol, u.username, u.password, u.host, u.hostname, u.port, u.pathname, u.search, u.hash]; }
function S(init) { return [...new URLSearchParams(init)].map((p) => p.join('=')).join('&'); }
</script></body></html>",
        queries: &[
            (
                r#"E(() => P(new URL("file:///C:/a/b")))"#,
                r#"["file:///C:/a/b", "file://", "file:", "", "", "", "", "", "/C:/a/b", "", ""]"#,
            ),
            (
                r#"E(() => P(new URL("file://host/a")))"#,
                r#"["file://host/a", "file://", "file:", "", "", "host", "host", "", "/a", "", ""]"#,
            ),
            (
                r#"E(() => P(new URL("file:a")))"#,
                r#"["file:///a", "file://", "file:", "", "", "", "", "", "/a", "", ""]"#,
            ),
            (
                r#"E(() => P(new URL("file:///a/../b")))"#,
                r#"["file:///b", "file://", "file:", "", "", "", "", "", "/b", "", ""]"#,
            ),
            (
                r#"E(() => P(new URL("file://localhost/a")))"#,
                r#"["file://localhost/a", "file://", "file:", "", "", "localhost", "localhost", "", "/a", "", ""]"#,
            ),
            (
                r#"E(() => P(new URL("file:/a")))"#,
                r#"["file:///a", "file://", "file:", "", "", "", "", "", "/a", "", ""]"#,
            ),
            (
                r#"E(() => P(new URL("FILE:///A")))"#,
                r#"["file:///A", "file://", "file:", "", "", "", "", "", "/A", "", ""]"#,
            ),
            (
                r#"E(() => P(new URL("mailto:user@example.com?subject=hi")))"#,
                r#"["mailto:user@example.com?subject=hi", "null", "mailto:", "", "", "", "", "", "user@example.com", "?subject=hi", ""]"#,
            ),
            (
                r#"E(() => P(new URL("mailto:a b")))"#,
                r#"["mailto:a b", "null", "mailto:", "", "", "", "", "", "a b", "", ""]"#,
            ),
            (
                r#"E(() => P(new URL("javascript:alert(1)//x")))"#,
                r#"["javascript:alert(1)//x", "null", "javascript:", "", "", "", "", "", "alert(1)//x", "", ""]"#,
            ),
            (
                r#"E(() => P(new URL("javascript:void(0)#f")))"#,
                r##"["javascript:void(0)#f", "null", "javascript:", "", "", "", "", "", "void(0)", "", "#f"]"##,
            ),
            (
                r#"E(() => P(new URL("data:text/html,<b>hi</b>#frag")))"#,
                r##"["data:text/html,<b>hi</b>#frag", "null", "data:", "", "", "", "", "", "text/html,<b>hi</b>", "", "#frag"]"##,
            ),
            (
                r#"E(() => P(new URL("data:,a b")))"#,
                r#"["data:,a b", "null", "data:", "", "", "", "", "", ",a b", "", ""]"#,
            ),
            (
                r#"E(() => P(new URL("blob:http://a.com/uuid")))"#,
                r#"["blob:http://a.com/uuid", "http://a.com", "blob:", "", "", "", "", "", "http://a.com/uuid", "", ""]"#,
            ),
            (
                r#"E(() => P(new URL("blob:null/uuid")))"#,
                r#"["blob:null/uuid", "null", "blob:", "", "", "", "", "", "null/uuid", "", ""]"#,
            ),
            (
                r#"E(() => P(new URL("blob:foo")))"#,
                r#"["blob:foo", "null", "blob:", "", "", "", "", "", "foo", "", ""]"#,
            ),
            (
                r#"E(() => P(new URL("about:blank")))"#,
                r#"["about:blank", "null", "about:", "", "", "", "", "", "blank", "", ""]"#,
            ),
            (
                r#"E(() => P(new URL("about:blank?x#y")))"#,
                r##"["about:blank?x#y", "null", "about:", "", "", "", "", "", "blank", "?x", "#y"]"##,
            ),
            (
                r#"E(() => P(new URL("tel:+1-555")))"#,
                r#"["tel:+1-555", "null", "tel:", "", "", "", "", "", "+1-555", "", ""]"#,
            ),
            (
                r#"E(() => P(new URL("foo://bar/baz?q#f")))"#,
                r##"["foo://bar/baz?q#f", "null", "foo:", "", "", "bar", "bar", "", "/baz", "?q", "#f"]"##,
            ),
            (
                r#"E(() => P(new URL("foo://Bar:80/baz")))"#,
                r#"["foo://Bar:80/baz", "null", "foo:", "", "", "Bar:80", "Bar", "80", "/baz", "", ""]"#,
            ),
            (
                r#"E(() => P(new URL("foo://a b/")))"#,
                r#""TypeError: Failed to construct 'URL': Invalid URL""#,
            ),
            (
                r#"E(() => P(new URL("foo:/a/b/../c")))"#,
                r#"["foo:/a/c", "null", "foo:", "", "", "", "", "", "/a/c", "", ""]"#,
            ),
            (
                r#"E(() => P(new URL("foo:a/b/../c")))"#,
                r#"["foo:a/b/../c", "null", "foo:", "", "", "", "", "", "a/b/../c", "", ""]"#,
            ),
            (
                r#"E(() => P(new URL("foo:///x")))"#,
                r#"["foo:///x", "null", "foo:", "", "", "", "", "", "/x", "", ""]"#,
            ),
            (
                r#"E(() => P(new URL("foo://")))"#,
                r#"["foo://", "null", "foo:", "", "", "", "", "", "", "", ""]"#,
            ),
            (
                r#"E(() => P(new URL("foo:")))"#,
                r#"["foo:", "null", "foo:", "", "", "", "", "", "", "", ""]"#,
            ),
            (
                r#"E(() => P(new URL("foo:?q")))"#,
                r#"["foo:?q", "null", "foo:", "", "", "", "", "", "", "?q", ""]"#,
            ),
            (
                r#"E(() => P(new URL("foo:#f")))"#,
                r##"["foo:#f", "null", "foo:", "", "", "", "", "", "", "", "#f"]"##,
            ),
            (
                r#"E(() => P(new URL("a+b-c.d:rest")))"#,
                r#"["a+b-c.d:rest", "null", "a+b-c.d:", "", "", "", "", "", "rest", "", ""]"#,
            ),
            (
                r#"E(() => P(new URL("git+ssh://u@h/p.git")))"#,
                r#"["git+ssh://u@h/p.git", "null", "git+ssh:", "u", "", "h", "h", "", "/p.git", "", ""]"#,
            ),
            (
                r#"E(() => P(new URL("chrome://settings/")))"#,
                r#"["chrome://settings/", "chrome://settings", "chrome:", "", "", "settings", "settings", "", "/", "", ""]"#,
            ),
            (
                r#"E(() => P(new URL("view-source:http://a/")))"#,
                r#"["view-source:http://a/", "null", "view-source:", "", "", "", "", "", "http://a/", "", ""]"#,
            ),
            (
                r#"E(() => P(new URL("http://a/", "foo:bar")))"#,
                r#"["http://a/", "http://a", "http:", "", "", "a", "a", "", "/", "", ""]"#,
            ),
        ],
    },
    Case {
        name: "parse-relative",
        html: r"<!DOCTYPE html><html><head></head><body><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : v; } catch (e) { return (e instanceof DOMException ? 'DOMException ' + e.name + ' [' + e.code + ']' : e.constructor.name) + ': ' + e.message; } }
function P(u) { return [u.href, u.origin, u.protocol, u.username, u.password, u.host, u.hostname, u.port, u.pathname, u.search, u.hash]; }
function S(init) { return [...new URLSearchParams(init)].map((p) => p.join('=')).join('&'); }
</script></body></html>",
        queries: &[
            (
                r#"E(() => P(new URL("x", "http://a.com/b/c")))"#,
                r#"["http://a.com/b/x", "http://a.com", "http:", "", "", "a.com", "a.com", "", "/b/x", "", ""]"#,
            ),
            (
                r#"E(() => P(new URL("/x", "http://a.com/b/c")))"#,
                r#"["http://a.com/x", "http://a.com", "http:", "", "", "a.com", "a.com", "", "/x", "", ""]"#,
            ),
            (
                r#"E(() => P(new URL("//h.com/x", "http://a.com/b/c")))"#,
                r#"["http://h.com/x", "http://h.com", "http:", "", "", "h.com", "h.com", "", "/x", "", ""]"#,
            ),
            (
                r#"E(() => P(new URL("//h.com/x", "https://a.com/b/c")))"#,
                r#"["https://h.com/x", "https://h.com", "https:", "", "", "h.com", "h.com", "", "/x", "", ""]"#,
            ),
            (
                r#"E(() => P(new URL("?q", "http://a.com/b/c?old#frag")))"#,
                r#"["http://a.com/b/c?q", "http://a.com", "http:", "", "", "a.com", "a.com", "", "/b/c", "?q", ""]"#,
            ),
            (
                r##"E(() => P(new URL("#f", "http://a.com/b/c?old#frag")))"##,
                r##"["http://a.com/b/c?old#f", "http://a.com", "http:", "", "", "a.com", "a.com", "", "/b/c", "?old", "#f"]"##,
            ),
            (
                r#"E(() => P(new URL("", "http://a.com/b/c?old#frag")))"#,
                r#"["http://a.com/b/c?old", "http://a.com", "http:", "", "", "a.com", "a.com", "", "/b/c", "?old", ""]"#,
            ),
            (
                r#"E(() => P(new URL("../x", "http://a.com/b/c/d")))"#,
                r#"["http://a.com/b/x", "http://a.com", "http:", "", "", "a.com", "a.com", "", "/b/x", "", ""]"#,
            ),
            (
                r#"E(() => P(new URL("./x", "http://a.com/b/c/")))"#,
                r#"["http://a.com/b/c/x", "http://a.com", "http:", "", "", "a.com", "a.com", "", "/b/c/x", "", ""]"#,
            ),
            (
                r#"E(() => P(new URL("..", "http://a.com/b/c/")))"#,
                r#"["http://a.com/b/", "http://a.com", "http:", "", "", "a.com", "a.com", "", "/b/", "", ""]"#,
            ),
            (
                r#"E(() => P(new URL(".", "http://a.com/b/c")))"#,
                r#"["http://a.com/b/", "http://a.com", "http:", "", "", "a.com", "a.com", "", "/b/", "", ""]"#,
            ),
            (
                r#"E(() => P(new URL("x", "http://a.com")))"#,
                r#"["http://a.com/x", "http://a.com", "http:", "", "", "a.com", "a.com", "", "/x", "", ""]"#,
            ),
            (
                r#"E(() => P(new URL("x", "http://a.com/")))"#,
                r#"["http://a.com/x", "http://a.com", "http:", "", "", "a.com", "a.com", "", "/x", "", ""]"#,
            ),
            (
                r#"E(() => P(new URL("x?q#f", "http://a.com/d/")))"#,
                r##"["http://a.com/d/x?q#f", "http://a.com", "http:", "", "", "a.com", "a.com", "", "/d/x", "?q", "#f"]"##,
            ),
            (
                r#"E(() => P(new URL("http:x", "http://a.com/d/e")))"#,
                r#"["http://a.com/d/x", "http://a.com", "http:", "", "", "a.com", "a.com", "", "/d/x", "", ""]"#,
            ),
            (
                r#"E(() => P(new URL("https:x", "http://a.com/d/e")))"#,
                r#"["https://x/", "https://x", "https:", "", "", "x", "x", "", "/", "", ""]"#,
            ),
            (
                r#"E(() => P(new URL("http:/x", "http://a.com/d/e")))"#,
                r#"["http://a.com/x", "http://a.com", "http:", "", "", "a.com", "a.com", "", "/x", "", ""]"#,
            ),
            (
                r#"E(() => P(new URL("x", "file:///a/b")))"#,
                r#"["file:///a/x", "file://", "file:", "", "", "", "", "", "/a/x", "", ""]"#,
            ),
            (
                r#"E(() => P(new URL("/x", "file:///a/b")))"#,
                r#"["file:///x", "file://", "file:", "", "", "", "", "", "/x", "", ""]"#,
            ),
            (
                r#"E(() => P(new URL("//h/x", "file:///a/b")))"#,
                r#"["file://h/x", "file://", "file:", "", "", "h", "h", "", "/x", "", ""]"#,
            ),
            (
                r#"E(() => P(new URL("x", "mailto:a@b")))"#,
                r#""TypeError: Failed to construct 'URL': Invalid URL""#,
            ),
            (
                r##"E(() => P(new URL("#f", "mailto:a@b")))"##,
                r##"["mailto:a@b#f", "null", "mailto:", "", "", "", "", "", "a@b", "", "#f"]"##,
            ),
            (
                r#"E(() => P(new URL("x", "data:text/plain,abc")))"#,
                r#""TypeError: Failed to construct 'URL': Invalid URL""#,
            ),
            (
                r##"E(() => P(new URL("#f", "data:text/plain,abc")))"##,
                r##"["data:text/plain,abc#f", "null", "data:", "", "", "", "", "", "text/plain,abc", "", "#f"]"##,
            ),
            (
                r#"E(() => P(new URL("x", "foo://h/a/b")))"#,
                r#"["foo://h/a/x", "null", "foo:", "", "", "h", "h", "", "/a/x", "", ""]"#,
            ),
            (
                r#"E(() => P(new URL("/x", "foo://h/a/b")))"#,
                r#"["foo://h/x", "null", "foo:", "", "", "h", "h", "", "/x", "", ""]"#,
            ),
            (
                r#"E(() => P(new URL("x", "foo:/a/b")))"#,
                r#"["foo:/a/x", "null", "foo:", "", "", "", "", "", "/a/x", "", ""]"#,
            ),
            (
                r#"E(() => P(new URL("\\\\x", "http://a/b/c")))"#,
                r#"["http://x/", "http://x", "http:", "", "", "x", "x", "", "/", "", ""]"#,
            ),
            (
                r#"E(() => P(new URL("x y", "http://a/b/")))"#,
                r#"["http://a/b/x%20y", "http://a", "http:", "", "", "a", "a", "", "/b/x%20y", "", ""]"#,
            ),
            (
                r#"E(() => P(new URL("C:/x", "file:///a/b")))"#,
                r#"["c:/x", "null", "c:", "", "", "", "", "", "/x", "", ""]"#,
            ),
            (
                r#"E(() => P(new URL("http://other/", "http://a/b")))"#,
                r#"["http://other/", "http://other", "http:", "", "", "other", "other", "", "/", "", ""]"#,
            ),
            (
                r#"E(() => P(new URL("   x   ", "http://a/b/")))"#,
                r#"["http://a/b/x", "http://a", "http:", "", "", "a", "a", "", "/b/x", "", ""]"#,
            ),
            (
                r#"E(() => P(new URL("x", "javascript:void(0)")))"#,
                r#""TypeError: Failed to construct 'URL': Invalid URL""#,
            ),
            (
                r##"E(() => P(new URL("#x", "javascript:void(0)")))"##,
                r##"["javascript:void(0)#x", "null", "javascript:", "", "", "", "", "", "void(0)", "", "#x"]"##,
            ),
        ],
    },
    Case {
        name: "setters-http-0",
        html: r"<!DOCTYPE html><html><head></head><body><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : v; } catch (e) { return (e instanceof DOMException ? 'DOMException ' + e.name + ' [' + e.code + ']' : e.constructor.name) + ': ' + e.message; } }
function P(u) { return [u.href, u.origin, u.protocol, u.username, u.password, u.host, u.hostname, u.port, u.pathname, u.search, u.hash]; }
function S(init) { return [...new URLSearchParams(init)].map((p) => p.join('=')).join('&'); }
</script></body></html>",
        queries: &[
            (
                r#"E(() => { const u = new URL("http://user:pw@a.com:8080/p/a?old=1#oldf"); u.href = "https://x.test/p?q#h"; return P(u); })"#,
                r##"["https://x.test/p?q#h", "https://x.test", "https:", "", "", "x.test", "x.test", "", "/p", "?q", "#h"]"##,
            ),
            (
                r#"E(() => { const u = new URL("http://user:pw@a.com:8080/p/a?old=1#oldf"); u.href = "foo"; return P(u); })"#,
                r#""TypeError: Failed to set the 'href' property on 'URL': Invalid URL""#,
            ),
            (
                r#"E(() => { const u = new URL("http://user:pw@a.com:8080/p/a?old=1#oldf"); u.href = ""; return P(u); })"#,
                r#""TypeError: Failed to set the 'href' property on 'URL': Invalid URL""#,
            ),
            (
                r#"E(() => { const u = new URL("http://user:pw@a.com:8080/p/a?old=1#oldf"); u.href = "mailto:a@b"; return P(u); })"#,
                r#"["mailto:a@b", "null", "mailto:", "", "", "", "", "", "a@b", "", ""]"#,
            ),
            (
                r#"E(() => { const u = new URL("http://user:pw@a.com:8080/p/a?old=1#oldf"); u.protocol = "https"; return P(u); })"#,
                r##"["https://user:pw@a.com:8080/p/a?old=1#oldf", "https://a.com:8080", "https:", "user", "pw", "a.com:8080", "a.com", "8080", "/p/a", "?old=1", "#oldf"]"##,
            ),
            (
                r#"E(() => { const u = new URL("http://user:pw@a.com:8080/p/a?old=1#oldf"); u.protocol = "https:"; return P(u); })"#,
                r##"["https://user:pw@a.com:8080/p/a?old=1#oldf", "https://a.com:8080", "https:", "user", "pw", "a.com:8080", "a.com", "8080", "/p/a", "?old=1", "#oldf"]"##,
            ),
            (
                r#"E(() => { const u = new URL("http://user:pw@a.com:8080/p/a?old=1#oldf"); u.protocol = "https://x"; return P(u); })"#,
                r##"["https://user:pw@a.com:8080/p/a?old=1#oldf", "https://a.com:8080", "https:", "user", "pw", "a.com:8080", "a.com", "8080", "/p/a", "?old=1", "#oldf"]"##,
            ),
            (
                r#"E(() => { const u = new URL("http://user:pw@a.com:8080/p/a?old=1#oldf"); u.protocol = "HTTPS:abc"; return P(u); })"#,
                r##"["https://user:pw@a.com:8080/p/a?old=1#oldf", "https://a.com:8080", "https:", "user", "pw", "a.com:8080", "a.com", "8080", "/p/a", "?old=1", "#oldf"]"##,
            ),
            (
                r#"E(() => { const u = new URL("http://user:pw@a.com:8080/p/a?old=1#oldf"); u.protocol = "ftp"; return P(u); })"#,
                r##"["ftp://user:pw@a.com:8080/p/a?old=1#oldf", "ftp://a.com:8080", "ftp:", "user", "pw", "a.com:8080", "a.com", "8080", "/p/a", "?old=1", "#oldf"]"##,
            ),
            (
                r#"E(() => { const u = new URL("http://user:pw@a.com:8080/p/a?old=1#oldf"); u.protocol = "ws"; return P(u); })"#,
                r##"["ws://user:pw@a.com:8080/p/a?old=1#oldf", "ws://a.com:8080", "ws:", "user", "pw", "a.com:8080", "a.com", "8080", "/p/a", "?old=1", "#oldf"]"##,
            ),
            (
                r#"E(() => { const u = new URL("http://user:pw@a.com:8080/p/a?old=1#oldf"); u.protocol = "file"; return P(u); })"#,
                r##"["http://user:pw@a.com:8080/p/a?old=1#oldf", "http://a.com:8080", "http:", "user", "pw", "a.com:8080", "a.com", "8080", "/p/a", "?old=1", "#oldf"]"##,
            ),
            (
                r#"E(() => { const u = new URL("http://user:pw@a.com:8080/p/a?old=1#oldf"); u.protocol = "foo"; return P(u); })"#,
                r##"["http://user:pw@a.com:8080/p/a?old=1#oldf", "http://a.com:8080", "http:", "user", "pw", "a.com:8080", "a.com", "8080", "/p/a", "?old=1", "#oldf"]"##,
            ),
            (
                r#"E(() => { const u = new URL("http://user:pw@a.com:8080/p/a?old=1#oldf"); u.protocol = "mailto"; return P(u); })"#,
                r##"["http://user:pw@a.com:8080/p/a?old=1#oldf", "http://a.com:8080", "http:", "user", "pw", "a.com:8080", "a.com", "8080", "/p/a", "?old=1", "#oldf"]"##,
            ),
            (
                r#"E(() => { const u = new URL("http://user:pw@a.com:8080/p/a?old=1#oldf"); u.protocol = "1x"; return P(u); })"#,
                r##"["http://user:pw@a.com:8080/p/a?old=1#oldf", "http://a.com:8080", "http:", "user", "pw", "a.com:8080", "a.com", "8080", "/p/a", "?old=1", "#oldf"]"##,
            ),
            (
                r#"E(() => { const u = new URL("http://user:pw@a.com:8080/p/a?old=1#oldf"); u.protocol = ""; return P(u); })"#,
                r##"["http://user:pw@a.com:8080/p/a?old=1#oldf", "http://a.com:8080", "http:", "user", "pw", "a.com:8080", "a.com", "8080", "/p/a", "?old=1", "#oldf"]"##,
            ),
            (
                r#"E(() => { const u = new URL("http://user:pw@a.com:8080/p/a?old=1#oldf"); u.protocol = "x y"; return P(u); })"#,
                r##"["http://user:pw@a.com:8080/p/a?old=1#oldf", "http://a.com:8080", "http:", "user", "pw", "a.com:8080", "a.com", "8080", "/p/a", "?old=1", "#oldf"]"##,
            ),
            (
                r#"E(() => { const u = new URL("http://user:pw@a.com:8080/p/a?old=1#oldf"); u.protocol = "http"; return P(u); })"#,
                r##"["http://user:pw@a.com:8080/p/a?old=1#oldf", "http://a.com:8080", "http:", "user", "pw", "a.com:8080", "a.com", "8080", "/p/a", "?old=1", "#oldf"]"##,
            ),
            (
                r#"E(() => { const u = new URL("http://user:pw@a.com:8080/p/a?old=1#oldf"); u.username = "u"; return P(u); })"#,
                r##"["http://u:pw@a.com:8080/p/a?old=1#oldf", "http://a.com:8080", "http:", "u", "pw", "a.com:8080", "a.com", "8080", "/p/a", "?old=1", "#oldf"]"##,
            ),
            (
                r#"E(() => { const u = new URL("http://user:pw@a.com:8080/p/a?old=1#oldf"); u.username = "u s:r@/?#\u00e9"; return P(u); })"#,
                r##"["http://u%20s%3Ar%40%2F%3F%23%C3%A9:pw@a.com:8080/p/a?old=1#oldf", "http://a.com:8080", "http:", "u%20s%3Ar%40%2F%3F%23%C3%A9", "pw", "a.com:8080", "a.com", "8080", "/p/a", "?old=1", "#oldf"]"##,
            ),
            (
                r#"E(() => { const u = new URL("http://user:pw@a.com:8080/p/a?old=1#oldf"); u.username = ""; return P(u); })"#,
                r##"["http://:pw@a.com:8080/p/a?old=1#oldf", "http://a.com:8080", "http:", "", "pw", "a.com:8080", "a.com", "8080", "/p/a", "?old=1", "#oldf"]"##,
            ),
            (
                r#"E(() => { const u = new URL("http://user:pw@a.com:8080/p/a?old=1#oldf"); u.password = "p"; return P(u); })"#,
                r##"["http://user:p@a.com:8080/p/a?old=1#oldf", "http://a.com:8080", "http:", "user", "p", "a.com:8080", "a.com", "8080", "/p/a", "?old=1", "#oldf"]"##,
            ),
            (
                r#"E(() => { const u = new URL("http://user:pw@a.com:8080/p/a?old=1#oldf"); u.password = "p s:r@/?#\u00e9"; return P(u); })"#,
                r##"["http://user:p%20s%3Ar%40%2F%3F%23%C3%A9@a.com:8080/p/a?old=1#oldf", "http://a.com:8080", "http:", "user", "p%20s%3Ar%40%2F%3F%23%C3%A9", "a.com:8080", "a.com", "8080", "/p/a", "?old=1", "#oldf"]"##,
            ),
            (
                r#"E(() => { const u = new URL("http://user:pw@a.com:8080/p/a?old=1#oldf"); u.password = ""; return P(u); })"#,
                r##"["http://user@a.com:8080/p/a?old=1#oldf", "http://a.com:8080", "http:", "user", "", "a.com:8080", "a.com", "8080", "/p/a", "?old=1", "#oldf"]"##,
            ),
            (
                r#"E(() => { const u = new URL("http://user:pw@a.com:8080/p/a?old=1#oldf"); u.host = "h.test"; return P(u); })"#,
                r##"["http://user:pw@h.test:8080/p/a?old=1#oldf", "http://h.test:8080", "http:", "user", "pw", "h.test:8080", "h.test", "8080", "/p/a", "?old=1", "#oldf"]"##,
            ),
            (
                r#"E(() => { const u = new URL("http://user:pw@a.com:8080/p/a?old=1#oldf"); u.host = "h.test:81"; return P(u); })"#,
                r##"["http://user:pw@h.test:81/p/a?old=1#oldf", "http://h.test:81", "http:", "user", "pw", "h.test:81", "h.test", "81", "/p/a", "?old=1", "#oldf"]"##,
            ),
            (
                r#"E(() => { const u = new URL("http://user:pw@a.com:8080/p/a?old=1#oldf"); u.host = "h.test:"; return P(u); })"#,
                r##"["http://user:pw@h.test:8080/p/a?old=1#oldf", "http://h.test:8080", "http:", "user", "pw", "h.test:8080", "h.test", "8080", "/p/a", "?old=1", "#oldf"]"##,
            ),
            (
                r#"E(() => { const u = new URL("http://user:pw@a.com:8080/p/a?old=1#oldf"); u.host = "h.test:99999"; return P(u); })"#,
                r##"["http://user:pw@h.test:8080/p/a?old=1#oldf", "http://h.test:8080", "http:", "user", "pw", "h.test:8080", "h.test", "8080", "/p/a", "?old=1", "#oldf"]"##,
            ),
            (
                r#"E(() => { const u = new URL("http://user:pw@a.com:8080/p/a?old=1#oldf"); u.host = "h.test:81abc"; return P(u); })"#,
                r##"["http://user:pw@h.test:81/p/a?old=1#oldf", "http://h.test:81", "http:", "user", "pw", "h.test:81", "h.test", "81", "/p/a", "?old=1", "#oldf"]"##,
            ),
            (
                r#"E(() => { const u = new URL("http://user:pw@a.com:8080/p/a?old=1#oldf"); u.host = "h.test/path"; return P(u); })"#,
                r##"["http://user:pw@h.test:8080/p/a?old=1#oldf", "http://h.test:8080", "http:", "user", "pw", "h.test:8080", "h.test", "8080", "/p/a", "?old=1", "#oldf"]"##,
            ),
            (
                r#"E(() => { const u = new URL("http://user:pw@a.com:8080/p/a?old=1#oldf"); u.host = "h.test?x"; return P(u); })"#,
                r##"["http://user:pw@h.test:8080/p/a?old=1#oldf", "http://h.test:8080", "http:", "user", "pw", "h.test:8080", "h.test", "8080", "/p/a", "?old=1", "#oldf"]"##,
            ),
        ],
    },
    Case {
        name: "setters-http-1",
        html: r"<!DOCTYPE html><html><head></head><body><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : v; } catch (e) { return (e instanceof DOMException ? 'DOMException ' + e.name + ' [' + e.code + ']' : e.constructor.name) + ': ' + e.message; } }
function P(u) { return [u.href, u.origin, u.protocol, u.username, u.password, u.host, u.hostname, u.port, u.pathname, u.search, u.hash]; }
function S(init) { return [...new URLSearchParams(init)].map((p) => p.join('=')).join('&'); }
</script></body></html>",
        queries: &[
            (
                r#"E(() => { const u = new URL("http://user:pw@a.com:8080/p/a?old=1#oldf"); u.host = "h.test#x"; return P(u); })"#,
                r##"["http://user:pw@h.test:8080/p/a?old=1#oldf", "http://h.test:8080", "http:", "user", "pw", "h.test:8080", "h.test", "8080", "/p/a", "?old=1", "#oldf"]"##,
            ),
            (
                r#"E(() => { const u = new URL("http://user:pw@a.com:8080/p/a?old=1#oldf"); u.host = "h.test\\\\x"; return P(u); })"#,
                r##"["http://user:pw@h.test:8080/p/a?old=1#oldf", "http://h.test:8080", "http:", "user", "pw", "h.test:8080", "h.test", "8080", "/p/a", "?old=1", "#oldf"]"##,
            ),
            (
                r#"E(() => { const u = new URL("http://user:pw@a.com:8080/p/a?old=1#oldf"); u.host = "[::1]:3000"; return P(u); })"#,
                r##"["http://user:pw@[::1]:3000/p/a?old=1#oldf", "http://[::1]:3000", "http:", "user", "pw", "[::1]:3000", "[::1]", "3000", "/p/a", "?old=1", "#oldf"]"##,
            ),
            (
                r#"E(() => { const u = new URL("http://user:pw@a.com:8080/p/a?old=1#oldf"); u.host = "[::1"; return P(u); })"#,
                r##"["http://user:pw@a.com:8080/p/a?old=1#oldf", "http://a.com:8080", "http:", "user", "pw", "a.com:8080", "a.com", "8080", "/p/a", "?old=1", "#oldf"]"##,
            ),
            (
                r#"E(() => { const u = new URL("http://user:pw@a.com:8080/p/a?old=1#oldf"); u.host = ""; return P(u); })"#,
                r##"["http://user:pw@a.com:8080/p/a?old=1#oldf", "http://a.com:8080", "http:", "user", "pw", "a.com:8080", "a.com", "8080", "/p/a", "?old=1", "#oldf"]"##,
            ),
            (
                r#"E(() => { const u = new URL("http://user:pw@a.com:8080/p/a?old=1#oldf"); u.host = "a b"; return P(u); })"#,
                r##"["http://user:pw@a%20b:8080/p/a?old=1#oldf", "http://a%20b:8080", "http:", "user", "pw", "a%20b:8080", "a%20b", "8080", "/p/a", "?old=1", "#oldf"]"##,
            ),
            (
                r#"E(() => { const u = new URL("http://user:pw@a.com:8080/p/a?old=1#oldf"); u.host = "A.B"; return P(u); })"#,
                r##"["http://user:pw@a.b:8080/p/a?old=1#oldf", "http://a.b:8080", "http:", "user", "pw", "a.b:8080", "a.b", "8080", "/p/a", "?old=1", "#oldf"]"##,
            ),
            (
                r#"E(() => { const u = new URL("http://user:pw@a.com:8080/p/a?old=1#oldf"); u.host = ":81"; return P(u); })"#,
                r##"["http://user:pw@a.com:8080/p/a?old=1#oldf", "http://a.com:8080", "http:", "user", "pw", "a.com:8080", "a.com", "8080", "/p/a", "?old=1", "#oldf"]"##,
            ),
            (
                r#"E(() => { const u = new URL("http://user:pw@a.com:8080/p/a?old=1#oldf"); u.host = "h.test:80"; return P(u); })"#,
                r##"["http://user:pw@h.test/p/a?old=1#oldf", "http://h.test", "http:", "user", "pw", "h.test", "h.test", "", "/p/a", "?old=1", "#oldf"]"##,
            ),
            (
                r#"E(() => { const u = new URL("http://user:pw@a.com:8080/p/a?old=1#oldf"); u.host = "h.test:443"; return P(u); })"#,
                r##"["http://user:pw@h.test:443/p/a?old=1#oldf", "http://h.test:443", "http:", "user", "pw", "h.test:443", "h.test", "443", "/p/a", "?old=1", "#oldf"]"##,
            ),
            (
                r#"E(() => { const u = new URL("http://user:pw@a.com:8080/p/a?old=1#oldf"); u.host = "m\u00fcnchen.de"; return P(u); })"#,
                r##"["http://user:pw@xn--mnchen-3ya.de:8080/p/a?old=1#oldf", "http://xn--mnchen-3ya.de:8080", "http:", "user", "pw", "xn--mnchen-3ya.de:8080", "xn--mnchen-3ya.de", "8080", "/p/a", "?old=1", "#oldf"]"##,
            ),
            (
                r#"E(() => { const u = new URL("http://user:pw@a.com:8080/p/a?old=1#oldf"); u.host = "127.1"; return P(u); })"#,
                r##"["http://user:pw@127.0.0.1:8080/p/a?old=1#oldf", "http://127.0.0.1:8080", "http:", "user", "pw", "127.0.0.1:8080", "127.0.0.1", "8080", "/p/a", "?old=1", "#oldf"]"##,
            ),
            (
                r#"E(() => { const u = new URL("http://user:pw@a.com:8080/p/a?old=1#oldf"); u.hostname = "h.test"; return P(u); })"#,
                r##"["http://user:pw@h.test:8080/p/a?old=1#oldf", "http://h.test:8080", "http:", "user", "pw", "h.test:8080", "h.test", "8080", "/p/a", "?old=1", "#oldf"]"##,
            ),
            (
                r#"E(() => { const u = new URL("http://user:pw@a.com:8080/p/a?old=1#oldf"); u.hostname = "h.test:81"; return P(u); })"#,
                r##"["http://user:pw@a.com:8080/p/a?old=1#oldf", "http://a.com:8080", "http:", "user", "pw", "a.com:8080", "a.com", "8080", "/p/a", "?old=1", "#oldf"]"##,
            ),
            (
                r#"E(() => { const u = new URL("http://user:pw@a.com:8080/p/a?old=1#oldf"); u.hostname = "h.test/x"; return P(u); })"#,
                r##"["http://user:pw@h.test:8080/p/a?old=1#oldf", "http://h.test:8080", "http:", "user", "pw", "h.test:8080", "h.test", "8080", "/p/a", "?old=1", "#oldf"]"##,
            ),
            (
                r#"E(() => { const u = new URL("http://user:pw@a.com:8080/p/a?old=1#oldf"); u.hostname = "h.test?x"; return P(u); })"#,
                r##"["http://user:pw@h.test:8080/p/a?old=1#oldf", "http://h.test:8080", "http:", "user", "pw", "h.test:8080", "h.test", "8080", "/p/a", "?old=1", "#oldf"]"##,
            ),
            (
                r#"E(() => { const u = new URL("http://user:pw@a.com:8080/p/a?old=1#oldf"); u.hostname = "[::1]"; return P(u); })"#,
                r##"["http://user:pw@[::1]:8080/p/a?old=1#oldf", "http://[::1]:8080", "http:", "user", "pw", "[::1]:8080", "[::1]", "8080", "/p/a", "?old=1", "#oldf"]"##,
            ),
            (
                r#"E(() => { const u = new URL("http://user:pw@a.com:8080/p/a?old=1#oldf"); u.hostname = ""; return P(u); })"#,
                r##"["http://user:pw@a.com:8080/p/a?old=1#oldf", "http://a.com:8080", "http:", "user", "pw", "a.com:8080", "a.com", "8080", "/p/a", "?old=1", "#oldf"]"##,
            ),
            (
                r#"E(() => { const u = new URL("http://user:pw@a.com:8080/p/a?old=1#oldf"); u.hostname = "a b"; return P(u); })"#,
                r##"["http://user:pw@a%20b:8080/p/a?old=1#oldf", "http://a%20b:8080", "http:", "user", "pw", "a%20b:8080", "a%20b", "8080", "/p/a", "?old=1", "#oldf"]"##,
            ),
            (
                r#"E(() => { const u = new URL("http://user:pw@a.com:8080/p/a?old=1#oldf"); u.hostname = "A.B"; return P(u); })"#,
                r##"["http://user:pw@a.b:8080/p/a?old=1#oldf", "http://a.b:8080", "http:", "user", "pw", "a.b:8080", "a.b", "8080", "/p/a", "?old=1", "#oldf"]"##,
            ),
            (
                r#"E(() => { const u = new URL("http://user:pw@a.com:8080/p/a?old=1#oldf"); u.port = "81"; return P(u); })"#,
                r##"["http://user:pw@a.com:81/p/a?old=1#oldf", "http://a.com:81", "http:", "user", "pw", "a.com:81", "a.com", "81", "/p/a", "?old=1", "#oldf"]"##,
            ),
            (
                r#"E(() => { const u = new URL("http://user:pw@a.com:8080/p/a?old=1#oldf"); u.port = "81abc"; return P(u); })"#,
                r##"["http://user:pw@a.com:81/p/a?old=1#oldf", "http://a.com:81", "http:", "user", "pw", "a.com:81", "a.com", "81", "/p/a", "?old=1", "#oldf"]"##,
            ),
            (
                r#"E(() => { const u = new URL("http://user:pw@a.com:8080/p/a?old=1#oldf"); u.port = "abc"; return P(u); })"#,
                r##"["http://user:pw@a.com:8080/p/a?old=1#oldf", "http://a.com:8080", "http:", "user", "pw", "a.com:8080", "a.com", "8080", "/p/a", "?old=1", "#oldf"]"##,
            ),
            (
                r#"E(() => { const u = new URL("http://user:pw@a.com:8080/p/a?old=1#oldf"); u.port = ""; return P(u); })"#,
                r##"["http://user:pw@a.com/p/a?old=1#oldf", "http://a.com", "http:", "user", "pw", "a.com", "a.com", "", "/p/a", "?old=1", "#oldf"]"##,
            ),
            (
                r#"E(() => { const u = new URL("http://user:pw@a.com:8080/p/a?old=1#oldf"); u.port = "99999"; return P(u); })"#,
                r##"["http://user:pw@a.com:8080/p/a?old=1#oldf", "http://a.com:8080", "http:", "user", "pw", "a.com:8080", "a.com", "8080", "/p/a", "?old=1", "#oldf"]"##,
            ),
            (
                r#"E(() => { const u = new URL("http://user:pw@a.com:8080/p/a?old=1#oldf"); u.port = "65535"; return P(u); })"#,
                r##"["http://user:pw@a.com:65535/p/a?old=1#oldf", "http://a.com:65535", "http:", "user", "pw", "a.com:65535", "a.com", "65535", "/p/a", "?old=1", "#oldf"]"##,
            ),
            (
                r#"E(() => { const u = new URL("http://user:pw@a.com:8080/p/a?old=1#oldf"); u.port = "65536"; return P(u); })"#,
                r##"["http://user:pw@a.com:8080/p/a?old=1#oldf", "http://a.com:8080", "http:", "user", "pw", "a.com:8080", "a.com", "8080", "/p/a", "?old=1", "#oldf"]"##,
            ),
            (
                r#"E(() => { const u = new URL("http://user:pw@a.com:8080/p/a?old=1#oldf"); u.port = "0"; return P(u); })"#,
                r##"["http://user:pw@a.com:0/p/a?old=1#oldf", "http://a.com:0", "http:", "user", "pw", "a.com:0", "a.com", "0", "/p/a", "?old=1", "#oldf"]"##,
            ),
            (
                r#"E(() => { const u = new URL("http://user:pw@a.com:8080/p/a?old=1#oldf"); u.port = "00081"; return P(u); })"#,
                r##"["http://user:pw@a.com:81/p/a?old=1#oldf", "http://a.com:81", "http:", "user", "pw", "a.com:81", "a.com", "81", "/p/a", "?old=1", "#oldf"]"##,
            ),
            (
                r#"E(() => { const u = new URL("http://user:pw@a.com:8080/p/a?old=1#oldf"); u.port = "80"; return P(u); })"#,
                r##"["http://user:pw@a.com/p/a?old=1#oldf", "http://a.com", "http:", "user", "pw", "a.com", "a.com", "", "/p/a", "?old=1", "#oldf"]"##,
            ),
        ],
    },
    Case {
        name: "setters-http-2",
        html: r"<!DOCTYPE html><html><head></head><body><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : v; } catch (e) { return (e instanceof DOMException ? 'DOMException ' + e.name + ' [' + e.code + ']' : e.constructor.name) + ': ' + e.message; } }
function P(u) { return [u.href, u.origin, u.protocol, u.username, u.password, u.host, u.hostname, u.port, u.pathname, u.search, u.hash]; }
function S(init) { return [...new URLSearchParams(init)].map((p) => p.join('=')).join('&'); }
</script></body></html>",
        queries: &[
            (
                r#"E(() => { const u = new URL("http://user:pw@a.com:8080/p/a?old=1#oldf"); u.port = "443"; return P(u); })"#,
                r##"["http://user:pw@a.com:443/p/a?old=1#oldf", "http://a.com:443", "http:", "user", "pw", "a.com:443", "a.com", "443", "/p/a", "?old=1", "#oldf"]"##,
            ),
            (
                r#"E(() => { const u = new URL("http://user:pw@a.com:8080/p/a?old=1#oldf"); u.port = " 81"; return P(u); })"#,
                r##"["http://user:pw@a.com:8080/p/a?old=1#oldf", "http://a.com:8080", "http:", "user", "pw", "a.com:8080", "a.com", "8080", "/p/a", "?old=1", "#oldf"]"##,
            ),
            (
                r#"E(() => { const u = new URL("http://user:pw@a.com:8080/p/a?old=1#oldf"); u.port = "8\\t1"; return P(u); })"#,
                r##"["http://user:pw@a.com:8/p/a?old=1#oldf", "http://a.com:8", "http:", "user", "pw", "a.com:8", "a.com", "8", "/p/a", "?old=1", "#oldf"]"##,
            ),
            (
                r#"E(() => { const u = new URL("http://user:pw@a.com:8080/p/a?old=1#oldf"); u.port = "-1"; return P(u); })"#,
                r##"["http://user:pw@a.com:8080/p/a?old=1#oldf", "http://a.com:8080", "http:", "user", "pw", "a.com:8080", "a.com", "8080", "/p/a", "?old=1", "#oldf"]"##,
            ),
            (
                r#"E(() => { const u = new URL("http://user:pw@a.com:8080/p/a?old=1#oldf"); u.port = "1e3"; return P(u); })"#,
                r##"["http://user:pw@a.com:1/p/a?old=1#oldf", "http://a.com:1", "http:", "user", "pw", "a.com:1", "a.com", "1", "/p/a", "?old=1", "#oldf"]"##,
            ),
            (
                r#"E(() => { const u = new URL("http://user:pw@a.com:8080/p/a?old=1#oldf"); u.pathname = "/a/b"; return P(u); })"#,
                r##"["http://user:pw@a.com:8080/a/b?old=1#oldf", "http://a.com:8080", "http:", "user", "pw", "a.com:8080", "a.com", "8080", "/a/b", "?old=1", "#oldf"]"##,
            ),
            (
                r#"E(() => { const u = new URL("http://user:pw@a.com:8080/p/a?old=1#oldf"); u.pathname = "a/b"; return P(u); })"#,
                r##"["http://user:pw@a.com:8080/a/b?old=1#oldf", "http://a.com:8080", "http:", "user", "pw", "a.com:8080", "a.com", "8080", "/a/b", "?old=1", "#oldf"]"##,
            ),
            (
                r#"E(() => { const u = new URL("http://user:pw@a.com:8080/p/a?old=1#oldf"); u.pathname = ""; return P(u); })"#,
                r##"["http://user:pw@a.com:8080/?old=1#oldf", "http://a.com:8080", "http:", "user", "pw", "a.com:8080", "a.com", "8080", "/", "?old=1", "#oldf"]"##,
            ),
            (
                r#"E(() => { const u = new URL("http://user:pw@a.com:8080/p/a?old=1#oldf"); u.pathname = "/a b?c#d%e"; return P(u); })"#,
                r##"["http://user:pw@a.com:8080/a%20b%3Fc%23d%e?old=1#oldf", "http://a.com:8080", "http:", "user", "pw", "a.com:8080", "a.com", "8080", "/a%20b%3Fc%23d%e", "?old=1", "#oldf"]"##,
            ),
            (
                r#"E(() => { const u = new URL("http://user:pw@a.com:8080/p/a?old=1#oldf"); u.pathname = "/a/../b/./c"; return P(u); })"#,
                r##"["http://user:pw@a.com:8080/b/c?old=1#oldf", "http://a.com:8080", "http:", "user", "pw", "a.com:8080", "a.com", "8080", "/b/c", "?old=1", "#oldf"]"##,
            ),
            (
                r#"E(() => { const u = new URL("http://user:pw@a.com:8080/p/a?old=1#oldf"); u.pathname = "\\\\a\\\\b"; return P(u); })"#,
                r##"["http://user:pw@a.com:8080//a//b?old=1#oldf", "http://a.com:8080", "http:", "user", "pw", "a.com:8080", "a.com", "8080", "//a//b", "?old=1", "#oldf"]"##,
            ),
            (
                r#"E(() => { const u = new URL("http://user:pw@a.com:8080/p/a?old=1#oldf"); u.pathname = "/\u00e9\ud83d\ude00"; return P(u); })"#,
                r##"["http://user:pw@a.com:8080/%C3%A9%F0%9F%98%80?old=1#oldf", "http://a.com:8080", "http:", "user", "pw", "a.com:8080", "a.com", "8080", "/%C3%A9%F0%9F%98%80", "?old=1", "#oldf"]"##,
            ),
            (
                r#"E(() => { const u = new URL("http://user:pw@a.com:8080/p/a?old=1#oldf"); u.pathname = "//x"; return P(u); })"#,
                r##"["http://user:pw@a.com:8080//x?old=1#oldf", "http://a.com:8080", "http:", "user", "pw", "a.com:8080", "a.com", "8080", "//x", "?old=1", "#oldf"]"##,
            ),
            (
                r#"E(() => { const u = new URL("http://user:pw@a.com:8080/p/a?old=1#oldf"); u.pathname = "/%41/%zz"; return P(u); })"#,
                r##"["http://user:pw@a.com:8080/%41/%zz?old=1#oldf", "http://a.com:8080", "http:", "user", "pw", "a.com:8080", "a.com", "8080", "/%41/%zz", "?old=1", "#oldf"]"##,
            ),
            (
                r#"E(() => { const u = new URL("http://user:pw@a.com:8080/p/a?old=1#oldf"); u.search = "q=1"; return P(u); })"#,
                r##"["http://user:pw@a.com:8080/p/a?q=1#oldf", "http://a.com:8080", "http:", "user", "pw", "a.com:8080", "a.com", "8080", "/p/a", "?q=1", "#oldf"]"##,
            ),
            (
                r#"E(() => { const u = new URL("http://user:pw@a.com:8080/p/a?old=1#oldf"); u.search = "?q=1"; return P(u); })"#,
                r##"["http://user:pw@a.com:8080/p/a?q=1#oldf", "http://a.com:8080", "http:", "user", "pw", "a.com:8080", "a.com", "8080", "/p/a", "?q=1", "#oldf"]"##,
            ),
            (
                r#"E(() => { const u = new URL("http://user:pw@a.com:8080/p/a?old=1#oldf"); u.search = "??q=1"; return P(u); })"#,
                r##"["http://user:pw@a.com:8080/p/a??q=1#oldf", "http://a.com:8080", "http:", "user", "pw", "a.com:8080", "a.com", "8080", "/p/a", "??q=1", "#oldf"]"##,
            ),
            (
                r#"E(() => { const u = new URL("http://user:pw@a.com:8080/p/a?old=1#oldf"); u.search = ""; return P(u); })"#,
                r##"["http://user:pw@a.com:8080/p/a#oldf", "http://a.com:8080", "http:", "user", "pw", "a.com:8080", "a.com", "8080", "/p/a", "", "#oldf"]"##,
            ),
            (
                r#"E(() => { const u = new URL("http://user:pw@a.com:8080/p/a?old=1#oldf"); u.search = "?"; return P(u); })"#,
                r##"["http://user:pw@a.com:8080/p/a#oldf", "http://a.com:8080", "http:", "user", "pw", "a.com:8080", "a.com", "8080", "/p/a", "", "#oldf"]"##,
            ),
            (
                r#"E(() => { const u = new URL("http://user:pw@a.com:8080/p/a?old=1#oldf"); u.search = "a b&c='d'#e"; return P(u); })"#,
                r##"["http://user:pw@a.com:8080/p/a?a%20b&c=%27d%27%23e#oldf", "http://a.com:8080", "http:", "user", "pw", "a.com:8080", "a.com", "8080", "/p/a", "?a%20b&c=%27d%27%23e", "#oldf"]"##,
            ),
            (
                r#"E(() => { const u = new URL("http://user:pw@a.com:8080/p/a?old=1#oldf"); u.search = "\u00e9\ud83d\ude00"; return P(u); })"#,
                r##"["http://user:pw@a.com:8080/p/a?%C3%A9%F0%9F%98%80#oldf", "http://a.com:8080", "http:", "user", "pw", "a.com:8080", "a.com", "8080", "/p/a", "?%C3%A9%F0%9F%98%80", "#oldf"]"##,
            ),
            (
                r#"E(() => { const u = new URL("http://user:pw@a.com:8080/p/a?old=1#oldf"); u.search = "q=1\\n2"; return P(u); })"#,
                r##"["http://user:pw@a.com:8080/p/a?q=1\\n2#oldf", "http://a.com:8080", "http:", "user", "pw", "a.com:8080", "a.com", "8080", "/p/a", "?q=1\\n2", "#oldf"]"##,
            ),
            (
                r#"E(() => { const u = new URL("http://user:pw@a.com:8080/p/a?old=1#oldf"); u.hash = "f"; return P(u); })"#,
                r##"["http://user:pw@a.com:8080/p/a?old=1#f", "http://a.com:8080", "http:", "user", "pw", "a.com:8080", "a.com", "8080", "/p/a", "?old=1", "#f"]"##,
            ),
            (
                r##"E(() => { const u = new URL("http://user:pw@a.com:8080/p/a?old=1#oldf"); u.hash = "#f"; return P(u); })"##,
                r##"["http://user:pw@a.com:8080/p/a?old=1#f", "http://a.com:8080", "http:", "user", "pw", "a.com:8080", "a.com", "8080", "/p/a", "?old=1", "#f"]"##,
            ),
            (
                r###"E(() => { const u = new URL("http://user:pw@a.com:8080/p/a?old=1#oldf"); u.hash = "##f"; return P(u); })"###,
                r###"["http://user:pw@a.com:8080/p/a?old=1##f", "http://a.com:8080", "http:", "user", "pw", "a.com:8080", "a.com", "8080", "/p/a", "?old=1", "##f"]"###,
            ),
            (
                r#"E(() => { const u = new URL("http://user:pw@a.com:8080/p/a?old=1#oldf"); u.hash = ""; return P(u); })"#,
                r#"["http://user:pw@a.com:8080/p/a?old=1", "http://a.com:8080", "http:", "user", "pw", "a.com:8080", "a.com", "8080", "/p/a", "?old=1", ""]"#,
            ),
            (
                r##"E(() => { const u = new URL("http://user:pw@a.com:8080/p/a?old=1#oldf"); u.hash = "#"; return P(u); })"##,
                r#"["http://user:pw@a.com:8080/p/a?old=1#", "http://a.com:8080", "http:", "user", "pw", "a.com:8080", "a.com", "8080", "/p/a", "?old=1", ""]"#,
            ),
            (
                r#"E(() => { const u = new URL("http://user:pw@a.com:8080/p/a?old=1#oldf"); u.hash = "a b<>`\"'\u00e9"; return P(u); })"#,
                r##"["http://user:pw@a.com:8080/p/a?old=1#a%20b%3C%3E%60%22'%C3%A9", "http://a.com:8080", "http:", "user", "pw", "a.com:8080", "a.com", "8080", "/p/a", "?old=1", "#a%20b%3C%3E%60%22'%C3%A9"]"##,
            ),
            (
                r#"E(() => { const u = new URL("http://user:pw@a.com:8080/p/a?old=1#oldf"); u.hash = "f\\t1"; return P(u); })"#,
                r##"["http://user:pw@a.com:8080/p/a?old=1#f\\t1", "http://a.com:8080", "http:", "user", "pw", "a.com:8080", "a.com", "8080", "/p/a", "?old=1", "#f\\t1"]"##,
            ),
        ],
    },
    Case {
        name: "setters-https-0",
        html: r"<!DOCTYPE html><html><head></head><body><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : v; } catch (e) { return (e instanceof DOMException ? 'DOMException ' + e.name + ' [' + e.code + ']' : e.constructor.name) + ': ' + e.message; } }
function P(u) { return [u.href, u.origin, u.protocol, u.username, u.password, u.host, u.hostname, u.port, u.pathname, u.search, u.hash]; }
function S(init) { return [...new URLSearchParams(init)].map((p) => p.join('=')).join('&'); }
</script></body></html>",
        queries: &[
            (
                r#"E(() => { const u = new URL("https://a.com/x"); u.href = "https://x.test/p?q#h"; return P(u); })"#,
                r##"["https://x.test/p?q#h", "https://x.test", "https:", "", "", "x.test", "x.test", "", "/p", "?q", "#h"]"##,
            ),
            (
                r#"E(() => { const u = new URL("https://a.com/x"); u.href = "foo"; return P(u); })"#,
                r#""TypeError: Failed to set the 'href' property on 'URL': Invalid URL""#,
            ),
            (
                r#"E(() => { const u = new URL("https://a.com/x"); u.href = ""; return P(u); })"#,
                r#""TypeError: Failed to set the 'href' property on 'URL': Invalid URL""#,
            ),
            (
                r#"E(() => { const u = new URL("https://a.com/x"); u.href = "mailto:a@b"; return P(u); })"#,
                r#"["mailto:a@b", "null", "mailto:", "", "", "", "", "", "a@b", "", ""]"#,
            ),
            (
                r#"E(() => { const u = new URL("https://a.com/x"); u.protocol = "https"; return P(u); })"#,
                r#"["https://a.com/x", "https://a.com", "https:", "", "", "a.com", "a.com", "", "/x", "", ""]"#,
            ),
            (
                r#"E(() => { const u = new URL("https://a.com/x"); u.protocol = "https:"; return P(u); })"#,
                r#"["https://a.com/x", "https://a.com", "https:", "", "", "a.com", "a.com", "", "/x", "", ""]"#,
            ),
            (
                r#"E(() => { const u = new URL("https://a.com/x"); u.protocol = "https://x"; return P(u); })"#,
                r#"["https://a.com/x", "https://a.com", "https:", "", "", "a.com", "a.com", "", "/x", "", ""]"#,
            ),
            (
                r#"E(() => { const u = new URL("https://a.com/x"); u.protocol = "HTTPS:abc"; return P(u); })"#,
                r#"["https://a.com/x", "https://a.com", "https:", "", "", "a.com", "a.com", "", "/x", "", ""]"#,
            ),
            (
                r#"E(() => { const u = new URL("https://a.com/x"); u.protocol = "ftp"; return P(u); })"#,
                r#"["ftp://a.com/x", "ftp://a.com", "ftp:", "", "", "a.com", "a.com", "", "/x", "", ""]"#,
            ),
            (
                r#"E(() => { const u = new URL("https://a.com/x"); u.protocol = "ws"; return P(u); })"#,
                r#"["ws://a.com/x", "ws://a.com", "ws:", "", "", "a.com", "a.com", "", "/x", "", ""]"#,
            ),
            (
                r#"E(() => { const u = new URL("https://a.com/x"); u.protocol = "file"; return P(u); })"#,
                r#"["file://a.com/x", "file://", "file:", "", "", "a.com", "a.com", "", "/x", "", ""]"#,
            ),
            (
                r#"E(() => { const u = new URL("https://a.com/x"); u.protocol = "foo"; return P(u); })"#,
                r#"["https://a.com/x", "https://a.com", "https:", "", "", "a.com", "a.com", "", "/x", "", ""]"#,
            ),
            (
                r#"E(() => { const u = new URL("https://a.com/x"); u.protocol = "mailto"; return P(u); })"#,
                r#"["https://a.com/x", "https://a.com", "https:", "", "", "a.com", "a.com", "", "/x", "", ""]"#,
            ),
            (
                r#"E(() => { const u = new URL("https://a.com/x"); u.protocol = "1x"; return P(u); })"#,
                r#"["https://a.com/x", "https://a.com", "https:", "", "", "a.com", "a.com", "", "/x", "", ""]"#,
            ),
            (
                r#"E(() => { const u = new URL("https://a.com/x"); u.protocol = ""; return P(u); })"#,
                r#"["https://a.com/x", "https://a.com", "https:", "", "", "a.com", "a.com", "", "/x", "", ""]"#,
            ),
            (
                r#"E(() => { const u = new URL("https://a.com/x"); u.protocol = "x y"; return P(u); })"#,
                r#"["https://a.com/x", "https://a.com", "https:", "", "", "a.com", "a.com", "", "/x", "", ""]"#,
            ),
            (
                r#"E(() => { const u = new URL("https://a.com/x"); u.protocol = "http"; return P(u); })"#,
                r#"["http://a.com/x", "http://a.com", "http:", "", "", "a.com", "a.com", "", "/x", "", ""]"#,
            ),
            (
                r#"E(() => { const u = new URL("https://a.com/x"); u.username = "u"; return P(u); })"#,
                r#"["https://u@a.com/x", "https://a.com", "https:", "u", "", "a.com", "a.com", "", "/x", "", ""]"#,
            ),
            (
                r#"E(() => { const u = new URL("https://a.com/x"); u.username = "u s:r@/?#\u00e9"; return P(u); })"#,
                r#"["https://u%20s%3Ar%40%2F%3F%23%C3%A9@a.com/x", "https://a.com", "https:", "u%20s%3Ar%40%2F%3F%23%C3%A9", "", "a.com", "a.com", "", "/x", "", ""]"#,
            ),
            (
                r#"E(() => { const u = new URL("https://a.com/x"); u.username = ""; return P(u); })"#,
                r#"["https://a.com/x", "https://a.com", "https:", "", "", "a.com", "a.com", "", "/x", "", ""]"#,
            ),
            (
                r#"E(() => { const u = new URL("https://a.com/x"); u.password = "p"; return P(u); })"#,
                r#"["https://:p@a.com/x", "https://a.com", "https:", "", "p", "a.com", "a.com", "", "/x", "", ""]"#,
            ),
            (
                r#"E(() => { const u = new URL("https://a.com/x"); u.password = "p s:r@/?#\u00e9"; return P(u); })"#,
                r#"["https://:p%20s%3Ar%40%2F%3F%23%C3%A9@a.com/x", "https://a.com", "https:", "", "p%20s%3Ar%40%2F%3F%23%C3%A9", "a.com", "a.com", "", "/x", "", ""]"#,
            ),
            (
                r#"E(() => { const u = new URL("https://a.com/x"); u.password = ""; return P(u); })"#,
                r#"["https://a.com/x", "https://a.com", "https:", "", "", "a.com", "a.com", "", "/x", "", ""]"#,
            ),
            (
                r#"E(() => { const u = new URL("https://a.com/x"); u.host = "h.test"; return P(u); })"#,
                r#"["https://h.test/x", "https://h.test", "https:", "", "", "h.test", "h.test", "", "/x", "", ""]"#,
            ),
            (
                r#"E(() => { const u = new URL("https://a.com/x"); u.host = "h.test:81"; return P(u); })"#,
                r#"["https://h.test:81/x", "https://h.test:81", "https:", "", "", "h.test:81", "h.test", "81", "/x", "", ""]"#,
            ),
            (
                r#"E(() => { const u = new URL("https://a.com/x"); u.host = "h.test:"; return P(u); })"#,
                r#"["https://h.test/x", "https://h.test", "https:", "", "", "h.test", "h.test", "", "/x", "", ""]"#,
            ),
            (
                r#"E(() => { const u = new URL("https://a.com/x"); u.host = "h.test:99999"; return P(u); })"#,
                r#"["https://h.test/x", "https://h.test", "https:", "", "", "h.test", "h.test", "", "/x", "", ""]"#,
            ),
            (
                r#"E(() => { const u = new URL("https://a.com/x"); u.host = "h.test:81abc"; return P(u); })"#,
                r#"["https://h.test:81/x", "https://h.test:81", "https:", "", "", "h.test:81", "h.test", "81", "/x", "", ""]"#,
            ),
            (
                r#"E(() => { const u = new URL("https://a.com/x"); u.host = "h.test/path"; return P(u); })"#,
                r#"["https://h.test/x", "https://h.test", "https:", "", "", "h.test", "h.test", "", "/x", "", ""]"#,
            ),
            (
                r#"E(() => { const u = new URL("https://a.com/x"); u.host = "h.test?x"; return P(u); })"#,
                r#"["https://h.test/x", "https://h.test", "https:", "", "", "h.test", "h.test", "", "/x", "", ""]"#,
            ),
        ],
    },
    Case {
        name: "setters-https-1",
        html: r"<!DOCTYPE html><html><head></head><body><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : v; } catch (e) { return (e instanceof DOMException ? 'DOMException ' + e.name + ' [' + e.code + ']' : e.constructor.name) + ': ' + e.message; } }
function P(u) { return [u.href, u.origin, u.protocol, u.username, u.password, u.host, u.hostname, u.port, u.pathname, u.search, u.hash]; }
function S(init) { return [...new URLSearchParams(init)].map((p) => p.join('=')).join('&'); }
</script></body></html>",
        queries: &[
            (
                r#"E(() => { const u = new URL("https://a.com/x"); u.host = "h.test#x"; return P(u); })"#,
                r#"["https://h.test/x", "https://h.test", "https:", "", "", "h.test", "h.test", "", "/x", "", ""]"#,
            ),
            (
                r#"E(() => { const u = new URL("https://a.com/x"); u.host = "h.test\\\\x"; return P(u); })"#,
                r#"["https://h.test/x", "https://h.test", "https:", "", "", "h.test", "h.test", "", "/x", "", ""]"#,
            ),
            (
                r#"E(() => { const u = new URL("https://a.com/x"); u.host = "[::1]:3000"; return P(u); })"#,
                r#"["https://[::1]:3000/x", "https://[::1]:3000", "https:", "", "", "[::1]:3000", "[::1]", "3000", "/x", "", ""]"#,
            ),
            (
                r#"E(() => { const u = new URL("https://a.com/x"); u.host = "[::1"; return P(u); })"#,
                r#"["https://a.com/x", "https://a.com", "https:", "", "", "a.com", "a.com", "", "/x", "", ""]"#,
            ),
            (
                r#"E(() => { const u = new URL("https://a.com/x"); u.host = ""; return P(u); })"#,
                r#"["https://a.com/x", "https://a.com", "https:", "", "", "a.com", "a.com", "", "/x", "", ""]"#,
            ),
            (
                r#"E(() => { const u = new URL("https://a.com/x"); u.host = "a b"; return P(u); })"#,
                r#"["https://a%20b/x", "https://a%20b", "https:", "", "", "a%20b", "a%20b", "", "/x", "", ""]"#,
            ),
            (
                r#"E(() => { const u = new URL("https://a.com/x"); u.host = "A.B"; return P(u); })"#,
                r#"["https://a.b/x", "https://a.b", "https:", "", "", "a.b", "a.b", "", "/x", "", ""]"#,
            ),
            (
                r#"E(() => { const u = new URL("https://a.com/x"); u.host = ":81"; return P(u); })"#,
                r#"["https://a.com/x", "https://a.com", "https:", "", "", "a.com", "a.com", "", "/x", "", ""]"#,
            ),
            (
                r#"E(() => { const u = new URL("https://a.com/x"); u.host = "h.test:80"; return P(u); })"#,
                r#"["https://h.test:80/x", "https://h.test:80", "https:", "", "", "h.test:80", "h.test", "80", "/x", "", ""]"#,
            ),
            (
                r#"E(() => { const u = new URL("https://a.com/x"); u.host = "h.test:443"; return P(u); })"#,
                r#"["https://h.test/x", "https://h.test", "https:", "", "", "h.test", "h.test", "", "/x", "", ""]"#,
            ),
            (
                r#"E(() => { const u = new URL("https://a.com/x"); u.host = "m\u00fcnchen.de"; return P(u); })"#,
                r#"["https://xn--mnchen-3ya.de/x", "https://xn--mnchen-3ya.de", "https:", "", "", "xn--mnchen-3ya.de", "xn--mnchen-3ya.de", "", "/x", "", ""]"#,
            ),
            (
                r#"E(() => { const u = new URL("https://a.com/x"); u.host = "127.1"; return P(u); })"#,
                r#"["https://127.0.0.1/x", "https://127.0.0.1", "https:", "", "", "127.0.0.1", "127.0.0.1", "", "/x", "", ""]"#,
            ),
            (
                r#"E(() => { const u = new URL("https://a.com/x"); u.hostname = "h.test"; return P(u); })"#,
                r#"["https://h.test/x", "https://h.test", "https:", "", "", "h.test", "h.test", "", "/x", "", ""]"#,
            ),
            (
                r#"E(() => { const u = new URL("https://a.com/x"); u.hostname = "h.test:81"; return P(u); })"#,
                r#"["https://a.com/x", "https://a.com", "https:", "", "", "a.com", "a.com", "", "/x", "", ""]"#,
            ),
            (
                r#"E(() => { const u = new URL("https://a.com/x"); u.hostname = "h.test/x"; return P(u); })"#,
                r#"["https://h.test/x", "https://h.test", "https:", "", "", "h.test", "h.test", "", "/x", "", ""]"#,
            ),
            (
                r#"E(() => { const u = new URL("https://a.com/x"); u.hostname = "h.test?x"; return P(u); })"#,
                r#"["https://h.test/x", "https://h.test", "https:", "", "", "h.test", "h.test", "", "/x", "", ""]"#,
            ),
            (
                r#"E(() => { const u = new URL("https://a.com/x"); u.hostname = "[::1]"; return P(u); })"#,
                r#"["https://[::1]/x", "https://[::1]", "https:", "", "", "[::1]", "[::1]", "", "/x", "", ""]"#,
            ),
            (
                r#"E(() => { const u = new URL("https://a.com/x"); u.hostname = ""; return P(u); })"#,
                r#"["https://a.com/x", "https://a.com", "https:", "", "", "a.com", "a.com", "", "/x", "", ""]"#,
            ),
            (
                r#"E(() => { const u = new URL("https://a.com/x"); u.hostname = "a b"; return P(u); })"#,
                r#"["https://a%20b/x", "https://a%20b", "https:", "", "", "a%20b", "a%20b", "", "/x", "", ""]"#,
            ),
            (
                r#"E(() => { const u = new URL("https://a.com/x"); u.hostname = "A.B"; return P(u); })"#,
                r#"["https://a.b/x", "https://a.b", "https:", "", "", "a.b", "a.b", "", "/x", "", ""]"#,
            ),
            (
                r#"E(() => { const u = new URL("https://a.com/x"); u.port = "81"; return P(u); })"#,
                r#"["https://a.com:81/x", "https://a.com:81", "https:", "", "", "a.com:81", "a.com", "81", "/x", "", ""]"#,
            ),
            (
                r#"E(() => { const u = new URL("https://a.com/x"); u.port = "81abc"; return P(u); })"#,
                r#"["https://a.com:81/x", "https://a.com:81", "https:", "", "", "a.com:81", "a.com", "81", "/x", "", ""]"#,
            ),
            (
                r#"E(() => { const u = new URL("https://a.com/x"); u.port = "abc"; return P(u); })"#,
                r#"["https://a.com/x", "https://a.com", "https:", "", "", "a.com", "a.com", "", "/x", "", ""]"#,
            ),
            (
                r#"E(() => { const u = new URL("https://a.com/x"); u.port = ""; return P(u); })"#,
                r#"["https://a.com/x", "https://a.com", "https:", "", "", "a.com", "a.com", "", "/x", "", ""]"#,
            ),
            (
                r#"E(() => { const u = new URL("https://a.com/x"); u.port = "99999"; return P(u); })"#,
                r#"["https://a.com/x", "https://a.com", "https:", "", "", "a.com", "a.com", "", "/x", "", ""]"#,
            ),
            (
                r#"E(() => { const u = new URL("https://a.com/x"); u.port = "65535"; return P(u); })"#,
                r#"["https://a.com:65535/x", "https://a.com:65535", "https:", "", "", "a.com:65535", "a.com", "65535", "/x", "", ""]"#,
            ),
            (
                r#"E(() => { const u = new URL("https://a.com/x"); u.port = "65536"; return P(u); })"#,
                r#"["https://a.com/x", "https://a.com", "https:", "", "", "a.com", "a.com", "", "/x", "", ""]"#,
            ),
            (
                r#"E(() => { const u = new URL("https://a.com/x"); u.port = "0"; return P(u); })"#,
                r#"["https://a.com:0/x", "https://a.com:0", "https:", "", "", "a.com:0", "a.com", "0", "/x", "", ""]"#,
            ),
            (
                r#"E(() => { const u = new URL("https://a.com/x"); u.port = "00081"; return P(u); })"#,
                r#"["https://a.com:81/x", "https://a.com:81", "https:", "", "", "a.com:81", "a.com", "81", "/x", "", ""]"#,
            ),
            (
                r#"E(() => { const u = new URL("https://a.com/x"); u.port = "80"; return P(u); })"#,
                r#"["https://a.com:80/x", "https://a.com:80", "https:", "", "", "a.com:80", "a.com", "80", "/x", "", ""]"#,
            ),
        ],
    },
    Case {
        name: "setters-https-2",
        html: r"<!DOCTYPE html><html><head></head><body><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : v; } catch (e) { return (e instanceof DOMException ? 'DOMException ' + e.name + ' [' + e.code + ']' : e.constructor.name) + ': ' + e.message; } }
function P(u) { return [u.href, u.origin, u.protocol, u.username, u.password, u.host, u.hostname, u.port, u.pathname, u.search, u.hash]; }
function S(init) { return [...new URLSearchParams(init)].map((p) => p.join('=')).join('&'); }
</script></body></html>",
        queries: &[
            (
                r#"E(() => { const u = new URL("https://a.com/x"); u.port = "443"; return P(u); })"#,
                r#"["https://a.com/x", "https://a.com", "https:", "", "", "a.com", "a.com", "", "/x", "", ""]"#,
            ),
            (
                r#"E(() => { const u = new URL("https://a.com/x"); u.port = " 81"; return P(u); })"#,
                r#"["https://a.com/x", "https://a.com", "https:", "", "", "a.com", "a.com", "", "/x", "", ""]"#,
            ),
            (
                r#"E(() => { const u = new URL("https://a.com/x"); u.port = "8\\t1"; return P(u); })"#,
                r#"["https://a.com:8/x", "https://a.com:8", "https:", "", "", "a.com:8", "a.com", "8", "/x", "", ""]"#,
            ),
            (
                r#"E(() => { const u = new URL("https://a.com/x"); u.port = "-1"; return P(u); })"#,
                r#"["https://a.com/x", "https://a.com", "https:", "", "", "a.com", "a.com", "", "/x", "", ""]"#,
            ),
            (
                r#"E(() => { const u = new URL("https://a.com/x"); u.port = "1e3"; return P(u); })"#,
                r#"["https://a.com:1/x", "https://a.com:1", "https:", "", "", "a.com:1", "a.com", "1", "/x", "", ""]"#,
            ),
            (
                r#"E(() => { const u = new URL("https://a.com/x"); u.pathname = "/a/b"; return P(u); })"#,
                r#"["https://a.com/a/b", "https://a.com", "https:", "", "", "a.com", "a.com", "", "/a/b", "", ""]"#,
            ),
            (
                r#"E(() => { const u = new URL("https://a.com/x"); u.pathname = "a/b"; return P(u); })"#,
                r#"["https://a.com/a/b", "https://a.com", "https:", "", "", "a.com", "a.com", "", "/a/b", "", ""]"#,
            ),
            (
                r#"E(() => { const u = new URL("https://a.com/x"); u.pathname = ""; return P(u); })"#,
                r#"["https://a.com/", "https://a.com", "https:", "", "", "a.com", "a.com", "", "/", "", ""]"#,
            ),
            (
                r#"E(() => { const u = new URL("https://a.com/x"); u.pathname = "/a b?c#d%e"; return P(u); })"#,
                r#"["https://a.com/a%20b%3Fc%23d%e", "https://a.com", "https:", "", "", "a.com", "a.com", "", "/a%20b%3Fc%23d%e", "", ""]"#,
            ),
            (
                r#"E(() => { const u = new URL("https://a.com/x"); u.pathname = "/a/../b/./c"; return P(u); })"#,
                r#"["https://a.com/b/c", "https://a.com", "https:", "", "", "a.com", "a.com", "", "/b/c", "", ""]"#,
            ),
            (
                r#"E(() => { const u = new URL("https://a.com/x"); u.pathname = "\\\\a\\\\b"; return P(u); })"#,
                r#"["https://a.com//a//b", "https://a.com", "https:", "", "", "a.com", "a.com", "", "//a//b", "", ""]"#,
            ),
            (
                r#"E(() => { const u = new URL("https://a.com/x"); u.pathname = "/\u00e9\ud83d\ude00"; return P(u); })"#,
                r#"["https://a.com/%C3%A9%F0%9F%98%80", "https://a.com", "https:", "", "", "a.com", "a.com", "", "/%C3%A9%F0%9F%98%80", "", ""]"#,
            ),
            (
                r#"E(() => { const u = new URL("https://a.com/x"); u.pathname = "//x"; return P(u); })"#,
                r#"["https://a.com//x", "https://a.com", "https:", "", "", "a.com", "a.com", "", "//x", "", ""]"#,
            ),
            (
                r#"E(() => { const u = new URL("https://a.com/x"); u.pathname = "/%41/%zz"; return P(u); })"#,
                r#"["https://a.com/%41/%zz", "https://a.com", "https:", "", "", "a.com", "a.com", "", "/%41/%zz", "", ""]"#,
            ),
            (
                r#"E(() => { const u = new URL("https://a.com/x"); u.search = "q=1"; return P(u); })"#,
                r#"["https://a.com/x?q=1", "https://a.com", "https:", "", "", "a.com", "a.com", "", "/x", "?q=1", ""]"#,
            ),
            (
                r#"E(() => { const u = new URL("https://a.com/x"); u.search = "?q=1"; return P(u); })"#,
                r#"["https://a.com/x?q=1", "https://a.com", "https:", "", "", "a.com", "a.com", "", "/x", "?q=1", ""]"#,
            ),
            (
                r#"E(() => { const u = new URL("https://a.com/x"); u.search = "??q=1"; return P(u); })"#,
                r#"["https://a.com/x??q=1", "https://a.com", "https:", "", "", "a.com", "a.com", "", "/x", "??q=1", ""]"#,
            ),
            (
                r#"E(() => { const u = new URL("https://a.com/x"); u.search = ""; return P(u); })"#,
                r#"["https://a.com/x", "https://a.com", "https:", "", "", "a.com", "a.com", "", "/x", "", ""]"#,
            ),
            (
                r#"E(() => { const u = new URL("https://a.com/x"); u.search = "?"; return P(u); })"#,
                r#"["https://a.com/x", "https://a.com", "https:", "", "", "a.com", "a.com", "", "/x", "", ""]"#,
            ),
            (
                r#"E(() => { const u = new URL("https://a.com/x"); u.search = "a b&c='d'#e"; return P(u); })"#,
                r#"["https://a.com/x?a%20b&c=%27d%27%23e", "https://a.com", "https:", "", "", "a.com", "a.com", "", "/x", "?a%20b&c=%27d%27%23e", ""]"#,
            ),
            (
                r#"E(() => { const u = new URL("https://a.com/x"); u.search = "\u00e9\ud83d\ude00"; return P(u); })"#,
                r#"["https://a.com/x?%C3%A9%F0%9F%98%80", "https://a.com", "https:", "", "", "a.com", "a.com", "", "/x", "?%C3%A9%F0%9F%98%80", ""]"#,
            ),
            (
                r#"E(() => { const u = new URL("https://a.com/x"); u.search = "q=1\\n2"; return P(u); })"#,
                r#"["https://a.com/x?q=1\\n2", "https://a.com", "https:", "", "", "a.com", "a.com", "", "/x", "?q=1\\n2", ""]"#,
            ),
            (
                r#"E(() => { const u = new URL("https://a.com/x"); u.hash = "f"; return P(u); })"#,
                r##"["https://a.com/x#f", "https://a.com", "https:", "", "", "a.com", "a.com", "", "/x", "", "#f"]"##,
            ),
            (
                r##"E(() => { const u = new URL("https://a.com/x"); u.hash = "#f"; return P(u); })"##,
                r##"["https://a.com/x#f", "https://a.com", "https:", "", "", "a.com", "a.com", "", "/x", "", "#f"]"##,
            ),
            (
                r###"E(() => { const u = new URL("https://a.com/x"); u.hash = "##f"; return P(u); })"###,
                r###"["https://a.com/x##f", "https://a.com", "https:", "", "", "a.com", "a.com", "", "/x", "", "##f"]"###,
            ),
            (
                r#"E(() => { const u = new URL("https://a.com/x"); u.hash = ""; return P(u); })"#,
                r#"["https://a.com/x", "https://a.com", "https:", "", "", "a.com", "a.com", "", "/x", "", ""]"#,
            ),
            (
                r##"E(() => { const u = new URL("https://a.com/x"); u.hash = "#"; return P(u); })"##,
                r#"["https://a.com/x#", "https://a.com", "https:", "", "", "a.com", "a.com", "", "/x", "", ""]"#,
            ),
            (
                r#"E(() => { const u = new URL("https://a.com/x"); u.hash = "a b<>`\"'\u00e9"; return P(u); })"#,
                r##"["https://a.com/x#a%20b%3C%3E%60%22'%C3%A9", "https://a.com", "https:", "", "", "a.com", "a.com", "", "/x", "", "#a%20b%3C%3E%60%22'%C3%A9"]"##,
            ),
            (
                r#"E(() => { const u = new URL("https://a.com/x"); u.hash = "f\\t1"; return P(u); })"#,
                r##"["https://a.com/x#f\\t1", "https://a.com", "https:", "", "", "a.com", "a.com", "", "/x", "", "#f\\t1"]"##,
            ),
        ],
    },
    Case {
        name: "setters-file-0",
        html: r"<!DOCTYPE html><html><head></head><body><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : v; } catch (e) { return (e instanceof DOMException ? 'DOMException ' + e.name + ' [' + e.code + ']' : e.constructor.name) + ': ' + e.message; } }
function P(u) { return [u.href, u.origin, u.protocol, u.username, u.password, u.host, u.hostname, u.port, u.pathname, u.search, u.hash]; }
function S(init) { return [...new URLSearchParams(init)].map((p) => p.join('=')).join('&'); }
</script></body></html>",
        queries: &[
            (
                r#"E(() => { const u = new URL("file:///dir/f.txt?q#h"); u.href = "https://x.test/p?q#h"; return P(u); })"#,
                r##"["https://x.test/p?q#h", "https://x.test", "https:", "", "", "x.test", "x.test", "", "/p", "?q", "#h"]"##,
            ),
            (
                r#"E(() => { const u = new URL("file:///dir/f.txt?q#h"); u.href = "foo"; return P(u); })"#,
                r#""TypeError: Failed to set the 'href' property on 'URL': Invalid URL""#,
            ),
            (
                r#"E(() => { const u = new URL("file:///dir/f.txt?q#h"); u.href = ""; return P(u); })"#,
                r#""TypeError: Failed to set the 'href' property on 'URL': Invalid URL""#,
            ),
            (
                r#"E(() => { const u = new URL("file:///dir/f.txt?q#h"); u.href = "mailto:a@b"; return P(u); })"#,
                r#"["mailto:a@b", "null", "mailto:", "", "", "", "", "", "a@b", "", ""]"#,
            ),
            (
                r#"E(() => { const u = new URL("file:///dir/f.txt?q#h"); u.protocol = "https"; return P(u); })"#,
                r##"["file:///dir/f.txt?q#h", "file://", "file:", "", "", "", "", "", "/dir/f.txt", "?q", "#h"]"##,
            ),
            (
                r#"E(() => { const u = new URL("file:///dir/f.txt?q#h"); u.protocol = "https:"; return P(u); })"#,
                r##"["file:///dir/f.txt?q#h", "file://", "file:", "", "", "", "", "", "/dir/f.txt", "?q", "#h"]"##,
            ),
            (
                r#"E(() => { const u = new URL("file:///dir/f.txt?q#h"); u.protocol = "https://x"; return P(u); })"#,
                r##"["file:///dir/f.txt?q#h", "file://", "file:", "", "", "", "", "", "/dir/f.txt", "?q", "#h"]"##,
            ),
            (
                r#"E(() => { const u = new URL("file:///dir/f.txt?q#h"); u.protocol = "HTTPS:abc"; return P(u); })"#,
                r##"["file:///dir/f.txt?q#h", "file://", "file:", "", "", "", "", "", "/dir/f.txt", "?q", "#h"]"##,
            ),
            (
                r#"E(() => { const u = new URL("file:///dir/f.txt?q#h"); u.protocol = "ftp"; return P(u); })"#,
                r##"["file:///dir/f.txt?q#h", "file://", "file:", "", "", "", "", "", "/dir/f.txt", "?q", "#h"]"##,
            ),
            (
                r#"E(() => { const u = new URL("file:///dir/f.txt?q#h"); u.protocol = "ws"; return P(u); })"#,
                r##"["file:///dir/f.txt?q#h", "file://", "file:", "", "", "", "", "", "/dir/f.txt", "?q", "#h"]"##,
            ),
            (
                r#"E(() => { const u = new URL("file:///dir/f.txt?q#h"); u.protocol = "file"; return P(u); })"#,
                r##"["file:///dir/f.txt?q#h", "file://", "file:", "", "", "", "", "", "/dir/f.txt", "?q", "#h"]"##,
            ),
            (
                r#"E(() => { const u = new URL("file:///dir/f.txt?q#h"); u.protocol = "foo"; return P(u); })"#,
                r##"["file:///dir/f.txt?q#h", "file://", "file:", "", "", "", "", "", "/dir/f.txt", "?q", "#h"]"##,
            ),
            (
                r#"E(() => { const u = new URL("file:///dir/f.txt?q#h"); u.protocol = "mailto"; return P(u); })"#,
                r##"["file:///dir/f.txt?q#h", "file://", "file:", "", "", "", "", "", "/dir/f.txt", "?q", "#h"]"##,
            ),
            (
                r#"E(() => { const u = new URL("file:///dir/f.txt?q#h"); u.protocol = "1x"; return P(u); })"#,
                r##"["file:///dir/f.txt?q#h", "file://", "file:", "", "", "", "", "", "/dir/f.txt", "?q", "#h"]"##,
            ),
            (
                r#"E(() => { const u = new URL("file:///dir/f.txt?q#h"); u.protocol = ""; return P(u); })"#,
                r##"["file:///dir/f.txt?q#h", "file://", "file:", "", "", "", "", "", "/dir/f.txt", "?q", "#h"]"##,
            ),
            (
                r#"E(() => { const u = new URL("file:///dir/f.txt?q#h"); u.protocol = "x y"; return P(u); })"#,
                r##"["file:///dir/f.txt?q#h", "file://", "file:", "", "", "", "", "", "/dir/f.txt", "?q", "#h"]"##,
            ),
            (
                r#"E(() => { const u = new URL("file:///dir/f.txt?q#h"); u.protocol = "http"; return P(u); })"#,
                r##"["file:///dir/f.txt?q#h", "file://", "file:", "", "", "", "", "", "/dir/f.txt", "?q", "#h"]"##,
            ),
            (
                r#"E(() => { const u = new URL("file:///dir/f.txt?q#h"); u.username = "u"; return P(u); })"#,
                r##"["file:///dir/f.txt?q#h", "file://", "file:", "", "", "", "", "", "/dir/f.txt", "?q", "#h"]"##,
            ),
            (
                r#"E(() => { const u = new URL("file:///dir/f.txt?q#h"); u.username = "u s:r@/?#\u00e9"; return P(u); })"#,
                r##"["file:///dir/f.txt?q#h", "file://", "file:", "", "", "", "", "", "/dir/f.txt", "?q", "#h"]"##,
            ),
            (
                r#"E(() => { const u = new URL("file:///dir/f.txt?q#h"); u.username = ""; return P(u); })"#,
                r##"["file:///dir/f.txt?q#h", "file://", "file:", "", "", "", "", "", "/dir/f.txt", "?q", "#h"]"##,
            ),
            (
                r#"E(() => { const u = new URL("file:///dir/f.txt?q#h"); u.password = "p"; return P(u); })"#,
                r##"["file:///dir/f.txt?q#h", "file://", "file:", "", "", "", "", "", "/dir/f.txt", "?q", "#h"]"##,
            ),
            (
                r#"E(() => { const u = new URL("file:///dir/f.txt?q#h"); u.password = "p s:r@/?#\u00e9"; return P(u); })"#,
                r##"["file:///dir/f.txt?q#h", "file://", "file:", "", "", "", "", "", "/dir/f.txt", "?q", "#h"]"##,
            ),
            (
                r#"E(() => { const u = new URL("file:///dir/f.txt?q#h"); u.password = ""; return P(u); })"#,
                r##"["file:///dir/f.txt?q#h", "file://", "file:", "", "", "", "", "", "/dir/f.txt", "?q", "#h"]"##,
            ),
            (
                r#"E(() => { const u = new URL("file:///dir/f.txt?q#h"); u.host = "h.test"; return P(u); })"#,
                r##"["file://h.test/dir/f.txt?q#h", "file://", "file:", "", "", "h.test", "h.test", "", "/dir/f.txt", "?q", "#h"]"##,
            ),
            (
                r#"E(() => { const u = new URL("file:///dir/f.txt?q#h"); u.host = "h.test:81"; return P(u); })"#,
                r##"["file://h.test/dir/f.txt?q#h", "file://", "file:", "", "", "h.test", "h.test", "", "/dir/f.txt", "?q", "#h"]"##,
            ),
            (
                r#"E(() => { const u = new URL("file:///dir/f.txt?q#h"); u.host = "h.test:"; return P(u); })"#,
                r##"["file://h.test/dir/f.txt?q#h", "file://", "file:", "", "", "h.test", "h.test", "", "/dir/f.txt", "?q", "#h"]"##,
            ),
            (
                r#"E(() => { const u = new URL("file:///dir/f.txt?q#h"); u.host = "h.test:99999"; return P(u); })"#,
                r##"["file://h.test/dir/f.txt?q#h", "file://", "file:", "", "", "h.test", "h.test", "", "/dir/f.txt", "?q", "#h"]"##,
            ),
            (
                r#"E(() => { const u = new URL("file:///dir/f.txt?q#h"); u.host = "h.test:81abc"; return P(u); })"#,
                r##"["file://h.test/dir/f.txt?q#h", "file://", "file:", "", "", "h.test", "h.test", "", "/dir/f.txt", "?q", "#h"]"##,
            ),
            (
                r#"E(() => { const u = new URL("file:///dir/f.txt?q#h"); u.host = "h.test/path"; return P(u); })"#,
                r##"["file://h.test/dir/f.txt?q#h", "file://", "file:", "", "", "h.test", "h.test", "", "/dir/f.txt", "?q", "#h"]"##,
            ),
            (
                r#"E(() => { const u = new URL("file:///dir/f.txt?q#h"); u.host = "h.test?x"; return P(u); })"#,
                r##"["file://h.test/dir/f.txt?q#h", "file://", "file:", "", "", "h.test", "h.test", "", "/dir/f.txt", "?q", "#h"]"##,
            ),
        ],
    },
    Case {
        name: "setters-file-1",
        html: r"<!DOCTYPE html><html><head></head><body><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : v; } catch (e) { return (e instanceof DOMException ? 'DOMException ' + e.name + ' [' + e.code + ']' : e.constructor.name) + ': ' + e.message; } }
function P(u) { return [u.href, u.origin, u.protocol, u.username, u.password, u.host, u.hostname, u.port, u.pathname, u.search, u.hash]; }
function S(init) { return [...new URLSearchParams(init)].map((p) => p.join('=')).join('&'); }
</script></body></html>",
        queries: &[
            (
                r#"E(() => { const u = new URL("file:///dir/f.txt?q#h"); u.host = "h.test#x"; return P(u); })"#,
                r##"["file://h.test/dir/f.txt?q#h", "file://", "file:", "", "", "h.test", "h.test", "", "/dir/f.txt", "?q", "#h"]"##,
            ),
            (
                r#"E(() => { const u = new URL("file:///dir/f.txt?q#h"); u.host = "h.test\\\\x"; return P(u); })"#,
                r##"["file://h.test/dir/f.txt?q#h", "file://", "file:", "", "", "h.test", "h.test", "", "/dir/f.txt", "?q", "#h"]"##,
            ),
            (
                r#"E(() => { const u = new URL("file:///dir/f.txt?q#h"); u.host = "[::1]:3000"; return P(u); })"#,
                r##"["file://[::1]/dir/f.txt?q#h", "file://", "file:", "", "", "[::1]", "[::1]", "", "/dir/f.txt", "?q", "#h"]"##,
            ),
            (
                r#"E(() => { const u = new URL("file:///dir/f.txt?q#h"); u.host = "[::1"; return P(u); })"#,
                r##"["file:///dir/f.txt?q#h", "file://", "file:", "", "", "", "", "", "/dir/f.txt", "?q", "#h"]"##,
            ),
            (
                r#"E(() => { const u = new URL("file:///dir/f.txt?q#h"); u.host = ""; return P(u); })"#,
                r##"["file:///dir/f.txt?q#h", "file://", "file:", "", "", "", "", "", "/dir/f.txt", "?q", "#h"]"##,
            ),
            (
                r#"E(() => { const u = new URL("file:///dir/f.txt?q#h"); u.host = "a b"; return P(u); })"#,
                r##"["file://a%20b/dir/f.txt?q#h", "file://", "file:", "", "", "a%20b", "a%20b", "", "/dir/f.txt", "?q", "#h"]"##,
            ),
            (
                r#"E(() => { const u = new URL("file:///dir/f.txt?q#h"); u.host = "A.B"; return P(u); })"#,
                r##"["file://a.b/dir/f.txt?q#h", "file://", "file:", "", "", "a.b", "a.b", "", "/dir/f.txt", "?q", "#h"]"##,
            ),
            (
                r#"E(() => { const u = new URL("file:///dir/f.txt?q#h"); u.host = ":81"; return P(u); })"#,
                r##"["file:///dir/f.txt?q#h", "file://", "file:", "", "", "", "", "", "/dir/f.txt", "?q", "#h"]"##,
            ),
            (
                r#"E(() => { const u = new URL("file:///dir/f.txt?q#h"); u.host = "h.test:80"; return P(u); })"#,
                r##"["file://h.test/dir/f.txt?q#h", "file://", "file:", "", "", "h.test", "h.test", "", "/dir/f.txt", "?q", "#h"]"##,
            ),
            (
                r#"E(() => { const u = new URL("file:///dir/f.txt?q#h"); u.host = "h.test:443"; return P(u); })"#,
                r##"["file://h.test/dir/f.txt?q#h", "file://", "file:", "", "", "h.test", "h.test", "", "/dir/f.txt", "?q", "#h"]"##,
            ),
            (
                r#"E(() => { const u = new URL("file:///dir/f.txt?q#h"); u.host = "m\u00fcnchen.de"; return P(u); })"#,
                r##"["file://xn--mnchen-3ya.de/dir/f.txt?q#h", "file://", "file:", "", "", "xn--mnchen-3ya.de", "xn--mnchen-3ya.de", "", "/dir/f.txt", "?q", "#h"]"##,
            ),
            (
                r#"E(() => { const u = new URL("file:///dir/f.txt?q#h"); u.host = "127.1"; return P(u); })"#,
                r##"["file://127.0.0.1/dir/f.txt?q#h", "file://", "file:", "", "", "127.0.0.1", "127.0.0.1", "", "/dir/f.txt", "?q", "#h"]"##,
            ),
            (
                r#"E(() => { const u = new URL("file:///dir/f.txt?q#h"); u.hostname = "h.test"; return P(u); })"#,
                r##"["file://h.test/dir/f.txt?q#h", "file://", "file:", "", "", "h.test", "h.test", "", "/dir/f.txt", "?q", "#h"]"##,
            ),
            (
                r#"E(() => { const u = new URL("file:///dir/f.txt?q#h"); u.hostname = "h.test:81"; return P(u); })"#,
                r##"["file:///dir/f.txt?q#h", "file://", "file:", "", "", "", "", "", "/dir/f.txt", "?q", "#h"]"##,
            ),
            (
                r#"E(() => { const u = new URL("file:///dir/f.txt?q#h"); u.hostname = "h.test/x"; return P(u); })"#,
                r##"["file://h.test/dir/f.txt?q#h", "file://", "file:", "", "", "h.test", "h.test", "", "/dir/f.txt", "?q", "#h"]"##,
            ),
            (
                r#"E(() => { const u = new URL("file:///dir/f.txt?q#h"); u.hostname = "h.test?x"; return P(u); })"#,
                r##"["file://h.test/dir/f.txt?q#h", "file://", "file:", "", "", "h.test", "h.test", "", "/dir/f.txt", "?q", "#h"]"##,
            ),
            (
                r#"E(() => { const u = new URL("file:///dir/f.txt?q#h"); u.hostname = "[::1]"; return P(u); })"#,
                r##"["file://[::1]/dir/f.txt?q#h", "file://", "file:", "", "", "[::1]", "[::1]", "", "/dir/f.txt", "?q", "#h"]"##,
            ),
            (
                r#"E(() => { const u = new URL("file:///dir/f.txt?q#h"); u.hostname = ""; return P(u); })"#,
                r##"["file:///dir/f.txt?q#h", "file://", "file:", "", "", "", "", "", "/dir/f.txt", "?q", "#h"]"##,
            ),
            (
                r#"E(() => { const u = new URL("file:///dir/f.txt?q#h"); u.hostname = "a b"; return P(u); })"#,
                r##"["file://a%20b/dir/f.txt?q#h", "file://", "file:", "", "", "a%20b", "a%20b", "", "/dir/f.txt", "?q", "#h"]"##,
            ),
            (
                r#"E(() => { const u = new URL("file:///dir/f.txt?q#h"); u.hostname = "A.B"; return P(u); })"#,
                r##"["file://a.b/dir/f.txt?q#h", "file://", "file:", "", "", "a.b", "a.b", "", "/dir/f.txt", "?q", "#h"]"##,
            ),
            (
                r#"E(() => { const u = new URL("file:///dir/f.txt?q#h"); u.port = "81"; return P(u); })"#,
                r##"["file:///dir/f.txt?q#h", "file://", "file:", "", "", "", "", "", "/dir/f.txt", "?q", "#h"]"##,
            ),
            (
                r#"E(() => { const u = new URL("file:///dir/f.txt?q#h"); u.port = "81abc"; return P(u); })"#,
                r##"["file:///dir/f.txt?q#h", "file://", "file:", "", "", "", "", "", "/dir/f.txt", "?q", "#h"]"##,
            ),
            (
                r#"E(() => { const u = new URL("file:///dir/f.txt?q#h"); u.port = "abc"; return P(u); })"#,
                r##"["file:///dir/f.txt?q#h", "file://", "file:", "", "", "", "", "", "/dir/f.txt", "?q", "#h"]"##,
            ),
            (
                r#"E(() => { const u = new URL("file:///dir/f.txt?q#h"); u.port = ""; return P(u); })"#,
                r##"["file:///dir/f.txt?q#h", "file://", "file:", "", "", "", "", "", "/dir/f.txt", "?q", "#h"]"##,
            ),
            (
                r#"E(() => { const u = new URL("file:///dir/f.txt?q#h"); u.port = "99999"; return P(u); })"#,
                r##"["file:///dir/f.txt?q#h", "file://", "file:", "", "", "", "", "", "/dir/f.txt", "?q", "#h"]"##,
            ),
            (
                r#"E(() => { const u = new URL("file:///dir/f.txt?q#h"); u.port = "65535"; return P(u); })"#,
                r##"["file:///dir/f.txt?q#h", "file://", "file:", "", "", "", "", "", "/dir/f.txt", "?q", "#h"]"##,
            ),
            (
                r#"E(() => { const u = new URL("file:///dir/f.txt?q#h"); u.port = "65536"; return P(u); })"#,
                r##"["file:///dir/f.txt?q#h", "file://", "file:", "", "", "", "", "", "/dir/f.txt", "?q", "#h"]"##,
            ),
            (
                r#"E(() => { const u = new URL("file:///dir/f.txt?q#h"); u.port = "0"; return P(u); })"#,
                r##"["file:///dir/f.txt?q#h", "file://", "file:", "", "", "", "", "", "/dir/f.txt", "?q", "#h"]"##,
            ),
            (
                r#"E(() => { const u = new URL("file:///dir/f.txt?q#h"); u.port = "00081"; return P(u); })"#,
                r##"["file:///dir/f.txt?q#h", "file://", "file:", "", "", "", "", "", "/dir/f.txt", "?q", "#h"]"##,
            ),
            (
                r#"E(() => { const u = new URL("file:///dir/f.txt?q#h"); u.port = "80"; return P(u); })"#,
                r##"["file:///dir/f.txt?q#h", "file://", "file:", "", "", "", "", "", "/dir/f.txt", "?q", "#h"]"##,
            ),
        ],
    },
    Case {
        name: "setters-file-2",
        html: r"<!DOCTYPE html><html><head></head><body><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : v; } catch (e) { return (e instanceof DOMException ? 'DOMException ' + e.name + ' [' + e.code + ']' : e.constructor.name) + ': ' + e.message; } }
function P(u) { return [u.href, u.origin, u.protocol, u.username, u.password, u.host, u.hostname, u.port, u.pathname, u.search, u.hash]; }
function S(init) { return [...new URLSearchParams(init)].map((p) => p.join('=')).join('&'); }
</script></body></html>",
        queries: &[
            (
                r#"E(() => { const u = new URL("file:///dir/f.txt?q#h"); u.port = "443"; return P(u); })"#,
                r##"["file:///dir/f.txt?q#h", "file://", "file:", "", "", "", "", "", "/dir/f.txt", "?q", "#h"]"##,
            ),
            (
                r#"E(() => { const u = new URL("file:///dir/f.txt?q#h"); u.port = " 81"; return P(u); })"#,
                r##"["file:///dir/f.txt?q#h", "file://", "file:", "", "", "", "", "", "/dir/f.txt", "?q", "#h"]"##,
            ),
            (
                r#"E(() => { const u = new URL("file:///dir/f.txt?q#h"); u.port = "8\\t1"; return P(u); })"#,
                r##"["file:///dir/f.txt?q#h", "file://", "file:", "", "", "", "", "", "/dir/f.txt", "?q", "#h"]"##,
            ),
            (
                r#"E(() => { const u = new URL("file:///dir/f.txt?q#h"); u.port = "-1"; return P(u); })"#,
                r##"["file:///dir/f.txt?q#h", "file://", "file:", "", "", "", "", "", "/dir/f.txt", "?q", "#h"]"##,
            ),
            (
                r#"E(() => { const u = new URL("file:///dir/f.txt?q#h"); u.port = "1e3"; return P(u); })"#,
                r##"["file:///dir/f.txt?q#h", "file://", "file:", "", "", "", "", "", "/dir/f.txt", "?q", "#h"]"##,
            ),
            (
                r#"E(() => { const u = new URL("file:///dir/f.txt?q#h"); u.pathname = "/a/b"; return P(u); })"#,
                r##"["file:///a/b?q#h", "file://", "file:", "", "", "", "", "", "/a/b", "?q", "#h"]"##,
            ),
            (
                r#"E(() => { const u = new URL("file:///dir/f.txt?q#h"); u.pathname = "a/b"; return P(u); })"#,
                r##"["file:///a/b?q#h", "file://", "file:", "", "", "", "", "", "/a/b", "?q", "#h"]"##,
            ),
            (
                r#"E(() => { const u = new URL("file:///dir/f.txt?q#h"); u.pathname = ""; return P(u); })"#,
                r##"["file:///?q#h", "file://", "file:", "", "", "", "", "", "/", "?q", "#h"]"##,
            ),
            (
                r#"E(() => { const u = new URL("file:///dir/f.txt?q#h"); u.pathname = "/a b?c#d%e"; return P(u); })"#,
                r##"["file:///a%20b%3Fc%23d%e?q#h", "file://", "file:", "", "", "", "", "", "/a%20b%3Fc%23d%e", "?q", "#h"]"##,
            ),
            (
                r#"E(() => { const u = new URL("file:///dir/f.txt?q#h"); u.pathname = "/a/../b/./c"; return P(u); })"#,
                r##"["file:///b/c?q#h", "file://", "file:", "", "", "", "", "", "/b/c", "?q", "#h"]"##,
            ),
            (
                r#"E(() => { const u = new URL("file:///dir/f.txt?q#h"); u.pathname = "\\\\a\\\\b"; return P(u); })"#,
                r##"["file:////a//b?q#h", "file://", "file:", "", "", "", "", "", "//a//b", "?q", "#h"]"##,
            ),
            (
                r#"E(() => { const u = new URL("file:///dir/f.txt?q#h"); u.pathname = "/\u00e9\ud83d\ude00"; return P(u); })"#,
                r##"["file:///%C3%A9%F0%9F%98%80?q#h", "file://", "file:", "", "", "", "", "", "/%C3%A9%F0%9F%98%80", "?q", "#h"]"##,
            ),
            (
                r#"E(() => { const u = new URL("file:///dir/f.txt?q#h"); u.pathname = "//x"; return P(u); })"#,
                r##"["file:////x?q#h", "file://", "file:", "", "", "", "", "", "//x", "?q", "#h"]"##,
            ),
            (
                r#"E(() => { const u = new URL("file:///dir/f.txt?q#h"); u.pathname = "/%41/%zz"; return P(u); })"#,
                r##"["file:///%41/%zz?q#h", "file://", "file:", "", "", "", "", "", "/%41/%zz", "?q", "#h"]"##,
            ),
            (
                r#"E(() => { const u = new URL("file:///dir/f.txt?q#h"); u.search = "q=1"; return P(u); })"#,
                r##"["file:///dir/f.txt?q=1#h", "file://", "file:", "", "", "", "", "", "/dir/f.txt", "?q=1", "#h"]"##,
            ),
            (
                r#"E(() => { const u = new URL("file:///dir/f.txt?q#h"); u.search = "?q=1"; return P(u); })"#,
                r##"["file:///dir/f.txt?q=1#h", "file://", "file:", "", "", "", "", "", "/dir/f.txt", "?q=1", "#h"]"##,
            ),
            (
                r#"E(() => { const u = new URL("file:///dir/f.txt?q#h"); u.search = "??q=1"; return P(u); })"#,
                r##"["file:///dir/f.txt??q=1#h", "file://", "file:", "", "", "", "", "", "/dir/f.txt", "??q=1", "#h"]"##,
            ),
            (
                r#"E(() => { const u = new URL("file:///dir/f.txt?q#h"); u.search = ""; return P(u); })"#,
                r##"["file:///dir/f.txt#h", "file://", "file:", "", "", "", "", "", "/dir/f.txt", "", "#h"]"##,
            ),
            (
                r#"E(() => { const u = new URL("file:///dir/f.txt?q#h"); u.search = "?"; return P(u); })"#,
                r##"["file:///dir/f.txt#h", "file://", "file:", "", "", "", "", "", "/dir/f.txt", "", "#h"]"##,
            ),
            (
                r#"E(() => { const u = new URL("file:///dir/f.txt?q#h"); u.search = "a b&c='d'#e"; return P(u); })"#,
                r##"["file:///dir/f.txt?a%20b&c=%27d%27%23e#h", "file://", "file:", "", "", "", "", "", "/dir/f.txt", "?a%20b&c=%27d%27%23e", "#h"]"##,
            ),
            (
                r#"E(() => { const u = new URL("file:///dir/f.txt?q#h"); u.search = "\u00e9\ud83d\ude00"; return P(u); })"#,
                r##"["file:///dir/f.txt?%C3%A9%F0%9F%98%80#h", "file://", "file:", "", "", "", "", "", "/dir/f.txt", "?%C3%A9%F0%9F%98%80", "#h"]"##,
            ),
            (
                r#"E(() => { const u = new URL("file:///dir/f.txt?q#h"); u.search = "q=1\\n2"; return P(u); })"#,
                r##"["file:///dir/f.txt?q=1\\n2#h", "file://", "file:", "", "", "", "", "", "/dir/f.txt", "?q=1\\n2", "#h"]"##,
            ),
            (
                r#"E(() => { const u = new URL("file:///dir/f.txt?q#h"); u.hash = "f"; return P(u); })"#,
                r##"["file:///dir/f.txt?q#f", "file://", "file:", "", "", "", "", "", "/dir/f.txt", "?q", "#f"]"##,
            ),
            (
                r##"E(() => { const u = new URL("file:///dir/f.txt?q#h"); u.hash = "#f"; return P(u); })"##,
                r##"["file:///dir/f.txt?q#f", "file://", "file:", "", "", "", "", "", "/dir/f.txt", "?q", "#f"]"##,
            ),
            (
                r###"E(() => { const u = new URL("file:///dir/f.txt?q#h"); u.hash = "##f"; return P(u); })"###,
                r###"["file:///dir/f.txt?q##f", "file://", "file:", "", "", "", "", "", "/dir/f.txt", "?q", "##f"]"###,
            ),
            (
                r#"E(() => { const u = new URL("file:///dir/f.txt?q#h"); u.hash = ""; return P(u); })"#,
                r#"["file:///dir/f.txt?q", "file://", "file:", "", "", "", "", "", "/dir/f.txt", "?q", ""]"#,
            ),
            (
                r##"E(() => { const u = new URL("file:///dir/f.txt?q#h"); u.hash = "#"; return P(u); })"##,
                r#"["file:///dir/f.txt?q#", "file://", "file:", "", "", "", "", "", "/dir/f.txt", "?q", ""]"#,
            ),
            (
                r#"E(() => { const u = new URL("file:///dir/f.txt?q#h"); u.hash = "a b<>`\"'\u00e9"; return P(u); })"#,
                r##"["file:///dir/f.txt?q#a%20b%3C%3E%60%22'%C3%A9", "file://", "file:", "", "", "", "", "", "/dir/f.txt", "?q", "#a%20b%3C%3E%60%22'%C3%A9"]"##,
            ),
            (
                r#"E(() => { const u = new URL("file:///dir/f.txt?q#h"); u.hash = "f\\t1"; return P(u); })"#,
                r##"["file:///dir/f.txt?q#f\\t1", "file://", "file:", "", "", "", "", "", "/dir/f.txt", "?q", "#f\\t1"]"##,
            ),
        ],
    },
    Case {
        name: "setters-mailto-0",
        html: r"<!DOCTYPE html><html><head></head><body><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : v; } catch (e) { return (e instanceof DOMException ? 'DOMException ' + e.name + ' [' + e.code + ']' : e.constructor.name) + ': ' + e.message; } }
function P(u) { return [u.href, u.origin, u.protocol, u.username, u.password, u.host, u.hostname, u.port, u.pathname, u.search, u.hash]; }
function S(init) { return [...new URLSearchParams(init)].map((p) => p.join('=')).join('&'); }
</script></body></html>",
        queries: &[
            (
                r#"E(() => { const u = new URL("mailto:a@b.c?s=1#f"); u.href = "https://x.test/p?q#h"; return P(u); })"#,
                r##"["https://x.test/p?q#h", "https://x.test", "https:", "", "", "x.test", "x.test", "", "/p", "?q", "#h"]"##,
            ),
            (
                r#"E(() => { const u = new URL("mailto:a@b.c?s=1#f"); u.href = "foo"; return P(u); })"#,
                r#""TypeError: Failed to set the 'href' property on 'URL': Invalid URL""#,
            ),
            (
                r#"E(() => { const u = new URL("mailto:a@b.c?s=1#f"); u.href = ""; return P(u); })"#,
                r#""TypeError: Failed to set the 'href' property on 'URL': Invalid URL""#,
            ),
            (
                r#"E(() => { const u = new URL("mailto:a@b.c?s=1#f"); u.href = "mailto:a@b"; return P(u); })"#,
                r#"["mailto:a@b", "null", "mailto:", "", "", "", "", "", "a@b", "", ""]"#,
            ),
            (
                r#"E(() => { const u = new URL("mailto:a@b.c?s=1#f"); u.protocol = "https"; return P(u); })"#,
                r##"["https://a@b.c/?s=1#f", "https://b.c", "https:", "a", "", "b.c", "b.c", "", "/", "?s=1", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("mailto:a@b.c?s=1#f"); u.protocol = "https:"; return P(u); })"#,
                r##"["https://a@b.c/?s=1#f", "https://b.c", "https:", "a", "", "b.c", "b.c", "", "/", "?s=1", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("mailto:a@b.c?s=1#f"); u.protocol = "https://x"; return P(u); })"#,
                r##"["https://a@b.c/?s=1#f", "https://b.c", "https:", "a", "", "b.c", "b.c", "", "/", "?s=1", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("mailto:a@b.c?s=1#f"); u.protocol = "HTTPS:abc"; return P(u); })"#,
                r##"["https://a@b.c/?s=1#f", "https://b.c", "https:", "a", "", "b.c", "b.c", "", "/", "?s=1", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("mailto:a@b.c?s=1#f"); u.protocol = "ftp"; return P(u); })"#,
                r##"["ftp://a@b.c/?s=1#f", "ftp://b.c", "ftp:", "a", "", "b.c", "b.c", "", "/", "?s=1", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("mailto:a@b.c?s=1#f"); u.protocol = "ws"; return P(u); })"#,
                r##"["ws://a@b.c/?s=1#f", "ws://b.c", "ws:", "a", "", "b.c", "b.c", "", "/", "?s=1", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("mailto:a@b.c?s=1#f"); u.protocol = "file"; return P(u); })"#,
                r##"["file:///a@b.c?s=1#f", "file://", "file:", "", "", "", "", "", "/a@b.c", "?s=1", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("mailto:a@b.c?s=1#f"); u.protocol = "foo"; return P(u); })"#,
                r##"["foo:a@b.c?s=1#f", "null", "foo:", "", "", "", "", "", "a@b.c", "?s=1", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("mailto:a@b.c?s=1#f"); u.protocol = "mailto"; return P(u); })"#,
                r##"["mailto:a@b.c?s=1#f", "null", "mailto:", "", "", "", "", "", "a@b.c", "?s=1", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("mailto:a@b.c?s=1#f"); u.protocol = "1x"; return P(u); })"#,
                r##"["mailto:a@b.c?s=1#f", "null", "mailto:", "", "", "", "", "", "a@b.c", "?s=1", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("mailto:a@b.c?s=1#f"); u.protocol = ""; return P(u); })"#,
                r##"["mailto:a@b.c?s=1#f", "null", "mailto:", "", "", "", "", "", "a@b.c", "?s=1", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("mailto:a@b.c?s=1#f"); u.protocol = "x y"; return P(u); })"#,
                r##"["mailto:a@b.c?s=1#f", "null", "mailto:", "", "", "", "", "", "a@b.c", "?s=1", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("mailto:a@b.c?s=1#f"); u.protocol = "http"; return P(u); })"#,
                r##"["http://a@b.c/?s=1#f", "http://b.c", "http:", "a", "", "b.c", "b.c", "", "/", "?s=1", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("mailto:a@b.c?s=1#f"); u.username = "u"; return P(u); })"#,
                r##"["mailto:a@b.c?s=1#f", "null", "mailto:", "", "", "", "", "", "a@b.c", "?s=1", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("mailto:a@b.c?s=1#f"); u.username = "u s:r@/?#\u00e9"; return P(u); })"#,
                r##"["mailto:a@b.c?s=1#f", "null", "mailto:", "", "", "", "", "", "a@b.c", "?s=1", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("mailto:a@b.c?s=1#f"); u.username = ""; return P(u); })"#,
                r##"["mailto:a@b.c?s=1#f", "null", "mailto:", "", "", "", "", "", "a@b.c", "?s=1", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("mailto:a@b.c?s=1#f"); u.password = "p"; return P(u); })"#,
                r##"["mailto:a@b.c?s=1#f", "null", "mailto:", "", "", "", "", "", "a@b.c", "?s=1", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("mailto:a@b.c?s=1#f"); u.password = "p s:r@/?#\u00e9"; return P(u); })"#,
                r##"["mailto:a@b.c?s=1#f", "null", "mailto:", "", "", "", "", "", "a@b.c", "?s=1", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("mailto:a@b.c?s=1#f"); u.password = ""; return P(u); })"#,
                r##"["mailto:a@b.c?s=1#f", "null", "mailto:", "", "", "", "", "", "a@b.c", "?s=1", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("mailto:a@b.c?s=1#f"); u.host = "h.test"; return P(u); })"#,
                r##"["mailto:a@b.c?s=1#f", "null", "mailto:", "", "", "", "", "", "a@b.c", "?s=1", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("mailto:a@b.c?s=1#f"); u.host = "h.test:81"; return P(u); })"#,
                r##"["mailto:a@b.c?s=1#f", "null", "mailto:", "", "", "", "", "", "a@b.c", "?s=1", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("mailto:a@b.c?s=1#f"); u.host = "h.test:"; return P(u); })"#,
                r##"["mailto:a@b.c?s=1#f", "null", "mailto:", "", "", "", "", "", "a@b.c", "?s=1", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("mailto:a@b.c?s=1#f"); u.host = "h.test:99999"; return P(u); })"#,
                r##"["mailto:a@b.c?s=1#f", "null", "mailto:", "", "", "", "", "", "a@b.c", "?s=1", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("mailto:a@b.c?s=1#f"); u.host = "h.test:81abc"; return P(u); })"#,
                r##"["mailto:a@b.c?s=1#f", "null", "mailto:", "", "", "", "", "", "a@b.c", "?s=1", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("mailto:a@b.c?s=1#f"); u.host = "h.test/path"; return P(u); })"#,
                r##"["mailto:a@b.c?s=1#f", "null", "mailto:", "", "", "", "", "", "a@b.c", "?s=1", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("mailto:a@b.c?s=1#f"); u.host = "h.test?x"; return P(u); })"#,
                r##"["mailto:a@b.c?s=1#f", "null", "mailto:", "", "", "", "", "", "a@b.c", "?s=1", "#f"]"##,
            ),
        ],
    },
    Case {
        name: "setters-mailto-1",
        html: r"<!DOCTYPE html><html><head></head><body><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : v; } catch (e) { return (e instanceof DOMException ? 'DOMException ' + e.name + ' [' + e.code + ']' : e.constructor.name) + ': ' + e.message; } }
function P(u) { return [u.href, u.origin, u.protocol, u.username, u.password, u.host, u.hostname, u.port, u.pathname, u.search, u.hash]; }
function S(init) { return [...new URLSearchParams(init)].map((p) => p.join('=')).join('&'); }
</script></body></html>",
        queries: &[
            (
                r#"E(() => { const u = new URL("mailto:a@b.c?s=1#f"); u.host = "h.test#x"; return P(u); })"#,
                r##"["mailto:a@b.c?s=1#f", "null", "mailto:", "", "", "", "", "", "a@b.c", "?s=1", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("mailto:a@b.c?s=1#f"); u.host = "h.test\\\\x"; return P(u); })"#,
                r##"["mailto:a@b.c?s=1#f", "null", "mailto:", "", "", "", "", "", "a@b.c", "?s=1", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("mailto:a@b.c?s=1#f"); u.host = "[::1]:3000"; return P(u); })"#,
                r##"["mailto:a@b.c?s=1#f", "null", "mailto:", "", "", "", "", "", "a@b.c", "?s=1", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("mailto:a@b.c?s=1#f"); u.host = "[::1"; return P(u); })"#,
                r##"["mailto:a@b.c?s=1#f", "null", "mailto:", "", "", "", "", "", "a@b.c", "?s=1", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("mailto:a@b.c?s=1#f"); u.host = ""; return P(u); })"#,
                r##"["mailto:a@b.c?s=1#f", "null", "mailto:", "", "", "", "", "", "a@b.c", "?s=1", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("mailto:a@b.c?s=1#f"); u.host = "a b"; return P(u); })"#,
                r##"["mailto:a@b.c?s=1#f", "null", "mailto:", "", "", "", "", "", "a@b.c", "?s=1", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("mailto:a@b.c?s=1#f"); u.host = "A.B"; return P(u); })"#,
                r##"["mailto:a@b.c?s=1#f", "null", "mailto:", "", "", "", "", "", "a@b.c", "?s=1", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("mailto:a@b.c?s=1#f"); u.host = ":81"; return P(u); })"#,
                r##"["mailto:a@b.c?s=1#f", "null", "mailto:", "", "", "", "", "", "a@b.c", "?s=1", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("mailto:a@b.c?s=1#f"); u.host = "h.test:80"; return P(u); })"#,
                r##"["mailto:a@b.c?s=1#f", "null", "mailto:", "", "", "", "", "", "a@b.c", "?s=1", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("mailto:a@b.c?s=1#f"); u.host = "h.test:443"; return P(u); })"#,
                r##"["mailto:a@b.c?s=1#f", "null", "mailto:", "", "", "", "", "", "a@b.c", "?s=1", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("mailto:a@b.c?s=1#f"); u.host = "m\u00fcnchen.de"; return P(u); })"#,
                r##"["mailto:a@b.c?s=1#f", "null", "mailto:", "", "", "", "", "", "a@b.c", "?s=1", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("mailto:a@b.c?s=1#f"); u.host = "127.1"; return P(u); })"#,
                r##"["mailto:a@b.c?s=1#f", "null", "mailto:", "", "", "", "", "", "a@b.c", "?s=1", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("mailto:a@b.c?s=1#f"); u.hostname = "h.test"; return P(u); })"#,
                r##"["mailto:a@b.c?s=1#f", "null", "mailto:", "", "", "", "", "", "a@b.c", "?s=1", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("mailto:a@b.c?s=1#f"); u.hostname = "h.test:81"; return P(u); })"#,
                r##"["mailto:a@b.c?s=1#f", "null", "mailto:", "", "", "", "", "", "a@b.c", "?s=1", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("mailto:a@b.c?s=1#f"); u.hostname = "h.test/x"; return P(u); })"#,
                r##"["mailto:a@b.c?s=1#f", "null", "mailto:", "", "", "", "", "", "a@b.c", "?s=1", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("mailto:a@b.c?s=1#f"); u.hostname = "h.test?x"; return P(u); })"#,
                r##"["mailto:a@b.c?s=1#f", "null", "mailto:", "", "", "", "", "", "a@b.c", "?s=1", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("mailto:a@b.c?s=1#f"); u.hostname = "[::1]"; return P(u); })"#,
                r##"["mailto:a@b.c?s=1#f", "null", "mailto:", "", "", "", "", "", "a@b.c", "?s=1", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("mailto:a@b.c?s=1#f"); u.hostname = ""; return P(u); })"#,
                r##"["mailto:a@b.c?s=1#f", "null", "mailto:", "", "", "", "", "", "a@b.c", "?s=1", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("mailto:a@b.c?s=1#f"); u.hostname = "a b"; return P(u); })"#,
                r##"["mailto:a@b.c?s=1#f", "null", "mailto:", "", "", "", "", "", "a@b.c", "?s=1", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("mailto:a@b.c?s=1#f"); u.hostname = "A.B"; return P(u); })"#,
                r##"["mailto:a@b.c?s=1#f", "null", "mailto:", "", "", "", "", "", "a@b.c", "?s=1", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("mailto:a@b.c?s=1#f"); u.port = "81"; return P(u); })"#,
                r##"["mailto:a@b.c?s=1#f", "null", "mailto:", "", "", "", "", "", "a@b.c", "?s=1", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("mailto:a@b.c?s=1#f"); u.port = "81abc"; return P(u); })"#,
                r##"["mailto:a@b.c?s=1#f", "null", "mailto:", "", "", "", "", "", "a@b.c", "?s=1", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("mailto:a@b.c?s=1#f"); u.port = "abc"; return P(u); })"#,
                r##"["mailto:a@b.c?s=1#f", "null", "mailto:", "", "", "", "", "", "a@b.c", "?s=1", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("mailto:a@b.c?s=1#f"); u.port = ""; return P(u); })"#,
                r##"["mailto:a@b.c?s=1#f", "null", "mailto:", "", "", "", "", "", "a@b.c", "?s=1", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("mailto:a@b.c?s=1#f"); u.port = "99999"; return P(u); })"#,
                r##"["mailto:a@b.c?s=1#f", "null", "mailto:", "", "", "", "", "", "a@b.c", "?s=1", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("mailto:a@b.c?s=1#f"); u.port = "65535"; return P(u); })"#,
                r##"["mailto:a@b.c?s=1#f", "null", "mailto:", "", "", "", "", "", "a@b.c", "?s=1", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("mailto:a@b.c?s=1#f"); u.port = "65536"; return P(u); })"#,
                r##"["mailto:a@b.c?s=1#f", "null", "mailto:", "", "", "", "", "", "a@b.c", "?s=1", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("mailto:a@b.c?s=1#f"); u.port = "0"; return P(u); })"#,
                r##"["mailto:a@b.c?s=1#f", "null", "mailto:", "", "", "", "", "", "a@b.c", "?s=1", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("mailto:a@b.c?s=1#f"); u.port = "00081"; return P(u); })"#,
                r##"["mailto:a@b.c?s=1#f", "null", "mailto:", "", "", "", "", "", "a@b.c", "?s=1", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("mailto:a@b.c?s=1#f"); u.port = "80"; return P(u); })"#,
                r##"["mailto:a@b.c?s=1#f", "null", "mailto:", "", "", "", "", "", "a@b.c", "?s=1", "#f"]"##,
            ),
        ],
    },
    Case {
        name: "setters-mailto-2",
        html: r"<!DOCTYPE html><html><head></head><body><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : v; } catch (e) { return (e instanceof DOMException ? 'DOMException ' + e.name + ' [' + e.code + ']' : e.constructor.name) + ': ' + e.message; } }
function P(u) { return [u.href, u.origin, u.protocol, u.username, u.password, u.host, u.hostname, u.port, u.pathname, u.search, u.hash]; }
function S(init) { return [...new URLSearchParams(init)].map((p) => p.join('=')).join('&'); }
</script></body></html>",
        queries: &[
            (
                r#"E(() => { const u = new URL("mailto:a@b.c?s=1#f"); u.port = "443"; return P(u); })"#,
                r##"["mailto:a@b.c?s=1#f", "null", "mailto:", "", "", "", "", "", "a@b.c", "?s=1", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("mailto:a@b.c?s=1#f"); u.port = " 81"; return P(u); })"#,
                r##"["mailto:a@b.c?s=1#f", "null", "mailto:", "", "", "", "", "", "a@b.c", "?s=1", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("mailto:a@b.c?s=1#f"); u.port = "8\\t1"; return P(u); })"#,
                r##"["mailto:a@b.c?s=1#f", "null", "mailto:", "", "", "", "", "", "a@b.c", "?s=1", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("mailto:a@b.c?s=1#f"); u.port = "-1"; return P(u); })"#,
                r##"["mailto:a@b.c?s=1#f", "null", "mailto:", "", "", "", "", "", "a@b.c", "?s=1", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("mailto:a@b.c?s=1#f"); u.port = "1e3"; return P(u); })"#,
                r##"["mailto:a@b.c?s=1#f", "null", "mailto:", "", "", "", "", "", "a@b.c", "?s=1", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("mailto:a@b.c?s=1#f"); u.pathname = "/a/b"; return P(u); })"#,
                r##"["mailto:a@b.c?s=1#f", "null", "mailto:", "", "", "", "", "", "a@b.c", "?s=1", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("mailto:a@b.c?s=1#f"); u.pathname = "a/b"; return P(u); })"#,
                r##"["mailto:a@b.c?s=1#f", "null", "mailto:", "", "", "", "", "", "a@b.c", "?s=1", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("mailto:a@b.c?s=1#f"); u.pathname = ""; return P(u); })"#,
                r##"["mailto:a@b.c?s=1#f", "null", "mailto:", "", "", "", "", "", "a@b.c", "?s=1", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("mailto:a@b.c?s=1#f"); u.pathname = "/a b?c#d%e"; return P(u); })"#,
                r##"["mailto:a@b.c?s=1#f", "null", "mailto:", "", "", "", "", "", "a@b.c", "?s=1", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("mailto:a@b.c?s=1#f"); u.pathname = "/a/../b/./c"; return P(u); })"#,
                r##"["mailto:a@b.c?s=1#f", "null", "mailto:", "", "", "", "", "", "a@b.c", "?s=1", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("mailto:a@b.c?s=1#f"); u.pathname = "\\\\a\\\\b"; return P(u); })"#,
                r##"["mailto:a@b.c?s=1#f", "null", "mailto:", "", "", "", "", "", "a@b.c", "?s=1", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("mailto:a@b.c?s=1#f"); u.pathname = "/\u00e9\ud83d\ude00"; return P(u); })"#,
                r##"["mailto:a@b.c?s=1#f", "null", "mailto:", "", "", "", "", "", "a@b.c", "?s=1", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("mailto:a@b.c?s=1#f"); u.pathname = "//x"; return P(u); })"#,
                r##"["mailto:a@b.c?s=1#f", "null", "mailto:", "", "", "", "", "", "a@b.c", "?s=1", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("mailto:a@b.c?s=1#f"); u.pathname = "/%41/%zz"; return P(u); })"#,
                r##"["mailto:a@b.c?s=1#f", "null", "mailto:", "", "", "", "", "", "a@b.c", "?s=1", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("mailto:a@b.c?s=1#f"); u.search = "q=1"; return P(u); })"#,
                r##"["mailto:a@b.c?q=1#f", "null", "mailto:", "", "", "", "", "", "a@b.c", "?q=1", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("mailto:a@b.c?s=1#f"); u.search = "?q=1"; return P(u); })"#,
                r##"["mailto:a@b.c?q=1#f", "null", "mailto:", "", "", "", "", "", "a@b.c", "?q=1", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("mailto:a@b.c?s=1#f"); u.search = "??q=1"; return P(u); })"#,
                r##"["mailto:a@b.c??q=1#f", "null", "mailto:", "", "", "", "", "", "a@b.c", "??q=1", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("mailto:a@b.c?s=1#f"); u.search = ""; return P(u); })"#,
                r##"["mailto:a@b.c#f", "null", "mailto:", "", "", "", "", "", "a@b.c", "", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("mailto:a@b.c?s=1#f"); u.search = "?"; return P(u); })"#,
                r##"["mailto:a@b.c#f", "null", "mailto:", "", "", "", "", "", "a@b.c", "", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("mailto:a@b.c?s=1#f"); u.search = "a b&c='d'#e"; return P(u); })"#,
                r##"["mailto:a@b.c?a%20b&c=%27d%27%23e#f", "null", "mailto:", "", "", "", "", "", "a@b.c", "?a%20b&c=%27d%27%23e", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("mailto:a@b.c?s=1#f"); u.search = "\u00e9\ud83d\ude00"; return P(u); })"#,
                r##"["mailto:a@b.c?%C3%A9%F0%9F%98%80#f", "null", "mailto:", "", "", "", "", "", "a@b.c", "?%C3%A9%F0%9F%98%80", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("mailto:a@b.c?s=1#f"); u.search = "q=1\\n2"; return P(u); })"#,
                r##"["mailto:a@b.c?q=1\\n2#f", "null", "mailto:", "", "", "", "", "", "a@b.c", "?q=1\\n2", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("mailto:a@b.c?s=1#f"); u.hash = "f"; return P(u); })"#,
                r##"["mailto:a@b.c?s=1#f", "null", "mailto:", "", "", "", "", "", "a@b.c", "?s=1", "#f"]"##,
            ),
            (
                r##"E(() => { const u = new URL("mailto:a@b.c?s=1#f"); u.hash = "#f"; return P(u); })"##,
                r##"["mailto:a@b.c?s=1#f", "null", "mailto:", "", "", "", "", "", "a@b.c", "?s=1", "#f"]"##,
            ),
            (
                r###"E(() => { const u = new URL("mailto:a@b.c?s=1#f"); u.hash = "##f"; return P(u); })"###,
                r###"["mailto:a@b.c?s=1##f", "null", "mailto:", "", "", "", "", "", "a@b.c", "?s=1", "##f"]"###,
            ),
            (
                r#"E(() => { const u = new URL("mailto:a@b.c?s=1#f"); u.hash = ""; return P(u); })"#,
                r#"["mailto:a@b.c?s=1", "null", "mailto:", "", "", "", "", "", "a@b.c", "?s=1", ""]"#,
            ),
            (
                r##"E(() => { const u = new URL("mailto:a@b.c?s=1#f"); u.hash = "#"; return P(u); })"##,
                r#"["mailto:a@b.c?s=1#", "null", "mailto:", "", "", "", "", "", "a@b.c", "?s=1", ""]"#,
            ),
            (
                r#"E(() => { const u = new URL("mailto:a@b.c?s=1#f"); u.hash = "a b<>`\"'\u00e9"; return P(u); })"#,
                r##"["mailto:a@b.c?s=1#a%20b%3C%3E%60%22'%C3%A9", "null", "mailto:", "", "", "", "", "", "a@b.c", "?s=1", "#a%20b%3C%3E%60%22'%C3%A9"]"##,
            ),
            (
                r#"E(() => { const u = new URL("mailto:a@b.c?s=1#f"); u.hash = "f\\t1"; return P(u); })"#,
                r##"["mailto:a@b.c?s=1#f\\t1", "null", "mailto:", "", "", "", "", "", "a@b.c", "?s=1", "#f\\t1"]"##,
            ),
        ],
    },
    Case {
        name: "setters-foo-0",
        html: r"<!DOCTYPE html><html><head></head><body><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : v; } catch (e) { return (e instanceof DOMException ? 'DOMException ' + e.name + ' [' + e.code + ']' : e.constructor.name) + ': ' + e.message; } }
function P(u) { return [u.href, u.origin, u.protocol, u.username, u.password, u.host, u.hostname, u.port, u.pathname, u.search, u.hash]; }
function S(init) { return [...new URLSearchParams(init)].map((p) => p.join('=')).join('&'); }
</script></body></html>",
        queries: &[
            (
                r#"E(() => { const u = new URL("foo://host:9/p?q#f"); u.href = "https://x.test/p?q#h"; return P(u); })"#,
                r##"["https://x.test/p?q#h", "https://x.test", "https:", "", "", "x.test", "x.test", "", "/p", "?q", "#h"]"##,
            ),
            (
                r#"E(() => { const u = new URL("foo://host:9/p?q#f"); u.href = "foo"; return P(u); })"#,
                r#""TypeError: Failed to set the 'href' property on 'URL': Invalid URL""#,
            ),
            (
                r#"E(() => { const u = new URL("foo://host:9/p?q#f"); u.href = ""; return P(u); })"#,
                r#""TypeError: Failed to set the 'href' property on 'URL': Invalid URL""#,
            ),
            (
                r#"E(() => { const u = new URL("foo://host:9/p?q#f"); u.href = "mailto:a@b"; return P(u); })"#,
                r#"["mailto:a@b", "null", "mailto:", "", "", "", "", "", "a@b", "", ""]"#,
            ),
            (
                r#"E(() => { const u = new URL("foo://host:9/p?q#f"); u.protocol = "https"; return P(u); })"#,
                r##"["https://host:9/p?q#f", "https://host:9", "https:", "", "", "host:9", "host", "9", "/p", "?q", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("foo://host:9/p?q#f"); u.protocol = "https:"; return P(u); })"#,
                r##"["https://host:9/p?q#f", "https://host:9", "https:", "", "", "host:9", "host", "9", "/p", "?q", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("foo://host:9/p?q#f"); u.protocol = "https://x"; return P(u); })"#,
                r##"["https://host:9/p?q#f", "https://host:9", "https:", "", "", "host:9", "host", "9", "/p", "?q", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("foo://host:9/p?q#f"); u.protocol = "HTTPS:abc"; return P(u); })"#,
                r##"["https://host:9/p?q#f", "https://host:9", "https:", "", "", "host:9", "host", "9", "/p", "?q", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("foo://host:9/p?q#f"); u.protocol = "ftp"; return P(u); })"#,
                r##"["ftp://host:9/p?q#f", "ftp://host:9", "ftp:", "", "", "host:9", "host", "9", "/p", "?q", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("foo://host:9/p?q#f"); u.protocol = "ws"; return P(u); })"#,
                r##"["ws://host:9/p?q#f", "ws://host:9", "ws:", "", "", "host:9", "host", "9", "/p", "?q", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("foo://host:9/p?q#f"); u.protocol = "file"; return P(u); })"#,
                r#"["file://host:9/p?q#f", "null", ":", "", "", "", "", "", "", "", ""]"#,
            ),
            (
                r#"E(() => { const u = new URL("foo://host:9/p?q#f"); u.protocol = "foo"; return P(u); })"#,
                r##"["foo://host:9/p?q#f", "null", "foo:", "", "", "host:9", "host", "9", "/p", "?q", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("foo://host:9/p?q#f"); u.protocol = "mailto"; return P(u); })"#,
                r##"["mailto://host:9/p?q#f", "null", "mailto:", "", "", "host:9", "host", "9", "/p", "?q", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("foo://host:9/p?q#f"); u.protocol = "1x"; return P(u); })"#,
                r##"["foo://host:9/p?q#f", "null", "foo:", "", "", "host:9", "host", "9", "/p", "?q", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("foo://host:9/p?q#f"); u.protocol = ""; return P(u); })"#,
                r##"["foo://host:9/p?q#f", "null", "foo:", "", "", "host:9", "host", "9", "/p", "?q", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("foo://host:9/p?q#f"); u.protocol = "x y"; return P(u); })"#,
                r##"["foo://host:9/p?q#f", "null", "foo:", "", "", "host:9", "host", "9", "/p", "?q", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("foo://host:9/p?q#f"); u.protocol = "http"; return P(u); })"#,
                r##"["http://host:9/p?q#f", "http://host:9", "http:", "", "", "host:9", "host", "9", "/p", "?q", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("foo://host:9/p?q#f"); u.username = "u"; return P(u); })"#,
                r##"["foo://u@host:9/p?q#f", "null", "foo:", "u", "", "host:9", "host", "9", "/p", "?q", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("foo://host:9/p?q#f"); u.username = "u s:r@/?#\u00e9"; return P(u); })"#,
                r##"["foo://u%20s%3Ar%40%2F%3F%23%C3%A9@host:9/p?q#f", "null", "foo:", "u%20s%3Ar%40%2F%3F%23%C3%A9", "", "host:9", "host", "9", "/p", "?q", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("foo://host:9/p?q#f"); u.username = ""; return P(u); })"#,
                r##"["foo://host:9/p?q#f", "null", "foo:", "", "", "host:9", "host", "9", "/p", "?q", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("foo://host:9/p?q#f"); u.password = "p"; return P(u); })"#,
                r##"["foo://:p@host:9/p?q#f", "null", "foo:", "", "p", "host:9", "host", "9", "/p", "?q", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("foo://host:9/p?q#f"); u.password = "p s:r@/?#\u00e9"; return P(u); })"#,
                r##"["foo://:p%20s%3Ar%40%2F%3F%23%C3%A9@host:9/p?q#f", "null", "foo:", "", "p%20s%3Ar%40%2F%3F%23%C3%A9", "host:9", "host", "9", "/p", "?q", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("foo://host:9/p?q#f"); u.password = ""; return P(u); })"#,
                r##"["foo://host:9/p?q#f", "null", "foo:", "", "", "host:9", "host", "9", "/p", "?q", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("foo://host:9/p?q#f"); u.host = "h.test"; return P(u); })"#,
                r##"["foo://h.test:9/p?q#f", "null", "foo:", "", "", "h.test:9", "h.test", "9", "/p", "?q", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("foo://host:9/p?q#f"); u.host = "h.test:81"; return P(u); })"#,
                r##"["foo://h.test:81/p?q#f", "null", "foo:", "", "", "h.test:81", "h.test", "81", "/p", "?q", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("foo://host:9/p?q#f"); u.host = "h.test:"; return P(u); })"#,
                r##"["foo://h.test:9/p?q#f", "null", "foo:", "", "", "h.test:9", "h.test", "9", "/p", "?q", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("foo://host:9/p?q#f"); u.host = "h.test:99999"; return P(u); })"#,
                r##"["foo://h.test:9/p?q#f", "null", "foo:", "", "", "h.test:9", "h.test", "9", "/p", "?q", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("foo://host:9/p?q#f"); u.host = "h.test:81abc"; return P(u); })"#,
                r##"["foo://h.test:81/p?q#f", "null", "foo:", "", "", "h.test:81", "h.test", "81", "/p", "?q", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("foo://host:9/p?q#f"); u.host = "h.test/path"; return P(u); })"#,
                r##"["foo://h.test:9/p?q#f", "null", "foo:", "", "", "h.test:9", "h.test", "9", "/p", "?q", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("foo://host:9/p?q#f"); u.host = "h.test?x"; return P(u); })"#,
                r##"["foo://h.test:9/p?q#f", "null", "foo:", "", "", "h.test:9", "h.test", "9", "/p", "?q", "#f"]"##,
            ),
        ],
    },
    Case {
        name: "setters-foo-1",
        html: r"<!DOCTYPE html><html><head></head><body><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : v; } catch (e) { return (e instanceof DOMException ? 'DOMException ' + e.name + ' [' + e.code + ']' : e.constructor.name) + ': ' + e.message; } }
function P(u) { return [u.href, u.origin, u.protocol, u.username, u.password, u.host, u.hostname, u.port, u.pathname, u.search, u.hash]; }
function S(init) { return [...new URLSearchParams(init)].map((p) => p.join('=')).join('&'); }
</script></body></html>",
        queries: &[
            (
                r#"E(() => { const u = new URL("foo://host:9/p?q#f"); u.host = "h.test#x"; return P(u); })"#,
                r##"["foo://h.test:9/p?q#f", "null", "foo:", "", "", "h.test:9", "h.test", "9", "/p", "?q", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("foo://host:9/p?q#f"); u.host = "h.test\\\\x"; return P(u); })"#,
                r##"["foo://host:9/p?q#f", "null", "foo:", "", "", "host:9", "host", "9", "/p", "?q", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("foo://host:9/p?q#f"); u.host = "[::1]:3000"; return P(u); })"#,
                r##"["foo://[::1]:3000/p?q#f", "null", "foo:", "", "", "[::1]:3000", "[::1]", "3000", "/p", "?q", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("foo://host:9/p?q#f"); u.host = "[::1"; return P(u); })"#,
                r##"["foo://host:9/p?q#f", "null", "foo:", "", "", "host:9", "host", "9", "/p", "?q", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("foo://host:9/p?q#f"); u.host = ""; return P(u); })"#,
                r##"["foo://host:9/p?q#f", "null", "foo:", "", "", "host:9", "host", "9", "/p", "?q", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("foo://host:9/p?q#f"); u.host = "a b"; return P(u); })"#,
                r##"["foo://host:9/p?q#f", "null", "foo:", "", "", "host:9", "host", "9", "/p", "?q", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("foo://host:9/p?q#f"); u.host = "A.B"; return P(u); })"#,
                r##"["foo://A.B:9/p?q#f", "null", "foo:", "", "", "A.B:9", "A.B", "9", "/p", "?q", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("foo://host:9/p?q#f"); u.host = ":81"; return P(u); })"#,
                r##"["foo://host:9/p?q#f", "null", "foo:", "", "", "host:9", "host", "9", "/p", "?q", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("foo://host:9/p?q#f"); u.host = "h.test:80"; return P(u); })"#,
                r##"["foo://h.test:80/p?q#f", "null", "foo:", "", "", "h.test:80", "h.test", "80", "/p", "?q", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("foo://host:9/p?q#f"); u.host = "h.test:443"; return P(u); })"#,
                r##"["foo://h.test:443/p?q#f", "null", "foo:", "", "", "h.test:443", "h.test", "443", "/p", "?q", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("foo://host:9/p?q#f"); u.host = "m\u00fcnchen.de"; return P(u); })"#,
                r##"["foo://m%C3%BCnchen.de:9/p?q#f", "null", "foo:", "", "", "m%C3%BCnchen.de:9", "m%C3%BCnchen.de", "9", "/p", "?q", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("foo://host:9/p?q#f"); u.host = "127.1"; return P(u); })"#,
                r##"["foo://127.1:9/p?q#f", "null", "foo:", "", "", "127.1:9", "127.1", "9", "/p", "?q", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("foo://host:9/p?q#f"); u.hostname = "h.test"; return P(u); })"#,
                r##"["foo://h.test:9/p?q#f", "null", "foo:", "", "", "h.test:9", "h.test", "9", "/p", "?q", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("foo://host:9/p?q#f"); u.hostname = "h.test:81"; return P(u); })"#,
                r##"["foo://host:9/p?q#f", "null", "foo:", "", "", "host:9", "host", "9", "/p", "?q", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("foo://host:9/p?q#f"); u.hostname = "h.test/x"; return P(u); })"#,
                r##"["foo://h.test:9/p?q#f", "null", "foo:", "", "", "h.test:9", "h.test", "9", "/p", "?q", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("foo://host:9/p?q#f"); u.hostname = "h.test?x"; return P(u); })"#,
                r##"["foo://h.test:9/p?q#f", "null", "foo:", "", "", "h.test:9", "h.test", "9", "/p", "?q", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("foo://host:9/p?q#f"); u.hostname = "[::1]"; return P(u); })"#,
                r##"["foo://[::1]:9/p?q#f", "null", "foo:", "", "", "[::1]:9", "[::1]", "9", "/p", "?q", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("foo://host:9/p?q#f"); u.hostname = ""; return P(u); })"#,
                r##"["foo://host:9/p?q#f", "null", "foo:", "", "", "host:9", "host", "9", "/p", "?q", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("foo://host:9/p?q#f"); u.hostname = "a b"; return P(u); })"#,
                r##"["foo://host:9/p?q#f", "null", "foo:", "", "", "host:9", "host", "9", "/p", "?q", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("foo://host:9/p?q#f"); u.hostname = "A.B"; return P(u); })"#,
                r##"["foo://A.B:9/p?q#f", "null", "foo:", "", "", "A.B:9", "A.B", "9", "/p", "?q", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("foo://host:9/p?q#f"); u.port = "81"; return P(u); })"#,
                r##"["foo://host:81/p?q#f", "null", "foo:", "", "", "host:81", "host", "81", "/p", "?q", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("foo://host:9/p?q#f"); u.port = "81abc"; return P(u); })"#,
                r##"["foo://host:81/p?q#f", "null", "foo:", "", "", "host:81", "host", "81", "/p", "?q", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("foo://host:9/p?q#f"); u.port = "abc"; return P(u); })"#,
                r##"["foo://host:9/p?q#f", "null", "foo:", "", "", "host:9", "host", "9", "/p", "?q", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("foo://host:9/p?q#f"); u.port = ""; return P(u); })"#,
                r##"["foo://host/p?q#f", "null", "foo:", "", "", "host", "host", "", "/p", "?q", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("foo://host:9/p?q#f"); u.port = "99999"; return P(u); })"#,
                r##"["foo://host:9/p?q#f", "null", "foo:", "", "", "host:9", "host", "9", "/p", "?q", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("foo://host:9/p?q#f"); u.port = "65535"; return P(u); })"#,
                r##"["foo://host:65535/p?q#f", "null", "foo:", "", "", "host:65535", "host", "65535", "/p", "?q", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("foo://host:9/p?q#f"); u.port = "65536"; return P(u); })"#,
                r##"["foo://host:9/p?q#f", "null", "foo:", "", "", "host:9", "host", "9", "/p", "?q", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("foo://host:9/p?q#f"); u.port = "0"; return P(u); })"#,
                r##"["foo://host:0/p?q#f", "null", "foo:", "", "", "host:0", "host", "0", "/p", "?q", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("foo://host:9/p?q#f"); u.port = "00081"; return P(u); })"#,
                r##"["foo://host:81/p?q#f", "null", "foo:", "", "", "host:81", "host", "81", "/p", "?q", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("foo://host:9/p?q#f"); u.port = "80"; return P(u); })"#,
                r##"["foo://host:80/p?q#f", "null", "foo:", "", "", "host:80", "host", "80", "/p", "?q", "#f"]"##,
            ),
        ],
    },
    Case {
        name: "setters-foo-2",
        html: r"<!DOCTYPE html><html><head></head><body><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : v; } catch (e) { return (e instanceof DOMException ? 'DOMException ' + e.name + ' [' + e.code + ']' : e.constructor.name) + ': ' + e.message; } }
function P(u) { return [u.href, u.origin, u.protocol, u.username, u.password, u.host, u.hostname, u.port, u.pathname, u.search, u.hash]; }
function S(init) { return [...new URLSearchParams(init)].map((p) => p.join('=')).join('&'); }
</script></body></html>",
        queries: &[
            (
                r#"E(() => { const u = new URL("foo://host:9/p?q#f"); u.port = "443"; return P(u); })"#,
                r##"["foo://host:443/p?q#f", "null", "foo:", "", "", "host:443", "host", "443", "/p", "?q", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("foo://host:9/p?q#f"); u.port = " 81"; return P(u); })"#,
                r##"["foo://host:9/p?q#f", "null", "foo:", "", "", "host:9", "host", "9", "/p", "?q", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("foo://host:9/p?q#f"); u.port = "8\\t1"; return P(u); })"#,
                r##"["foo://host:8/p?q#f", "null", "foo:", "", "", "host:8", "host", "8", "/p", "?q", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("foo://host:9/p?q#f"); u.port = "-1"; return P(u); })"#,
                r##"["foo://host:9/p?q#f", "null", "foo:", "", "", "host:9", "host", "9", "/p", "?q", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("foo://host:9/p?q#f"); u.port = "1e3"; return P(u); })"#,
                r##"["foo://host:1/p?q#f", "null", "foo:", "", "", "host:1", "host", "1", "/p", "?q", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("foo://host:9/p?q#f"); u.pathname = "/a/b"; return P(u); })"#,
                r##"["foo://host:9/a/b?q#f", "null", "foo:", "", "", "host:9", "host", "9", "/a/b", "?q", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("foo://host:9/p?q#f"); u.pathname = "a/b"; return P(u); })"#,
                r##"["foo://host:9/a/b?q#f", "null", "foo:", "", "", "host:9", "host", "9", "/a/b", "?q", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("foo://host:9/p?q#f"); u.pathname = ""; return P(u); })"#,
                r##"["foo://host:9?q#f", "null", "foo:", "", "", "host:9", "host", "9", "", "?q", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("foo://host:9/p?q#f"); u.pathname = "/a b?c#d%e"; return P(u); })"#,
                r##"["foo://host:9/a%20b%3Fc%23d%e?q#f", "null", "foo:", "", "", "host:9", "host", "9", "/a%20b%3Fc%23d%e", "?q", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("foo://host:9/p?q#f"); u.pathname = "/a/../b/./c"; return P(u); })"#,
                r##"["foo://host:9/b/c?q#f", "null", "foo:", "", "", "host:9", "host", "9", "/b/c", "?q", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("foo://host:9/p?q#f"); u.pathname = "\\\\a\\\\b"; return P(u); })"#,
                r##"["foo://host:9/\\\\a\\\\b?q#f", "null", "foo:", "", "", "host:9", "host", "9", "/\\\\a\\\\b", "?q", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("foo://host:9/p?q#f"); u.pathname = "/\u00e9\ud83d\ude00"; return P(u); })"#,
                r##"["foo://host:9/%C3%A9%F0%9F%98%80?q#f", "null", "foo:", "", "", "host:9", "host", "9", "/%C3%A9%F0%9F%98%80", "?q", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("foo://host:9/p?q#f"); u.pathname = "//x"; return P(u); })"#,
                r##"["foo://host:9//x?q#f", "null", "foo:", "", "", "host:9", "host", "9", "//x", "?q", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("foo://host:9/p?q#f"); u.pathname = "/%41/%zz"; return P(u); })"#,
                r##"["foo://host:9/%41/%zz?q#f", "null", "foo:", "", "", "host:9", "host", "9", "/%41/%zz", "?q", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("foo://host:9/p?q#f"); u.search = "q=1"; return P(u); })"#,
                r##"["foo://host:9/p?q=1#f", "null", "foo:", "", "", "host:9", "host", "9", "/p", "?q=1", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("foo://host:9/p?q#f"); u.search = "?q=1"; return P(u); })"#,
                r##"["foo://host:9/p?q=1#f", "null", "foo:", "", "", "host:9", "host", "9", "/p", "?q=1", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("foo://host:9/p?q#f"); u.search = "??q=1"; return P(u); })"#,
                r##"["foo://host:9/p??q=1#f", "null", "foo:", "", "", "host:9", "host", "9", "/p", "??q=1", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("foo://host:9/p?q#f"); u.search = ""; return P(u); })"#,
                r##"["foo://host:9/p#f", "null", "foo:", "", "", "host:9", "host", "9", "/p", "", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("foo://host:9/p?q#f"); u.search = "?"; return P(u); })"#,
                r##"["foo://host:9/p#f", "null", "foo:", "", "", "host:9", "host", "9", "/p", "", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("foo://host:9/p?q#f"); u.search = "a b&c='d'#e"; return P(u); })"#,
                r##"["foo://host:9/p?a%20b&c=%27d%27%23e#f", "null", "foo:", "", "", "host:9", "host", "9", "/p", "?a%20b&c=%27d%27%23e", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("foo://host:9/p?q#f"); u.search = "\u00e9\ud83d\ude00"; return P(u); })"#,
                r##"["foo://host:9/p?%C3%A9%F0%9F%98%80#f", "null", "foo:", "", "", "host:9", "host", "9", "/p", "?%C3%A9%F0%9F%98%80", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("foo://host:9/p?q#f"); u.search = "q=1\\n2"; return P(u); })"#,
                r##"["foo://host:9/p?q=1\\n2#f", "null", "foo:", "", "", "host:9", "host", "9", "/p", "?q=1\\n2", "#f"]"##,
            ),
            (
                r#"E(() => { const u = new URL("foo://host:9/p?q#f"); u.hash = "f"; return P(u); })"#,
                r##"["foo://host:9/p?q#f", "null", "foo:", "", "", "host:9", "host", "9", "/p", "?q", "#f"]"##,
            ),
            (
                r##"E(() => { const u = new URL("foo://host:9/p?q#f"); u.hash = "#f"; return P(u); })"##,
                r##"["foo://host:9/p?q#f", "null", "foo:", "", "", "host:9", "host", "9", "/p", "?q", "#f"]"##,
            ),
            (
                r###"E(() => { const u = new URL("foo://host:9/p?q#f"); u.hash = "##f"; return P(u); })"###,
                r###"["foo://host:9/p?q##f", "null", "foo:", "", "", "host:9", "host", "9", "/p", "?q", "##f"]"###,
            ),
            (
                r#"E(() => { const u = new URL("foo://host:9/p?q#f"); u.hash = ""; return P(u); })"#,
                r#"["foo://host:9/p?q", "null", "foo:", "", "", "host:9", "host", "9", "/p", "?q", ""]"#,
            ),
            (
                r##"E(() => { const u = new URL("foo://host:9/p?q#f"); u.hash = "#"; return P(u); })"##,
                r#"["foo://host:9/p?q#", "null", "foo:", "", "", "host:9", "host", "9", "/p", "?q", ""]"#,
            ),
            (
                r#"E(() => { const u = new URL("foo://host:9/p?q#f"); u.hash = "a b<>`\"'\u00e9"; return P(u); })"#,
                r##"["foo://host:9/p?q#a%20b%3C%3E%60%22'%C3%A9", "null", "foo:", "", "", "host:9", "host", "9", "/p", "?q", "#a%20b%3C%3E%60%22'%C3%A9"]"##,
            ),
            (
                r#"E(() => { const u = new URL("foo://host:9/p?q#f"); u.hash = "f\\t1"; return P(u); })"#,
                r##"["foo://host:9/p?q#f\\t1", "null", "foo:", "", "", "host:9", "host", "9", "/p", "?q", "#f\\t1"]"##,
            ),
        ],
    },
    Case {
        name: "setters-data-0",
        html: r"<!DOCTYPE html><html><head></head><body><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : v; } catch (e) { return (e instanceof DOMException ? 'DOMException ' + e.name + ' [' + e.code + ']' : e.constructor.name) + ': ' + e.message; } }
function P(u) { return [u.href, u.origin, u.protocol, u.username, u.password, u.host, u.hostname, u.port, u.pathname, u.search, u.hash]; }
function S(init) { return [...new URLSearchParams(init)].map((p) => p.join('=')).join('&'); }
</script></body></html>",
        queries: &[
            (
                r#"E(() => { const u = new URL("data:text/plain,hello#frag"); u.href = "https://x.test/p?q#h"; return P(u); })"#,
                r##"["https://x.test/p?q#h", "https://x.test", "https:", "", "", "x.test", "x.test", "", "/p", "?q", "#h"]"##,
            ),
            (
                r#"E(() => { const u = new URL("data:text/plain,hello#frag"); u.href = "foo"; return P(u); })"#,
                r#""TypeError: Failed to set the 'href' property on 'URL': Invalid URL""#,
            ),
            (
                r#"E(() => { const u = new URL("data:text/plain,hello#frag"); u.href = ""; return P(u); })"#,
                r#""TypeError: Failed to set the 'href' property on 'URL': Invalid URL""#,
            ),
            (
                r#"E(() => { const u = new URL("data:text/plain,hello#frag"); u.href = "mailto:a@b"; return P(u); })"#,
                r#"["mailto:a@b", "null", "mailto:", "", "", "", "", "", "a@b", "", ""]"#,
            ),
            (
                r#"E(() => { const u = new URL("data:text/plain,hello#frag"); u.protocol = "https"; return P(u); })"#,
                r##"["https://text/plain,hello#frag", "https://text", "https:", "", "", "text", "text", "", "/plain,hello", "", "#frag"]"##,
            ),
            (
                r#"E(() => { const u = new URL("data:text/plain,hello#frag"); u.protocol = "https:"; return P(u); })"#,
                r##"["https://text/plain,hello#frag", "https://text", "https:", "", "", "text", "text", "", "/plain,hello", "", "#frag"]"##,
            ),
            (
                r#"E(() => { const u = new URL("data:text/plain,hello#frag"); u.protocol = "https://x"; return P(u); })"#,
                r##"["https://text/plain,hello#frag", "https://text", "https:", "", "", "text", "text", "", "/plain,hello", "", "#frag"]"##,
            ),
            (
                r#"E(() => { const u = new URL("data:text/plain,hello#frag"); u.protocol = "HTTPS:abc"; return P(u); })"#,
                r##"["https://text/plain,hello#frag", "https://text", "https:", "", "", "text", "text", "", "/plain,hello", "", "#frag"]"##,
            ),
            (
                r#"E(() => { const u = new URL("data:text/plain,hello#frag"); u.protocol = "ftp"; return P(u); })"#,
                r##"["ftp://text/plain,hello#frag", "ftp://text", "ftp:", "", "", "text", "text", "", "/plain,hello", "", "#frag"]"##,
            ),
            (
                r#"E(() => { const u = new URL("data:text/plain,hello#frag"); u.protocol = "ws"; return P(u); })"#,
                r##"["ws://text/plain,hello#frag", "ws://text", "ws:", "", "", "text", "text", "", "/plain,hello", "", "#frag"]"##,
            ),
            (
                r#"E(() => { const u = new URL("data:text/plain,hello#frag"); u.protocol = "file"; return P(u); })"#,
                r##"["file:///text/plain,hello#frag", "file://", "file:", "", "", "", "", "", "/text/plain,hello", "", "#frag"]"##,
            ),
            (
                r#"E(() => { const u = new URL("data:text/plain,hello#frag"); u.protocol = "foo"; return P(u); })"#,
                r##"["foo:text/plain,hello#frag", "null", "foo:", "", "", "", "", "", "text/plain,hello", "", "#frag"]"##,
            ),
            (
                r#"E(() => { const u = new URL("data:text/plain,hello#frag"); u.protocol = "mailto"; return P(u); })"#,
                r##"["mailto:text/plain,hello#frag", "null", "mailto:", "", "", "", "", "", "text/plain,hello", "", "#frag"]"##,
            ),
            (
                r#"E(() => { const u = new URL("data:text/plain,hello#frag"); u.protocol = "1x"; return P(u); })"#,
                r##"["data:text/plain,hello#frag", "null", "data:", "", "", "", "", "", "text/plain,hello", "", "#frag"]"##,
            ),
            (
                r#"E(() => { const u = new URL("data:text/plain,hello#frag"); u.protocol = ""; return P(u); })"#,
                r##"["data:text/plain,hello#frag", "null", "data:", "", "", "", "", "", "text/plain,hello", "", "#frag"]"##,
            ),
            (
                r#"E(() => { const u = new URL("data:text/plain,hello#frag"); u.protocol = "x y"; return P(u); })"#,
                r##"["data:text/plain,hello#frag", "null", "data:", "", "", "", "", "", "text/plain,hello", "", "#frag"]"##,
            ),
            (
                r#"E(() => { const u = new URL("data:text/plain,hello#frag"); u.protocol = "http"; return P(u); })"#,
                r##"["http://text/plain,hello#frag", "http://text", "http:", "", "", "text", "text", "", "/plain,hello", "", "#frag"]"##,
            ),
            (
                r#"E(() => { const u = new URL("data:text/plain,hello#frag"); u.username = "u"; return P(u); })"#,
                r##"["data:text/plain,hello#frag", "null", "data:", "", "", "", "", "", "text/plain,hello", "", "#frag"]"##,
            ),
            (
                r#"E(() => { const u = new URL("data:text/plain,hello#frag"); u.username = "u s:r@/?#\u00e9"; return P(u); })"#,
                r##"["data:text/plain,hello#frag", "null", "data:", "", "", "", "", "", "text/plain,hello", "", "#frag"]"##,
            ),
            (
                r#"E(() => { const u = new URL("data:text/plain,hello#frag"); u.username = ""; return P(u); })"#,
                r##"["data:text/plain,hello#frag", "null", "data:", "", "", "", "", "", "text/plain,hello", "", "#frag"]"##,
            ),
            (
                r#"E(() => { const u = new URL("data:text/plain,hello#frag"); u.password = "p"; return P(u); })"#,
                r##"["data:text/plain,hello#frag", "null", "data:", "", "", "", "", "", "text/plain,hello", "", "#frag"]"##,
            ),
            (
                r#"E(() => { const u = new URL("data:text/plain,hello#frag"); u.password = "p s:r@/?#\u00e9"; return P(u); })"#,
                r##"["data:text/plain,hello#frag", "null", "data:", "", "", "", "", "", "text/plain,hello", "", "#frag"]"##,
            ),
            (
                r#"E(() => { const u = new URL("data:text/plain,hello#frag"); u.password = ""; return P(u); })"#,
                r##"["data:text/plain,hello#frag", "null", "data:", "", "", "", "", "", "text/plain,hello", "", "#frag"]"##,
            ),
            (
                r#"E(() => { const u = new URL("data:text/plain,hello#frag"); u.host = "h.test"; return P(u); })"#,
                r##"["data:text/plain,hello#frag", "null", "data:", "", "", "", "", "", "text/plain,hello", "", "#frag"]"##,
            ),
            (
                r#"E(() => { const u = new URL("data:text/plain,hello#frag"); u.host = "h.test:81"; return P(u); })"#,
                r##"["data:text/plain,hello#frag", "null", "data:", "", "", "", "", "", "text/plain,hello", "", "#frag"]"##,
            ),
            (
                r#"E(() => { const u = new URL("data:text/plain,hello#frag"); u.host = "h.test:"; return P(u); })"#,
                r##"["data:text/plain,hello#frag", "null", "data:", "", "", "", "", "", "text/plain,hello", "", "#frag"]"##,
            ),
            (
                r#"E(() => { const u = new URL("data:text/plain,hello#frag"); u.host = "h.test:99999"; return P(u); })"#,
                r##"["data:text/plain,hello#frag", "null", "data:", "", "", "", "", "", "text/plain,hello", "", "#frag"]"##,
            ),
            (
                r#"E(() => { const u = new URL("data:text/plain,hello#frag"); u.host = "h.test:81abc"; return P(u); })"#,
                r##"["data:text/plain,hello#frag", "null", "data:", "", "", "", "", "", "text/plain,hello", "", "#frag"]"##,
            ),
            (
                r#"E(() => { const u = new URL("data:text/plain,hello#frag"); u.host = "h.test/path"; return P(u); })"#,
                r##"["data:text/plain,hello#frag", "null", "data:", "", "", "", "", "", "text/plain,hello", "", "#frag"]"##,
            ),
            (
                r#"E(() => { const u = new URL("data:text/plain,hello#frag"); u.host = "h.test?x"; return P(u); })"#,
                r##"["data:text/plain,hello#frag", "null", "data:", "", "", "", "", "", "text/plain,hello", "", "#frag"]"##,
            ),
        ],
    },
    Case {
        name: "setters-data-1",
        html: r"<!DOCTYPE html><html><head></head><body><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : v; } catch (e) { return (e instanceof DOMException ? 'DOMException ' + e.name + ' [' + e.code + ']' : e.constructor.name) + ': ' + e.message; } }
function P(u) { return [u.href, u.origin, u.protocol, u.username, u.password, u.host, u.hostname, u.port, u.pathname, u.search, u.hash]; }
function S(init) { return [...new URLSearchParams(init)].map((p) => p.join('=')).join('&'); }
</script></body></html>",
        queries: &[
            (
                r#"E(() => { const u = new URL("data:text/plain,hello#frag"); u.host = "h.test#x"; return P(u); })"#,
                r##"["data:text/plain,hello#frag", "null", "data:", "", "", "", "", "", "text/plain,hello", "", "#frag"]"##,
            ),
            (
                r#"E(() => { const u = new URL("data:text/plain,hello#frag"); u.host = "h.test\\\\x"; return P(u); })"#,
                r##"["data:text/plain,hello#frag", "null", "data:", "", "", "", "", "", "text/plain,hello", "", "#frag"]"##,
            ),
            (
                r#"E(() => { const u = new URL("data:text/plain,hello#frag"); u.host = "[::1]:3000"; return P(u); })"#,
                r##"["data:text/plain,hello#frag", "null", "data:", "", "", "", "", "", "text/plain,hello", "", "#frag"]"##,
            ),
            (
                r#"E(() => { const u = new URL("data:text/plain,hello#frag"); u.host = "[::1"; return P(u); })"#,
                r##"["data:text/plain,hello#frag", "null", "data:", "", "", "", "", "", "text/plain,hello", "", "#frag"]"##,
            ),
            (
                r#"E(() => { const u = new URL("data:text/plain,hello#frag"); u.host = ""; return P(u); })"#,
                r##"["data:text/plain,hello#frag", "null", "data:", "", "", "", "", "", "text/plain,hello", "", "#frag"]"##,
            ),
            (
                r#"E(() => { const u = new URL("data:text/plain,hello#frag"); u.host = "a b"; return P(u); })"#,
                r##"["data:text/plain,hello#frag", "null", "data:", "", "", "", "", "", "text/plain,hello", "", "#frag"]"##,
            ),
            (
                r#"E(() => { const u = new URL("data:text/plain,hello#frag"); u.host = "A.B"; return P(u); })"#,
                r##"["data:text/plain,hello#frag", "null", "data:", "", "", "", "", "", "text/plain,hello", "", "#frag"]"##,
            ),
            (
                r#"E(() => { const u = new URL("data:text/plain,hello#frag"); u.host = ":81"; return P(u); })"#,
                r##"["data:text/plain,hello#frag", "null", "data:", "", "", "", "", "", "text/plain,hello", "", "#frag"]"##,
            ),
            (
                r#"E(() => { const u = new URL("data:text/plain,hello#frag"); u.host = "h.test:80"; return P(u); })"#,
                r##"["data:text/plain,hello#frag", "null", "data:", "", "", "", "", "", "text/plain,hello", "", "#frag"]"##,
            ),
            (
                r#"E(() => { const u = new URL("data:text/plain,hello#frag"); u.host = "h.test:443"; return P(u); })"#,
                r##"["data:text/plain,hello#frag", "null", "data:", "", "", "", "", "", "text/plain,hello", "", "#frag"]"##,
            ),
            (
                r#"E(() => { const u = new URL("data:text/plain,hello#frag"); u.host = "m\u00fcnchen.de"; return P(u); })"#,
                r##"["data:text/plain,hello#frag", "null", "data:", "", "", "", "", "", "text/plain,hello", "", "#frag"]"##,
            ),
            (
                r#"E(() => { const u = new URL("data:text/plain,hello#frag"); u.host = "127.1"; return P(u); })"#,
                r##"["data:text/plain,hello#frag", "null", "data:", "", "", "", "", "", "text/plain,hello", "", "#frag"]"##,
            ),
            (
                r#"E(() => { const u = new URL("data:text/plain,hello#frag"); u.hostname = "h.test"; return P(u); })"#,
                r##"["data:text/plain,hello#frag", "null", "data:", "", "", "", "", "", "text/plain,hello", "", "#frag"]"##,
            ),
            (
                r#"E(() => { const u = new URL("data:text/plain,hello#frag"); u.hostname = "h.test:81"; return P(u); })"#,
                r##"["data:text/plain,hello#frag", "null", "data:", "", "", "", "", "", "text/plain,hello", "", "#frag"]"##,
            ),
            (
                r#"E(() => { const u = new URL("data:text/plain,hello#frag"); u.hostname = "h.test/x"; return P(u); })"#,
                r##"["data:text/plain,hello#frag", "null", "data:", "", "", "", "", "", "text/plain,hello", "", "#frag"]"##,
            ),
            (
                r#"E(() => { const u = new URL("data:text/plain,hello#frag"); u.hostname = "h.test?x"; return P(u); })"#,
                r##"["data:text/plain,hello#frag", "null", "data:", "", "", "", "", "", "text/plain,hello", "", "#frag"]"##,
            ),
            (
                r#"E(() => { const u = new URL("data:text/plain,hello#frag"); u.hostname = "[::1]"; return P(u); })"#,
                r##"["data:text/plain,hello#frag", "null", "data:", "", "", "", "", "", "text/plain,hello", "", "#frag"]"##,
            ),
            (
                r#"E(() => { const u = new URL("data:text/plain,hello#frag"); u.hostname = ""; return P(u); })"#,
                r##"["data:text/plain,hello#frag", "null", "data:", "", "", "", "", "", "text/plain,hello", "", "#frag"]"##,
            ),
            (
                r#"E(() => { const u = new URL("data:text/plain,hello#frag"); u.hostname = "a b"; return P(u); })"#,
                r##"["data:text/plain,hello#frag", "null", "data:", "", "", "", "", "", "text/plain,hello", "", "#frag"]"##,
            ),
            (
                r#"E(() => { const u = new URL("data:text/plain,hello#frag"); u.hostname = "A.B"; return P(u); })"#,
                r##"["data:text/plain,hello#frag", "null", "data:", "", "", "", "", "", "text/plain,hello", "", "#frag"]"##,
            ),
            (
                r#"E(() => { const u = new URL("data:text/plain,hello#frag"); u.port = "81"; return P(u); })"#,
                r##"["data:text/plain,hello#frag", "null", "data:", "", "", "", "", "", "text/plain,hello", "", "#frag"]"##,
            ),
            (
                r#"E(() => { const u = new URL("data:text/plain,hello#frag"); u.port = "81abc"; return P(u); })"#,
                r##"["data:text/plain,hello#frag", "null", "data:", "", "", "", "", "", "text/plain,hello", "", "#frag"]"##,
            ),
            (
                r#"E(() => { const u = new URL("data:text/plain,hello#frag"); u.port = "abc"; return P(u); })"#,
                r##"["data:text/plain,hello#frag", "null", "data:", "", "", "", "", "", "text/plain,hello", "", "#frag"]"##,
            ),
            (
                r#"E(() => { const u = new URL("data:text/plain,hello#frag"); u.port = ""; return P(u); })"#,
                r##"["data:text/plain,hello#frag", "null", "data:", "", "", "", "", "", "text/plain,hello", "", "#frag"]"##,
            ),
            (
                r#"E(() => { const u = new URL("data:text/plain,hello#frag"); u.port = "99999"; return P(u); })"#,
                r##"["data:text/plain,hello#frag", "null", "data:", "", "", "", "", "", "text/plain,hello", "", "#frag"]"##,
            ),
            (
                r#"E(() => { const u = new URL("data:text/plain,hello#frag"); u.port = "65535"; return P(u); })"#,
                r##"["data:text/plain,hello#frag", "null", "data:", "", "", "", "", "", "text/plain,hello", "", "#frag"]"##,
            ),
            (
                r#"E(() => { const u = new URL("data:text/plain,hello#frag"); u.port = "65536"; return P(u); })"#,
                r##"["data:text/plain,hello#frag", "null", "data:", "", "", "", "", "", "text/plain,hello", "", "#frag"]"##,
            ),
            (
                r#"E(() => { const u = new URL("data:text/plain,hello#frag"); u.port = "0"; return P(u); })"#,
                r##"["data:text/plain,hello#frag", "null", "data:", "", "", "", "", "", "text/plain,hello", "", "#frag"]"##,
            ),
            (
                r#"E(() => { const u = new URL("data:text/plain,hello#frag"); u.port = "00081"; return P(u); })"#,
                r##"["data:text/plain,hello#frag", "null", "data:", "", "", "", "", "", "text/plain,hello", "", "#frag"]"##,
            ),
            (
                r#"E(() => { const u = new URL("data:text/plain,hello#frag"); u.port = "80"; return P(u); })"#,
                r##"["data:text/plain,hello#frag", "null", "data:", "", "", "", "", "", "text/plain,hello", "", "#frag"]"##,
            ),
        ],
    },
    Case {
        name: "setters-data-2",
        html: r"<!DOCTYPE html><html><head></head><body><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : v; } catch (e) { return (e instanceof DOMException ? 'DOMException ' + e.name + ' [' + e.code + ']' : e.constructor.name) + ': ' + e.message; } }
function P(u) { return [u.href, u.origin, u.protocol, u.username, u.password, u.host, u.hostname, u.port, u.pathname, u.search, u.hash]; }
function S(init) { return [...new URLSearchParams(init)].map((p) => p.join('=')).join('&'); }
</script></body></html>",
        queries: &[
            (
                r#"E(() => { const u = new URL("data:text/plain,hello#frag"); u.port = "443"; return P(u); })"#,
                r##"["data:text/plain,hello#frag", "null", "data:", "", "", "", "", "", "text/plain,hello", "", "#frag"]"##,
            ),
            (
                r#"E(() => { const u = new URL("data:text/plain,hello#frag"); u.port = " 81"; return P(u); })"#,
                r##"["data:text/plain,hello#frag", "null", "data:", "", "", "", "", "", "text/plain,hello", "", "#frag"]"##,
            ),
            (
                r#"E(() => { const u = new URL("data:text/plain,hello#frag"); u.port = "8\\t1"; return P(u); })"#,
                r##"["data:text/plain,hello#frag", "null", "data:", "", "", "", "", "", "text/plain,hello", "", "#frag"]"##,
            ),
            (
                r#"E(() => { const u = new URL("data:text/plain,hello#frag"); u.port = "-1"; return P(u); })"#,
                r##"["data:text/plain,hello#frag", "null", "data:", "", "", "", "", "", "text/plain,hello", "", "#frag"]"##,
            ),
            (
                r#"E(() => { const u = new URL("data:text/plain,hello#frag"); u.port = "1e3"; return P(u); })"#,
                r##"["data:text/plain,hello#frag", "null", "data:", "", "", "", "", "", "text/plain,hello", "", "#frag"]"##,
            ),
            (
                r#"E(() => { const u = new URL("data:text/plain,hello#frag"); u.pathname = "/a/b"; return P(u); })"#,
                r##"["data:text/plain,hello#frag", "null", "data:", "", "", "", "", "", "text/plain,hello", "", "#frag"]"##,
            ),
            (
                r#"E(() => { const u = new URL("data:text/plain,hello#frag"); u.pathname = "a/b"; return P(u); })"#,
                r##"["data:text/plain,hello#frag", "null", "data:", "", "", "", "", "", "text/plain,hello", "", "#frag"]"##,
            ),
            (
                r#"E(() => { const u = new URL("data:text/plain,hello#frag"); u.pathname = ""; return P(u); })"#,
                r##"["data:text/plain,hello#frag", "null", "data:", "", "", "", "", "", "text/plain,hello", "", "#frag"]"##,
            ),
            (
                r#"E(() => { const u = new URL("data:text/plain,hello#frag"); u.pathname = "/a b?c#d%e"; return P(u); })"#,
                r##"["data:text/plain,hello#frag", "null", "data:", "", "", "", "", "", "text/plain,hello", "", "#frag"]"##,
            ),
            (
                r#"E(() => { const u = new URL("data:text/plain,hello#frag"); u.pathname = "/a/../b/./c"; return P(u); })"#,
                r##"["data:text/plain,hello#frag", "null", "data:", "", "", "", "", "", "text/plain,hello", "", "#frag"]"##,
            ),
            (
                r#"E(() => { const u = new URL("data:text/plain,hello#frag"); u.pathname = "\\\\a\\\\b"; return P(u); })"#,
                r##"["data:text/plain,hello#frag", "null", "data:", "", "", "", "", "", "text/plain,hello", "", "#frag"]"##,
            ),
            (
                r#"E(() => { const u = new URL("data:text/plain,hello#frag"); u.pathname = "/\u00e9\ud83d\ude00"; return P(u); })"#,
                r##"["data:text/plain,hello#frag", "null", "data:", "", "", "", "", "", "text/plain,hello", "", "#frag"]"##,
            ),
            (
                r#"E(() => { const u = new URL("data:text/plain,hello#frag"); u.pathname = "//x"; return P(u); })"#,
                r##"["data:text/plain,hello#frag", "null", "data:", "", "", "", "", "", "text/plain,hello", "", "#frag"]"##,
            ),
            (
                r#"E(() => { const u = new URL("data:text/plain,hello#frag"); u.pathname = "/%41/%zz"; return P(u); })"#,
                r##"["data:text/plain,hello#frag", "null", "data:", "", "", "", "", "", "text/plain,hello", "", "#frag"]"##,
            ),
            (
                r#"E(() => { const u = new URL("data:text/plain,hello#frag"); u.search = "q=1"; return P(u); })"#,
                r##"["data:text/plain,hello?q=1#frag", "null", "data:", "", "", "", "", "", "text/plain,hello", "?q=1", "#frag"]"##,
            ),
            (
                r#"E(() => { const u = new URL("data:text/plain,hello#frag"); u.search = "?q=1"; return P(u); })"#,
                r##"["data:text/plain,hello?q=1#frag", "null", "data:", "", "", "", "", "", "text/plain,hello", "?q=1", "#frag"]"##,
            ),
            (
                r#"E(() => { const u = new URL("data:text/plain,hello#frag"); u.search = "??q=1"; return P(u); })"#,
                r##"["data:text/plain,hello??q=1#frag", "null", "data:", "", "", "", "", "", "text/plain,hello", "??q=1", "#frag"]"##,
            ),
            (
                r#"E(() => { const u = new URL("data:text/plain,hello#frag"); u.search = ""; return P(u); })"#,
                r##"["data:text/plain,hello#frag", "null", "data:", "", "", "", "", "", "text/plain,hello", "", "#frag"]"##,
            ),
            (
                r#"E(() => { const u = new URL("data:text/plain,hello#frag"); u.search = "?"; return P(u); })"#,
                r##"["data:text/plain,hello#frag", "null", "data:", "", "", "", "", "", "text/plain,hello", "", "#frag"]"##,
            ),
            (
                r#"E(() => { const u = new URL("data:text/plain,hello#frag"); u.search = "a b&c='d'#e"; return P(u); })"#,
                r##"["data:text/plain,hello?a%20b&c=%27d%27%23e#frag", "null", "data:", "", "", "", "", "", "text/plain,hello", "?a%20b&c=%27d%27%23e", "#frag"]"##,
            ),
            (
                r#"E(() => { const u = new URL("data:text/plain,hello#frag"); u.search = "\u00e9\ud83d\ude00"; return P(u); })"#,
                r##"["data:text/plain,hello?%C3%A9%F0%9F%98%80#frag", "null", "data:", "", "", "", "", "", "text/plain,hello", "?%C3%A9%F0%9F%98%80", "#frag"]"##,
            ),
            (
                r#"E(() => { const u = new URL("data:text/plain,hello#frag"); u.search = "q=1\\n2"; return P(u); })"#,
                r##"["data:text/plain,hello?q=1\\n2#frag", "null", "data:", "", "", "", "", "", "text/plain,hello", "?q=1\\n2", "#frag"]"##,
            ),
            (
                r#"E(() => { const u = new URL("data:text/plain,hello#frag"); u.hash = "f"; return P(u); })"#,
                r##"["data:text/plain,hello#f", "null", "data:", "", "", "", "", "", "text/plain,hello", "", "#f"]"##,
            ),
            (
                r##"E(() => { const u = new URL("data:text/plain,hello#frag"); u.hash = "#f"; return P(u); })"##,
                r##"["data:text/plain,hello#f", "null", "data:", "", "", "", "", "", "text/plain,hello", "", "#f"]"##,
            ),
            (
                r###"E(() => { const u = new URL("data:text/plain,hello#frag"); u.hash = "##f"; return P(u); })"###,
                r###"["data:text/plain,hello##f", "null", "data:", "", "", "", "", "", "text/plain,hello", "", "##f"]"###,
            ),
            (
                r#"E(() => { const u = new URL("data:text/plain,hello#frag"); u.hash = ""; return P(u); })"#,
                r#"["data:text/plain,hello", "null", "data:", "", "", "", "", "", "text/plain,hello", "", ""]"#,
            ),
            (
                r##"E(() => { const u = new URL("data:text/plain,hello#frag"); u.hash = "#"; return P(u); })"##,
                r#"["data:text/plain,hello#", "null", "data:", "", "", "", "", "", "text/plain,hello", "", ""]"#,
            ),
            (
                r#"E(() => { const u = new URL("data:text/plain,hello#frag"); u.hash = "a b<>`\"'\u00e9"; return P(u); })"#,
                r##"["data:text/plain,hello#a%20b%3C%3E%60%22'%C3%A9", "null", "data:", "", "", "", "", "", "text/plain,hello", "", "#a%20b%3C%3E%60%22'%C3%A9"]"##,
            ),
            (
                r#"E(() => { const u = new URL("data:text/plain,hello#frag"); u.hash = "f\\t1"; return P(u); })"#,
                r##"["data:text/plain,hello#f\\t1", "null", "data:", "", "", "", "", "", "text/plain,hello", "", "#f\\t1"]"##,
            ),
        ],
    },
    Case {
        name: "setters-misc",
        html: r"<!DOCTYPE html><html><head></head><body><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : v; } catch (e) { return (e instanceof DOMException ? 'DOMException ' + e.name + ' [' + e.code + ']' : e.constructor.name) + ': ' + e.message; } }
function P(u) { return [u.href, u.origin, u.protocol, u.username, u.password, u.host, u.hostname, u.port, u.pathname, u.search, u.hash]; }
function S(init) { return [...new URLSearchParams(init)].map((p) => p.join('=')).join('&'); }
</script></body></html>",
        queries: &[
            (
                r"E(() => { const u = new URL('http://a/'); u.href = 'foo'; })",
                r#""TypeError: Failed to set the 'href' property on 'URL': Invalid URL""#,
            ),
            (
                r"E(() => { const u = new URL('http://a/'); u.href = 'http://[x'; })",
                r#""TypeError: Failed to set the 'href' property on 'URL': Invalid URL""#,
            ),
            (
                r"E(() => { const u = new URL('http://a/'); u.origin = 'http://b/'; return u.href; })",
                r#""http://a/""#,
            ),
            (
                r"E(() => { 'use strict'; const u = new URL('http://a/'); u.origin = 'http://b/'; })",
                r#""TypeError: Cannot set property origin of [object URL] which has only a getter""#,
            ),
            (
                r"E(() => { 'use strict'; const u = new URL('http://a/'); u.searchParams = 1; })",
                r#""TypeError: Cannot set property searchParams of [object URL] which has only a getter""#,
            ),
            (
                r"E(() => { const u = new URL('http://a/'); u.port = {toString() { return '81'; }}; return u.port; })",
                r#""81""#,
            ),
            (
                r"E(() => { const u = new URL('http://a/'); u.port = 81; return u.port; })",
                r#""81""#,
            ),
            (
                r"E(() => { const u = new URL('http://a/'); u.pathname = null; return u.pathname; })",
                r#""/null""#,
            ),
            (
                r"E(() => { const u = new URL('http://a/'); u.hash = undefined; return u.hash; })",
                r##""#undefined""##,
            ),
            (
                r"E(() => { const u = new URL('http://a/'); u.search = Symbol(); })",
                r#""TypeError: Failed to set the 'search' property on 'URL': Cannot convert a Symbol value to a string""#,
            ),
            (
                r"E(() => { const u = new URL('http://a/'); u.hash = '\ud800'; return u.hash; })",
                r##""#%EF%BF%BD""##,
            ),
            (
                r"E(() => { const u = new URL('http://a/'); u.search = 'x\ud800'; return u.search; })",
                r#""?x%EF%BF%BD""#,
            ),
            (
                r"E(() => { const u = new URL('http://a/?a=1'); const sp = u.searchParams; u.search = 'b=2&c=3'; return [sp.toString(), sp.get('a'), sp.get('b'), sp.size]; })",
                r#"["b=2&c=3", null, "2", 2]"#,
            ),
            (
                r"E(() => { const u = new URL('http://a/?a=1'); const sp = u.searchParams; u.href = 'http://b/?z=9'; return [sp.toString(), sp === u.searchParams]; })",
                r#"["z=9", true]"#,
            ),
            (
                r"E(() => { const u = new URL('http://a/?a=1'); const sp = u.searchParams; u.search = ''; return [sp.toString(), sp.size, u.search, u.href]; })",
                r#"["", 0, "", "http://a/"]"#,
            ),
            (
                r"E(() => { const u = new URL('http://a/?a=1#h'); u.searchParams.append('b', '2 3'); return [u.search, u.href]; })",
                r#"["?a=1&b=2+3", "http://a/?a=1&b=2+3#h"]"#,
            ),
            (
                r"E(() => { const u = new URL('http://a/?a=1'); u.searchParams.delete('a'); return [u.search, u.href]; })",
                r#"["", "http://a/"]"#,
            ),
            (
                r"E(() => { const u = new URL('http://a/?a=1'); u.searchParams.set('a', ''); return [u.search, u.href]; })",
                r#"["?a=", "http://a/?a="]"#,
            ),
            (
                r"E(() => { const u = new URL('http://a/?a=%7e+x'); u.searchParams.sort(); return [u.search, u.href]; })",
                r#"["?a=%7E+x", "http://a/?a=%7E+x"]"#,
            ),
            (
                r"E(() => { const u = new URL('http://a/?a=%7e+x'); const before = u.search; u.searchParams.get('a'); return [u.search === before, u.search]; })",
                r#"[true, "?a=%7e+x"]"#,
            ),
            (
                r"E(() => { const u = new URL('http://a/?a=1&&b'); u.searchParams.append('c', 'd'); return u.search; })",
                r#""?a=1&b=&c=d""#,
            ),
            (
                r"E(() => { const u = new URL('http://a/?a=1'); u.searchParams.delete('zz'); return u.search; })",
                r#""?a=1""#,
            ),
            (
                r"E(() => { const u = new URL('mailto:a@b?x=1'); u.searchParams.append('y', '2'); return u.href; })",
                r#""mailto:a@b?x=1&y=2""#,
            ),
            (
                r"E(() => { const u = new URL('http://a/?'); const sp = u.searchParams; return [sp.size, u.search, u.href]; })",
                r#"[0, "", "http://a/?"]"#,
            ),
            (
                r"E(() => { const u = new URL('http://a/?'); u.searchParams.delete('q'); return [u.search, u.href]; })",
                r#"["", "http://a/"]"#,
            ),
            (
                r"E(() => { const u = new URL('http://a/?x'); u.searchParams.delete('q'); return [u.search, u.href]; })",
                r#"["?x=", "http://a/?x="]"#,
            ),
        ],
    },
    Case {
        name: "params-construct",
        html: r"<!DOCTYPE html><html><head></head><body><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : v; } catch (e) { return (e instanceof DOMException ? 'DOMException ' + e.name + ' [' + e.code + ']' : e.constructor.name) + ': ' + e.message; } }
function P(u) { return [u.href, u.origin, u.protocol, u.username, u.password, u.host, u.hostname, u.port, u.pathname, u.search, u.hash]; }
function S(init) { return [...new URLSearchParams(init)].map((p) => p.join('=')).join('&'); }
</script></body></html>",
        queries: &[
            (
                r"[S(''), S('a=1&b=2'), S('?a=1&b=2'), S('??a=1'), S('a'), S('a='), S('=b'), S('='), S('a=1&a=2'), S('&&a=1&&'), S('a=b=c'), S('a+b=c+d'), S('a%20b=%41'), S('%zz=%'), S('a=%e9'), S('a=\u00e9'), S('%F0%9F%98%80=1'), S(' a = b ')]",
                r#"["", "a=1&b=2", "a=1&b=2", "?a=1", "a=", "a=", "=b", "=", "a=1&a=2", "a=1", "a=b=c", "a b=c d", "a b=A", "%zz=%", "a=�", "a=é", "😀=1", " a = b "]"#,
            ),
            (
                r"[S([['a', '1'], ['b', '2']]), S({a: '1', b: '2'}), S(new Map([['a', '1']])), S(new URLSearchParams('x=1')), S(undefined), S(null), S(5), S(true), S({}), S([]), S(new URLSearchParams()), S({toString() { return 'q=7'; }})]",
                r#"["a=1&b=2", "a=1&b=2", "a=1", "x=1", "", "null=", "5=", "true=", "", "", "", "toString=toString() { return 'q=7'; }"]"#,
            ),
            (
                r"E(() => S([['a']]))",
                r#""TypeError: Failed to construct 'URLSearchParams': Failed to construct 'URLSearchParams': Sequence initializer must only contain pair elements""#,
            ),
            (
                r"E(() => S([['a', 'b', 'c']]))",
                r#""TypeError: Failed to construct 'URLSearchParams': Failed to construct 'URLSearchParams': Sequence initializer must only contain pair elements""#,
            ),
            (
                r"E(() => S([1]))",
                r#""TypeError: Failed to construct 'URLSearchParams': The provided value cannot be converted to a sequence.""#,
            ),
            (
                r"E(() => S(['ab']))",
                r#""TypeError: Failed to construct 'URLSearchParams': The provided value cannot be converted to a sequence.""#,
            ),
            (
                r"E(() => S([[]]))",
                r#""TypeError: Failed to construct 'URLSearchParams': Failed to construct 'URLSearchParams': Sequence initializer must only contain pair elements""#,
            ),
            (
                r"E(() => S({a: 1, b: null, c: undefined, d: {}}))",
                r#""a=1&b=null&c=undefined&d=[object Object]""#,
            ),
            (
                r"E(() => S(Object.create({inherited: 'x'}, {own: {value: 'y', enumerable: true}, hidden: {value: 'z'}})))",
                r#""own=y""#,
            ),
            (
                r"E(() => S({[Symbol('s')]: 'x', k: 'v'}))",
                r#""TypeError: Failed to construct 'URLSearchParams': Cannot convert a Symbol value to a string""#,
            ),
            (r"E(() => S(new Proxy({a: '1'}, {})))", r#""a=1""#),
            (r"E(() => S(function () {}))", r#""""#),
            (
                r"E(() => S(Symbol()))",
                r#""TypeError: Failed to construct 'URLSearchParams': Cannot convert a Symbol value to a string""#,
            ),
            (
                r"E(() => S({a: Symbol()}))",
                r#""TypeError: Failed to construct 'URLSearchParams': Cannot convert a Symbol value to a string""#,
            ),
            (
                r"E(() => S([[Symbol(), 'x']]))",
                r#""TypeError: Failed to construct 'URLSearchParams': Cannot convert a Symbol value to a string""#,
            ),
            (r"E(() => S('\ud800=\udc00'))", r#""�=�""#),
            (
                r"E(() => [...new URLSearchParams([['\ud800', 'a\udfff']])][0].map((s) => s.charCodeAt(0).toString(16)).join())",
                r#""fffd,61""#,
            ),
            (r"E(() => S(new Set([['a', '1']])))", r#""a=1""#),
            (
                r"E(() => S((function* () { yield ['a', '1']; yield ['b', '2']; })()))",
                r#""a=1&b=2""#,
            ),
            (r"E(() => S({get a() { return 'v'; }}))", r#""a=v""#),
            (
                r"E(() => { const o = {a: '1'}; return S(Object.defineProperty(o, 'b', {get() { throw new RangeError('boom'); }, enumerable: true})); })",
                r#""RangeError: boom""#,
            ),
            (
                r"E(() => new URLSearchParams('a', 'b').toString())",
                r#""a=""#,
            ),
            (
                r"E(() => new URLSearchParams({a: '1', a2: '2'}).toString())",
                r#""a=1&a2=2""#,
            ),
            (
                r"E(() => { const l = [['a', '1']]; const p = new URLSearchParams(l); l.push(['b', '2']); return p.toString(); })",
                r#""a=1""#,
            ),
            (
                r"E(() => { const u = new URL('http://a/?x=1'); const p = new URLSearchParams(u.searchParams); p.append('y', '2'); return [p.toString(), u.search]; })",
                r#"["x=1&y=2", "?x=1"]"#,
            ),
        ],
    },
    Case {
        name: "params-methods",
        html: r"<!DOCTYPE html><html><head></head><body><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : v; } catch (e) { return (e instanceof DOMException ? 'DOMException ' + e.name + ' [' + e.code + ']' : e.constructor.name) + ': ' + e.message; } }
function P(u) { return [u.href, u.origin, u.protocol, u.username, u.password, u.host, u.hostname, u.port, u.pathname, u.search, u.hash]; }
function S(init) { return [...new URLSearchParams(init)].map((p) => p.join('=')).join('&'); }
</script></body></html>",
        queries: &[
            (
                r"(() => { const p = new URLSearchParams('a=1&b=2&a=3'); return [p.get('a'), p.get('b'), p.get('c'), p.getAll('a'), p.getAll('c'), p.has('a'), p.has('c'), p.has('a', '3'), p.has('a', '9'), p.has('a', undefined), p.size]; })()",
                r#"["1", "2", null, ["1", "3"], [], true, false, true, false, true, 3]"#,
            ),
            (
                r"(() => { const p = new URLSearchParams('a=1&b=2&a=3'); p.delete('a'); return [p.toString(), p.size]; })()",
                r#"["b=2", 1]"#,
            ),
            (
                r"(() => { const p = new URLSearchParams('a=1&b=2&a=3'); p.delete('a', '3'); return [p.toString(), p.size]; })()",
                r#"["a=1&b=2", 2]"#,
            ),
            (
                r"(() => { const p = new URLSearchParams('a=1&b=2&a=3'); p.delete('a', undefined); return p.toString(); })()",
                r#""b=2""#,
            ),
            (
                r"(() => { const p = new URLSearchParams('a=1&b=2&a=3'); p.delete('a', ''); return p.toString(); })()",
                r#""a=1&b=2&a=3""#,
            ),
            (
                r"(() => { const p = new URLSearchParams('a=1&b=2&a=3'); p.set('a', 'x'); return p.toString(); })()",
                r#""a=x&b=2""#,
            ),
            (
                r"(() => { const p = new URLSearchParams('a=1&b=2&a=3'); p.set('c', 'x'); return p.toString(); })()",
                r#""a=1&b=2&a=3&c=x""#,
            ),
            (
                r"(() => { const p = new URLSearchParams('a=1&b=2&a=3'); p.append('a', 'x'); p.append('d'  + 1, 5); return p.toString(); })()",
                r#""a=1&b=2&a=3&a=x&d1=5""#,
            ),
            (
                r"(() => { const p = new URLSearchParams('b=2&a=1&c=3&a=0&B=1'); p.sort(); return p.toString(); })()",
                r#""B=1&a=1&a=0&b=2&c=3""#,
            ),
            (
                r"(() => { const p = new URLSearchParams([['\ue000', '1'], ['\ud83d\ude00', '2'], ['z', '3'], ['\uffff', '4']]); p.sort(); return [...p.keys()].map((k) => k.charCodeAt(0).toString(16)).join(); })()",
                r#""7a,d83d,e000,ffff""#,
            ),
            (
                r"(() => { const p = new URLSearchParams('x=1&x=0&y=1&y=0&x=2'); p.sort(); return p.toString(); })()",
                r#""x=1&x=0&x=2&y=1&y=0""#,
            ),
            (
                r"E(() => new URLSearchParams().append('a'))",
                r#""TypeError: Failed to execute 'append' on 'URLSearchParams': 2 arguments required, but only 1 present.""#,
            ),
            (
                r"E(() => new URLSearchParams().append())",
                r#""TypeError: Failed to execute 'append' on 'URLSearchParams': 2 arguments required, but only 0 present.""#,
            ),
            (
                r"E(() => new URLSearchParams().get())",
                r#""TypeError: Failed to execute 'get' on 'URLSearchParams': 1 argument required, but only 0 present.""#,
            ),
            (
                r"E(() => new URLSearchParams().set('a'))",
                r#""TypeError: Failed to execute 'set' on 'URLSearchParams': 2 arguments required, but only 1 present.""#,
            ),
            (
                r"E(() => new URLSearchParams().has())",
                r#""TypeError: Failed to execute 'has' on 'URLSearchParams': 1 argument required, but only 0 present.""#,
            ),
            (
                r"E(() => new URLSearchParams().delete())",
                r#""TypeError: Failed to execute 'delete' on 'URLSearchParams': 1 argument required, but only 0 present.""#,
            ),
            (
                r"E(() => new URLSearchParams().getAll())",
                r#""TypeError: Failed to execute 'getAll' on 'URLSearchParams': 1 argument required, but only 0 present.""#,
            ),
            (
                r"E(() => { const p = new URLSearchParams(); p.append(undefined, null); return p.toString(); })",
                r#""undefined=null""#,
            ),
            (
                r"E(() => { const p = new URLSearchParams(); p.append(1, {}); return p.toString(); })",
                r#""1=%5Bobject+Object%5D""#,
            ),
            (
                r"E(() => { const p = new URLSearchParams(); p.append(Symbol(), 'x'); })",
                r#""TypeError: Failed to execute 'append' on 'URLSearchParams': Cannot convert a Symbol value to a string""#,
            ),
            (
                r"E(() => { const p = new URLSearchParams('a=b'); p.append('\ud800', 'x'); return p.toString(); })",
                r#""a=b&%EF%BF%BD=x""#,
            ),
            (
                r"E(() => URLSearchParams.prototype.append.call(null, 'a', 'b'))",
                r#""TypeError: Illegal invocation""#,
            ),
            (
                r"[new URLSearchParams('a=1').toString === URLSearchParams.prototype.toString, typeof URLSearchParams.prototype.toString, URLSearchParams.prototype.toString.length]",
                r#"[true, "function", 0]"#,
            ),
            (
                r"[new URLSearchParams('a=ä ö&b=*-._~!\'()').toString(), new URLSearchParams({x: 'a/b?c#d&e=f+g%h'}).toString(), new URLSearchParams({'é': '😀'}).toString()]",
                r#"["a=%C3%A4+%C3%B6&b=*-._%7E%21%27%28%29", "x=a%2Fb%3Fc%23d%26e%3Df%2Bg%25h", "%C3%A9=%F0%9F%98%80"]"#,
            ),
        ],
    },
    Case {
        name: "params-iteration",
        html: r"<!DOCTYPE html><html><head></head><body><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : v; } catch (e) { return (e instanceof DOMException ? 'DOMException ' + e.name + ' [' + e.code + ']' : e.constructor.name) + ': ' + e.message; } }
function P(u) { return [u.href, u.origin, u.protocol, u.username, u.password, u.host, u.hostname, u.port, u.pathname, u.search, u.hash]; }
function S(init) { return [...new URLSearchParams(init)].map((p) => p.join('=')).join('&'); }
</script></body></html>",
        queries: &[
            (
                r"(() => { const p = new URLSearchParams('a=1&b=2'); return [[...p].join('|'), [...p.entries()].join('|'), [...p.keys()].join(), [...p.values()].join(), Array.from(p).length]; })()",
                r#"["a,1|b,2", "a,1|b,2", "a,b", "1,2", 2]"#,
            ),
            (
                r"(() => { const p = new URLSearchParams('a=1&b=2'); const out = []; p.forEach(function (v, k, o) { out.push(k + v + (o === p) + (this === undefined ? 'u' : typeof this)); }); return out; })()",
                r#"["a1trueobject", "b2trueobject"]"#,
            ),
            (
                r"(() => { const p = new URLSearchParams('a=1&b=2'); const out = []; p.forEach(function (v, k) { out.push(this.tag + k); }, {tag: 'T'}); return out; })()",
                r#"["Ta", "Tb"]"#,
            ),
            (
                r"E(() => new URLSearchParams('a=1').forEach())",
                r#""TypeError: Failed to execute 'forEach' on 'URLSearchParams': 1 argument required, but only 0 present.""#,
            ),
            (
                r"E(() => new URLSearchParams('a=1').forEach(1))",
                r#""TypeError: Failed to execute 'forEach' on 'URLSearchParams': parameter 1 is not of type 'Function'.""#,
            ),
            (
                r"(() => { const p = new URLSearchParams('a=1&b=2&c=3'); const out = []; for (const [k] of p) { out.push(k); if (k === 'a') p.delete('b'); } return out; })()",
                r#"["a", "c"]"#,
            ),
            (
                r"(() => { const p = new URLSearchParams('a=1'); const out = []; for (const [k] of p) { out.push(k); if (out.length < 5) p.append(k + 'x', '1'); } return out; })()",
                r#"["a", "ax", "axx", "axxx", "axxxx"]"#,
            ),
            (
                r"(() => { const p = new URLSearchParams('a=1&b=2'); const it = p[Symbol.iterator](); const first = it.next(); p.append('c', '3'); return [first.value.join(), it.next().value.join(), it.next().value.join(), it.next().done, it.next().done]; })()",
                r#"["a,1", "b,2", "c,3", true, true]"#,
            ),
            (
                r"(() => { const p = new URLSearchParams('a=1'); const it = p.entries(); return [Object.prototype.toString.call(it), typeof it.next, it[Symbol.iterator]() === it, Object.getPrototypeOf(Object.getPrototypeOf(it)) === Object.getPrototypeOf(Object.getPrototypeOf([][Symbol.iterator]())), Object.getOwnPropertyNames(Object.getPrototypeOf(it)).join(), String(Object.keys(it.next()))]; })()",
                r#"["[object URLSearchParams Iterator]", "function", true, true, "next", "done,value"]"#,
            ),
            (
                r"(() => { const p = new URLSearchParams('a=1'); const it = p.keys(); const r = it.next(); const e = it.next(); return [JSON.stringify(r), JSON.stringify(e), JSON.stringify(it.next())]; })()",
                r#"["{\"done\":false,\"value\":\"a\"}", "{\"done\":true}", "{\"done\":true}"]"#,
            ),
            (
                r"E(() => { const p = new URLSearchParams('a=1'); return p.entries().next.call({}); })",
                r#""TypeError: Illegal invocation""#,
            ),
            (
                r"(() => { const u = new URL('http://a/?k=1&k=2'); return [...u.searchParams.values()].join(); })()",
                r#""1,2""#,
            ),
            (
                r"(() => { const p = new URLSearchParams('a=1'); const [x] = p; return [Array.isArray(x), x.length]; })()",
                r"[true, 2]",
            ),
            (
                r"(() => { const p = new URLSearchParams('a=1&b=2'); return Object.fromEntries(p); })()",
                r#"{"a": "1", "b": "2"}"#,
            ),
            (
                r"(() => { const p = new URLSearchParams('a=1&a=2'); return JSON.stringify(Object.fromEntries(p)); })()",
                r#""{\"a\":\"2\"}""#,
            ),
            (
                r"(() => { const p = new URLSearchParams([['a', '1'], ['a', '2']]); return new Map(p).get('a'); })()",
                r#""2""#,
            ),
            (
                r"(() => { const p = new URLSearchParams('a=1'); return [typeof p.entries, p.entries === p[Symbol.iterator], p.size]; })()",
                r#"["function", true, 1]"#,
            ),
        ],
    },
];
