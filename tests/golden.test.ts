import { describe, expect, test } from "bun:test";
import { resolve } from "node:path";
import type { LibreAiAgentHandoffV1 } from "../../../packages/contracts/src/generated/agent-handoff.v1";
import type { LibreAiOrchestratorEventV1 } from "../../../packages/contracts/src/generated/orchestrator-event.v1";
import { loadCanonicalContractRegistry } from "../../../packages/contracts/src/registry";

type GoldenCase = {
  name: string;
  fixture: string;
  types: LibreAiOrchestratorEventV1["type"][];
  streamSha256: string;
};
type Metadata = { profile: string; cases: GoldenCase[] };
type Loaded = GoldenCase & { stream: string; events: LibreAiOrchestratorEventV1[] };

const path = (name: string) => resolve(import.meta.dir, "../fixtures", name);
const registry = await loadCanonicalContractRegistry();
const metadata = (await Bun.file(path("golden.v1.json")).json()) as Metadata;
const rawHandoff: unknown = await Bun.file(path("handoff.valid.json")).json();
registry.assert("agent-handoff.v1.schema.json", rawHandoff);
const handoff = rawHandoff as LibreAiAgentHandoffV1;
const cases: Loaded[] = await Promise.all(
  metadata.cases.map(async (item) => {
    const stream = await Bun.file(path(item.fixture)).text();
    const raw: unknown[] = stream
      .trimEnd()
      .split("\n")
      .map((line) => JSON.parse(line) as unknown);
    for (const event of raw) registry.assert("orchestrator-event.v1.schema.json", event);
    return { ...item, stream, events: raw as LibreAiOrchestratorEventV1[] };
  }),
);

const sha256 = (value: string) => new Bun.CryptoHasher("sha256").update(value).digest("hex");
function expectedId(event: LibreAiOrchestratorEventV1): string {
  const material = [
    "libre-ai.orchestrator-event.v1",
    event.tenantId,
    event.missionId,
    event.orchestratorId,
    String(event.sequence),
    event.type,
    "",
  ].join("\n");
  return `urn:libre-ai:event:${sha256(material)}`;
}

function named(name: string): Loaded {
  const item = cases.find((candidate) => candidate.name === name);
  if (!item) throw new Error(`Missing golden ${name}`);
  return item;
}

describe("agent-orchestrator Rust/TypeScript goldens", () => {
  test("uses the planning-only canonical handoff", () => {
    expect(metadata.profile).toBe("libre-ai.agent-orchestrator.g2-simulator.v1");
    expect(handoff.capabilities).toEqual(["plan"]);
  });

  for (const item of cases) {
    test(`${item.name} validates with stable IDs and checksum`, () => {
      expect(item.events.map((event) => event.type)).toEqual(item.types);
      expect(item.events.map((event) => event.sequence)).toEqual(
        item.events.map((_, index) => index + 1),
      );
      expect(item.events.map(expectedId)).toEqual(item.events.map((event) => event.id));
      expect(sha256(item.stream)).toBe(item.streamSha256);
    });
  }

  test("keeps canonical schema separate from stricter G2 semantics", () => {
    const complete = named("complete");
    const paused = structuredClone(complete.events[0]);
    if (!paused) throw new Error("Missing event");
    paused.type = "paused";
    paused.data = {};
    expect(registry.validate("orchestrator-event.v1.schema.json", paused).ok).toBeTrue();
    const result = structuredClone(complete.events[2]);
    if (!result) throw new Error("Missing result");
    delete result.data.evidence;
    expect(registry.validate("orchestrator-event.v1.schema.json", result).ok).toBeFalse();
  });
});
