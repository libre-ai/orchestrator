import { createHash, randomUUID } from "node:crypto";
import { existsSync } from "node:fs";
import { mkdir, mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  buildJobs,
  buildPrompt,
  dedupeJobs,
  extractVerdict,
  parsePlan,
  type ReviewJob,
  type ReviewPlan,
  runBatched,
  validateVerdict,
} from "./fanout-core";

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

async function buildEvidenceDigest(plan: ReviewPlan): Promise<string> {
  if (plan.evidence.length === 0) return "(no shared evidence files declared)";
  const sections: string[] = [];
  for (const path of plan.evidence) {
    const content = await git(["show", `${plan.commit}:${path}`]);
    const sha256 = createHash("sha256").update(content).digest("hex");
    sections.push(`### ${path} (sha256 ${sha256})\n\n${content.trimEnd()}`);
  }
  return sections.join("\n\n");
}

async function runJob(
  plan: ReviewPlan,
  job: ReviewJob,
  worktreeBase: string,
  evidenceDigest: string,
): Promise<JobResult> {
  const startedAt = new Date().toISOString();
  const start = performance.now();
  const worktree = join(worktreeBase, job.role);
  const errors: string[] = [];
  try {
    await git(["worktree", "add", "--detach", worktree, job.commit]);
    try {
      const prompt = buildPrompt(job, evidenceDigest);
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
      if (exitCode !== 0) {
        errors.push(`pi exited ${exitCode}: ${stderr.trim().slice(0, 500)}`);
      } else {
        const dirty = (await git(["status", "--porcelain"], worktree)).trim();
        if (dirty.length > 0) {
          errors.push(`worktree dirty after review pass, pass invalid:\n${dirty}`);
        } else {
          const verdict = extractVerdict(stdout);
          errors.push(...validateVerdict(verdict, job));
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
          }
        }
      }
    } finally {
      await git(["worktree", "remove", "--force", worktree]);
    }
  } catch (cause) {
    errors.push(String(cause));
  }
  return {
    job,
    ok: errors.length === 0,
    errors,
    durationMs: Math.round(performance.now() - start),
  };
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
  if (!existsSync("docs/reviews/AGENT-REVIEW-PROTOCOL.md")) {
    console.error("run from the repository root (docs/reviews/AGENT-REVIEW-PROTOCOL.md not found)");
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
  const evidenceDigest = await buildEvidenceDigest(plan);
  const worktreeBase = await mkdtemp(join(tmpdir(), "review-fanout-"));
  try {
    const results = await runBatched(run, plan.concurrency, (job) =>
      runJob(plan, job, worktreeBase, evidenceDigest),
    );
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
