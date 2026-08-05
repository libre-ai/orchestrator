import { describe, expect, test } from "bun:test";
import {
  checkAgentHarnessCapabilityBoundary,
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

  test.each([
    "use std::process::Command;",
    "use std::fs;",
    "std::os::unix::net::UnixStream::pair()",
  ])("host capability allowed under src/host: %s", (source) => {
    expect(forbiddenOutsideHost("crates/agent-harness/src/host/process.rs", source)).toEqual([]);
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
