import { describe, expect, test } from "bun:test";

const sdkRevision = "ac9f2020425733183839a58fc2c3928a4de5c066";
const contractsRevision = "5b9b6668909119b670e0db62174419ab04e5b402";
const vectorHash = "d3a63edb3f146abe9061a3af0a9ae1bbb2d8d66f3d67fa34bbf7e859c0d0447e";
const expectedDomains = new Map([
  ["graph", 11],
  ["causal", 9],
  ["decision", 11],
  ["effect", 11],
  ["authority", 4],
  ["transfer", 8],
]);

async function sha256(path: string): Promise<string> {
  const hasher = new Bun.CryptoHasher("sha256");
  hasher.update(await Bun.file(path).bytes());
  return hasher.digest("hex");
}

describe("authorized execution authority", () => {
  test("pins the reviewed SDK and Contracts commits", async () => {
    const cargo = await Bun.file("Cargo.toml").text();
    const packageManifest = await Bun.file("package.json").json();
    expect(cargo).toContain(`rev = "${sdkRevision}"`);
    expect(packageManifest.devDependencies["@libre-ai/contracts-authority"]).toBe(
      `github:libre-ai/contracts#${contractsRevision}`,
    );
  });

  test("reads the exact locked vector authority from the checkout", async () => {
    const root = "node_modules/@libre-ai/contracts-authority";
    const vectorsPath = `${root}/contracts/fixtures/authorized-execution-v1/semantic-vectors.v1.json`;
    const catalog = await Bun.file(`${root}/contracts/catalog.v1.json`).json();
    const vectors = await Bun.file(vectorsPath).json();
    const counts = new Map<string, number>();
    for (const vector of vectors.cases) {
      counts.set(vector.domain, (counts.get(vector.domain) ?? 0) + 1);
    }
    expect(vectors.schemaVersion).toBe("libre-ai.authorized-execution-semantic-vectors.v1");
    expect(vectors.cases).toHaveLength(54);
    expect(counts).toEqual(expectedDomains);
    expect(await sha256(vectorsPath)).toBe(vectorHash);
    for (const id of [
      "effect-attestation-v1",
      "execution-authorization-v2",
      "execution-graph-v1",
      "execution-plan-body-v2",
      "execution-transfer-v1",
      "human-decision-request-v1",
      "human-decision-response-v1",
      "orchestrator-event-v3",
      "retention-policy-schema-v2",
      "retention-policy-v2",
      "step-invocation-v1",
    ]) {
      expect(catalog.contracts.find((entry: { id: string }) => entry.id === id)?.status).toBe(
        "locked",
      );
    }
  });
});
