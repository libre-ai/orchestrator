# `libre-ai-agent-orchestrator`

G2-only deterministic Missions simulator and semantic `OrchestratorEvent` validator.

## Boundary

Runtime JSON is limited to locked canonical contracts:

- stdin: `AgentHandoff` v1;
- stdout: `OrchestratorEvent` v1 NDJSON;
- results: canonical Artifact/Evidence references.

Missions remains authoritative for approval, authorization, durable idempotency, persistence and verdict. A submitted result is never success.

## G2 profile

- `complete`: `started → progressed → result-submitted`;
- `blocked`: `started → blocked → decision-requested`;
- `failed`: `started → failed`;
- insufficient virtual duration: `started → budget-exceeded`.

The simulator performs zero tool calls and requires `network=none`. It has no DB, HTTP, Biscuit/secret handling, UI, persistence, approval, provider/model, subprocess, tool execution, harness, general planner or Knowledge graph dependency.

```bash
cargo run -p libre-ai-agent-orchestrator -- simulate-v1 \
  --mission-id urn:libre-ai:mission:mission-1 \
  --started-at 2026-07-16T00:00:00Z \
  --scenario complete \
  --max-duration-seconds 2 --max-tool-calls 0 --network none \
  --artifact-id urn:libre-ai:artifact:artifact-1 \
  --artifact-digest bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb \
  --artifact-media-type application/json \
  --evidence-id urn:libre-ai:evidence:report-1 \
  --evidence-digest cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc \
  --evidence-media-type application/json \
  < fixtures/handoff.valid.json
```

Any real execution or control protocol requires a separate architecture/security lock.
