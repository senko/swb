//! `file:` URLs: files and directory listings.

use std::ffi::OsString;
use std::fmt::Write as _;
use std::fs;
use std::io::{self, Read as _};
use std::path::Path;

use url::Url;

use super::MAX_BODY_SIZE;
use crate::error::NetError;
use crate::escape::escape_html;
use crate::file_types::mime_type_for_extension;
use crate::headers::Headers;
use crate::response::Response;

/// Reads the file or directory at `url`.
///
/// - A file: status 200, `Content-Type` from the extension (none if the
///   extension is unknown).
/// - A directory: status 200, an HTML page with links to the entries.
/// - A missing file: status 404 with a short text body.
pub(crate) fn fetch(url: &Url) -> Result<Response, NetError> {
    let path = url.to_file_path().map_err(|()| NetError::InvalidUrl {
        url: url.clone(),
        reason: "not a local file path".to_string(),
    })?;
    let metadata = match fs::metadata(&path) {
        Ok(metadata) => metadata,
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
            ) =>
        {
            return Ok(not_found(url, &path));
        }
        Err(error) => return Err(error.into()),
    };
    if metadata.is_dir() {
        return directory_listing(url, &path);
    }
    // Devices, FIFOs and the like could block or never end.
    if !metadata.is_file() {
        return Err(NetError::Io(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{} is not a regular file", path.display()),
        )));
    }
    let mut body = Vec::new();
    fs::File::open(&path)?
        .take(MAX_BODY_SIZE + 1)
        .read_to_end(&mut body)?;
    if body.len() as u64 > MAX_BODY_SIZE {
        return Err(NetError::BodyTooLarge {
            limit: MAX_BODY_SIZE,
        });
    }
    let mut headers = Headers::new();
    if let Some(mime_type) = path
        .extension()
        .and_then(|extension| extension.to_str())
        .and_then(mime_type_for_extension)
    {
        headers.append("content-type", mime_type);
    }
    Ok(Response {
        url: url.clone(),
        status: 200,
        headers,
        body,
        redirected: false,
    })
}

fn not_found(url: &Url, path: &Path) -> Response {
    let mut headers = Headers::new();
    headers.append("content-type", "text/plain;charset=utf-8");
    Response {
        url: url.clone(),
        status: 404,
        headers,
        body: format!("File not found: {}\n", path.display()).into_bytes(),
        redirected: false,
    }
}

/// Generates an HTML page that lists the entries of `dir`, sorted by name.
/// Links are absolute `file:` URLs, so they work with and without a trailing
/// slash in `url`.
fn directory_listing(url: &Url, dir: &Path) -> Result<Response, NetError> {
    let mut entries: Vec<(OsString, bool)> = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        // fs::metadata follows symbolic links.
        let is_dir = fs::metadata(entry.path()).is_ok_and(|m| m.is_dir());
        entries.push((entry.file_name(), is_dir));
    }
    entries.sort();

    let title = escape_html(&format!("Index of {}", dir.display()));
    let mut html = format!(
        "<!DOCTYPE html>\n<html>\n<head>\n<meta charset=\"utf-8\">\n\
         <title>{title}</title>\n</head>\n<body>\n<h1>{title}</h1>\n<ul>\n"
    );
    if let Some(parent) = dir.parent()
        && let Ok(href) = Url::from_directory_path(parent)
    {
        push_link(&mut html, &href, "../");
    }
    for (name, is_dir) in &entries {
        let path = dir.join(name);
        let (href, suffix) = if *is_dir {
            (Url::from_directory_path(&path), "/")
        } else {
            (Url::from_file_path(&path), "")
        };
        if let Ok(href) = href {
            push_link(
                &mut html,
                &href,
                &format!("{}{suffix}", name.to_string_lossy()),
            );
        }
    }
    html.push_str("</ul>\n</body>\n</html>\n");

    let mut headers = Headers::new();
    headers.append("content-type", "text/html;charset=utf-8");
    Ok(Response {
        url: url.clone(),
        status: 200,
        headers,
        body: html.into_bytes(),
        redirected: false,
    })
}

fn push_link(html: &mut String, href: &Url, label: &str) {
    let _ = writeln!(
        html,
        "<li><a href=\"{}\">{}</a></li>",
        escape_html(href.as_str()),
        escape_html(label)
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::TempDir;

    fn file_url(path: &Path) -> Url {
        Url::from_file_path(path).unwrap()
    }

    #[test]
    fn reads_file_with_content_type() {
        let dir = TempDir::new();
        let path = dir.path().join("page.HTML");
        fs::write(&path, "<p>hi</p>").unwrap();
        let response = fetch(&file_url(&path)).unwrap();
        assert_eq!(response.status, 200);
        assert_eq!(response.body, b"<p>hi</p>");
        assert_eq!(response.headers.get("content-type"), Some("text/html"));
    }

    #[cfg(unix)]
    #[test]
    fn devices_are_not_read() {
        // /dev/zero has no end; reading it must fail at once.
        let url = Url::parse("file:///dev/zero").unwrap();
        assert!(matches!(fetch(&url), Err(NetError::Io(_))));
    }

    #[test]
    fn content_types_by_extension() {
        let dir = TempDir::new();
        for (name, expected) in [
            ("a.css", Some("text/css")),
            ("a.png", Some("image/png")),
            ("a.woff2", Some("font/woff2")),
            ("a.unknown", None),
            ("noextension", None),
        ] {
            let path = dir.path().join(name);
            fs::write(&path, "x").unwrap();
            let response = fetch(&file_url(&path)).unwrap();
            assert_eq!(response.headers.get("content-type"), expected, "{name}");
        }
    }

    #[test]
    fn url_with_fragment_and_query() {
        let dir = TempDir::new();
        let path = dir.path().join("a.txt");
        fs::write(&path, "text").unwrap();
        let mut url = file_url(&path);
        url.set_query(Some("q"));
        url.set_fragment(Some("f"));
        let response = fetch(&url).unwrap();
        assert_eq!(response.body, b"text");
        assert_eq!(response.url, url);
    }

    #[test]
    fn missing_file_is_404() {
        let dir = TempDir::new();
        let response = fetch(&file_url(&dir.path().join("missing.html"))).unwrap();
        assert_eq!(response.status, 404);
        // A path below a regular file.
        let file = dir.path().join("file.txt");
        fs::write(&file, "x").unwrap();
        let response = fetch(&file_url(&file.join("below"))).unwrap();
        assert_eq!(response.status, 404);
    }

    #[test]
    fn directory_listing_is_sorted_with_links() {
        let dir = TempDir::new();
        fs::write(dir.path().join("b.txt"), "b").unwrap();
        fs::write(dir.path().join("a <&>.txt"), "a").unwrap();
        fs::create_dir(dir.path().join("c")).unwrap();
        let response = fetch(&file_url(dir.path())).unwrap();
        assert_eq!(response.status, 200);
        assert_eq!(response.content_type().unwrap().essence, "text/html");
        let html = String::from_utf8(response.body).unwrap();

        let base = Url::from_directory_path(dir.path()).unwrap();
        let a = format!("<a href=\"{base}a%20%3C&amp;%3E.txt\">a &lt;&amp;&gt;.txt</a>");
        let b = format!("<a href=\"{base}b.txt\">b.txt</a>");
        let c = format!("<a href=\"{base}c/\">c/</a>");
        let positions: Vec<usize> = [&a, &b, &c]
            .iter()
            .map(|link| {
                html.find(link.as_str())
                    .unwrap_or_else(|| panic!("{link} not in {html}"))
            })
            .collect();
        assert!(positions.is_sorted(), "{html}");
        assert!(html.contains(">../</a>"));
    }

    #[test]
    fn remote_host_is_invalid() {
        let url = Url::parse("file://example.com/etc/hosts").unwrap();
        assert!(matches!(fetch(&url), Err(NetError::InvalidUrl { .. })));
    }
}
