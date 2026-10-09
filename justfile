# Developer commands. Run `just --list` for an overview.

set shell := ["bash", "-euo", "pipefail", "-c"]

# Prefer the rustup toolchain over a system-wide cargo.
export PATH := env_var('HOME') + "/.cargo/bin:" + env_var('PATH')

default: check

# Run every check that must pass before a commit.
check: fmt-check lint test deny tools-check

# Format all code.
fmt:
    cargo fmt --all

# Fail if code is not formatted.
fmt-check:
    cargo fmt --all -- --check

# Run clippy on all crates and targets; warnings are errors.
lint:
    cargo clippy --workspace --all-targets --locked -- -D warnings

# Run all tests.
test:
    cargo test --workspace --locked

# Check dependency licenses, bans and sources (offline-safe subset).
deny:
    cargo deny --offline check licenses bans sources

# Check security advisories (needs network).
audit:
    cargo deny check advisories

# Build and run the browser (release mode). Example: just run https://senko.net/
run *ARGS:
    cargo run --release -p swb -- {{ARGS}}

# Build the release binary.
build:
    cargo build --release -p swb

# --- JavaScript engine tools (crates/js, tools/). See docs/testing.md. ---

# The test262 commit that `just test262` uses (the only place that names it).
test262_commit := "2e0a56762801e275a9fdf96dc49d90ba0cddcf63"
test262_url := env_var_or_default("TEST262_URL", "https://github.com/tc39/test262.git")

# Run the test262 subset with swb-js and compare the pass counts with crates/js/test262/scores.json (--update writes it). TEST262_DIR overrides out/test262.
test262 *ARGS:
    #!/usr/bin/env bash
    set -euo pipefail
    dir="${TEST262_DIR:-out/test262}"
    if [ ! -d "$dir/harness" ]; then
        echo "cloning test262 into $dir at {{test262_commit}}"
        mkdir -p "$(dirname "$dir")"
        GIT_CONFIG_GLOBAL=/dev/null git clone --quiet --no-checkout "{{test262_url}}" "$dir"
        GIT_CONFIG_GLOBAL=/dev/null git -C "$dir" checkout --quiet "{{test262_commit}}"
    fi
    if [ -e "$dir/.git" ] && [ "$(git -C "$dir" rev-parse HEAD 2>/dev/null)" != "{{test262_commit}}" ]; then
        echo "warning: $dir is not at {{test262_commit}}; the scores may not match" >&2
    fi
    cargo run --release -q -p swb-js -- test262 --dir "$dir" {{ARGS}}

# Run JavaScript files in swb-js and Node.js 22 and diff stdout and the uncaught error. Example: just jsdiff a.js b.js
jsdiff +FILES:
    cargo build --release -q -p swb-js
    uv run --project tools swbtools jsdiff {{FILES}}

# Benchmark the JavaScript engine against node --jitless; with DIR, also lex, parse and compile its scripts. Example: just jsbench out/bbc-js
jsbench *DIR:
    uv run --project tools swbtools jsbench {{DIR}}

# --- Test tools (tools/, Python with uv). See docs/testing.md. ---

# Run a test tool command. Example: just tools list
tools *ARGS:
    uv run --project tools swbtools {{ARGS}}

# Download a page into fixtures/pages/NAME. Example: just capture https://senko.net/ senko-net
capture URL NAME *ARGS:
    uv run --project tools swbtools capture {{URL}} {{NAME}} {{ARGS}}

# Add the responses that a fixture does not have (after swb learned to load more). Example: just capture-missing senko-net
capture-missing NAME *ARGS: build
    uv run --project tools swbtools capture-missing {{NAME}} {{ARGS}}

# Replace the images and non-free fonts of fixtures with placeholders and free fonts. Example: just substitute bbc
substitute +NAMES:
    uv run --project tools swbtools substitute {{NAMES}}

# Write Chromium's boxes and screenshot for fixtures (default: all).
reference *NAMES:
    uv run --project tools swbtools reference {{ if NAMES == "" { "--all" } else { NAMES } }}

# Build swb, run it on fixtures and compare with Chromium (default: all). Reports: out/compare/.
compare *NAMES: build
    uv run --project tools swbtools compare {{ if NAMES == "" { "--all" } else { NAMES } }}

# Compare all fixtures and write the scores to fixtures/scores.json.
update-scores: build
    uv run --project tools swbtools compare --all --update-scores

# Time swb's pipeline stages for fixtures (default: all); prints a Markdown table.
perf *NAMES: build
    uv run --project tools swbtools perf {{NAMES}}

# Run swb on the hostile-page set (default: all cases) and check time and memory limits. Pages and logs: out/hostile/.
hostile *NAMES: build
    uv run --project tools swbtools hostile {{NAMES}}

# Write tests/layout/*.boxes.json with Chromium (default: all layout tests).
layout-refs *NAMES:
    uv run --project tools swbtools layout-refs {{NAMES}}

# Ask Chromium (and swb with --with-swb) about the cases of JSON files. Example: just probe --with-swb tools/probes/example.json
probe *ARGS:
    uv run --project tools swbtools probe {{ARGS}}

# Write layout dumps, DOM dumps and screenshots of all fixtures and layout tests to DIR. Compare two snapshots with `diff -r`.
snapshot DIR: build
    tools/snapshot.sh {{DIR}}

# Prepare the persistent review worktree (../swb-review). Example: just review-tree staged
review-tree *ARGS:
    tools/review-tree.sh {{ARGS}}

# Lint, format check and tests of the Python tools. The automation tests use the release binary.
tools-check: build
    uv --directory tools run ruff check
    uv --directory tools run ruff format --check
    uv --directory tools run pytest -q

# Format the Python tools and apply safe lint fixes.
tools-fmt:
    uv --directory tools run ruff format
    uv --directory tools run ruff check --fix
