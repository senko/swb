//! Turns what the user typed (or passed on the command line) into a URL.

use std::path::Path;

use swb_net::Url;

/// Parses what the user typed into the address bar:
///
/// - an absolute file path becomes a `file:` URL;
/// - input with a scheme is parsed as is;
/// - anything else gets `https://` prepended.
///
/// Relative paths are not looked up, so that `news` opens `https://news/`
/// even if the working directory has a file with that name.
pub(crate) fn parse_address(input: &str) -> Result<Url, String> {
    let input = input.trim();
    if input.is_empty() {
        return Err("empty address".to_owned());
    }
    if Path::new(input).is_absolute() {
        return Url::from_file_path(input).map_err(|()| format!("{input}: not a valid path"));
    }
    if has_scheme(input) {
        return Url::parse(input).map_err(|e| format!("{input}: {e}"));
    }
    Url::parse(&format!("https://{input}")).map_err(|e| format!("{input}: {e}"))
}

/// Parses the URL argument of the command line: an existing file path
/// (relative to the working directory or absolute) becomes a `file:` URL;
/// anything else is parsed as by [`parse_address`].
pub(crate) fn parse_command_line_url(input: &str) -> Result<Url, String> {
    let trimmed = input.trim();
    let path = Path::new(trimmed);
    if !trimmed.is_empty() && path.exists() {
        let absolute = path.canonicalize().map_err(|e| format!("{trimmed}: {e}"))?;
        return Url::from_file_path(&absolute).map_err(|()| format!("{trimmed}: not a valid path"));
    }
    parse_address(input)
}

/// True if the input starts with `scheme:` (and is not `host:port`).
fn has_scheme(input: &str) -> bool {
    let Some((scheme, rest)) = input.split_once(':') else {
        return false;
    };
    let valid_scheme = scheme
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic())
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
    let looks_like_port = rest
        .split('/')
        .next()
        .is_some_and(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()));
    valid_scheme && !looks_like_port
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adds_https() {
        assert_eq!(
            parse_address("senko.net").unwrap().as_str(),
            "https://senko.net/"
        );
        assert_eq!(
            parse_address("localhost:8080/x").unwrap().as_str(),
            "https://localhost:8080/x"
        );
    }

    #[test]
    fn keeps_schemes() {
        assert_eq!(
            parse_address("http://a.b/").unwrap().as_str(),
            "http://a.b/"
        );
        assert_eq!(
            parse_address("about:blank").unwrap().as_str(),
            "about:blank"
        );
        assert!(parse_address("data:text/html,hi").is_ok());
    }

    #[test]
    fn rejects_empty() {
        assert!(parse_address("  ").is_err());
        assert!(parse_command_line_url("").is_err());
    }

    #[test]
    fn address_bar_ignores_relative_paths() {
        // `src` exists in the crate directory, where tests run.
        assert!(Path::new("src").exists());
        assert_eq!(parse_address("src").unwrap().as_str(), "https://src/");
        assert_eq!(
            parse_command_line_url("src").unwrap().scheme(),
            "file",
            "the command line opens existing files"
        );
        assert_eq!(
            parse_address("/tmp/a.html").unwrap().as_str(),
            "file:///tmp/a.html"
        );
    }
}
