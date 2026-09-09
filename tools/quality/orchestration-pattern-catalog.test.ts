import { describe, expect, test } from "bun:test";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

import {
  type CatalogFindingCode,
  checkPatternCatalogFile,
  validatePatternCatalog,
} from "./orchestration-pattern-catalog";

const validCatalog = {
  schemaVersion: "libre-ai.orchestration-pattern-catalog.v1",
  status: "research-non-normative",
  sources: [
    {
      id: "langgraph-js",
      repository: "https://github.com/langchain-ai/langgraphjs",
      revision: "c6910a9ec1c2a84b76fb79b091b0f697a567e027",
      license: "MIT",
      studiedAt: "2026-09-09",
    },
  ],
  patterns: [
    {
      id: "effect-crash-window",
      sourceId: "langgraph-js",
      upstreamSurfaces: ["persistence"],
      question: "How is a committed effect distinguished from a lost result?",
      authorities: ["orchestrator", "harness"],
      currentState: "insufficient",
      disposition: "candidate",
      candidateContracts: ["effect-attestation.v1"],
      failureScenarios: ["effect committed before result persistence"],
    },
  ],
};

function codes(value: unknown): readonly CatalogFindingCode[] {
  return validatePatternCatalog(value).map((finding) => finding.code);
}

function withPattern(overrides: Record<string, unknown>): unknown {
  return {
    ...validCatalog,
    patterns: [{ ...validCatalog.patterns[0], ...overrides }],
  };
}

describe("orchestration pattern catalogue", () => {
  test("accepts a pinned non-normative catalogue", () => {
    expect(validatePatternCatalog(validCatalog)).toEqual([]);
  });

  const invalidCases: readonly (readonly [CatalogFindingCode, unknown])[] = [
    ["catalog.schema-version", { ...validCatalog, schemaVersion: "v2" }],
    ["catalog.status", { ...validCatalog, status: "canonical" }],
    [
      "catalog.source-revision",
      { ...validCatalog, sources: [{ ...validCatalog.sources[0], revision: "main" }] },
    ],
    [
      "catalog.framework-contract-leak",
      {
        ...validCatalog,
        patterns: [
          { ...validCatalog.patterns[0], candidateContracts: ["langgraph-checkpoint.v1"] },
        ],
      },
    ],
    [
      "catalog.failure-scenario-missing",
      { ...validCatalog, patterns: [{ ...validCatalog.patterns[0], failureScenarios: [] }] },
    ],
  ];

  test.each(invalidCases)("returns %s without reflecting rejected values", (expected, value) => {
    expect(codes(value)).toContain(expected);
  });

  test("rejects a non-object and empty top-level collections", () => {
    expect(codes(null)).toEqual(["catalog.not-object"]);
    expect(codes({ ...validCatalog, sources: [] })).toContain("catalog.sources");
    expect(codes({ ...validCatalog, patterns: [] })).toContain("catalog.patterns");
  });

  test.each([
    ["catalog.source-id", [{ ...validCatalog.sources[0], id: "INVALID" }]],
    ["catalog.source-duplicate", [validCatalog.sources[0], { ...validCatalog.sources[0] }]],
    [
      "catalog.source-repository",
      [{ ...validCatalog.sources[0], repository: "git://untrusted.invalid/repository" }],
    ],
    ["catalog.source-license", [{ ...validCatalog.sources[0], license: "unknown" }]],
    ["catalog.source-studied-at", [{ ...validCatalog.sources[0], studiedAt: "2026-02-31" }]],
  ] satisfies readonly (readonly [
    CatalogFindingCode,
    unknown,
  ])[])("validates source structure with %s", (expected, sources) => {
    expect(codes({ ...validCatalog, sources })).toContain(expected);
  });

  test.each([
    ["catalog.pattern-id", { id: "INVALID" }],
    ["catalog.pattern-source", { sourceId: "absent-source" }],
    ["catalog.patterns", { upstreamSurfaces: [] }],
    ["catalog.question", { question: "This is not a question" }],
    ["catalog.authority", { authorities: ["framework"] }],
    ["catalog.current-state", { currentState: "unknown" }],
    ["catalog.disposition", { disposition: "adopt" }],
    ["catalog.candidate-contract", { candidateContracts: ["not-versioned"] }],
    ["catalog.failure-scenario-missing", { failureScenarios: [""] }],
  ] satisfies readonly (readonly [
    CatalogFindingCode,
    Record<string, unknown>,
  ])[])("validates pattern structure with %s", (expected, overrides) => {
    expect(codes(withPattern(overrides))).toContain(expected);
  });

  test("rejects duplicate pattern identifiers", () => {
    expect(
      codes({
        ...validCatalog,
        patterns: [validCatalog.patterns[0], { ...validCatalog.patterns[0] }],
      }),
    ).toContain("catalog.pattern-duplicate");
  });

  test.each([
    "missions",
    "orchestrator",
    "harness",
    "proof-artifact",
    "worker-internal",
  ])("accepts the closed authority %s", (authority) => {
    expect(validatePatternCatalog(withPattern({ authorities: [authority] }))).toEqual([]);
  });

  test.each([
    "covered",
    "partial",
    "insufficient",
    "missing",
    "intentionally-refused",
    "worker-internal",
  ])("accepts the closed current state %s", (currentState) => {
    expect(validatePatternCatalog(withPattern({ currentState }))).toEqual([]);
  });

  test.each([
    "candidate",
    "investigate",
    "refused",
    "worker-internal",
  ])("accepts the closed disposition %s", (disposition) => {
    const candidateContracts =
      disposition === "refused" || disposition === "worker-internal"
        ? []
        : ["effect-attestation.v1"];
    expect(validatePatternCatalog(withPattern({ disposition, candidateContracts }))).toEqual([]);
  });

  test("refused and worker-internal patterns cannot nominate canonical contracts", () => {
    expect(
      codes(withPattern({ disposition: "refused", candidateContracts: ["effect-attestation.v1"] })),
    ).toContain("catalog.candidate-contract");
    expect(
      codes(
        withPattern({
          disposition: "worker-internal",
          candidateContracts: ["effect-attestation.v1"],
        }),
      ),
    ).toContain("catalog.candidate-contract");
  });

  test("findings never reflect rejected content", () => {
    const secret = "do-not-reflect-this-value";
    const findings = validatePatternCatalog({ ...validCatalog, status: secret });
    expect(JSON.stringify(findings)).not.toContain(secret);
  });

  test("file gate refuses malformed JSON with a closed code", async () => {
    const directory = await mkdtemp(join(tmpdir(), "libre-ai-pattern-catalog-"));
    try {
      const path = join(directory, "catalog.json");
      await writeFile(path, "{");
      expect(await checkPatternCatalogFile(path)).toEqual([
        { code: "catalog.invalid-json", path: "/" },
      ]);
    } finally {
      await rm(directory, { recursive: true });
    }
  });
});
