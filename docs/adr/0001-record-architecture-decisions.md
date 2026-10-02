# ADR 0001: Record architecture decisions

- Status: accepted
- Date: 2026-10-02

## Context

One agent maintains this project over a long time, across many sessions with
no shared memory. Decisions and their reasons must be in the repository, or
they are lost.

## Decision

Record each significant decision as an ADR in `docs/adr/`.

- File name: `NNNN-short-title.md`, numbered in sequence.
- Sections: Status, Date, Context, Decision, Consequences.
- Status is one of: proposed, accepted, superseded by ADR NNNN.
- Do not rewrite an accepted ADR when a decision changes. Write a new ADR and
  mark the old one as superseded.

A decision is significant if a future maintainer could ask "why is it like
this?" and the answer is not obvious from the code. Examples: choice of a
library, a data structure that many crates use, a testing approach, a
deliberate deviation from a specification.

## Consequences

- Each ADR costs a few minutes to write.
- Future sessions can read `docs/adr/` to recover the reasons for the current
  design.
