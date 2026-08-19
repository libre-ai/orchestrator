# orchestrator

Agent orchestration brick of the Libre AI constellation (couche 2) — the
agent-orchestrator crate, the review fan-out and its proof surface.

Born from the hub dismantling ([ADR-0020](https://github.com/libre-ai/governance/blob/main/docs/adr/0020-general-activation-and-hub-dismantling.md)). Consumed as a sha-pinned Cargo git-dep.

## Verify

```sh
bun install --frozen-lockfile && bun run check
cargo test --locked
```

## État du projet

<!-- libre-ai:project-status:begin -->
<!-- Section générée depuis project.v1.yaml — ne pas éditer à la main. -->

- Situation actuelle : Née verte en γ 3.4 (crate + tools/review + preuve + 3 docs d'application couche 2).
- Maturité : usable
- Exposition : spec-published
- Confiance : medium
- Preuves vérifiées le : 2026-08-18
- Avancement : 100 % du périmètre actuellement déclaré

<!-- libre-ai:project-status:end -->

La fiche [`project.v1.yaml`](./project.v1.yaml) est l'autorité de l'état du projet ; cette section en est générée et le gate de flotte échoue si elles divergent.
