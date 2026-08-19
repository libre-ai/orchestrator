// Mechanizes the 2026-08-18 owner arbitrage on docs/apps/memory.md: the
// DRAFT banner is restored verbatim at the top of the product doc, and the
// rest of the doc tracks node_modules/@libre-ai/governance's draft spec
// byte-for-byte except at the divergence points documented below. Any other
// difference -- on either side -- fails this check.
//
// Reads the governance draft from the installed git-dep only; never fetches
// over the network.

const GOVERNANCE_DRAFT_PATH =
  "node_modules/@libre-ai/governance/docs/parity/draft-specs/DRAFT-SPEC-memory.md";
const LOCAL_SPEC_PATH = "docs/apps/memory.md";

const DRAFT_BANNER =
  "**DRAFT — Specification Lock pending.** This specification is a locked contract (immutable after pronouncement) under the orchestrator Specification Lock (ADR-0011). Changes to memory schema, classification rules (K2), or envelope integrity (K3) require a new ADR, independent security review, and owner approval. Governance path: Gate → ADR → Specification Lock → Release.";

interface WordingDivergence {
  readonly description: string;
  readonly sourceText: string;
  readonly localText: string;
}

// Every small, intentional wording divergence between the governance draft
// and the product doc, as of 2026-08-18 (the "promotion" commit 13164a5
// finalized draft hedges; one line also fixes a typo present upstream).
// A change to either side without updating this list fails the check.
const WORDING_DIVERGENCES: readonly WordingDivergence[] = [
  {
    description: "'Candidate:' hedge removed from the JSON-serialization bullet",
    sourceText:
      "- Candidate: canonical JSON serialization of memory for digest determinism (contract: MemoryRecord v1).",
    localText:
      "- Canonical JSON serialization of memory for digest determinism (contract: MemoryRecord v1).",
  },
  {
    description: "'(for Gate B candidate)' removed from the Proof-of-concept heading",
    sourceText: "**Proof of concept (for Gate B candidate):**",
    localText: "**Proof of concept:**",
  },
  {
    description: "'(candidate)' removed from the milestone-gate sentence",
    sourceText: "Milestone gate (candidate): ExportMemories contract",
    localText: "Milestone gate: ExportMemories contract",
  },
  {
    description: "local fixes a missing closing parenthesis present in the governance draft",
    sourceText: "before v2 is enabled (old vectors remain until replacement.\n- RLS policy",
    localText: "before v2 is enabled (old vectors remain until replacement).\n- RLS policy",
  },
  {
    description:
      "the multiplication sign was normalized to '+' and the unsourced recall-loss figure softened",
    sourceText:
      "- 10× export snapshots (variable time windows); digests are deterministic; lineage verifiable.\n" +
      "- Embedding service failure simulation: 1 hour downtime; queries fall back to keyword search with <10% recall loss.",
    localText:
      "- 10+ export snapshots (variable time windows); digests are deterministic; lineage verifiable.\n" +
      "- Embedding service failure simulation: 1 hour downtime; queries fall back to keyword search with minimal recall loss.",
  },
];

interface DroppedSection {
  readonly description: string;
  /** Exact line marking the start of the governance-only span (inclusive). */
  readonly startAnchor: string;
  /** Exact line marking the end of the span (exclusive); null = end of file. */
  readonly endAnchor: string | null;
}

// Governance-draft-only spans the product doc never carries at all.
const DROPPED_SECTIONS: readonly DroppedSection[] = [
  {
    description: "Resource floor section (infra sizing detail kept only in the governance draft)",
    startAnchor: "## Resource floor",
    endAnchor: "## Contracts",
  },
  {
    description: "Critical-gaps analysis",
    startAnchor: "Critical gaps (not closed by v1):",
    endAnchor: "## Work packages",
  },
  {
    description:
      "Benchmark/parity table, the DRAFT banner in its original position, and the " +
      "owner-review summary -- the banner is restored at the top of the product doc instead",
    startAnchor: "\n\n---\n\n# Benchmark & Feature Parity Table",
    endAnchor: null,
  },
];

// The Contracts bullet list: the governance draft cites schema paths as
// already canonical; the product doc says "to be authored" -- neither is
// true yet in either repository, and the two lists are kept intentionally
// distinct rather than mechanically derived from one another. Only
// presence and line count are asserted, not content equality.
const CONTRACTS_SECTION_HEADING = "## Contracts";
const CONTRACTS_SOURCE_FIRST_LINE_PREFIX = "- **Memory Record v1**";
const CONTRACTS_SOURCE_LINE_COUNT = 8;
const CONTRACTS_LOCAL_FIRST_LINE_PREFIX = "- **K3 Envelope v1**";
const CONTRACTS_LOCAL_LINE_COUNT = 10;

const problems: string[] = [];

function requireUniqueIndex(text: string, needle: string, context: string): number {
  const first = text.indexOf(needle);
  if (first === -1) {
    problems.push(`${context}: anchor not found (upstream text moved -- update this script)`);
    return -1;
  }
  const second = text.indexOf(needle, first + needle.length);
  if (second !== -1) {
    problems.push(`${context}: anchor is not unique in the file`);
    return -1;
  }
  return first;
}

/** Replace a unique occurrence of `needle` with `token`, recording a problem if absent or duplicated. */
function stripOnce(text: string, needle: string, token: string, context: string): string {
  const index = requireUniqueIndex(text, needle, context);
  if (index === -1) return text;
  return text.slice(0, index) + token + text.slice(index + needle.length);
}

/** Take `count` newline-joined lines starting at the line beginning with `firstLinePrefix`. */
function takeLines(
  text: string,
  firstLinePrefix: string,
  count: number,
  context: string,
): string | null {
  const lines = text.split("\n");
  const startIndex = lines.findIndex((line) => line.startsWith(firstLinePrefix));
  if (startIndex === -1) {
    problems.push(`${context}: no line starts with -- "${firstLinePrefix}"`);
    return null;
  }
  return lines.slice(startIndex, startIndex + count).join("\n");
}

async function main(): Promise<void> {
  const sourceFile = Bun.file(GOVERNANCE_DRAFT_PATH);
  if (!(await sourceFile.exists())) {
    console.error(
      `${GOVERNANCE_DRAFT_PATH} not found -- the @libre-ai/governance git-dep is not installed; ` +
        'run "bun install" from the repository root',
    );
    process.exit(2);
  }
  const localFile = Bun.file(LOCAL_SPEC_PATH);
  if (!(await localFile.exists())) {
    console.error(`${LOCAL_SPEC_PATH} not found`);
    process.exit(2);
  }

  let source = await sourceFile.text();
  let local = await localFile.text();

  // 1. The DRAFT banner is present, verbatim, in both files. In the
  //    governance draft it sits in its original position, inside the
  //    trailing span dropped by DROPPED_SECTIONS below -- no normalization
  //    needed there. In the product doc it has been relocated to the head
  //    (nothing but the H1 title precedes it), so it is removed here,
  //    together with its trailing blank line, collapsing the doc back to
  //    exactly the shape it has upstream at that position (title directly
  //    followed by the metadata bullets).
  const bannerInLocalIndex = requireUniqueIndex(local, DRAFT_BANNER, "DRAFT banner (product doc)");
  requireUniqueIndex(source, DRAFT_BANNER, "DRAFT banner (governance draft)");
  if (bannerInLocalIndex !== -1) {
    const before = local.slice(0, bannerInLocalIndex);
    const nonBlankLinesBefore = before.split("\n").filter((line) => line.trim().length > 0);
    if (nonBlankLinesBefore.length !== 1 || !nonBlankLinesBefore[0]?.startsWith("# Memory")) {
      problems.push(
        "DRAFT banner is not at the head of docs/apps/memory.md -- only the H1 title may precede it",
      );
    }
  }
  local = stripOnce(local, `${DRAFT_BANNER}\n\n`, "", "DRAFT banner (product doc, normalize)");

  // 2. Small wording divergences: both sides must still carry their known
  //    text; normalize both to the same token so the final byte-exact
  //    comparison does not flag them.
  WORDING_DIVERGENCES.forEach((divergence, index) => {
    const token = `[[WORDING${index}]]`;
    source = stripOnce(
      source,
      divergence.sourceText,
      token,
      `${divergence.description} (governance draft)`,
    );
    local = stripOnce(
      local,
      divergence.localText,
      token,
      `${divergence.description} (product doc)`,
    );
  });

  // 3. The Contracts bullet list: assert presence and line count on both
  //    sides, then normalize both to the same token (content is
  //    intentionally not compared -- see the comment above the constants).
  const sourceContracts = takeLines(
    source,
    CONTRACTS_SOURCE_FIRST_LINE_PREFIX,
    CONTRACTS_SOURCE_LINE_COUNT,
    "Contracts bullets (governance draft)",
  );
  const localContracts = takeLines(
    local,
    CONTRACTS_LOCAL_FIRST_LINE_PREFIX,
    CONTRACTS_LOCAL_LINE_COUNT,
    "Contracts bullets (product doc)",
  );
  if (!source.includes(CONTRACTS_SECTION_HEADING)) {
    problems.push("Contracts bullets (governance draft): '## Contracts' heading not found");
  }
  if (!local.includes(CONTRACTS_SECTION_HEADING)) {
    problems.push("Contracts bullets (product doc): '## Contracts' heading not found");
  }
  if (sourceContracts !== null) source = source.replace(sourceContracts, "[[CONTRACTS]]");
  if (localContracts !== null) local = local.replace(localContracts, "[[CONTRACTS]]");

  // 4. Sections the product doc never carries: assert the span exists in the
  //    governance draft, then drop it there so the byte-exact pass below
  //    does not expect it locally.
  for (const section of DROPPED_SECTIONS) {
    const startIndex = requireUniqueIndex(
      source,
      section.startAnchor,
      `${section.description} (start)`,
    );
    if (startIndex === -1) continue;
    if (section.endAnchor === null) {
      // Dropped through end of file: trim to the anchor and restore exactly
      // one trailing newline, matching how the product doc ends locally.
      source = `${source.slice(0, startIndex).replace(/\s+$/, "")}\n`;
      continue;
    }
    const endIndex = source.indexOf(section.endAnchor, startIndex);
    if (endIndex === -1) {
      problems.push(`${section.description}: end anchor not found after start`);
      continue;
    }
    source = source.slice(0, startIndex) + source.slice(endIndex);
  }

  // 5. Whatever remains must be byte-exact identical.
  if (problems.length === 0 && source !== local) {
    const sourceLines = source.split("\n");
    const localLines = local.split("\n");
    const maxLines = Math.max(sourceLines.length, localLines.length);
    let firstDiffLine = -1;
    for (let i = 0; i < maxLines; i += 1) {
      if (sourceLines[i] !== localLines[i]) {
        firstDiffLine = i;
        break;
      }
    }
    problems.push(
      "governance draft and product doc diverge beyond the documented delta, " +
        `first at normalized line ${firstDiffLine + 1}:\n` +
        `  governance: ${JSON.stringify(sourceLines[firstDiffLine] ?? "<EOF>")}\n` +
        `  product doc: ${JSON.stringify(localLines[firstDiffLine] ?? "<EOF>")}`,
    );
  }

  if (problems.length > 0) {
    console.error(`${LOCAL_SPEC_PATH} is out of sync with ${GOVERNANCE_DRAFT_PATH}:`);
    for (const problem of problems) console.error(`- ${problem}`);
    console.error(
      "\nEither restore the documented delta above, or -- for a legitimate upstream change -- " +
        "update tools/quality/check-memory-draft-sync.ts to document the new divergence.",
    );
    process.exit(1);
  }

  console.log(
    `${LOCAL_SPEC_PATH} in sync with ${GOVERNANCE_DRAFT_PATH} ` +
      `(DRAFT banner restored, ${WORDING_DIVERGENCES.length} wording divergence(s), ` +
      `${DROPPED_SECTIONS.length} dropped section(s) -- byte-exact elsewhere)`,
  );
}

await main();
