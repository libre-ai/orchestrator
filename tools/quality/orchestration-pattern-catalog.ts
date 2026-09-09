export type CatalogFindingCode =
  | "catalog.not-object"
  | "catalog.invalid-json"
  | "catalog.schema-version"
  | "catalog.status"
  | "catalog.sources"
  | "catalog.patterns"
  | "catalog.source-id"
  | "catalog.source-duplicate"
  | "catalog.source-repository"
  | "catalog.source-revision"
  | "catalog.source-license"
  | "catalog.source-studied-at"
  | "catalog.pattern-id"
  | "catalog.pattern-duplicate"
  | "catalog.pattern-source"
  | "catalog.question"
  | "catalog.authority"
  | "catalog.current-state"
  | "catalog.disposition"
  | "catalog.candidate-contract"
  | "catalog.framework-contract-leak"
  | "catalog.failure-scenario-missing";

export interface CatalogFinding {
  readonly code: CatalogFindingCode;
  readonly path: string;
}

const SCHEMA_VERSION = "libre-ai.orchestration-pattern-catalog.v1";
const STATUS = "research-non-normative";
const IDENTIFIER_PATTERN = /^[a-z][a-z0-9-]{2,63}$/;
const CONTRACT_PATTERN = /^[a-z][a-z0-9-]*\.v[1-9][0-9]*$/;
const REPOSITORY_PATTERN = /^https:\/\/github\.com\/[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/;
const REVISION_PATTERN = /^[0-9a-f]{40}$/;
const FRAMEWORK_PATTERN = /lang(?:graph|chain|smith)/i;

const AUTHORITIES = new Set([
  "missions",
  "orchestrator",
  "harness",
  "proof-artifact",
  "worker-internal",
]);
const CURRENT_STATES = new Set([
  "covered",
  "partial",
  "insufficient",
  "missing",
  "intentionally-refused",
  "worker-internal",
]);
const DISPOSITIONS = new Set(["candidate", "investigate", "refused", "worker-internal"]);

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isNonEmptyString(value: unknown): value is string {
  return typeof value === "string" && value.trim().length > 0;
}

function isIsoDate(value: unknown): value is string {
  if (typeof value !== "string" || !/^\d{4}-\d{2}-\d{2}$/.test(value)) return false;
  const date = new Date(`${value}T00:00:00.000Z`);
  return !Number.isNaN(date.valueOf()) && date.toISOString().slice(0, 10) === value;
}

function finding(code: CatalogFindingCode, path: string): CatalogFinding {
  return { code, path };
}

function validateSources(value: unknown, findings: CatalogFinding[]): ReadonlySet<string> {
  const sourceIds = new Set<string>();
  if (!Array.isArray(value) || value.length === 0) {
    findings.push(finding("catalog.sources", "/sources"));
    return sourceIds;
  }

  value.forEach((source, index) => {
    const path = `/sources/${index}`;
    if (!isRecord(source)) {
      findings.push(finding("catalog.source-id", path));
      return;
    }

    const id = source.id;
    if (typeof id !== "string" || !IDENTIFIER_PATTERN.test(id)) {
      findings.push(finding("catalog.source-id", `${path}/id`));
    } else if (sourceIds.has(id)) {
      findings.push(finding("catalog.source-duplicate", `${path}/id`));
    } else {
      sourceIds.add(id);
    }

    if (typeof source.repository !== "string" || !REPOSITORY_PATTERN.test(source.repository)) {
      findings.push(finding("catalog.source-repository", `${path}/repository`));
    }
    if (typeof source.revision !== "string" || !REVISION_PATTERN.test(source.revision)) {
      findings.push(finding("catalog.source-revision", `${path}/revision`));
    }
    if (source.license !== "MIT") {
      findings.push(finding("catalog.source-license", `${path}/license`));
    }
    if (!isIsoDate(source.studiedAt)) {
      findings.push(finding("catalog.source-studied-at", `${path}/studiedAt`));
    }
  });
  return sourceIds;
}

function validateStringArray(
  value: unknown,
  code: CatalogFindingCode,
  path: string,
  findings: CatalogFinding[],
): readonly string[] | null {
  if (!Array.isArray(value) || value.length === 0) {
    findings.push(finding(code, path));
    return null;
  }
  const strings: string[] = [];
  value.forEach((item, index) => {
    if (!isNonEmptyString(item)) {
      findings.push(finding(code, `${path}/${index}`));
    } else {
      strings.push(item);
    }
  });
  return strings;
}

function validatePatterns(
  value: unknown,
  sourceIds: ReadonlySet<string>,
  findings: CatalogFinding[],
): void {
  if (!Array.isArray(value) || value.length === 0) {
    findings.push(finding("catalog.patterns", "/patterns"));
    return;
  }

  const patternIds = new Set<string>();
  value.forEach((pattern, index) => {
    const path = `/patterns/${index}`;
    if (!isRecord(pattern)) {
      findings.push(finding("catalog.pattern-id", path));
      return;
    }

    const id = pattern.id;
    if (typeof id !== "string" || !IDENTIFIER_PATTERN.test(id)) {
      findings.push(finding("catalog.pattern-id", `${path}/id`));
    } else if (patternIds.has(id)) {
      findings.push(finding("catalog.pattern-duplicate", `${path}/id`));
    } else {
      patternIds.add(id);
    }

    if (typeof pattern.sourceId !== "string" || !sourceIds.has(pattern.sourceId)) {
      findings.push(finding("catalog.pattern-source", `${path}/sourceId`));
    }

    validateStringArray(
      pattern.upstreamSurfaces,
      "catalog.patterns",
      `${path}/upstreamSurfaces`,
      findings,
    );

    if (
      typeof pattern.question !== "string" ||
      pattern.question.trim().length < 2 ||
      !pattern.question.trimEnd().endsWith("?")
    ) {
      findings.push(finding("catalog.question", `${path}/question`));
    }

    const authorities = validateStringArray(
      pattern.authorities,
      "catalog.authority",
      `${path}/authorities`,
      findings,
    );
    authorities?.forEach((authority, authorityIndex) => {
      if (!AUTHORITIES.has(authority)) {
        findings.push(finding("catalog.authority", `${path}/authorities/${authorityIndex}`));
      }
    });

    if (typeof pattern.currentState !== "string" || !CURRENT_STATES.has(pattern.currentState)) {
      findings.push(finding("catalog.current-state", `${path}/currentState`));
    }
    if (typeof pattern.disposition !== "string" || !DISPOSITIONS.has(pattern.disposition)) {
      findings.push(finding("catalog.disposition", `${path}/disposition`));
    }

    const candidateContracts = pattern.candidateContracts;
    if (!Array.isArray(candidateContracts)) {
      findings.push(finding("catalog.candidate-contract", `${path}/candidateContracts`));
    } else {
      candidateContracts.forEach((contract, contractIndex) => {
        const contractPath = `${path}/candidateContracts/${contractIndex}`;
        if (typeof contract !== "string" || !CONTRACT_PATTERN.test(contract)) {
          findings.push(finding("catalog.candidate-contract", contractPath));
          return;
        }
        if (FRAMEWORK_PATTERN.test(contract)) {
          findings.push(finding("catalog.framework-contract-leak", contractPath));
        }
      });
      if (
        (pattern.disposition === "refused" || pattern.disposition === "worker-internal") &&
        candidateContracts.length > 0
      ) {
        findings.push(finding("catalog.candidate-contract", `${path}/candidateContracts`));
      }
    }

    validateStringArray(
      pattern.failureScenarios,
      "catalog.failure-scenario-missing",
      `${path}/failureScenarios`,
      findings,
    );
  });
}

export function validatePatternCatalog(value: unknown): readonly CatalogFinding[] {
  if (!isRecord(value)) return [finding("catalog.not-object", "/")];

  const findings: CatalogFinding[] = [];
  if (value.schemaVersion !== SCHEMA_VERSION) {
    findings.push(finding("catalog.schema-version", "/schemaVersion"));
  }
  if (value.status !== STATUS) findings.push(finding("catalog.status", "/status"));

  const sourceIds = validateSources(value.sources, findings);
  validatePatterns(value.patterns, sourceIds, findings);
  return findings;
}

export async function checkPatternCatalogFile(path: string): Promise<readonly CatalogFinding[]> {
  try {
    const text = await Bun.file(path).text();
    return validatePatternCatalog(JSON.parse(text));
  } catch {
    return [finding("catalog.invalid-json", "/")];
  }
}

if (import.meta.main) {
  const findings = await checkPatternCatalogFile(
    "docs/research/orchestration-patterns/catalog.v1.json",
  );
  for (const item of findings) console.error(`${item.code} ${item.path}`);
  if (findings.length > 0) process.exit(1);
  console.log("Orchestration pattern catalogue verified");
}
