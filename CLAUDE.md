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

Work on one feature at a time:

1. Plan the milestone as an ordered list of features in `docs/roadmap.md`.
   For each feature, state its scope: the cases that the target pages need.
2. Implement the feature on top of `main`, with tests. Run `just check`.
3. Review: one sub-agent with a clean context (type `reviewer`) reviews the
   final diff with the checklist below. Fix what it finds. Review the fixes
   again only if they are large or concern security or performance, and
   then only the fix delta.
4. Commit. Then start the next feature.

Do not run implementation work in parallel: parallel work on the central
files (`block.rs`, `box_tree.rs`, `inline/`, `display_list.rs`,
`raster.rs`, the style property tables) costs more in rebases and merges
than it saves. Parallel sub-agents are allowed only for read-only work
(measurements, research) or for changes in crates that the plan names as
separate.

Findings outside the feature's scope go to the backlog in
`docs/roadmap.md`. Do not fix them in the same task.

### Agents

The main agent (the orchestrator) runs on Opus. It plans, writes small
changes, docs, merges and commits itself, and starts sub-agents only for
work that needs a clean context or much reading.

Sub-agent types (`.claude/agents/`). Always pass a type:

| Type               | Model, effort  | Use |
|--------------------|----------------|-----|
| `implementer-hard` | Opus, high     | Algorithmically hard features; the plan names them. |
| `implementer`      | Sonnet, medium | Other features, fix rounds. |
| `reviewer`         | Sonnet, medium | The review of each feature. |
| `measurer`         | Haiku, low     | Chromium probes, snapshots, `compare`, performance runs. |

Rules for sub-agents:

- A task must be small enough to finish in one session. If a sub-agent
  stops (for example at a usage limit) and its transcript is long, a new
  sub-agent continues from the patch and the notes file.
- Report in at most 30 lines: what changed, scores, open issues. Details go
  to a notes file in the scratchpad, which the orchestrator reads only when
  needed.
- Use the shared tools (docs/testing.md): the Chromium probe tool (add
  cases; do not write new probe scripts), the hostile-page set, and the
  persistent review worktree with its own target directory. Never build a
  tree with another tree's target directory.
- Do not read the source code of other browsers or rendering engines
  (Chromium, Blink, WebKit, Gecko, Servo, Skia). Derive behaviour from the
  specifications and from black-box measurements in Chromium (ADR 0021).

Review checklist:

- Correctness on the cases of the feature's scope, compared with Chromium.
- Robustness: the hostile-page set passes; no new panics.
- Provenance: no code or data copied from other projects.
- Code quality, tests, docs.

No open-ended fuzzing, no timing hunts beyond the hostile-page set.

### Commits and docs

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
- `docs/credits.md` when ideas come from a document or another project.

Maintenance: at the end of each milestone, review the whole codebase for
duplication, dead code, unclear names and outdated docs, and clean up. Use
at most two `reviewer` sub-agents (read-only, each with a part of the
crates), make the changes yourself or with one `implementer`, and show
with `just snapshot` that behaviour is unchanged.

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
