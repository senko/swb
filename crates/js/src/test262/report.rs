//! The result table, the failure causes, the report file and the scores
//! file of a run.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use super::meta::Subset;
use super::run::{Status, combine, group_of};
use super::{Args, Done};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Counts {
    pass: u32,
    fail: u32,
    unsupported: u32,
    skipped: u32,
}

impl Counts {
    fn add(&mut self, status: &Status) {
        match status {
            Status::Pass => self.pass += 1,
            Status::Fail(_) => self.fail += 1,
            Status::Unsupported(_) => self.unsupported += 1,
            Status::Skipped(_) => self.skipped += 1,
        }
    }
}

/// Prints the table and the failure causes, writes the report and the
/// scores, and returns the exit code.
pub(super) fn summarise(
    args: &Args,
    subset: &Subset,
    roots: &[String],
    scores: &BTreeMap<String, u32>,
    done: &[Done],
) -> u8 {
    let mut groups: BTreeMap<String, Counts> = BTreeMap::new();
    let mut causes: BTreeMap<String, u32> = BTreeMap::new();
    let mut skips: BTreeMap<String, u32> = BTreeMap::new();
    let mut unsupported: BTreeMap<String, u32> = BTreeMap::new();
    let mut panics = 0;
    let mut report = String::new();
    for test in done {
        let status = combine(&test.modes);
        groups.entry(group_of(&test.path)).or_default().add(&status);
        match &status {
            Status::Fail(message) => {
                let cause = message.split_once("] ").map_or(&message[..], |(_, m)| m);
                if cause == "panic" {
                    panics += 1;
                }
                *causes.entry(cause.to_owned()).or_default() += 1;
            }
            Status::Unsupported(message) => *unsupported.entry(message.clone()).or_default() += 1,
            Status::Skipped(reason) => *skips.entry(reason.clone()).or_default() += 1,
            Status::Pass => {}
        }
        for (mode, status) in &test.modes {
            let (word, message) = match status {
                Status::Pass => ("pass", ""),
                Status::Fail(m) => ("fail", m.as_str()),
                Status::Unsupported(m) => ("unsupported", m.as_str()),
                Status::Skipped(m) => ("skipped", m.as_str()),
            };
            let _ = writeln!(report, "{}\t{mode}\t{word}\t{message}", test.path);
        }
    }
    if let Some(path) = &args.report
        && let Err(error) = std::fs::write(path, report)
    {
        eprintln!("swb-js test262: cannot write {path}: {error}");
    }
    let regressions = if args.update {
        Vec::new()
    } else {
        regressions(scores, roots, &groups)
    };
    for line in &regressions {
        eprintln!("REGRESSION {line}");
    }
    print_table(&groups, scores);
    print_top("Top failures", &causes, args.top);
    print_top("Unsupported constructs", &unsupported, 5);
    print_top("Skip reasons", &skips, 8);
    let total = groups.values().fold(Counts::default(), |mut t, c| {
        t.pass += c.pass;
        t.fail += c.fail;
        t.unsupported += c.unsupported;
        t.skipped += c.skipped;
        t
    });
    println!(
        "\ntotal: {} pass, {} fail, {} unsupported, {} skipped ({} tests)",
        total.pass,
        total.fail,
        total.unsupported,
        total.skipped,
        done.len()
    );
    if args.update {
        return write_scores(args, subset, roots, scores, &groups);
    }
    if panics > 0 {
        eprintln!("{panics} test(s) panicked");
    }
    u8::from(!regressions.is_empty() || panics > 0)
}

fn print_top(title: &str, map: &BTreeMap<String, u32>, n: usize) {
    if map.is_empty() || n == 0 {
        return;
    }
    let mut items: Vec<_> = map.iter().collect();
    items.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    println!("\n{title}:");
    for (message, count) in items.into_iter().take(n) {
        println!("{count:6}  {message}");
    }
}

/// The recorded pass counts. A missing file is empty (with a warning); a
/// file that cannot be read or parsed is an error.
pub(super) fn read_scores(path: &str) -> Result<BTreeMap<String, u32>, String> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            eprintln!("swb-js test262: warning: no scores file {path}; starting empty");
            return Ok(BTreeMap::new());
        }
        Err(error) => return Err(format!("cannot read the scores file {path}: {error}")),
    };
    serde_json::from_str(&text)
        .map_err(|error| format!("the scores file {path} is corrupt: {error}"))
}

/// Whether the run covers a group completely: a root is the group or a
/// directory above it. A root below the group (or a single file) covers
/// only a part of it.
fn covers(roots: &[String], group: &str) -> bool {
    roots.iter().any(|root| {
        group == root
            || group
                .strip_prefix(root.as_str())
                .is_some_and(|rest| rest.starts_with('/'))
    })
}

/// The recorded groups that went down, as `GROUP: NOW < RECORDED`. Only
/// groups that the run covers completely count.
fn regressions(
    scores: &BTreeMap<String, u32>,
    roots: &[String],
    groups: &BTreeMap<String, Counts>,
) -> Vec<String> {
    let mut found = Vec::new();
    for (group, &recorded) in scores {
        if !covers(roots, group) {
            continue;
        }
        let now = groups.get(group).map_or(0, |c| c.pass);
        if now < recorded {
            found.push(format!("{group}: {now} < {recorded}"));
        }
    }
    found
}

fn print_table(groups: &BTreeMap<String, Counts>, scores: &BTreeMap<String, u32>) {
    println!(
        "{:<58} {:>6} {:>6} {:>6} {:>6}  change",
        "group", "pass", "fail", "unsup", "skip"
    );
    for (group, c) in groups {
        let change = match scores.get(group) {
            Some(&old) if c.pass > old => format!("+{}", c.pass - old),
            Some(&old) if c.pass < old => format!("-{}", old - c.pass),
            Some(_) => String::new(),
            None => "new".to_owned(),
        };
        println!(
            "{group:<58} {:>6} {:>6} {:>6} {:>6}  {change}",
            c.pass, c.fail, c.unsupported, c.skipped
        );
    }
}

/// The scores file after an update: the recorded counts, with the counts of
/// the groups of the subset that the run covered. `Err` names the reason
/// that the run cannot update the file.
fn updated_scores(
    scores: &BTreeMap<String, u32>,
    subset: &Subset,
    roots: &[String],
    groups: &BTreeMap<String, Counts>,
) -> Result<BTreeMap<String, u32>, String> {
    if let Some(partial) = groups.keys().find(|group| !covers(roots, group)) {
        return Err(format!(
            "--update needs a run of whole groups; the run covers only a part of {partial} \
             (name the group directory, or run without PATH)"
        ));
    }
    let mut updated = scores.clone();
    for (group, c) in groups {
        if subset.dirs.contains(group) {
            updated.insert(group.clone(), c.pass);
        }
    }
    Ok(updated)
}

/// Writes the pass counts of the subset groups that ran into the scores
/// file (other groups keep their counts).
fn write_scores(
    args: &Args,
    subset: &Subset,
    roots: &[String],
    scores: &BTreeMap<String, u32>,
    groups: &BTreeMap<String, Counts>,
) -> u8 {
    let updated = match updated_scores(scores, subset, roots, groups) {
        Ok(updated) => updated,
        Err(message) => {
            eprintln!("swb-js test262: {message}");
            return 64;
        }
    };
    let mut text = serde_json::to_string_pretty(&updated).unwrap_or_default();
    text.push('\n');
    match std::fs::write(&args.scores, text) {
        Ok(()) => {
            println!("wrote {}", args.scores);
            0
        }
        Err(error) => {
            eprintln!("swb-js test262: cannot write {}: {error}", args.scores);
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    use super::super::meta::parse_subset;

    fn counts(pass: u32) -> Counts {
        Counts {
            pass,
            ..Counts::default()
        }
    }

    fn map<T: Clone>(items: &[(&str, T)]) -> BTreeMap<String, T> {
        items
            .iter()
            .map(|(k, v)| ((*k).to_owned(), v.clone()))
            .collect()
    }

    #[test]
    fn coverage_is_by_whole_groups() {
        let roots = |r: &[&str]| r.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        assert!(covers(&roots(&["test/language/a/b"]), "test/language/a/b"));
        assert!(covers(&roots(&["test/language"]), "test/language/a/b"));
        assert!(!covers(
            &roots(&["test/language/a/b/c.js"]),
            "test/language/a/b"
        ));
        assert!(!covers(
            &roots(&["test/language/a/b/c"]),
            "test/language/a/b"
        ));
        assert!(!covers(
            &roots(&["test/language/a/bc"]),
            "test/language/a/b"
        ));
    }

    #[test]
    fn narrow_runs_do_not_regress_a_group() {
        let scores = map(&[("test/language/a/b", 5), ("test/language/a/c", 2)]);
        let groups = map(&[("test/language/a/b", counts(1))]);
        // One file of a group: not a regression.
        let narrow = ["test/language/a/b/x.js".to_owned()];
        let found = regressions(&scores, &narrow, &groups);
        assert!(found.is_empty(), "{found:?}");
        // The whole group: a regression; the other group is not covered.
        let whole = ["test/language/a/b".to_owned()];
        assert_eq!(
            regressions(&scores, &whole, &groups),
            ["test/language/a/b: 1 < 5"]
        );
    }

    #[test]
    fn update_writes_covered_subset_groups_only() {
        let subset = parse_subset("dir test/language/a/b\n");
        let scores = map(&[("test/language/a/b", 5), ("test/language/a/c", 2)]);
        let groups = map(&[
            ("test/language/a/b", counts(7)),
            ("test/language/a/d", counts(3)),
        ]);
        let all = ["test/language".to_owned()];
        let updated = updated_scores(&scores, &subset, &all, &groups).expect("whole groups");
        assert_eq!(
            updated,
            map(&[("test/language/a/b", 7), ("test/language/a/c", 2)])
        );
        // A partial run is refused.
        let part = ["test/language/a/b/x.js".to_owned()];
        let error = updated_scores(&scores, &subset, &part, &groups).expect_err("partial");
        assert!(
            error.contains("--update needs a run of whole groups"),
            "{error}"
        );
    }

    #[test]
    fn scores_file_errors() {
        let dir = std::env::temp_dir().join(format!("swb-js-scores-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = |name: &str| dir.join(name).to_str().expect("utf-8").to_owned();
        assert_eq!(read_scores(&path("missing.json")), Ok(BTreeMap::new()));
        std::fs::write(path("bad.json"), "{not json").expect("write");
        assert!(read_scores(&path("bad.json")).is_err());
        // A directory cannot be read as a file.
        assert!(read_scores(&dir.to_string_lossy()).is_err());
        std::fs::write(path("ok.json"), "{\"g\": 3}").expect("write");
        assert_eq!(read_scores(&path("ok.json")), Ok(map(&[("g", 3)])));
    }
}
