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
fn dump_ast_prints_the_tree_and_reports_a_syntax_error() {
    let dir = temp_dir("dump-ast");
    let file = dir.join("t.js");
    std::fs::write(&file, "var a = 1; function f(x) { return x + a; }").unwrap();
    let out = shell(&["--dump-ast", file.to_str().unwrap()]);
    assert_eq!(code(&out), 0);
    let stdout = text(&out.stdout);
    assert!(
        stdout.starts_with("(var (a 1)) (function f (x) (return (+ x a)))\n"),
        "{stdout}"
    );
    assert!(
        stdout.contains("scope 1 Function: x r0 Parameter"),
        "{stdout}"
    );
    let bad = dir.join("bad.js");
    std::fs::write(&bad, "var = ;").unwrap();
    let out = shell(&["--dump-ast", bad.to_str().unwrap()]);
    assert_eq!(code(&out), 2);
    assert!(
        text(&out.stderr).contains("bad.js:1:5"),
        "{}",
        text(&out.stderr)
    );
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
fn jobs_run_after_each_script() {
    let out = shell(&[
        "-e",
        "Promise.resolve().then(() => print('job')); print('script');",
    ]);
    assert_eq!(code(&out), 0, "{}", text(&out.stderr));
    assert_eq!(text(&out.stdout), "script\njob\n");
}

#[test]
fn unhandled_rejections_are_reported_and_exit_1() {
    let dir = temp_dir("rejection");
    let file = dir.join("r.js");
    std::fs::write(
        &file,
        "function f() { return Promise.reject(new TypeError('no')); }\nf();\n\
         Promise.reject(42);\nPromise.reject(1).catch(() => print('handled'));\n",
    )
    .unwrap();
    let out = shell(&[file.to_str().unwrap()]);
    assert_eq!(code(&out), 1);
    assert_eq!(text(&out.stdout), "handled\n");
    let path = file.to_str().unwrap();
    assert_eq!(
        text(&out.stderr),
        format!(
            "Uncaught (in promise) TypeError: no\n    at f ({path}:1:38)\n    at {path}:2:1\n\
             Uncaught (in promise) 42\n"
        )
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
    let out = shell(&["-e", "print(typeof $262.createRealm, typeof $262.agent)"]);
    assert_eq!(text(&out.stdout), "undefined undefined\n");
}

/// `$262.detachArrayBuffer` detaches (test262's INTERPRETING.md); another
/// value is a `TypeError`.
#[test]
fn detach_array_buffer() {
    let out = shell(&[
        "-e",
        "var b = new ArrayBuffer(8); var d = new DataView(b); $262.detachArrayBuffer(b); \
         $262.detachArrayBuffer(b); print(b.detached, b.byteLength, ArrayBuffer.isView(d)); \
         try { d.getInt8(0); } catch (e) { print(e.message); } \
         try { $262.detachArrayBuffer({}); } catch (e) { print(e.name); }",
    ]);
    assert_eq!(
        text(&out.stdout),
        "true 0 true\nCannot perform DataView.prototype.getInt8 on a detached or out-of-bounds ArrayBuffer\nTypeError\n"
    );
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

/// `--time-zone` sets the local zone of `Date` (a rule needs no database;
/// `UTC` is a name) and `--now` the clock.
#[test]
fn time_zone_and_clock_options() {
    let script = "print(new Date(2024, 6, 1, 12).toISOString(), new Date(2024, 0, 1, 12).toISOString(), \
                  Date.now(), new Date().toISOString())";
    let out = shell(&[
        "--time-zone",
        "EST5EDT,M3.2.0,M11.1.0",
        "--now",
        "1700000000123.7",
        "-e",
        script,
    ]);
    assert_eq!(code(&out), 0, "{}", text(&out.stderr));
    assert_eq!(
        text(&out.stdout),
        "2024-07-01T16:00:00.000Z 2024-01-01T17:00:00.000Z 1700000000123 2023-11-14T22:13:20.123Z\n"
    );
    let out = shell(&[
        "--time-zone",
        "UTC",
        "-e",
        "print(new Date(2024, 6, 1, 12).toISOString())",
    ]);
    assert_eq!(text(&out.stdout), "2024-07-01T12:00:00.000Z\n");
}

#[test]
fn bad_command_lines_exit_64() {
    assert_eq!(code(&shell(&[])), 64);
    assert_eq!(code(&shell(&["--nope"])), 64);
    assert_eq!(code(&shell(&["--time-limit", "x", "-e", "1"])), 64);
    assert_eq!(code(&shell(&["--now", "x", "-e", "1"])), 64);
    assert_eq!(code(&shell(&["--now"])), 64);
    assert_eq!(code(&shell(&["--time-zone"])), 64);
    let bad_zone = shell(&["--time-zone", "No/Such_Zone", "-e", "1"]);
    assert_eq!(code(&bad_zone), 64);
    assert!(text(&bad_zone.stderr).contains("unknown time zone \"No/Such_Zone\""));
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
    std::fs::write(
        harness.join("doneprintHandle.js"),
        "function $DONE(e) { print(e ? 'Test262:AsyncTestFailure:' + e : 'Test262:AsyncTestComplete'); }",
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
    write(
        "skip.js",
        "/*---\nflags: [CanBlockIsTrue]\n---*/\nthrow 1;\n",
    );
    // Module tests (ADR 0034): the imports are files of the directory.
    write("dep_FIXTURE.js", "export const v = 1;\n");
    write(
        "module.js",
        "/*---\nflags: [module]\n---*/\nimport { v } from './dep_FIXTURE.js';\nassert.sameValue(v, 1);\nassert.sameValue(this, undefined);\n",
    );
    write(
        "neg_resolution.js",
        "/*---\nnegative:\n  phase: resolution\n  type: SyntaxError\nflags: [module]\n---*/\nimport { nope } from './dep_FIXTURE.js';\n",
    );
    write(
        "async.js",
        "/*---\nflags: [async]\n---*/\nPromise.resolve(2).then(v => assert.sameValue(v, 2)).then($DONE, $DONE);\n",
    );
    // The engine compiles every construct since ADR 0034 3a-3; its thrown
    // refusals still count apart.
    write(
        "unsupported.js",
        "/*---\n---*/\nthrow new SyntaxError('not supported yet (construct)');\n",
    );
    // `import()` in a script test loads files relative to the test.
    write(
        "dynamic.js",
        "/*---\nflags: [async]\n---*/\nimport('./dep_FIXTURE.js').then(ns => assert.sameValue(ns.v, 1)).then($DONE, $DONE);\n",
    );
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
    // pass, neg, neg_runtime, strict, async, dynamic, module,
    // neg_resolution pass; fail; unsupported; skipped.
    assert!(
        stdout.contains("test/language/mini/group")
            && stdout.contains("total: 8 pass, 1 fail, 1 unsupported, 1 skipped (11 tests)"),
        "{stdout}\n{}",
        text(&out.stderr)
    );
    assert!(stdout.contains("one is not two"), "{stdout}");
    assert_eq!(code(&out), 0, "no scores yet: nothing can regress");
    // --update writes the pass counts; an unchanged run is fine.
    assert_eq!(code(&run(&["--update"])), 0);
    let written = std::fs::read_to_string(&scores).unwrap();
    assert!(
        written.contains("\"test/language/mini/group\": 8"),
        "{written}"
    );
    assert_eq!(code(&run(&[])), 0);
    // A recorded count above the current one is a regression.
    std::fs::write(&scores, "{\"test/language/mini/group\": 9}").unwrap();
    let out = run(&[]);
    assert_eq!(code(&out), 1);
    assert!(text(&out.stderr).contains("REGRESSION test/language/mini/group: 8 < 9"));
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
        "{\"test/language/mini/group\": 9}"
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
    assert!(stdout.contains("8 pass, 2 fail"), "{stdout}");
}

#[test]
fn a_missing_file_outside_the_root_gives_the_root_error() {
    // A missing file outside the root gets the same error as an existing
    // one: a script learns nothing about files outside its root.
    let dir = temp_dir("module-missing");
    let outside = temp_dir("module-missing-outside");
    std::fs::write(outside.join("secret.js"), "").unwrap();
    let outside_name = outside.file_name().unwrap().to_str().unwrap().to_owned();
    let write = |name: &str, text: &str| std::fs::write(dir.join(name), text).unwrap();
    write(
        "escape-existing.js",
        &format!("import '../{outside_name}/secret.js';\n"),
    );
    write(
        "escape-missing.js",
        &format!("import '../{outside_name}/nope.js';\n"),
    );
    write(
        "escape-missing-dir.js",
        "import '../no-such-dir-at-all/nope.js';\n",
    );
    let path = |name: &str| dir.join(name).to_str().unwrap().to_owned();
    for name in [
        "escape-existing.js",
        "escape-missing.js",
        "escape-missing-dir.js",
    ] {
        let out = shell(&["--module", &path(name)]);
        assert_eq!(code(&out), 1, "{name}");
        let stderr = text(&out.stderr);
        assert!(
            stderr.contains("Cannot load module") && stderr.contains("outside"),
            "{name}: {stderr}"
        );
        assert!(!stderr.contains("Cannot find module"), "{name}: {stderr}");
    }
}

#[test]
fn module_files_run_with_their_imports() {
    // ADR 0034 section 7: `--module FILE`; the imports are files relative
    // to the importing module, inside FILE's directory.
    let dir = temp_dir("module");
    let sub = dir.join("sub");
    std::fs::create_dir_all(&sub).unwrap();
    let write = |name: &str, text: &str| std::fs::write(dir.join(name), text).unwrap();
    write(
        "main.js",
        "import { a } from './sub/a.js';\nimport * as ns from './sub/a.js';\nprint(a, Object.keys(ns), this);\n",
    );
    write(
        "sub/a.js",
        "import { b } from '../b.js';\nexport const a = 'A' + b;\n",
    );
    write("b.js", "export const b = 'B';\n");
    // A real file outside the root, reached by a `..` specifier, an
    // absolute specifier and a symbolic link inside the root.
    let outside = temp_dir("module-outside");
    let secret = outside.join("secret.js");
    std::fs::write(&secret, "print('SECRET-TEXT-OF-THE-OUTSIDE-FILE');\n").unwrap();
    let outside_name = outside.file_name().unwrap().to_str().unwrap().to_owned();
    write(
        "escape-dotdot.js",
        &format!("import '../{outside_name}/secret.js';\n"),
    );
    write(
        "escape-absolute.js",
        &format!("import '{}';\n", secret.display()),
    );
    std::os::unix::fs::symlink(&secret, dir.join("linked.js")).unwrap();
    write("escape-symlink.js", "import './linked.js';\n");
    write("link.js", "\nimport { nope } from './b.js';\n");
    write(
        "type-js.js",
        "import './b.js' with { type: 'javascript' };\n",
    );
    write("type-json.js", "import './b.js' with { type: 'json' };\n");
    write("bad-dep.js", "import './bad.js';\n");
    write("bad.js", "let x = ;\n");
    write(
        "throws.js",
        "import './b.js';\nthrow new TypeError('boom');\n",
    );
    let path = |name: &str| dir.join(name).to_str().unwrap().to_owned();
    let out = shell(&["--module", &path("main.js")]);
    assert_eq!(code(&out), 0, "{}", text(&out.stderr));
    assert_eq!(text(&out.stdout), "AB a undefined\n");
    // A request out of the directory is a load error.
    for name in [
        "escape-dotdot.js",
        "escape-absolute.js",
        "escape-symlink.js",
    ] {
        let out = shell(&["--module", &path(name)]);
        assert_eq!(code(&out), 1, "{name}");
        let stderr = text(&out.stderr);
        assert!(
            stderr.contains("Uncaught TypeError: Cannot load module") && stderr.contains("outside"),
            "{name}: {stderr}"
        );
        assert!(
            !text(&out.stdout).contains("SECRET-TEXT") && !stderr.contains("SECRET-TEXT"),
            "{name}: the outside file ran"
        );
    }
    // Chromium's messages for the attribute `type`; "json" and "css" are
    // known types that are not supported yet.
    let out = shell(&["--module", &path("type-js.js")]);
    assert_eq!(code(&out), 1);
    assert!(
        text(&out.stderr)
            .contains("Uncaught TypeError: \"javascript\" is not a valid module type."),
        "{}",
        text(&out.stderr)
    );
    let out = shell(&["--module", &path("type-json.js")]);
    assert_eq!(code(&out), 1);
    assert!(
        text(&out.stderr).contains("Uncaught TypeError: Unsupported module type 'json'"),
        "{}",
        text(&out.stderr)
    );
    // A link error names the importing module's line and column.
    let out = shell(&["--module", &path("link.js")]);
    assert_eq!(code(&out), 1);
    let stderr = text(&out.stderr);
    assert!(
        stderr.contains("Uncaught SyntaxError: The requested module './b.js' does not provide an export named 'nope'")
            && stderr.contains("link.js:2:10"),
        "{stderr}"
    );
    // A dependency that does not parse: its error and position, exit 2.
    let out = shell(&["--module", &path("bad-dep.js")]);
    assert_eq!(code(&out), 2);
    let stderr = text(&out.stderr);
    assert!(
        stderr.contains("SyntaxError: Unexpected token ';'") && stderr.contains("bad.js:1:9"),
        "{stderr}"
    );
    let out = shell(&["--module", &path("throws.js")]);
    assert_eq!(code(&out), 1);
    let stderr = text(&out.stderr);
    assert!(
        stderr.contains("Uncaught TypeError: boom") && stderr.contains("throws.js:2:7"),
        "{stderr}"
    );
    // Scripts and modules run in one global scope.
    let out = shell(&["-e", "globalThis.g = 'G'", "--module", &path("main.js")]);
    assert_eq!(code(&out), 0, "{}", text(&out.stderr));
}

#[test]
fn module_files_with_top_level_await() {
    // ADR 0034 section 3 (3a-2): the shell runs the jobs after Evaluate and
    // reports a rejection of the module's promise as an uncaught error.
    let dir = temp_dir("module-tla");
    let write = |name: &str, text: &str| std::fs::write(dir.join(name), text).unwrap();
    write(
        "main.js",
        "import './a.js'; print('main');\nPromise.resolve().then(() => print('job'));\n",
    );
    write("a.js", "print('a1');\nawait null;\nprint('a2');\n");
    write(
        "rejects.js",
        "import './a.js';\nprint('r');\nawait 0;\nthrow new RangeError('late');\n",
    );
    write(
        "never.js",
        "await new Promise(() => {});\nprint('never');\n",
    );
    let path = |name: &str| dir.join(name).to_str().unwrap().to_owned();
    let out = shell(&["--module", &path("main.js")]);
    assert_eq!(code(&out), 0, "{}", text(&out.stderr));
    assert_eq!(text(&out.stdout), "a1\na2\nmain\njob\n");
    let out = shell(&["--module", &path("rejects.js")]);
    assert_eq!(code(&out), 1);
    assert_eq!(text(&out.stdout), "a1\na2\nr\n");
    let stderr = text(&out.stderr);
    assert!(
        stderr.contains("Uncaught RangeError: late") && stderr.contains("rejects.js:4:7"),
        "{stderr}"
    );
    // A module that never settles ends without a report.
    let out = shell(&["--module", &path("never.js")]);
    assert_eq!(code(&out), 0, "{}", text(&out.stderr));
    assert_eq!(text(&out.stdout), "");
}

#[test]
fn dynamic_imports_from_scripts_and_modules() {
    // ADR 0034 section 4 (3a-3): `import()` loads files relative to the
    // importing script or module, inside its root; `import.meta.url` is
    // the module's file URL.
    let dir = temp_dir("dynamic");
    let sub = dir.join("sub");
    std::fs::create_dir_all(&sub).unwrap();
    let write = |name: &str, text: &str| std::fs::write(dir.join(name), text).unwrap();
    write(
        "sub/a.js",
        "export const a = 'A';\nprint('meta', import.meta.url === 'file://' + globalThis.base + '/sub/a.js');\n",
    );
    write(
        "sub/s.js",
        "import('./a.js').then(ns => print('s', ns.a));\n\
         import('../m.js').catch(e => print(e.name, e.message.includes('outside')));\n\
         import('./a.js', { with: { type: 'javascript' } }).catch(e => print(e.message));\n",
    );
    write(
        "m.js",
        "const { a } = await import('./sub/a.js');\nprint('m', a);\n",
    );
    write("missing.js", "import('./nope.js');\n");
    let base = dir.canonicalize().unwrap();
    let define = format!("globalThis.base = {:?}", base.to_str().unwrap());
    let path = |name: &str| dir.join(name).to_str().unwrap().to_owned();
    let out = shell(&["-e", &define, &path("sub/s.js")]);
    assert_eq!(code(&out), 0, "{}", text(&out.stderr));
    // The module runs in the job of its load, before the reactions of the
    // rejections.
    assert_eq!(
        text(&out.stdout),
        "meta true\nTypeError true\n\"javascript\" is not a valid module type.\ns A\n"
    );
    let out = shell(&["-e", &define, "--module", &path("m.js")]);
    assert_eq!(code(&out), 0, "{}", text(&out.stderr));
    assert_eq!(text(&out.stdout), "meta true\nm A\n");
    // An import that nobody handles is an unhandled rejection.
    let out = shell(&[&path("missing.js")]);
    assert_eq!(code(&out), 1);
    assert!(
        text(&out.stderr)
            .contains("Uncaught (in promise) TypeError: Cannot find module './nope.js'"),
        "{}",
        text(&out.stderr)
    );
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
