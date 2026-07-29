# orchestrator

Agent orchestration brick of the Libre AI constellation (couche 2) — the
agent-orchestrator crate, the review fan-out and its proof surface.

Born from the hub dismantling ([ADR-0020](https://github.com/libre-ai/governance/blob/main/docs/adr/0020-general-activation-and-hub-dismantling.md)). Consumed as a sha-pinned Cargo git-dep.

## Verify

```sh
bun install --frozen-lockfile && bun run check
cargo test --locked
```
