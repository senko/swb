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

# Write tests/layout/*.boxes.json with Chromium (default: all layout tests).
layout-refs *NAMES:
    uv run --project tools swbtools layout-refs {{NAMES}}

# Lint, format check and tests of the Python tools. The automation tests use the release binary.
tools-check: build
    uv --directory tools run ruff check
    uv --directory tools run ruff format --check
    uv --directory tools run pytest -q

# Format the Python tools and apply safe lint fixes.
tools-fmt:
    uv --directory tools run ruff format
    uv --directory tools run ruff check --fix
