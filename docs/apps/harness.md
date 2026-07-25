# Harness — Attested execution confinement

- **Path:** `crates/agent-harness`
- **Owner:** Polaris / Harness (sandbox and attestation for agent workers)
- **Runtime:** Rust; local process confinement only at this stage (ADR-0018 D2)
- **Tenant model:** organization; a profile is bound to the run that requested it and never shared across tenants

## Purpose and actors

The harness is the boundary a worker cannot talk its way out of. It takes a requested profile, applies the controls that profile prescribes, runs the work inside them, and emits a signed attestation binding what was asked to what was actually enforced. Its value is not that it restricts — it is that it **proves** it restricted.

**Actors:**

- **Orchestrator** (caller): requests execution under a named profile and consumes the attestation. It cannot relax a profile.
- **Workers** (confined): replaceable processes performing a step. A worker's own permission model is never trusted; ADR-0004 §3 makes this explicit.
- **Operators** (auditors): verify an attestation against the profile digest and the effective controls, independently of the run that produced it.
- **Owner** (nominative acts): pronounces the bootstrap hard stop of the first confinement merge.

**Doctrine constraints.** Everything a worker emits is `operational` and never authority (K2). Anything a worker produces that will reach a model is enveloped and signed (K3). The attestation binds requested profile, effective controls, worker manifests and kernel capabilities — an attestation that binds less than it claims is invalid, not partial.

## Journeys

1. **Request a profile.** The orchestrator names a profile. The harness resolves it, computes its digest, and refuses if the requested identifier and the resolved content disagree.

2. **Apply filesystem confinement.** The workspace root is canonicalized, symlinks resolved under the declared policy, and the read-only, writable and denied sets applied. A path escaping the workspace root after canonicalization is refused before any process starts.

3. **Run confined.** The worker process is spawned inside the applied controls. Output is bounded per tool and in total; exceeding a bound truncates and refuses rather than buffering without limit.

4. **Scan outputs fail-closed.** Outputs are scanned before leaving the boundary. A scan that cannot complete refuses the result — an unscanned output is treated as a failed one, never as a clean one.

5. **Attest.** The harness emits a signed attestation binding the requested profile digest, the effective controls actually applied, the worker manifests and the kernel capabilities present. Requested and effective are both recorded, so a divergence is visible rather than smoothed over.

6. **Verify independently.** An operator, holding only the attestation and the profile, re-verifies the binding without access to the run, the worker or the harness that produced it.

## Non-goals

- **Trusting the worker.** No confinement decision is delegated to the process being confined.
- **Best-effort confinement.** A control that cannot be applied is a refusal, never a warning followed by execution.
- **Attesting what was not enforced.** The attestation records effective controls, not intended ones.
- **Network egress, secrets, providers.** Closed at this stage by ADR-0018 D2. The profile schema can express them; the runtime refuses them until their own package and review exist.
- **Being a scheduler.** Sequencing, budgets and liveness belong to the orchestrator.
- **Retaining worker content.** Operational logs are private by default, restricted to closed categories and counters; content fields are off unless a profile explicitly allows them.

## Domain protocol

**Commands:** `ResolveProfile`, `ApplyControls`, `RunConfined`, `ScanOutputs`, `EmitAttestation`.

**Queries:** `GetProfile`, `GetEffectiveControls`, `VerifyAttestation`, `ExplainRefusal`.

**Events:** `ProfileResolved`, `ControlsApplied`, `RunStarted`, `OutputTruncated`, `RunCompleted`, `RunRefused`, `AttestationEmitted`.

Requested and effective controls are distinct fields throughout. Collapsing them into one would make a silently degraded confinement indistinguishable from an honoured one.

## Refusal matrix

- `harness.profile_unresolved` — the named profile does not resolve.
- `harness.profile_digest_mismatch` — resolved content does not hash to the requested digest.
- `harness.platform_unsupported` — the host platform is not in the profile's supported set.
- `harness.control_not_enforceable` — a prescribed control cannot be applied on this host.
- `harness.path_escapes_workspace` — a path leaves the workspace root after canonicalization.
- `harness.symlink_policy_violation` — a symlink resolves outside the policy's allowance.
- `harness.write_outside_writable_set` — a write targets a path not in the writable set.
- `harness.denied_path_touched` — a path in the denied set was accessed.
- `harness.output_limit_exceeded` — per-tool or total output bytes exceeded.
- `harness.output_scan_incomplete` — the fail-closed output scan could not complete.
- `harness.capability_not_enabled` — the profile requests a capability closed at this stage.
- `harness.attestation_binding_incomplete` — profile, effective controls, worker manifests or kernel capabilities are not all bound.
- `harness.attestation_unsigned` — the attestation carries no valid signature.

## Data

The harness owns no product data. It persists exactly three things, all under the shared tenant barrier: profiles (immutable, content-addressed by digest), attestations (append-only, signed, never edited), and refusal records (code, profile digest, run reference — never the offending content).

Worker output is not stored. Only its digest, its byte count and its truncation flag survive the boundary. Operational logs follow `harness-profile.v1`: private by default, closed categories and counters, content fields disabled unless the profile enables them, external telemetry off, bounded retention, coarse timestamps.

## Authentication and authorization

The harness accepts work only from an orchestrator presenting a Biscuit token whose agent facts are present and whose `capability_scope` covers the requested profile's controls. Per-agent revocation is checked fail-closed: an unavailable revocation store denies.

Attestations are signed with Ed25519. The production signing key is an owner ceremony, deferred and separate; the crate takes the key as a parameter and refuses to emit an unsigned attestation rather than emitting a weaker one.

## Runtime boundaries

**Rust (pure):** profile parsing and digesting, control resolution, path canonicalization decisions, attestation assembly and verification. Deterministic and testable without a host.

**Rust (host boundary):** filesystem control application, process spawn, output capture and truncation. This is the only part holding OS capabilities, and it holds exactly one at this stage — spawning a local process.

**Worker (external):** confined. It receives a bounded opaque payload and returns a bounded opaque result.

**Closed at this stage:** network namespaces and egress, secret injection, provider access, container or VM isolation. The profile schema can express controls the runtime refuses; expressible is not enabled.

## Accessibility and degraded mode

The harness has no human interface. Its human-facing surface is the attestation, which must be verifiable by a person with a digest and a public key, without running any part of this system.

Degraded modes fail closed without exception. A control that cannot be applied refuses the run. A scan that cannot complete refuses the result. An unavailable signing key refuses the attestation, and therefore the step — a run whose confinement cannot be attested is indistinguishable from an unconfined one, and is treated as such.

## Contracts

- `contracts/schemas/harness-profile.v1.schema.json`
- `contracts/schemas/harness-attestation.v1.schema.json`
- `contracts/schemas/execution-plan-body.v1.schema.json`
- `contracts/authz/authority-v1.datalog`
- `contracts/fixtures/agent-orchestration-v1/signature-vectors.v1.json`
- `contracts/fixtures/agent-orchestration-v1/digest-vectors.v1.json`

Locked by ADR-0004; no implementation package amends them.

## Evidence

Adversarial tests are the evidence that matters here, and they are written before the confinement they attack: a symlink escaping the workspace root; a path traversal surviving naive normalization; a write to a denied path; an output exceeding its per-tool bound; a scan interrupted mid-way; an attestation with one binding removed; a tampered profile digest. Each must produce its refusal code, not a warning.

Plus: an attestation verified independently from the run that produced it, and a proof that requested and effective controls are recorded separately.

Published under `distribution/evidence/` with coverage metrics (I-20).

## Work packages

1. **Profile and attestation core** — parsing, digesting, assembly, verification, pure and hostless. Its adversarial suite lands with it.
2. **Filesystem confinement** — canonicalization, symlink policy, read-only/writable/denied enforcement on a real host. First security-critical merge of this surface: bootstrap hard stop (ADR-0011 D4).
3. **Bounded process execution** — spawn, output capture, per-tool and total limits, fail-closed scan, attestation emission.

Each declares exclusive write paths and carries the ADR-0004 §6 criteria.

## Release and rollback

**Release gates.** Every refusal code reachable by an adversarial test. No path escape on the platform's canonicalization semantics. No attestation emitted with an incomplete binding or without a signature. Requested and effective controls distinguishable in every attestation. Independent review by reviewers distinct from the implementer (K4).

**Rollback.** Reverting the deployable is sufficient: profiles are content-addressed and immutable, attestations are append-only. An attestation emitted by a rolled-back version stays valid and verifiable — it records what was enforced at the time, which does not become false because the code changed.
