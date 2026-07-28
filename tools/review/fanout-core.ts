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
    "reviewPassId, role, mode, commitSha, contractHashes ([{path, sha256}]), commands (the commands you actually ran),",
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
