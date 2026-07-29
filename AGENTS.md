# orchestrator Canonical Agent Rules

Agent orchestration brick of the Libre AI constellation (couche 2,
ADR-0018 D3): the agent-orchestrator crate at the root, the review
fan-out (tools/review, consuming @libre-ai/envelope as a sha-pinned
git-dep) and its capability-boundary proof surface
(verification/agent-orchestrator). Contract types resolve from the
sdk-rs projection as a sha-pinned Cargo git-dep under
[sources.allow-org]; vector fixtures from the pinned contracts
authority git-dep (bun install precedes cargo test). rust-quality owns
the Rust suite; check:review-tools owns the review fan-out suite.

Run `bun run check` and `cargo test --locked` before pushing; never hide
a red test. Security > quality > performance > completeness.
