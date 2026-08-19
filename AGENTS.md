# orchestrator Canonical Agent Rules

## Authority

Agent orchestration brick (couche 2) of the Libre AI constellation: an
effect-free Rust decision core (`agent-orchestrator` crate) plus
fan-out review tooling (`tools/review`) with self-cleaning worktrees.
It separates deciding from acting — the decision is a pure, replayable
function testable on opaque identifiers, and the effect stays outside,
with the caller. Contract types resolve from `libre-ai/sdk-rs` as a
pinned Cargo git-dep; locked event vectors replayed by
`tests/locked_event_vectors.rs` come from the contracts authority
(https://raw.githubusercontent.com/libre-ai/contracts/main/AGENTS.md).
`tools/review` wraps shared proof in `libre-ai/envelope` (K3) before
handing it to a review model. Fleet doctrine and the gate template
live upstream:
https://raw.githubusercontent.com/libre-ai/governance/main/AGENTS.md

## Boundaries

- The crate owns no side effect of its own: no process, file, socket
  or secret — enforced by
  `verification/agent-orchestrator/check-capabilities.ts`.
- Contract types and event vectors are canonical in `libre-ai/sdk-rs`
  and `libre-ai/contracts`, never redefined here.
- Current exposure and acceptance state live in this repository's own
  `project.v1.yaml`, aggregated by governance — never duplicated here.

## Quality gates

Run `bun run check`; for the Rust crate, `cargo test --locked
--all-features`. `rust-quality` owns the Rust suite,
`check:review-tools` owns the review fan-out suite. Never hide a red
test.

## Agents

- Read actual state before editing.
- Stage files before running tree-walking gates.
- Worktrees created for the review fan-out are removed in `finally`,
  always — the fleet's reference cleanup model.
- Security > quality > performance > completeness.
