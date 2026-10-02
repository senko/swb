# swb: instructions for agents

swb is a web browser written from scratch in Rust. The owner does not read or
write code. Claude designs, implements and maintains everything.

Before you start work, read:

1. [docs/ground-rules.md](docs/ground-rules.md): the owner's requirements.
2. [docs/architecture.md](docs/architecture.md): crates and data flow.
3. [docs/roadmap.md](docs/roadmap.md) and the latest entry in
   [docs/devlog.md](docs/devlog.md): current state and next steps.
4. [docs/targets.md](docs/targets.md): the pages that must work.
5. The ADRs in [docs/adr/](docs/adr/) that relate to your task.

## Commands

- `just check`: format check, clippy (warnings are errors), all tests,
  cargo-deny. Must pass before every commit.
- `just fmt`, `just lint`, `just test`: the individual steps.
- `just run <url>`: build and run the browser.
- Use the rustup toolchain in `~/.cargo/bin`. `/usr/bin/cargo` is too old.
  The justfile sets `PATH`. In raw shell commands, run
  `export PATH="$HOME/.cargo/bin:$PATH"` first.

## Workflow

1. Plan the work. Update `docs/roadmap.md` if the plan changes.
2. Implement with tests.
3. Run `just check`.
4. Review: start a sub-agent with a clean context to review the staged diff
   (correctness, code quality, tests, docs). Fix what it finds.
5. Commit.

Commits:

- The owner approves each commit manually. Make coarse commits: one commit
  for a substantial body of work (what would otherwise be one branch).
- At every commit, all checks pass and the browser binary is usable.
- Experiments that can fail go on a separate branch.
- Run `git commit` in its own Bash call. Do not chain it with other commands.
- Commit message: a summary line, then a body that says what changed and why,
  notable decisions, and references to ADRs.

Documentation, in the same commit as the code:

- `docs/architecture.md` when the structure changes.
- A new ADR for each significant decision (see ADR 0001).
- An entry in `docs/devlog.md`.
- `docs/roadmap.md` and `docs/targets.md` when status changes.
- `docs/credits.md` when ideas come from another project's code or a document.

Maintenance: at the end of each milestone, review the whole codebase for
duplication, dead code, unclear names and outdated docs, and clean up.

## Code conventions

- Every crate has `[lints] workspace = true`. The workspace lints are in the
  root `Cargo.toml`.
- No `unsafe` code (the lint forbids it). If it is ever necessary, write an
  ADR first.
- Content from the network (HTML, CSS, images, fonts, headers, URLs) must
  never cause a panic. No `unwrap()` outside tests (the lint warns).
  `expect()` only for internal invariants, with a message that states the
  invariant. Index into slices only when the index is known to be valid.
- Errors: `thiserror` enums in library crates, `anyhow` in the binary.
- Logging: `log` macros. `warn!` for unsupported or malformed content that
  affects the result, `debug!`/`trace!` for details.
- Each module starts with a `//!` comment that states its purpose. When code
  implements a spec algorithm, give the spec section URL.
- Public items have doc comments. Use `pub` only for the crate API, otherwise
  `pub(crate)` (the `unreachable_pub` lint checks this).
- Prefer plain data structures and functions over deep trait hierarchies.
  Keep functions short.
- Unit tests go in the same file (`#[cfg(test)] mod tests`). Integration
  tests go in `crates/<name>/tests/`. Tests never access the network.
- Dependencies: declare versions only in the root `[workspace.dependencies]`;
  crates use `name.workspace = true`. Follow ADR 0003 (licenses and scope).
- When you implement a web feature, follow the spec. Comment on deliberate
  deviations.

## Writing style

Docs, comments and commit messages: plain, direct, Simplified Technical
English. Active voice. No filler, no marketing language.
