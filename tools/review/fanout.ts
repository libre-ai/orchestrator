import { createHash, getRandomValues, randomUUID } from "node:crypto";
import { existsSync } from "node:fs";
import { appendFile, mkdir, mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import type { EnvelopeKey } from "@libre-ai/envelope";
import {
  buildJobs,
  buildPrompt,
  dedupeJobs,
  extractVerdict,
  guardEvidence,
  parseJournalLines,
  parsePlan,
  type ReviewEvent,
  type ReviewEventType,
  type ReviewJob,
  type ReviewPlan,
  reconcileReplay,
  replayPassStatuses,
  runBatched,
  validateVerdict,
} from "./fanout-core";

type Emit = (
  type: ReviewEventType,
  detail?: Record<string, unknown>,
  span?: { readonly startedAt: string },
) => Promise<void>;

/**
 * Append-only JSONL journal, written while passes run rather than at the end.
 *
 * One line per event, appended in `a` mode: concurrent passes each write a
 * single short line, which POSIX append keeps atomic at this size. Nothing here
 * buffers — a hung or killed pass is exactly when the journal earns its keep,
 * and it would flush nothing at exit.
 */
function journalWriter(path: string, job: ReviewJob): Emit {
  return async (type, detail, span) => {
    const now = new Date().toISOString();
    const event: ReviewEvent = {
      reviewPassId: job.reviewPassId,
      role: job.role,
      type,
      at: span?.startedAt ?? now,
      ...(span === undefined ? {} : { endedAt: now }),
      ...(detail === undefined ? {} : { detail }),
    };
    await appendFile(path, `${JSON.stringify(event)}\n`);
  };
}

// Review fan-out orchestrator (docs/reviews/AGENT-REVIEW-PROTOCOL.md,
// "Orchestration mechanics"). Launches one pi review pass per role against an
// immutable commit, in parallel batches, with deduplication and a
// machine-readable verdict envelope per pass.
//
// Usage: bun tools/review/fanout.ts <plan.json> [--dry-run] [--force]

interface JobResult {
  job: ReviewJob;
  ok: boolean;
  errors: string[];
  durationMs: number;
}

async function git(args: string[], cwd = "."): Promise<string> {
  const proc = Bun.spawn(["git", ...args], { cwd, stdout: "pipe", stderr: "pipe" });
  const [stdout, stderr, exitCode] = await Promise.all([
    new Response(proc.stdout).text(),
    new Response(proc.stderr).text(),
    proc.exited,
  ]);
  if (exitCode !== 0) {
    throw new Error(`git ${args.join(" ")} failed (${exitCode}): ${stderr.trim()}`);
  }
  return stdout;
}

async function buildEvidenceDigest(plan: ReviewPlan, key: EnvelopeKey): Promise<string> {
  if (plan.evidence.length === 0) return "(no shared evidence files declared)";
  const sections: string[] = [];
  for (const path of plan.evidence) {
    const content = await git(["show", `${plan.commit}:${path}`]);
    const sha256 = createHash("sha256").update(content).digest("hex");
    // K3 enforcement: wrap evidence in integrity envelope before the prompt.
    // guardEvidence verifies undetectability of alteration, escapes delimiter code points.
    const guarded = guardEvidence(path, content, key, new Date().toISOString());
    sections.push(`### ${path} (sha256 ${sha256})\n\n${guarded}`);
  }
  return sections.join("\n\n");
}

async function runJob(
  plan: ReviewPlan,
  job: ReviewJob,
  worktreeBase: string,
  evidenceDigest: string,
  emit: Emit,
): Promise<JobResult> {
  const startedAt = new Date().toISOString();
  const start = performance.now();
  const worktree = join(worktreeBase, job.role);
  const errors: string[] = [];
  await emit("pass_start", { commit: job.commit, mode: job.mode });
  try {
    await git(["worktree", "add", "--detach", worktree, job.commit]);
    await emit("worktree_ready");
    try {
      const prompt = buildPrompt(job, evidenceDigest);
      const agentStartedAt = new Date().toISOString();
      const proc = Bun.spawn(
        [
          "pi",
          "--print",
          "--provider",
          plan.provider,
          "--model",
          plan.model,
          "--thinking",
          plan.thinking,
          prompt,
        ],
        { cwd: worktree, stdout: "pipe", stderr: "pipe" },
      );
      const [stdout, stderr, exitCode] = await Promise.all([
        new Response(proc.stdout).text(),
        new Response(proc.stderr).text(),
        proc.exited,
      ]);
      // The agent call is the only step that spans time, so it is the only
      // event carrying both endpoints.
      await emit(
        "agent_call",
        { exitCode, provider: plan.provider, model: plan.model, thinking: plan.thinking },
        { startedAt: agentStartedAt },
      );
      if (exitCode !== 0) {
        errors.push(`pi exited ${exitCode}: ${stderr.trim().slice(0, 500)}`);
      } else {
        const dirty = (await git(["status", "--porcelain"], worktree)).trim();
        if (dirty.length > 0) {
          errors.push(`worktree dirty after review pass, pass invalid:\n${dirty}`);
          // A reviewer that wrote to the tree it was reviewing is a breach, not
          // a finding: the write already happened and no rerun undoes it.
          await emit("worktree_dirty", { paths: dirty.split("\n").length });
        } else {
          const verdict = extractVerdict(stdout);
          errors.push(...validateVerdict(verdict, job));
          if (errors.length > 0) await emit("verdict_rejected", { violations: errors.length });
          if (errors.length === 0) {
            const record = {
              ...(verdict as Record<string, unknown>),
              runner: {
                orchestrator: "tools/review/fanout.ts",
                provider: plan.provider,
                model: plan.model,
                thinking: plan.thinking,
                startedAt,
                durationMs: Math.round(performance.now() - start),
              },
            };
            await Bun.write(job.outputPath, `${JSON.stringify(record, null, 2)}\n`);
            await emit("verdict_recorded", { outputPath: job.outputPath });
          }
        }
      }
    } finally {
      await git(["worktree", "remove", "--force", worktree]);
      await emit("worktree_removed");
    }
  } catch (cause) {
    errors.push(String(cause));
  }
  const durationMs = Math.round(performance.now() - start);
  await emit("pass_end", { ok: errors.length === 0, durationMs });
  return { job, ok: errors.length === 0, errors, durationMs };
}

/**
 * Replay the journal and check it against what the run itself observed.
 *
 * The journal is only worth keeping if it can stand in for the verdict files,
 * so every run verifies that claim: if the replayed status disagrees with the
 * in-memory result, the journal is wrong and says so loudly rather than being
 * trusted later by an auditor who has nothing else left to read.
 */
async function reportJournal(journalPath: string, results: readonly JobResult[]): Promise<void> {
  const journal = Bun.file(journalPath);
  if (!(await journal.exists())) {
    console.error(`journal missing at ${journalPath} — the run produced no replayable trace`);
    return;
  }
  const { events, corrupted } = parseJournalLines(await journal.text());
  for (const entry of corrupted) {
    console.error(`journal line ${entry.line} unreadable (${entry.error}) — kept, not trusted`);
  }
  const replay = replayPassStatuses(events);
  console.log(`journal ${journalPath} — ${events.length} events across ${replay.length} pass(es)`);

  const observed = new Map(results.map((result) => [result.job.reviewPassId, result.ok]));
  for (const disagreement of reconcileReplay(replay, observed)) {
    console.error(
      `journal disagrees with the run on ${disagreement.role} (${disagreement.reviewPassId}): ` +
        `replayed ${disagreement.replayed}, observed ${
          disagreement.observedOk ? "success" : "fail"
        } — ${disagreement.reason}`,
    );
  }
}

async function main(): Promise<void> {
  const args = process.argv.slice(2);
  const dryRun = args.includes("--dry-run");
  const force = args.includes("--force");
  const planPath = args.find((arg) => !arg.startsWith("--"));
  if (planPath === undefined) {
    console.error("usage: bun tools/review/fanout.ts <plan.json> [--dry-run] [--force]");
    process.exit(2);
  }
  // The protocol doc is not vendored in-repo: it resolves from the governance
  // git-dep (root package.json devDependency), never from a bare repo-root path.
  const protocolPath = "node_modules/@libre-ai/governance/docs/reviews/AGENT-REVIEW-PROTOCOL.md";
  if (!existsSync(protocolPath)) {
    console.error(
      `run from the repository root with dependencies installed (${protocolPath} not found)`,
    );
    process.exit(2);
  }

  const plan = parsePlan(await Bun.file(planPath).json());
  await git(["cat-file", "-e", `${plan.commit}^{commit}`]);

  const jobs = buildJobs(plan, () => `rp-${plan.commit.slice(0, 7)}-${randomUUID()}`);
  const { run, skipped } = dedupeJobs(jobs, (job) => existsSync(job.outputPath), force);

  if (force) {
    const collisions = run.filter((job) => existsSync(job.outputPath));
    if (collisions.length > 0) {
      console.error(
        "refusing to overwrite recorded verdicts (immutable audit records); archive them first:",
      );
      for (const job of collisions) console.error(`- ${job.outputPath}`);
      process.exit(2);
    }
  }

  for (const job of skipped) {
    console.log(`skip ${job.role}: verdict already recorded at ${job.outputPath}`);
  }
  if (run.length === 0) {
    console.log("nothing to run");
    return;
  }
  console.log(
    `${run.length} pass(es) on ${plan.commit.slice(0, 7)} — concurrency ${plan.concurrency}, ${plan.provider}/${plan.model}:${plan.thinking}`,
  );
  if (dryRun) {
    for (const job of run) console.log(`- ${job.role} → ${job.outputPath}`);
    return;
  }

  await mkdir(plan.outputDir, { recursive: true });
  // Ephemeral per-run HMAC key for evidence integrity (K3 intra-run boundary).
  // Held in memory only, never logged or persisted. Identifies this orchestration run
  // and binds all evidence to it; an altered or stripped envelope is offline-verifiable.
  const envelopeKey: EnvelopeKey = {
    id: `fanout-run-${plan.commit.slice(0, 7)}-${randomUUID()}`,
    secret: getRandomValues(new Uint8Array(32)),
  };
  const evidenceDigest = await buildEvidenceDigest(plan, envelopeKey);
  const worktreeBase = await mkdtemp(join(tmpdir(), "review-fanout-"));
  try {
    const journalPath = join(plan.outputDir, "events.jsonl");
    const results = await runBatched(run, plan.concurrency, (job) =>
      runJob(plan, job, worktreeBase, evidenceDigest, journalWriter(journalPath, job)),
    );
    await reportJournal(journalPath, results);
    let failed = 0;
    for (const result of results) {
      if (result.ok) {
        console.log(`ok   ${result.job.role} (${result.durationMs} ms) → ${result.job.outputPath}`);
      } else {
        failed += 1;
        console.error(`FAIL ${result.job.role} (${result.durationMs} ms)`);
        for (const error of result.errors) console.error(`     ${error}`);
      }
    }
    if (failed > 0) {
      console.error(`${failed}/${results.length} pass(es) failed`);
      process.exit(1);
    }
  } finally {
    await rm(worktreeBase, { recursive: true, force: true });
    await git(["worktree", "prune"]);
  }
}

await main();
