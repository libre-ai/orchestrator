import { describe, expect, test } from "bun:test";
import {
  checkAgentHarnessCapabilityBoundary,
  dependencyNames,
  forbiddenDependencySections,
  forbiddenEverywhere,
  forbiddenOutsideHost,
} from "./check-capabilities";

describe("WP-G3-H01 harness capability boundary", () => {
  test("confines the host capability to src/host and keeps the closed surfaces closed", async () => {
    expect(await checkAgentHarnessCapabilityBoundary()).toEqual([]);
  });

  test.each([
    "use std::net::TcpStream;",
    "use std::env;",
    'let key = std::env::var("SECRET");',
    "tokio::spawn(async {});",
    "use std as platform;",
    'extern "C" { fn effect(); }',
    "unsafe { effect(); }",
  ])("closed everywhere, host included: %s", (source) => {
    expect(
      forbiddenEverywhere("crates/agent-harness/src/host/process.rs", source).length,
    ).toBeGreaterThan(0);
  });

  test.each([
    "use std::process::Command;",
    "use std::fs;",
    "std::os::unix::net::UnixStream::pair()",
    "let started = Instant::now();",
    "OpenOptions::new()",
  ])("host capability refused outside src/host: %s", (source) => {
    expect(
      forbiddenOutsideHost("crates/agent-harness/src/profile.rs", source).length,
    ).toBeGreaterThan(0);
  });

  // The array-of-tables header is a section too: missing it left the parser
  // inside [dependencies] and read the block's keys as dependency names.
  test("an array-of-tables header closes the dependency section", () => {
    const manifest = [
      "[dependencies]",
      'serde = "=1.0.228"',
      "",
      "[[bench]]",
      'name = "glob"',
      "harness = false",
    ].join("\n");
    expect(dependencyNames(manifest)).toEqual(["serde"]);
  });

  test.each([
    "use std::process::Command;",
    "use std::fs;",
    "std::os::unix::net::UnixStream::pair()",
  ])("host capability allowed under src/host: %s", (source) => {
    expect(forbiddenOutsideHost("crates/agent-harness/src/host/process.rs", source)).toEqual([]);
  });

  // xhigh review of f27b3c9: the exemption was a substring test, so a
  // trailing comment bought real unsafe a pass, and prose containing the
  // word failed the gate.
  test("real unsafe cannot buy an exemption with a comment", () => {
    expect(
      forbiddenEverywhere(
        "crates/agent-harness/src/host/process.rs",
        "unsafe { syscall(); } // forbid(unsafe_code)",
      ).length,
    ).toBeGreaterThan(0);
  });

  test("the crate-level attribute and prose about unsafety do not fire", () => {
    expect(forbiddenEverywhere("x.rs", "#![forbid(unsafe_code)]")).toEqual([]);
    expect(forbiddenEverywhere("x.rs", "/// widening this path would be unsafe")).toEqual([]);
    expect(forbiddenEverywhere("x.rs", "// an unsafe block here would be refused")).toEqual([]);
  });

  test.each([
    "[dev-dependencies]",
    "[build-dependencies]",
    "[dependencies.reqwest]",
    "[target.'cfg(unix)'.dependencies]",
  ])("detects forbidden dependency section: %s", (manifest) => {
    expect(forbiddenDependencySections(manifest).length).toBeGreaterThan(0);
  });
});
