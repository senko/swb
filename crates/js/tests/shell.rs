//! Tests of the `swb-js` shell: output, exit codes, `$262`, the limits,
//! and the test262 runner on a tiny test262 directory of our own.

#![allow(clippy::unwrap_used)]

use std::path::PathBuf;
use std::process::{Command, Output};

fn shell(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_swb-js"))
        .args(args)
        .output()
        .unwrap()
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn code(output: &Output) -> i32 {
    output.status.code().unwrap()
}

/// A fresh temporary directory.
fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("swb-js-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn print_and_console_log_go_to_stdout() {
    let out = shell(&["-e", "print(1, 'a'); console.log({ x: [1, 2] }); 'ignored'"]);
    assert_eq!(code(&out), 0);
    assert_eq!(text(&out.stdout), "1 a\n{ x: [ 1, 2 ] }\n");
    assert_eq!(text(&out.stderr), "");
}

#[test]
fn files_run_in_one_global_scope() {
    let dir = temp_dir("files");
    std::fs::write(dir.join("a.js"), "var x = 40;").unwrap();
    std::fs::write(dir.join("b.js"), "print(x + 2);").unwrap();
    let out = shell(&[
        dir.join("a.js").to_str().unwrap(),
        dir.join("b.js").to_str().unwrap(),
    ]);
    assert_eq!(code(&out), 0);
    assert_eq!(text(&out.stdout), "42\n");
}

#[test]
fn uncaught_error_exits_1_with_the_position() {
    let dir = temp_dir("uncaught");
    let file = dir.join("e.js");
    std::fs::write(&file, "print('before');\n\nnull.x;\n").unwrap();
    let out = shell(&[file.to_str().unwrap()]);
    assert_eq!(code(&out), 1);
    assert_eq!(text(&out.stdout), "before\n");
    let stderr = text(&out.stderr);
    assert!(
        stderr.starts_with("Uncaught TypeError: Cannot read properties of null (reading 'x')\n"),
        "{stderr}"
    );
    assert!(stderr.contains("e.js:3:"), "{stderr}");
}

#[test]
fn thrown_values_are_reported() {
    let out = shell(&["-e", "throw 'text'"]);
    assert_eq!(code(&out), 1);
    assert!(
        text(&out.stderr).starts_with("Uncaught "),
        "{:?}",
        out.stderr
    );
}

#[test]
fn compile_error_exits_2() {
    let out = shell(&["-e", "var 1;"]);
    assert_eq!(code(&out), 2);
    assert!(text(&out.stderr).starts_with("SyntaxError: "));
    assert!(text(&out.stderr).contains("<eval>:1:5"));
    // Nothing runs when the compile fails.
    assert_eq!(text(&out.stdout), "");
}

#[test]
fn time_limit_exits_3() {
    let out = shell(&["--time-limit", "100", "-e", "for (;;) {}"]);
    assert_eq!(code(&out), 3);
    assert!(text(&out.stderr).contains("time limit"), "{:?}", out.stderr);
}

#[test]
fn heap_limit_exits_3() {
    let out = shell(&[
        "--heap-limit",
        "16",
        "-e",
        "var a = []; for (var i = 0; ; i++) { a[i] = { i: i, s: 'x' + i }; }",
    ]);
    assert_eq!(code(&out), 3);
    assert!(text(&out.stderr).contains("heap limit"), "{:?}", out.stderr);
}

#[test]
fn stress_mode_gives_the_same_output() {
    let source = "var a = []; for (var i = 0; i < 200; i++) a.push({ i: i }); print(a.length)";
    let out = shell(&["--stress", "-e", source]);
    assert_eq!(code(&out), 0);
    assert_eq!(text(&out.stdout), "200\n");
}

#[test]
fn eval_script_runs_in_the_global_scope() {
    let source = "
        $262.evalScript('var made = 7; function f() { return made + 1; }');
        print(made, f(), $262.global === this);
        try { $262.evalScript('var 1'); } catch (e) { print(e.name); }
        try { $262.evalScript('throw new RangeError(\"r\")'); } catch (e) { print(e.name, e.message); }
        $262.gc();
    ";
    let out = shell(&["-e", source]);
    assert_eq!(code(&out), 0, "{}", text(&out.stderr));
    assert_eq!(text(&out.stdout), "7 8 true\nSyntaxError\nRangeError r\n");
}

#[test]
fn missing_host_functions_are_absent() {
    let out = shell(&[
        "-e",
        "print(typeof $262.createRealm, typeof $262.detachArrayBuffer, typeof $262.agent)",
    ]);
    assert_eq!(text(&out.stdout), "undefined undefined undefined\n");
}

#[test]
fn disassemble_and_compile_only() {
    let out = shell(&["--disassemble", "--compile-only", "-e", "print(1)"]);
    assert_eq!(code(&out), 0);
    let stdout = text(&out.stdout);
    assert!(stdout.starts_with("function \"\": Script"), "{stdout}");
    assert!(!stdout.contains("\n1\n"), "compile-only must not run");
    // Without --compile-only the script runs after the listing.
    let out = shell(&["--disassemble", "-e", "print('ran')"]);
    assert!(text(&out.stdout).ends_with("ran\n"));
    // A compile error with --compile-only.
    assert_eq!(code(&shell(&["--compile-only", "-e", "var 1"])), 2);
}

#[test]
fn bad_command_lines_exit_64() {
    assert_eq!(code(&shell(&[])), 64);
    assert_eq!(code(&shell(&["--nope"])), 64);
    assert_eq!(code(&shell(&["--time-limit", "x", "-e", "1"])), 64);
    let help = shell(&["--help"]);
    assert_eq!(code(&help), 0);
    let help = text(&help.stdout);
    assert!(help.contains("createRealm, detachArrayBuffer and agent are not available"));
}

/// A test262 directory with a small harness and a few tests of our own.
fn mini_test262(name: &str) -> PathBuf {
    let dir = temp_dir(name);
    let harness = dir.join("harness");
    std::fs::create_dir_all(&harness).unwrap();
    std::fs::write(
        harness.join("sta.js"),
        "function Test262Error(m) { this.message = m || ''; }
         Test262Error.prototype.toString = function () { return 'Test262Error: ' + this.message; };",
    )
    .unwrap();
    std::fs::write(
        harness.join("assert.js"),
        "function assert(c, m) { if (c !== true) throw new Test262Error(m); }
         assert.sameValue = function (a, b, m) { if (a !== b) throw new Test262Error(m); };",
    )
    .unwrap();
    let tests = dir.join("test/language/mini/group");
    std::fs::create_dir_all(&tests).unwrap();
    let write = |name: &str, body: &str| std::fs::write(tests.join(name), body).unwrap();
    write(
        "pass.js",
        "/*---\ndescription: d\n---*/\nassert.sameValue(1 + 1, 2);\n",
    );
    write(
        "fail.js",
        "/*---\ndescription: d\n---*/\nassert.sameValue(1, 2, 'one is not two');\n",
    );
    write(
        "neg.js",
        "/*---\nnegative:\n  phase: parse\n  type: SyntaxError\n---*/\nvar 1;\n",
    );
    write(
        "neg_runtime.js",
        "/*---\nnegative:\n  phase: runtime\n  type: Test262Error\n---*/\nassert(false);\n",
    );
    write("skip.js", "/*---\nflags: [async]\n---*/\nthrow 1;\n");
    write("unsupported.js", "/*---\n---*/\nclass C {}\n");
    write(
        "strict.js",
        "/*---\nflags: [onlyStrict]\n---*/\nassert(this === undefined || true);\n",
    );
    dir
}

#[test]
fn test262_runner_counts_and_ratchets() {
    let dir = mini_test262("t262-ratchet");
    let scores = dir.join("scores.json");
    let config = dir.join("subset.txt");
    std::fs::write(&config, "dir test/language/mini/group\n").unwrap();
    let run = |extra: &[&str]| {
        let mut args = vec![
            "test262",
            "--dir",
            dir.to_str().unwrap(),
            "--config",
            config.to_str().unwrap(),
            "--scores",
            scores.to_str().unwrap(),
        ];
        args.extend_from_slice(extra);
        shell(&args)
    };
    let out = run(&[]);
    let stdout = text(&out.stdout);
    // pass, neg, neg_runtime, strict pass; fail; unsupported; skipped.
    assert!(
        stdout.contains("test/language/mini/group")
            && stdout.contains("total: 4 pass, 1 fail, 1 unsupported, 1 skipped (7 tests)"),
        "{stdout}\n{}",
        text(&out.stderr)
    );
    assert!(stdout.contains("one is not two"), "{stdout}");
    assert_eq!(code(&out), 0, "no scores yet: nothing can regress");
    // --update writes the pass counts; an unchanged run is fine.
    assert_eq!(code(&run(&["--update"])), 0);
    let written = std::fs::read_to_string(&scores).unwrap();
    assert!(
        written.contains("\"test/language/mini/group\": 4"),
        "{written}"
    );
    assert_eq!(code(&run(&[])), 0);
    // A recorded count above the current one is a regression.
    std::fs::write(&scores, "{\"test/language/mini/group\": 5}").unwrap();
    let out = run(&[]);
    assert_eq!(code(&out), 1);
    assert!(text(&out.stderr).contains("REGRESSION test/language/mini/group: 4 < 5"));
    // A path argument runs only that test; a group is not judged by a part.
    let out = run(&["test/language/mini/group/pass.js"]);
    assert!(text(&out.stdout).contains("total: 1 pass, 0 fail, 0 unsupported, 0 skipped"));
    assert_eq!(code(&out), 0, "{}", text(&out.stderr));
    // --update refuses a partial run and leaves the file alone.
    let out = run(&["--update", "test/language/mini/group/pass.js"]);
    assert_eq!(code(&out), 64);
    assert!(text(&out.stderr).contains("--update needs a run of whole groups"));
    assert_eq!(
        std::fs::read_to_string(&scores).unwrap(),
        "{\"test/language/mini/group\": 5}"
    );
    // A run that finds no tests fails.
    let out = run(&["test/language/zzz"]);
    assert_eq!(code(&out), 1);
    assert!(text(&out.stderr).contains("no tests found"));
    // A corrupt scores file is an error, also for --update.
    std::fs::write(&scores, "{oops").unwrap();
    for extra in [&[][..], &["--update"][..]] {
        let out = run(extra);
        assert_eq!(code(&out), 1);
        assert!(
            text(&out.stderr).contains("corrupt"),
            "{}",
            text(&out.stderr)
        );
    }
    assert_eq!(std::fs::read_to_string(&scores).unwrap(), "{oops");
    // A missing scores file starts empty, with a warning.
    std::fs::remove_file(&scores).unwrap();
    let out = run(&[]);
    assert_eq!(code(&out), 0);
    assert!(text(&out.stderr).contains("no scores file"));
}

#[test]
fn test262_update_writes_only_subset_groups() {
    let dir = mini_test262("t262-update");
    let other = dir.join("test/language/mini/other");
    std::fs::create_dir_all(&other).unwrap();
    std::fs::write(other.join("p.js"), "/*---\n---*/\nassert(true);\n").unwrap();
    let scores = dir.join("scores.json");
    let config = dir.join("subset.txt");
    std::fs::write(&config, "dir test/language/mini/group\n").unwrap();
    let out = shell(&[
        "test262",
        "--dir",
        dir.to_str().unwrap(),
        "--config",
        config.to_str().unwrap(),
        "--scores",
        scores.to_str().unwrap(),
        "--all",
        "--update",
    ]);
    assert_eq!(code(&out), 0, "{}", text(&out.stderr));
    let written = std::fs::read_to_string(&scores).unwrap();
    assert!(written.contains("mini/group"), "{written}");
    assert!(!written.contains("mini/other"), "{written}");
}

#[test]
fn unreadable_test_file_is_a_failure() {
    let dir = mini_test262("t262-unreadable");
    // A dangling symbolic link is listed as a test but cannot be read.
    let bad = dir.join("test/language/mini/group/unreadable.js");
    std::os::unix::fs::symlink(dir.join("nowhere"), &bad).unwrap();
    let scores = dir.join("scores.json");
    let out = shell(&[
        "test262",
        "--dir",
        dir.to_str().unwrap(),
        "--scores",
        scores.to_str().unwrap(),
        "test/language/mini/group",
    ]);
    let stdout = text(&out.stdout);
    assert!(stdout.contains("cannot read"), "{stdout}");
    assert!(stdout.contains("4 pass, 2 fail"), "{stdout}");
}

#[test]
fn nested_eval_script_is_a_range_error() {
    let out = shell(&["-e", "function f() { $262.evalScript('f()'); } f()"]);
    assert_eq!(code(&out), 1, "{}", text(&out.stderr));
    assert!(
        text(&out.stderr).contains("RangeError"),
        "{}",
        text(&out.stderr)
    );
}

#[test]
fn dash_reads_standard_input() {
    use std::io::Write;
    use std::process::Stdio;
    let mut child = Command::new(env!("CARGO_BIN_EXE_swb-js"))
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"print(6 * 7)")
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert_eq!(code(&out), 0);
    assert_eq!(text(&out.stdout), "42\n");
}

#[test]
fn closed_stdout_ends_the_run() {
    use std::process::Stdio;
    for source in ["for (;;) print(1)", "for (;;) console.log(1)"] {
        let mut child = Command::new(env!("CARGO_BIN_EXE_swb-js"))
            .args(["-e", source])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        // Close the read end at once, like `| head -0`.
        drop(child.stdout.take());
        let out = child.wait_with_output().unwrap();
        assert_eq!(code(&out), 3, "{source}: {}", text(&out.stderr));
    }
}
