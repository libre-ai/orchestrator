const CRATE_ROOT = "crates/agent-orchestrator";
const ALLOWED_DEPENDENCIES = new Set([
  "chrono",
  "libre-ai-contract-types",
  "serde",
  "serde_jcs",
  "serde_json",
  "sha2",
]);
const FORBIDDEN_SOURCE = [
  "std::process",
  "std::fs",
  "std::net",
  "std::env",
  "std::os::unix::net",
  "std::thread",
  "std::time",
  "Utc::now",
  "Local::now",
  "SystemTime",
  "Instant::now",
  "tokio::",
  "reqwest::",
  "hyper::",
  "Command::new",
  "TcpStream",
  "TcpListener",
  "UdpSocket",
  "UnixStream",
  "UnixListener",
  "OpenOptions",
];
const FORBIDDEN_SOURCE_PATTERNS: ReadonlyArray<readonly [string, RegExp]> = [
  ["std-os-capability", /\bstd\s*::\s*(?:process|fs|net|env|thread|time)\b/u],
  [
    "grouped-std-os-capability",
    /\bstd\s*::\s*\{[\s\S]{0,500}\b(?:process|fs|net|env|thread|time)\b/u,
  ],
  ["std-alias", /\b(?:use|extern\s+crate)\s+std\s+as\b/u],
  ["ffi", /\bextern\s*"C"/u],
];

export function forbiddenSourceCapabilities(path: string, source: string): string[] {
  const failures: string[] = [];
  for (const forbidden of FORBIDDEN_SOURCE) {
    if (source.includes(forbidden)) failures.push(`capability-forbidden:${path}:${forbidden}`);
  }
  for (const [label, pattern] of FORBIDDEN_SOURCE_PATTERNS) {
    if (pattern.test(source)) failures.push(`capability-forbidden:${path}:${label}`);
  }
  const executableUnsafe = source
    .split("\n")
    .some((line) => !line.includes("forbid(unsafe_code)") && /\bunsafe\b/.test(line));
  if (executableUnsafe) failures.push(`unsafe-forbidden:${path}`);
  return failures;
}

export function forbiddenDependencySections(manifest: string): string[] {
  const failures: string[] = [];
  for (const line of manifest.split("\n")) {
    const section = /^\[([^\]]+)\](?:\s*#.*)?$/.exec(line.trim())?.[1];
    const alternateDependencySection =
      section !== undefined &&
      section !== "dependencies" &&
      (section.endsWith("dependencies") ||
        section.startsWith("dependencies.") ||
        section.includes(".dependencies."));
    if (alternateDependencySection) failures.push(`dependency-section-forbidden:${section}`);
  }
  return failures;
}

function dependencyNames(manifest: string): string[] {
  const names: string[] = [];
  let inDependencies = false;
  for (const line of manifest.split("\n")) {
    const section = /^\[([^\]]+)\](?:\s*#.*)?$/.exec(line.trim())?.[1];
    if (section !== undefined) {
      inDependencies = section === "dependencies";
      continue;
    }
    if (!inDependencies) continue;
    const name = /^([a-z0-9_-]+)(?:\.[a-z0-9_-]+)?\s*=/.exec(line.trim())?.[1];
    if (name !== undefined) names.push(name);
  }
  return names;
}

export async function checkAgentOrchestratorCapabilityBoundary(): Promise<string[]> {
  const failures: string[] = [];
  const manifest = await Bun.file(`${CRATE_ROOT}/Cargo.toml`).text();
  failures.push(...forbiddenDependencySections(manifest));
  const dependencies = dependencyNames(manifest);
  for (const dependency of dependencies) {
    if (!ALLOWED_DEPENDENCIES.has(dependency)) {
      failures.push(`dependency-not-allowed:${dependency}`);
    }
  }
  for (const required of ALLOWED_DEPENDENCIES) {
    if (!dependencies.includes(required)) failures.push(`dependency-missing:${required}`);
  }

  for (const forbiddenPath of [
    `${CRATE_ROOT}/build.rs`,
    `${CRATE_ROOT}/src/main.rs`,
    `${CRATE_ROOT}/src/bin`,
  ]) {
    if (await Bun.file(forbiddenPath).exists())
      failures.push(`runtime-entry-forbidden:${forbiddenPath}`);
  }

  const glob = new Bun.Glob(`${CRATE_ROOT}/src/**/*.rs`);
  let sourceCount = 0;
  for await (const path of glob.scan({ cwd: ".", onlyFiles: true })) {
    sourceCount += 1;
    if (path.includes(`${CRATE_ROOT}/src/bin/`)) failures.push(`runtime-entry-forbidden:${path}`);
    const source = await Bun.file(path).text();
    failures.push(...forbiddenSourceCapabilities(path, source));
  }
  if (sourceCount === 0) failures.push("runtime-source-missing");
  return failures;
}

if (import.meta.main) {
  const failures = await checkAgentOrchestratorCapabilityBoundary();
  if (failures.length > 0) {
    console.error(failures.join("\n"));
    process.exit(1);
  }
  console.log("Agent orchestrator capability boundary verified: simulation-only pure Rust core");
}
