# orchestrator

Agent orchestration brick of the Libre AI constellation (couche 2) — the
agent-orchestrator crate, the review fan-out and its proof surface.

Born from the hub dismantling ([ADR-0020](https://github.com/libre-ai/governance/blob/main/docs/adr/0020-general-activation-and-hub-dismantling.md)). Consumed as a sha-pinned Cargo git-dep.

## Authorized-execution API

The `0.2.0` API validates locked contract documents and evaluates graph,
causal, human-decision, generation-transfer and effect-continuity semantics
without performing I/O. This synthetic example is compiled by
`cargo test --doc --locked` through the crate-level README inclusion:

```rust
use libre_ai_agent_orchestrator::{evaluate_graph, parse_authorized_graph};
use libre_ai_contract_types::ContractRegistry;
use serde_json::json;

let registry = ContractRegistry::embedded().expect("embedded schemas are build-time authorities");
let document = json!({
    "schemaVersion": "libre-ai.execution-graph.v1",
    "id": "urn:libre-ai:graph:synthetic-example",
    "organizationId": "ten_1234567890abcdef",
    "entryStepId": "urn:libre-ai:step:calculate",
    "steps": [
        {
            "stepId": "urn:libre-ai:step:calculate",
            "kind": "calculation",
            "outcomeCodes": ["ready"],
            "retryPolicy": { "maximumAttempts": 1, "retryableOutcomeCodes": [] }
        },
        {
            "stepId": "urn:libre-ai:step:terminal",
            "kind": "terminal",
            "outcomeCodes": []
        }
    ],
    "edges": [{
        "edgeId": "urn:libre-ai:edge:complete",
        "fromStepId": "urn:libre-ai:step:calculate",
        "outcomeCode": "ready",
        "toStepId": "urn:libre-ai:step:terminal"
    }],
    "createdAt": "2026-09-10T10:00:00Z",
    "graphDigest": "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd"
});
let graph = parse_authorized_graph(&registry, &document).expect("valid synthetic graph");
assert_eq!(evaluate_graph(&graph).code(), "graph-valid");
```

Returned applications are pure proposals. They require a separately
authorized persistence and effect boundary before they can change external
state.

## Verify

```sh
bun install --frozen-lockfile && bun run check
cargo test --locked
```

## État du projet

<!-- libre-ai:project-status:begin -->
<!-- Section générée depuis project.v1.yaml — ne pas éditer à la main. -->

- Situation actuelle : Le noyau natif authorized-execution 0.2.0 de WP-G3-O02 est prouvé sur un commit immuable. Le run boundary, les effets réels et le déploiement restent bloqués et WP-G3-O01 n'est pas revendiqué.
- Maturité : usable
- Exposition : spec-published
- Confiance : medium
- Preuves vérifiées le : 2026-09-10
- Avancement : 100 % du périmètre actuellement déclaré

<!-- libre-ai:project-status:end -->

La fiche [`project.v1.yaml`](./project.v1.yaml) est l'autorité de l'état du projet ; cette section en est générée et le gate de flotte échoue si elles divergent.
