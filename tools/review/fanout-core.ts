import { type EnvelopeKey, renderGuarded, wrapUntrusted } from "@libre-ai/envelope";
import Ajv2020 from "ajv/dist/2020";

// Pure orchestration logic for role-separated review fan-out.
// Side effects (git, process spawn, filesystem) live in tools/review/fanout.ts
// so every decision below stays unit-testable.

export interface ReviewPlan {
  subject: string;
  commit: string;
  roles: string[];
  mode: "candidate-integration" | "specialized-role";
  evidence: string[];
  concurrency: number;
  provider: string;
  model: string;
  thinking: string;
  outputDir: string;
}

export interface ReviewJob {
  reviewPassId: string;
  subject: string;
  commit: string;
  role: string;
  mode: ReviewPlan["mode"];
  outputPath: string;
}

export interface DedupeResult {
  run: ReviewJob[];
  skipped: ReviewJob[];
}

const COMMIT_SHA_PATTERN = /^[0-9a-f]{40}$/;
const EVIDENCE_PATH_PATTERN = /^[A-Za-z0-9][A-Za-z0-9._/-]*$/;
const DEFAULT_CONCURRENCY = 5;

// Verdict envelope schema. Kept tool-side on purpose: it is promoted to
// contracts/ only when a second producer emits the shape (decision-log
// 2026-07-02, "no abstraction without a producer").
export const reviewVerdictSchema = {
  $schema: "https://json-schema.org/draft/2020-12/schema",
  $id: "review-verdict.v0.1",
  type: "object",
  additionalProperties: false,
  required: [
    "reviewPassId",
    "role",
    "mode",
    "commitSha",
    "contractHashes",
    "commands",
    "findings",
    "residualRisks",
    "verdict",
  ],
  properties: {
    reviewPassId: { type: "string", minLength: 1 },
    role: { type: "string", minLength: 1 },
    mode: { enum: ["candidate-integration", "specialized-role"] },
    commitSha: { type: "string", pattern: "^[0-9a-f]{40}$" },
    contractHashes: {
      type: "array",
      items: {
        type: "object",
        additionalProperties: false,
        required: ["path", "sha256"],
        properties: {
          path: { type: "string", minLength: 1 },
          sha256: { type: "string", pattern: "^[0-9a-f]{64}$" },
        },
      },
    },
    commands: { type: "array", items: { type: "string", minLength: 1 } },
    findings: {
      type: "object",
      additionalProperties: false,
      required: ["blocking", "major", "minor", "nonBlocking"],
      properties: {
        blocking: { $ref: "#/$defs/findingList" },
        major: { $ref: "#/$defs/findingList" },
        minor: { $ref: "#/$defs/findingList" },
        nonBlocking: { $ref: "#/$defs/findingList" },
      },
    },
    residualRisks: { type: "array", items: { type: "string", minLength: 1 } },
    verdict: {
      enum: ["approve", "approve-with-minor-reservations", "reject"],
    },
    runner: { type: "object" },
  },
  $defs: {
    findingList: {
      type: "array",
      items: {
        type: "object",
        additionalProperties: false,
        required: ["title", "detail"],
        properties: {
          title: { type: "string", minLength: 1 },
          detail: { type: "string", minLength: 1 },
        },
      },
    },
  },
} as const;

const ajv = new Ajv2020({ allErrors: true });
const validateAgainstSchema = ajv.compile(reviewVerdictSchema);

export function parsePlan(raw: unknown): ReviewPlan {
  if (typeof raw !== "object" || raw === null || Array.isArray(raw)) {
    throw new Error("plan must be a JSON object");
  }
  const plan = raw as Record<string, unknown>;
  const errors: string[] = [];

  const subject = typeof plan.subject === "string" ? plan.subject.trim() : "";
  if (subject.length === 0) errors.push("subject: non-empty string required");

  const commit = typeof plan.commit === "string" ? plan.commit : "";
  if (!COMMIT_SHA_PATTERN.test(commit)) {
    errors.push("commit: full 40-hex commit SHA required");
  }

  const roles = Array.isArray(plan.roles)
    ? plan.roles.filter(
        (role): role is string => typeof role === "string" && role.trim().length > 0,
      )
    : [];
  if (roles.length === 0) errors.push("roles: non-empty string array required");
  if (new Set(roles).size !== roles.length) errors.push("roles: duplicates are not allowed");

  const mode = plan.mode ?? "specialized-role";
  if (mode !== "candidate-integration" && mode !== "specialized-role") {
    errors.push('mode: "candidate-integration" or "specialized-role"');
  }

  const evidence = Array.isArray(plan.evidence)
    ? plan.evidence.filter((path): path is string => typeof path === "string")
    : [];
  // Evidence paths reach the model-facing prompt as raw section headers
  // (`### ${path}`) and a `git show <commit>:<path>` argument. Fail closed at
  // parse time: repo-relative, safe character set, no traversal — a path
  // carrying guard delimiters, spaces or newlines never enters the pipeline
  // (K4 review of f49fc18, finding 3).
  for (const path of evidence) {
    if (!EVIDENCE_PATH_PATTERN.test(path) || path.includes("..")) {
      errors.push(
        `evidence: unsafe path (allowed: [A-Za-z0-9._/-], repo-relative, no ".."): ${JSON.stringify(path)}`,
      );
    }
  }

  const concurrency =
    plan.concurrency === undefined ? DEFAULT_CONCURRENCY : Number(plan.concurrency);
  if (!Number.isInteger(concurrency) || concurrency < 1) {
    errors.push("concurrency: positive integer required");
  }

  const provider = typeof plan.provider === "string" ? plan.provider.trim() : "";
  if (provider.length === 0) errors.push("provider: non-empty string required");
  const model = typeof plan.model === "string" ? plan.model.trim() : "";
  if (model.length === 0) errors.push("model: non-empty string required");
  const thinking =
    typeof plan.thinking === "string" && plan.thinking.trim().length > 0
      ? plan.thinking.trim()
      : "xhigh";

  const outputDir = typeof plan.outputDir === "string" ? plan.outputDir.trim() : "";
  if (outputDir.length === 0) errors.push("outputDir: non-empty string required");

  if (errors.length > 0) {
    throw new Error(`invalid review plan:\n- ${errors.join("\n- ")}`);
  }

  return {
    subject,
    commit,
    roles,
    mode: mode as ReviewPlan["mode"],
    evidence,
    concurrency,
    provider,
    model,
    thinking,
    outputDir,
  };
}

export function buildJobs(plan: ReviewPlan, makeId: () => string): ReviewJob[] {
  return plan.roles.map((role) => ({
    reviewPassId: makeId(),
    subject: plan.subject,
    commit: plan.commit,
    role,
    mode: plan.mode,
    outputPath: `${plan.outputDir}/${role}.verdict.json`,
  }));
}

// A recorded verdict for the same (commit, role) is immutable audit evidence:
// never re-launch it silently. `force` re-runs but the caller must refuse to
// overwrite the existing record (see fanout.ts).
export function dedupeJobs(
  jobs: ReviewJob[],
  hasRecordedVerdict: (job: ReviewJob) => boolean,
  force: boolean,
): DedupeResult {
  if (force) return { run: [...jobs], skipped: [] };
  const run: ReviewJob[] = [];
  const skipped: ReviewJob[] = [];
  for (const job of jobs) {
    (hasRecordedVerdict(job) ? skipped : run).push(job);
  }
  return { run, skipped };
}

// The protocol document lives in libre-ai/governance, not in this repo: a
// local copy would drift from the one the git-dep pins. Resolve it from the
// installed dependency instead of assuming a repo-relative path that this
// repo never carries (2026-08-18 fix — the check used to look for
// docs/reviews/AGENT-REVIEW-PROTOCOL.md at the repo root, a file that only
// ever existed inside node_modules/@libre-ai/governance).
export const GOVERNANCE_PROTOCOL_PATH =
  "node_modules/@libre-ai/governance/docs/reviews/AGENT-REVIEW-PROTOCOL.md";

export interface ProtocolResolution {
  readonly ok: boolean;
  readonly path: string;
  readonly message?: string;
}

/**
 * Locate the review protocol inside the installed @libre-ai/governance
 * git-dep. `exists` is injected so this stays unit-testable without a real
 * node_modules tree (same pattern as dedupeJobs' hasRecordedVerdict).
 */
export function resolveReviewProtocol(exists: (path: string) => boolean): ProtocolResolution {
  if (exists(GOVERNANCE_PROTOCOL_PATH)) {
    return { ok: true, path: GOVERNANCE_PROTOCOL_PATH };
  }
  const message = exists("node_modules/@libre-ai/governance")
    ? `${GOVERNANCE_PROTOCOL_PATH} not found in the installed @libre-ai/governance dependency — ` +
      "the protocol doc moved or the git-dep pin is stale; check docs/reviews/ in that package"
    : `${GOVERNANCE_PROTOCOL_PATH} not found — @libre-ai/governance is not installed; ` +
      "run this from the repository root after `bun install`";
  return { ok: false, path: GOVERNANCE_PROTOCOL_PATH, message };
}

export function buildPrompt(job: ReviewJob, evidenceDigest: string): string {
  const scope =
    job.mode === "candidate-integration"
      ? "candidate-integration scope (security, quality, performance, completeness, sovereignty/privacy)"
      : `specialized role "${job.role}"`;
  return [
    `You are performing a review-only pass under docs/reviews/AGENT-REVIEW-PROTOCOL.md for subject "${job.subject}", ${scope}.`,
    "",
    `Target: immutable commit ${job.commit}. Your working directory is a detached worktree checked out at that exact commit.`,
    "Do not modify, create or delete any file: a dirty worktree invalidates the pass. Reproduce evidence with read-only commands before any verdict.",
    "",
    `Assigned reviewPassId: ${job.reviewPassId}. Echo it verbatim in your verdict.`,
    "",
    "Shared evidence digest, prepared once by the orchestrator from the target commit (verify against the worktree when in doubt):",
    "",
    evidenceDigest,
    "",
    "Deliverable: end your reply with exactly one fenced ```json block containing the verdict envelope with fields:",
    // The runner rejects on exact equality of these four fields; the prompt
    // states each required literal so nothing is left to be guessed (a guessed
    // `mode` failed 2/2 real passes on 2026-08-04).
    `reviewPassId (exactly "${job.reviewPassId}"), role (exactly "${job.role}"), mode (exactly "${job.mode}"), commitSha (exactly "${job.commit}"),`,
    "contractHashes ([{path, sha256}]), commands (the commands you actually ran),",
    "findings ({blocking, major, minor, nonBlocking}: arrays of {title, detail}), residualRisks (string array),",
    'verdict ("approve" | "approve-with-minor-reservations" | "reject").',
    "One verdict exactly; a generic pass cannot satisfy a specialized role.",
  ].join("\n");
}

export function extractVerdict(output: string): unknown {
  const fences = [...output.matchAll(/```json\s*\n([\s\S]*?)```/g)];
  const last = fences.at(-1);
  if (last === undefined) {
    throw new Error("no fenced ```json verdict block found in agent output");
  }
  try {
    return JSON.parse(last[1] ?? "");
  } catch (cause) {
    throw new Error(`verdict block is not valid JSON: ${String(cause)}`);
  }
}

export function validateVerdict(candidate: unknown, job: ReviewJob): string[] {
  const errors: string[] = [];
  if (!validateAgainstSchema(candidate)) {
    for (const error of validateAgainstSchema.errors ?? []) {
      errors.push(`${error.instancePath || "/"}: ${error.message ?? "invalid"}`);
    }
    return errors;
  }
  const verdict = candidate as {
    reviewPassId: string;
    role: string;
    mode: string;
    commitSha: string;
  };
  if (verdict.reviewPassId !== job.reviewPassId) {
    errors.push(`reviewPassId mismatch: expected ${job.reviewPassId}, got ${verdict.reviewPassId}`);
  }
  if (verdict.role !== job.role) {
    errors.push(`role mismatch: expected ${job.role}, got ${verdict.role}`);
  }
  if (verdict.mode !== job.mode) {
    errors.push(`mode mismatch: expected ${job.mode}, got ${verdict.mode}`);
  }
  if (verdict.commitSha !== job.commit) {
    errors.push(`commitSha mismatch: expected ${job.commit}, got ${verdict.commitSha}`);
  }
  return errors;
}

/**
 * Wrap evidence content in an integrity-signed envelope (K3 enforcement).
 * External tool output is untrusted; the envelope makes it impossible to
 * forge guard delimiters or alter content undetectably. renderGuarded verifies
 * before rendering.
 */
export function guardEvidence(
  path: string,
  content: string,
  key: EnvelopeKey,
  capturedAt: string,
): string {
  const envelope = wrapUntrusted(
    {
      source: "tool-output",
      label: path,
      content,
      capturedAt,
    },
    key,
  );
  return renderGuarded(envelope, key);
}

// Bounded-concurrency executor. Order of results matches order of items.
// Rejections propagate: callers wanting per-item fault isolation wrap `run`.
export async function runBatched<T, R>(
  items: readonly T[],
  limit: number,
  run: (item: T, index: number) => Promise<R>,
): Promise<R[]> {
  if (!Number.isInteger(limit) || limit < 1) {
    throw new Error("limit: positive integer required");
  }
  const results = new Array<R>(items.length);
  let next = 0;
  const workers = Array.from({ length: Math.min(limit, items.length) }, async () => {
    while (next < items.length) {
      const index = next;
      next += 1;
      const item = items[index] as T;
      results[index] = await run(item, index);
    }
  });
  await Promise.all(workers);
  return results;
}

// ── Run journal ───────────────────────────────────────────────────────────────
//
// A pass already records a verdict file when it succeeds. What was never
// recorded is WHEN each step happened, and what happened on a pass that
// produced no verdict at all — one that died before writing left a line on
// stderr and nothing else. Three properties are deliberate:
//
//   * success is earned — a pass reads `fail` until its events say otherwise,
//     so an interrupted journal never reconstructs as a green pass;
//   * the agent call is the only event that spans time, so it carries both
//     endpoints; every other event is a point and has `at` alone;
//   * events are appended while the pass runs, never batched at the end. A hung
//     pass is precisely when its journal is worth having, and a hung pass
//     flushes nothing at exit.

export type ReviewEventType =
  | "pass_start"
  | "worktree_ready"
  | "agent_call"
  | "worktree_dirty"
  | "verdict_recorded"
  | "verdict_rejected"
  | "worktree_removed"
  | "pass_end";

export interface ReviewEvent {
  readonly reviewPassId: string;
  readonly role: string;
  readonly type: ReviewEventType;
  /** When the event was recorded, ISO 8601. */
  readonly at: string;
  /** Set only by `agent_call`, the one event that spans time. */
  readonly endedAt?: string;
  readonly detail?: Readonly<Record<string, unknown>>;
}

export interface PassReplay {
  readonly reviewPassId: string;
  readonly role: string;
  readonly status: "success" | "fail";
  readonly startedAt: string;
  readonly endedAt: string | null;
  readonly reason: string;
}

/**
 * Reconstruct each pass's outcome from its events alone.
 *
 * This is the journal's acceptance test: if the status it reports here ever
 * disagrees with the verdict files on disk, one of the two is lying, and the
 * journal is the one that can be checked without trusting the reviewer.
 */
export function replayPassStatuses(events: readonly ReviewEvent[]): PassReplay[] {
  const byPass = new Map<string, ReviewEvent[]>();
  for (const event of events) {
    const bucket = byPass.get(event.reviewPassId);
    if (bucket === undefined) byPass.set(event.reviewPassId, [event]);
    else bucket.push(event);
  }

  return [...byPass.values()].map((passEvents) => {
    const first = passEvents[0] as ReviewEvent;
    const has = (type: ReviewEventType) => passEvents.some((event) => event.type === type);
    const end = passEvents.find((event) => event.type === "pass_end");
    // Earned, not assumed: the verdict, the closing event AND that event's own
    // ok are all required. verdict_recorded lands before worktree cleanup, so
    // a cleanup or emission failure closes the pass with ok=false — a journal
    // that certified such a pass would outrank the run that refused it
    // (2026-08-04 retro-K4 architecture finding). A missing ok fails closed.
    const closedOk = end?.detail?.ok === true;
    const status = has("verdict_recorded") && closedOk ? "success" : "fail";
    const reason =
      status === "success"
        ? "verdict recorded and pass closed"
        : has("worktree_dirty")
          ? "worktree dirty after the pass — the reviewer wrote to the tree"
          : has("verdict_rejected")
            ? "verdict rejected by validation"
            : // Ordered before the agent_call case on purpose: a pass whose
              // verdict landed but whose closing event never did is interrupted,
              // and saying "produced no verdict" of it would be false.
              end === undefined
              ? "pass never closed — interrupted or still running"
              : has("verdict_recorded")
                ? "pass closed failed after its verdict — cleanup or emission failed"
                : has("agent_call")
                  ? "agent ran without producing an accepted verdict"
                  : "pass closed without a verdict";
    return {
      reviewPassId: first.reviewPassId,
      role: first.role,
      status,
      startedAt: first.at,
      endedAt: end?.at ?? null,
      reason,
    };
  });
}

export interface ReplayDisagreement {
  readonly reviewPassId: string;
  readonly role: string;
  readonly replayed: PassReplay["status"];
  readonly observedOk: boolean;
  readonly reason: string;
}

/**
 * Compare replayed statuses with what this invocation observed, by identity.
 *
 * The journal is append-only across attempts sharing an output directory, so
 * it legitimately holds passes this invocation never ran. Those are history:
 * only a pass whose reviewPassId this run generated can contradict this run
 * (matching by role compared a dead attempt to its retry and manufactured a
 * disagreement — 2026-08-04 retro-K4 architecture finding).
 */
export function reconcileReplay(
  replay: readonly PassReplay[],
  observed: ReadonlyMap<string, boolean>,
): ReplayDisagreement[] {
  const disagreements: ReplayDisagreement[] = [];
  for (const pass of replay) {
    const observedOk = observed.get(pass.reviewPassId);
    if (observedOk === undefined) continue;
    if (observedOk !== (pass.status === "success")) {
      disagreements.push({
        reviewPassId: pass.reviewPassId,
        role: pass.role,
        replayed: pass.status,
        observedOk,
        reason: pass.reason,
      });
    }
  }
  return disagreements;
}

export interface ParsedJournal {
  readonly events: ReviewEvent[];
  readonly corrupted: { readonly line: number; readonly error: string }[];
}

/**
 * Parse a JSONL journal, keeping what an interruption left readable.
 *
 * A torn final append is exactly what the journal exists to survive; letting
 * it abort the whole replay poisoned every later run of the same output
 * directory (2026-08-04 retro-K4 architecture finding). Corruption is
 * reported line by line, never silently dropped — and never fatal.
 */
export function parseJournalLines(text: string): ParsedJournal {
  const events: ReviewEvent[] = [];
  const corrupted: { line: number; error: string }[] = [];
  const lines = text.split("\n");
  for (const [index, raw] of lines.entries()) {
    if (raw.trim().length === 0) continue;
    try {
      events.push(JSON.parse(raw) as ReviewEvent);
    } catch (cause) {
      corrupted.push({ line: index + 1, error: String(cause) });
    }
  }
  return { events, corrupted };
}
