//! The frontmatter of a test262 test (INTERPRETING.md of test262) and the
//! subset file that names the directories and features to run.

use std::collections::BTreeSet;

/// The fields of the `/*--- ... ---*/` block that the runner uses.
#[derive(Debug, Default, PartialEq)]
pub(crate) struct Meta {
    pub(crate) includes: Vec<String>,
    pub(crate) flags: Vec<String>,
    pub(crate) features: Vec<String>,
    pub(crate) negative: Option<Negative>,
}

/// `negative:` (phase and constructor name).
#[derive(Debug, PartialEq)]
pub(crate) struct Negative {
    pub(crate) phase: String,
    pub(crate) kind: String,
}

/// Reads the frontmatter of a test. A test without it has no fields.
pub(crate) fn parse_meta(source: &str) -> Meta {
    let Some(start) = source.find("/*---") else {
        return Meta::default();
    };
    let body = &source[start + 5..];
    let body = body.find("---*/").map_or(body, |end| &body[..end]);
    // Top-level keys with the lines that belong to them.
    let mut keys: Vec<(&str, &str, Vec<&str>)> = Vec::new();
    // A line ends at LF, CR (so CRLF gives an empty line, which is skipped),
    // U+2028 or U+2029: test262 has tests with CR line ends.
    for line in body.split(['\n', '\r', '\u{2028}', '\u{2029}']) {
        if line.trim().is_empty() {
            continue;
        }
        if line.starts_with([' ', '\t']) {
            if let Some(last) = keys.last_mut() {
                last.2.push(line.trim());
            }
        } else if let Some((key, value)) = line.split_once(':') {
            keys.push((key.trim(), value.trim(), Vec::new()));
        }
    }
    let mut meta = Meta::default();
    for (key, inline, block) in keys {
        match key {
            "includes" => meta.includes = list(inline, &block),
            "flags" => meta.flags = list(inline, &block),
            "features" => meta.features = list(inline, &block),
            "negative" => meta.negative = negative(&block),
            _ => {}
        }
    }
    meta
}

/// A YAML list: `[a, b]` inline or `- a` lines.
fn list(inline: &str, block: &[&str]) -> Vec<String> {
    let mut items = Vec::new();
    if let Some(inner) = inline.strip_prefix('[') {
        let inner = inner.trim_end().trim_end_matches(']');
        items.extend(inner.split(',').map(|item| item.trim().to_owned()));
    }
    for line in block {
        if let Some(item) = line.strip_prefix('-') {
            items.push(item.trim().to_owned());
        }
    }
    items.retain(|item| !item.is_empty());
    items
}

fn negative(block: &[&str]) -> Option<Negative> {
    let mut phase = None;
    let mut kind = None;
    for line in block {
        match line.split_once(':') {
            Some(("phase", value)) => phase = Some(value.trim().to_owned()),
            Some(("type", value)) => kind = Some(value.trim().to_owned()),
            _ => {}
        }
    }
    Some(Negative {
        phase: phase?,
        kind: kind?,
    })
}

/// The subset: the groups to run and the features in scope.
#[derive(Debug, Default)]
pub(crate) struct Subset {
    /// The `dir` lines: a group (`test/built-ins/Array`), or a directory
    /// or file below a group (`test/built-ins/TypedArray/prototype/set`),
    /// which selects only the tests at or below it.
    pub(crate) dirs: Vec<String>,
    pub(crate) features: BTreeSet<String>,
}

/// Whether a `dir` entry names a whole group (see [`super::run::group_of`])
/// rather than a part of one.
fn is_group(dir: &str) -> bool {
    super::run::group_of(&format!("{dir}/x.js")) == dir
}

impl Subset {
    /// Whether the subset selects the test at `path`: its group is a `dir`
    /// entry, or the test is at or below an entry that names a part of a
    /// group.
    pub(crate) fn selects(&self, path: &str) -> bool {
        let group = super::run::group_of(path);
        self.dirs.iter().any(|dir| {
            if is_group(dir) {
                *dir == group
            } else {
                path == dir
                    || path
                        .strip_prefix(dir.as_str())
                        .is_some_and(|rest| rest.starts_with('/'))
            }
        })
    }

    /// The groups that the subset runs (the scores file has one count per
    /// group, also for a group of which only a part runs).
    pub(crate) fn groups(&self) -> Vec<String> {
        let mut groups: Vec<String> = self
            .dirs
            .iter()
            .map(|dir| {
                if is_group(dir) {
                    dir.clone()
                } else {
                    super::run::group_of(&format!("{dir}/x.js"))
                }
            })
            .collect();
        groups.sort();
        groups.dedup();
        groups
    }

    /// Whether `group` is one of [`Subset::groups`].
    pub(crate) fn has_group(&self, group: &str) -> bool {
        self.groups().iter().any(|g| g == group)
    }
}

/// Parses the subset file: `dir PATH` and `feature NAME` lines; `#`
/// starts a comment.
pub(crate) fn parse_subset(text: &str) -> Subset {
    let mut subset = Subset::default();
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        match line.split_once(char::is_whitespace) {
            Some(("dir", path)) => subset.dirs.push(path.trim().to_owned()),
            Some(("feature", name)) => {
                subset.features.insert(name.trim().to_owned());
            }
            _ => {}
        }
    }
    subset
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frontmatter_inline_and_block_lists() {
        let meta = parse_meta(
            "// c\n/*---\ndescription: |\n  text\n  more: text\nincludes: [assert.js, sta.js]\nflags:\n  - onlyStrict\n  - raw\nfeatures: [a, b]\nnegative:\n  phase: parse\n  type: SyntaxError\n---*/\nx;",
        );
        assert_eq!(meta.includes, ["assert.js", "sta.js"]);
        assert_eq!(meta.flags, ["onlyStrict", "raw"]);
        assert_eq!(meta.features, ["a", "b"]);
        assert_eq!(
            meta.negative,
            Some(Negative {
                phase: "parse".into(),
                kind: "SyntaxError".into()
            })
        );
    }

    #[test]
    fn frontmatter_missing_or_empty() {
        assert_eq!(parse_meta("x = 1;"), Meta::default());
        let meta = parse_meta("/*---\nflags: []\n---*/");
        assert_eq!(meta, Meta::default());
    }

    /// Line ends are LF, CR, CRLF, U+2028 and U+2029.
    #[test]
    fn frontmatter_line_terminators() {
        let lf = "/*---\nincludes: [a.js]\nflags:\n  - raw\nnegative:\n  phase: parse\n  type: SyntaxError\n---*/";
        let expected = parse_meta(lf);
        assert_eq!(expected.includes, ["a.js"]);
        assert_eq!(expected.flags, ["raw"]);
        for end in ["\r", "\r\n", "\u{2028}", "\u{2029}"] {
            assert_eq!(parse_meta(&lf.replace('\n', end)), expected, "{end:?}");
        }
    }

    /// A `dir` line can name a group or a part of one; the part counts
    /// for its group.
    #[test]
    fn subset_parts_of_groups() {
        let subset = parse_subset(
            "dir test/built-ins/Map\ndir test/built-ins/T/prototype/set\n\
             dir test/built-ins/T/name.js\ndir test/language/a/b\n",
        );
        assert!(subset.selects("test/built-ins/Map/x.js"));
        assert!(subset.selects("test/built-ins/Map/prototype/get/x.js"));
        assert!(subset.selects("test/built-ins/T/prototype/set/x.js"));
        assert!(subset.selects("test/built-ins/T/prototype/set/deep/x.js"));
        assert!(subset.selects("test/built-ins/T/name.js"));
        assert!(!subset.selects("test/built-ins/T/prototype/setter/x.js"));
        assert!(!subset.selects("test/built-ins/T/prototype/at/x.js"));
        assert!(!subset.selects("test/built-ins/T/length.js"));
        assert!(subset.selects("test/language/a/b/x.js"));
        assert!(subset.selects("test/language/a/b/c/x.js"));
        assert!(!subset.selects("test/language/a/x.js"));
        assert_eq!(
            subset.groups(),
            [
                "test/built-ins/Map",
                "test/built-ins/T",
                "test/language/a/b"
            ]
        );
        assert!(subset.has_group("test/built-ins/T"));
        assert!(!subset.has_group("test/built-ins/T/prototype/set"));
    }

    #[test]
    fn subset_file() {
        let subset = parse_subset("# c\ndir test/a/b # x\nfeature f1\n\nfeature f2\n");
        assert_eq!(subset.dirs, ["test/a/b"]);
        assert_eq!(
            subset
                .features
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            ["f1", "f2"]
        );
    }
}
