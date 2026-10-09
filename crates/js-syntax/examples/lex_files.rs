//! Lexes JavaScript files and reports the token count, the lexing speed
//! and the errors. A measurement tool for the lexer, not part of
//! `just check`.
//!
//! Usage: `cargo run --release -p swb-js-syntax --example lex_files --
//! [--runs N] FILE_OR_DIRECTORY...` (a directory means its `*.js` files).
//!
//! This driver lexes without the parser on purpose (it measures the lexer
//! alone), so it chooses the goal symbol with a previous-token heuristic: `/` starts a regular expression unless the
//! previous token ends an expression (a name, a literal, `)`, `]`, `}`,
//! `this`, `super`, `++`, `--`), and `}` continues a template when it
//! closes a substitution. The heuristic is wrong in known cases (for
//! example `if (x) /re/.test(s)`); its errors are reported, not the
//! lexer's.

use std::process::ExitCode;
use std::time::{Duration, Instant};

#[path = "common/mod.rs"]
mod common;

use swb_js_syntax::{Goal, Interner, Lexer, LexerOptions, SyntaxError, Token, TokenKind};
use swb_js_text::{CodeUnit, Str16, String16};

/// The goal heuristic of this driver.
#[derive(Default)]
struct Heuristic {
    previous: Option<TokenKind>,
    /// For each open template substitution, the depth of `{` inside it.
    substitutions: Vec<u32>,
}

impl Heuristic {
    fn goal(&self) -> Goal {
        let regexp = self.previous.is_none_or(regexp_allowed_after);
        let template_tail = self.substitutions.last() == Some(&0);
        match (regexp, template_tail, self.previous.is_none()) {
            (_, _, true) => Goal::HashbangOrRegExp,
            (true, true, _) => Goal::RegExpOrTemplateTail,
            (false, true, _) => Goal::TemplateTail,
            (true, false, _) => Goal::RegExp,
            (false, false, _) => Goal::Div,
        }
    }

    fn update(&mut self, token: &Token) {
        match token.kind {
            TokenKind::TemplateHead => self.substitutions.push(0),
            TokenKind::TemplateTail => {
                self.substitutions.pop();
            }
            TokenKind::LBrace => {
                if let Some(depth) = self.substitutions.last_mut() {
                    *depth += 1;
                }
            }
            TokenKind::RBrace => {
                if let Some(depth) = self.substitutions.last_mut() {
                    *depth = depth.saturating_sub(1);
                }
            }
            _ => {}
        }
        self.previous = Some(token.kind);
    }
}

/// Whether `/` after a token of this kind starts a regular expression.
fn regexp_allowed_after(kind: TokenKind) -> bool {
    !matches!(
        kind,
        TokenKind::Identifier
            | TokenKind::PrivateName
            | TokenKind::Number
            | TokenKind::BigInt
            | TokenKind::String
            | TokenKind::NoSubstitutionTemplate
            | TokenKind::TemplateTail
            | TokenKind::RegExp
            | TokenKind::RParen
            | TokenKind::RBracket
            | TokenKind::RBrace
            | TokenKind::This
            | TokenKind::Super
            | TokenKind::Null
            | TokenKind::True
            | TokenKind::False
            | TokenKind::PlusPlus
            | TokenKind::MinusMinus
    )
}

/// The result of lexing one file.
struct Outcome {
    tokens: usize,
    error: Option<SyntaxError>,
}

fn lex<U: CodeUnit>(units: &[U]) -> Outcome {
    let mut lexer = Lexer::new(units, LexerOptions::script(), Interner::new());
    let mut heuristic = Heuristic::default();
    let mut tokens = 0;
    loop {
        match lexer.next_token(heuristic.goal()) {
            Ok(token) if token.kind == TokenKind::Eof => {
                return Outcome {
                    tokens,
                    error: None,
                };
            }
            Ok(token) => {
                heuristic.update(&token);
                tokens += 1;
            }
            Err(error) => {
                return Outcome {
                    tokens,
                    error: Some(error),
                };
            }
        }
    }
}

fn lex_text(text: &String16) -> Outcome {
    match text.as_str16() {
        Str16::Latin1(units) => lex(units),
        Str16::Wide(units) => lex(units),
    }
}

/// The source around an offset, on one line.
fn context(text: &String16, offset: u32) -> String {
    let source = text.as_str16();
    let start = (offset as usize).saturating_sub(40);
    let end = (offset as usize + 40).min(source.len());
    source
        .slice(start, end)
        .map(|s| s.to_string_lossy().replace(['\n', '\r'], " "))
        .unwrap_or_default()
}

fn main() -> ExitCode {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let mut runs = 10;
    if args.first().is_some_and(|a| a == "--runs") {
        runs = args.get(1).and_then(|n| n.parse().ok()).unwrap_or(runs);
        args.drain(..2.min(args.len()));
    }
    let paths = match common::js_files(&args) {
        Ok(paths) if !paths.is_empty() => paths,
        Ok(_) => {
            eprintln!("usage: lex_files [--runs N] FILE_OR_DIRECTORY...");
            return ExitCode::FAILURE;
        }
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let mut sources = Vec::new();
    let mut bytes = 0;
    for path in &paths {
        match std::fs::read(path) {
            Ok(data) => {
                bytes += data.len();
                let text = String16::from(String::from_utf8_lossy(&data).as_ref());
                sources.push((path, text));
            }
            Err(error) => eprintln!("{}: {error}", path.display()),
        }
    }

    let mut complete = 0;
    let mut tokens = 0;
    for (path, text) in &sources {
        let outcome = lex_text(text);
        tokens += outcome.tokens;
        match outcome.error {
            None => complete += 1,
            Some(error) => println!(
                "{}: offset {}: {} (after {} tokens) near: {}",
                path.display(),
                error.offset,
                error.message,
                outcome.tokens,
                context(text, error.offset)
            ),
        }
    }

    let mut best = Duration::MAX;
    for _ in 0..runs {
        let start = Instant::now();
        let mut count = 0;
        for (_, text) in &sources {
            count += lex_text(text).tokens;
        }
        best = best.min(start.elapsed());
        std::hint::black_box(count);
    }
    let wide = sources.iter().filter(|(_, t)| !t.is_latin1()).count();
    let seconds = best.as_secs_f64();
    println!(
        "{} files ({wide} need two-byte units), {complete} lexed to the end",
        sources.len()
    );
    println!("{tokens} tokens, {bytes} bytes");
    println!(
        "best of {runs} runs: {:.1} ms, {:.0} MB/s, {:.1} million tokens/s",
        seconds * 1000.0,
        bytes as f64 / seconds / 1e6,
        tokens as f64 / seconds / 1e6
    );
    ExitCode::SUCCESS
}
