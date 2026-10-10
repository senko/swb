//! `swb-js`: the shell of the JavaScript engine (ADR 0026 section 13).
//! It runs script files and `-e CODE`, prints the parse tree with
//! `--dump-ast FILE`, and has two subcommands for the test tools:
//! `test262` (the test262 runner) and `bench-compile` (parse and compile
//! time and memory for a directory of scripts).

mod bench;
mod shell;
mod test262;

use std::cell::RefCell;
use std::io::{BufWriter, Read};
use std::process::ExitCode;
use std::rc::Rc;
use std::time::Duration;

use swb_js_syntax::{LineIndex, dump};
use swb_js_text::{RecursionBudget, String16};

use shell::{Options, STACK_SIZE};

const HELP: &str = "\
swb-js: the shell of swb's JavaScript engine

Usage:
  swb-js [OPTIONS] FILE...        run script files in one global scope
  swb-js [OPTIONS] -e CODE        run CODE
  swb-js [OPTIONS] -              read the script from standard input
  swb-js test262 [--help]         the test262 runner (just test262)
  swb-js bench-compile DIR        parse and compile the scripts of DIR
  swb-js --dump-ast FILE          print the syntax tree, scopes and references

Options:
  -e CODE               run CODE (before the files, if both are given)
  --heap-limit MIB      heap limit in MiB (default 1024)
  --stress              collect garbage at every safepoint
  --time-limit MS       end a run after MS milliseconds
  --disassemble         print the bytecode of each script before it runs
  --compile-only        compile only (with --disassemble: list, do not run)
  -h, --help            this text

print(...) and console.log(...) write to stdout. $262 has evalScript(src),
gc() and global; createRealm, detachArrayBuffer and agent are not available.

Exit codes: 0 success; 1 uncaught error (Uncaught Name: message and the
position on stderr); 2 compile error; 3 termination (time or heap limit);
64 bad command line.
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // Scripts run on a thread with a fixed stack size (ADR 0026 section 9).
    let handle = std::thread::Builder::new()
        .stack_size(STACK_SIZE)
        .spawn(move || dispatch(&args));
    match handle.map(std::thread::JoinHandle::join) {
        Ok(Ok(code)) => ExitCode::from(code),
        Ok(Err(_)) => {
            eprintln!("swb-js: internal panic");
            ExitCode::from(70)
        }
        Err(error) => {
            eprintln!("swb-js: cannot start the thread: {error}");
            ExitCode::from(70)
        }
    }
}

fn dispatch(args: &[String]) -> u8 {
    match args.first().map(String::as_str) {
        Some("test262") => test262::main(&args[1..]),
        Some("bench-compile") => bench::main(&args[1..]),
        Some("--dump-ast") => dump_ast_main(&args[1..]),
        _ => shell_main(args),
    }
}

/// `--dump-ast FILE`: parses the file and prints the dumps of
/// `swb_js_syntax::dump` (S-expressions, scopes, references).
fn dump_ast_main(args: &[String]) -> u8 {
    let [file] = args else {
        return usage_error("--dump-ast needs one file");
    };
    let bytes = match std::fs::read(file) {
        Ok(bytes) => bytes,
        Err(error) => {
            eprintln!("swb-js: cannot read {file}: {error}");
            return 66;
        }
    };
    let source = String16::from(String::from_utf8_lossy(&bytes).as_ref());
    let mut budget = RecursionBudget::default();
    match swb_js_syntax::parse_script(source.as_str16(), &mut budget) {
        Ok(script) => {
            println!("{}", dump::dump_ast(&script));
            println!("{}", dump::dump_scopes(&script));
            println!("{}", dump::dump_references(&script));
            0
        }
        Err(error) => {
            let location = LineIndex::new(source.as_str16()).location(error.offset);
            eprintln!("{file}:{}:{}: {error}", location.line, location.column);
            2
        }
    }
}

/// The parsed command line of the shell.
#[derive(Default)]
struct Command {
    options: Options,
    code: Option<String>,
    files: Vec<String>,
    disassemble: bool,
    compile_only: bool,
}

fn usage_error(message: &str) -> u8 {
    eprintln!("swb-js: {message}\n\n{HELP}");
    64
}

fn number(value: Option<&String>, option: &str) -> Result<u64, String> {
    value
        .and_then(|v| v.parse().ok())
        .ok_or_else(|| format!("{option} needs a number"))
}

fn parse_command(args: &[String]) -> Result<Option<Command>, String> {
    let mut command = Command::default();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "-h" | "--help" => return Ok(None),
            "-e" => {
                command.code = Some(iter.next().ok_or("-e needs code")?.clone());
            }
            "--heap-limit" => {
                let mib = number(iter.next(), arg)?;
                command.options.heap_limit = Some((mib as usize) << 20);
            }
            "--time-limit" => {
                command.options.time_limit = Some(Duration::from_millis(number(iter.next(), arg)?));
            }
            "--stress" => command.options.stress = true,
            "--disassemble" => command.disassemble = true,
            "--compile-only" => command.compile_only = true,
            other if other.starts_with('-') && other != "-" => {
                return Err(format!("unknown option {other}"));
            }
            file => command.files.push(file.to_owned()),
        }
    }
    Ok(Some(command))
}

fn shell_main(args: &[String]) -> u8 {
    let command = match parse_command(args) {
        Ok(Some(command)) => command,
        Ok(None) => {
            print!("{HELP}");
            return 0;
        }
        Err(message) => return usage_error(&message),
    };
    if command.code.is_none() && command.files.is_empty() {
        return usage_error("no file and no -e");
    }
    let out: shell::Output = Rc::new(RefCell::new(BufWriter::new(std::io::stdout())));
    let code = run_command(&command, &out);
    let _ = out.borrow_mut().flush();
    code
}

/// Runs `-e` code and the files in order; stops at the first error.
fn run_command(command: &Command, out: &shell::Output) -> u8 {
    let mut rt = match shell::new_runtime(&command.options, Rc::clone(out)) {
        Ok(rt) => rt,
        Err(error) => {
            eprintln!("swb-js: cannot create the runtime: {error}");
            return 70;
        }
    };
    let mut scripts: Vec<(String, String)> = Vec::new();
    if let Some(code) = &command.code {
        scripts.push(("<eval>".to_owned(), code.clone()));
    }
    for file in &command.files {
        let read = if file == "-" {
            // `-` is the standard input.
            let mut bytes = Vec::new();
            std::io::stdin().read_to_end(&mut bytes).map(|_| bytes)
        } else {
            std::fs::read(file)
        };
        match read {
            Ok(bytes) => {
                let name = if file == "-" { "<stdin>" } else { file };
                scripts.push((
                    name.to_owned(),
                    String::from_utf8_lossy(&bytes).into_owned(),
                ));
            }
            Err(error) => {
                eprintln!("swb-js: cannot read {file}: {error}");
                return 66;
            }
        }
    }
    for (name, source) in &scripts {
        let result = run_script(&mut rt, command, name, source, out);
        if let Err(code) = result {
            return code;
        }
    }
    0
}

fn run_script(
    rt: &mut swb_js::Runtime,
    command: &Command,
    name: &str,
    source: &str,
    out: &shell::Output,
) -> Result<(), u8> {
    let fail = |error: &swb_js::ScriptError| {
        let _ = out.borrow_mut().flush();
        eprintln!("{}", shell::describe_error(error, name, source));
        shell::exit_code(error)
    };
    if command.disassemble || command.compile_only {
        match rt.disassemble(source) {
            Ok(listing) if command.disassemble => {
                let _ = write!(out.borrow_mut(), "{listing}");
            }
            Ok(_) => {}
            Err(error) => return Err(fail(&error)),
        }
    }
    if command.compile_only {
        return Ok(());
    }
    match shell::run(rt, &command.options, source, name) {
        Ok(_) => Ok(()),
        Err(error) => Err(fail(&error)),
    }
}
