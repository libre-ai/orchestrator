# First real review fan-out campaign

Owner-arbitration: 2026-08-19

Criterion under evidence: "review fan-out proven on a real campaign (not only a dry-run)"
(produit-zéro fiche, critère ③). This document is the evidence for `libre-ai/orchestrator`;
it does not itself update the produit-zéro fiche — that fiche lives outside this repository
and is out of scope here (see "Scope note" below).

## Tool bug found and fixed first

`tools/review/fanout.ts` refused to run at all, dry-run included:

```
run from the repository root (docs/reviews/AGENT-REVIEW-PROTOCOL.md not found)
```

The precondition checked `existsSync("docs/reviews/AGENT-REVIEW-PROTOCOL.md")`, a path that
does not exist anywhere in this repository — neither at the root nor under `tools/review/`.
The protocol document is never vendored in-tree; it only resolves through the `@libre-ai/governance`
git-dependency once `bun install` has populated `node_modules`. The check was fixed to test the
real resolved path:

```diff
- if (!existsSync("docs/reviews/AGENT-REVIEW-PROTOCOL.md")) {
-   console.error("run from the repository root (docs/reviews/AGENT-REVIEW-PROTOCOL.md not found)");
+ const protocolPath = "node_modules/@libre-ai/governance/docs/reviews/AGENT-REVIEW-PROTOCOL.md";
+ if (!existsSync(protocolPath)) {
+   console.error(`run from the repository root with dependencies installed (${protocolPath} not found)`);
```

Verified after the fix: `bun run lint` and `bun run typecheck` green at repo root; `bun run
check:review-tools` green (36/36 unit tests, `fanout-core.test.ts` unaffected — it never
exercised this CLI-level check).

This same investigation surfaced a second, deeper fact, left unresolved by design (see
"Independent finding" below): the fix only holds once `bun install` has run at the repo root.
`docs/reviews/AGENT-REVIEW-PROTOCOL.md` is not a Git object in this repository at any commit —
a bare `git worktree add --detach` checkout (exactly what each review pass runs in) cannot see
it, only the outer invocation that ran `bun tools/review/fanout.ts` from an already-`bun
install`-ed checkout can.

## Environment bug found and fixed first (machine-local, not this repo)

The `pi` worker binary was broken before any of the above: `pi --version` failed because a
`brew upgrade node` revision bump (`26.5.0_1` → `26.7.0`) deleted the exact Homebrew keg path
the local pi deployment pins by `realpathSync(process.execPath)`. Fixed with the existing
recovery script (`~/.local/bin/pi-repin-node.sh`), which re-pins onto the current keg and
re-qualifies: `pi 0.84.2`, `deployed=true`. This is a pre-existing, known, machine-local
failure mode, unrelated to `orchestrator`; noted here only because it blocked this campaign
until repaired.

## Provider/model reality check

`plan.example.json` names `provider: openai-codex`, `model: gpt-5.6-sol`. On this machine:

- `pi auth check --provider openai-codex` → `{"status":"ready","authType":"oauth"}` (only
  ready provider; `anthropic`, `google`, `openai` are all `credentials_not_configured`).
- `gpt-5.6-sol` and two other listed models (`gpt-5.3-codex-spark`, `gpt-5.4`) fail immediately
  with `Codex error: The 'X' model is not supported when using Codex with a ChatGPT account.`
- `gpt-5.4-mini`, `gpt-5.5`, `gpt-5.6-luna`, `gpt-5.6-terra` all answer normally.

The campaign plan used the highest-tier model that actually works on this account:
`gpt-5.6-terra`.

## Campaign plan used

Committed at `tools/review/plan.orchestrator-ca44eb3.json`:

```json
{
  "subject": "orchestrator",
  "commit": "ca44eb33ce3ac2e9e39bea6f913be714b974a2c3",
  "roles": ["security", "quality"],
  "mode": "specialized-role",
  "evidence": ["AGENTS.md", "README.md", "Cargo.toml"],
  "concurrency": 2,
  "provider": "openai-codex",
  "model": "gpt-5.6-terra",
  "thinking": "xhigh",
  "outputDir": "docs/reviews/orchestrator/ca44eb3"
}
```

- **Target commit**: `ca44eb33ce3ac2e9e39bea6f913be714b974a2c3` — `origin/main` HEAD of
  `libre-ai/orchestrator` at fetch time (2026-08-19). The local clone's own `HEAD` was 1 commit
  ahead / 4 behind `origin/main` at the time; the campaign ran from a fresh detached worktree of
  `origin/main` (fetched first) rather than the stale local branch, so the reviewed commit is the
  repository's real current state, not a local-only artifact.
- **Roles**: `security`, `quality` — the first two of the five named in
  `AGENT-REVIEW-PROTOCOL.md`'s candidate-integration scope list ("security, quality,
  performance, completeness and sovereignty/privacy"), run here as two separate
  `specialized-role` passes per the task scope (2 roles, concurrency 2).
- **Command executed** (no `--dry-run`):
  ```
  bun tools/review/fanout.ts tools/review/plan.orchestrator-ca44eb3.json
  ```
  run from the repository root of a temporary detached worktree
  (`git worktree add --detach <tmp> origin/main`, `bun install` at root).

## Raw outcome

```
2 pass(es) on ca44eb3 — concurrency 2, openai-codex/gpt-5.6-terra:xhigh
journal docs/reviews/orchestrator/ca44eb3/events.jsonl — 12 events across 2 pass(es)
ok   security (195637 ms) → docs/reviews/orchestrator/ca44eb3/security.verdict.json
ok   quality (175113 ms) → docs/reviews/orchestrator/ca44eb3/quality.verdict.json
```

- 2 jobs run, 2 jobs ok, 0 failed.
- 2 verdicts recorded, both schema-valid against `review-verdict.v0.1` (Ajv) and cross-checked
  (`reviewPassId`, `role`, `mode`, `commitSha` all matched their job — no `verdict_rejected` event
  in the journal).
- 0 `worktree_dirty` events: both per-job detached worktrees (`git worktree add --detach`, one
  per role, under a `mkdtemp` base) were removed clean; the runner's own `finally` ran
  `git worktree remove --force` + `git worktree prune` for both.
- Journal replay reconciliation: no "journal disagrees" line was printed — the append-only
  JSONL trace agrees with what the run observed for both passes.
- Wall-clock: `2026-08-19T03:15:18.849Z` (first `pass_start`) →
  `2026-08-19T03:18:34.483Z` (last `pass_end`) — **3 min 16 s** end-to-end for both roles at
  concurrency 2 (agent call durations: security 195.6 s, quality 175.1 s, run mostly overlapped).
- Artifacts, all committed in this PR:
  - `docs/reviews/orchestrator/ca44eb3/events.jsonl` (append-only run journal, 12 events)
  - `docs/reviews/orchestrator/ca44eb3/security.verdict.json`
  - `docs/reviews/orchestrator/ca44eb3/quality.verdict.json`

## Verdicts

Both roles independently returned **reject**, on the same blocking finding, reached by
different read-only command sequences in their own isolated worktrees (no shared state beyond
the evidence digest built once by the orchestrator):

- **security** (`rp-ca44eb3-aa969d2b-…`): blocking — `git cat-file -e
HEAD:docs/reviews/AGENT-REVIEW-PROTOCOL.md` fails (exit 128) inside the reviewed worktree;
  the fan-out tool's own precondition (`tools/review/fanout.ts:222-224` at review time) exits 2
  under the same condition; `check:review-tools` does not cover this precondition. Residual
  risks noted: the governance pin SHA is consistent across `project.v1.yaml`/`package.json`/
  `bun.lock`/CI and confirmed signed via the GitHub API, but its object is absent from the local
  Git database; no Bun gate was run inside the review worktree (no `node_modules`, and
  installing one would have dirtied the immutable review worktree).
- **quality** (`rp-ca44eb3-ac506eca-…`): blocking — same absence of
  `docs/reviews/AGENT-REVIEW-PROTOCOL.md` from both `HEAD` and `HEAD^`, same exit-2 precondition
  in `fanout.ts`, concluding the declared review workflow "cannot execute from this checkout."
  Residual risk noted: dynamic quality gates were not run, by design (the review mandate
  forbids writing to the reviewed worktree).

## Independent finding (not acted on here — scope boundary)

Both reviewers converged, from clean read-only worktrees with no shared reasoning, on a real
architectural gap the fix above does not close: `docs/reviews/AGENT-REVIEW-PROTOCOL.md` is not
a Git-tracked path in `orchestrator` at any commit, only a resolved dependency artifact after
`bun install`. A bare `git worktree add --detach` checkout of this repository — which is
exactly what each fan-out review pass runs inside — cannot see it. The fix in this PR makes the
top-level `bun tools/review/fanout.ts` invocation correct (it runs from an installed checkout),
but does not change what a reviewer sees inside its own bare worktree. Two independent,
schema-valid, cross-checked `reject` verdicts are the intended output of a working fan-out on a
target with a real defect — not a bug in the harness. Whether to vendor the protocol doc
in-tree, or to give reviewers an explicit note that `node_modules` is intentionally absent from
their worktree, is an architecture decision left to the owner; it is not addressed by this PR.

## Scope note: produit-zéro fiche criterion ③

`orchestrator`'s own `project.v1.yaml` carries two `exit_criteria` under phase `service`:
`consumed-pinned` (already `accepted`) and `compat-policy` (`pending`, about `cargo test
--locked` symbol/refusal-code stability — unrelated to review fan-out). Neither matches "review
fan-out proven on a real campaign", so this PR does not edit `project.v1.yaml`. The produit-zéro
fiche referenced in the task brief (critère ③) is not part of this repository; updating it is
out of scope for an orchestrator-only PR and is left to the coordinator/owner.
