import { describe, expect, test } from "bun:test";
import {
  checkAgentOrchestratorCapabilityBoundary,
  forbiddenDependencySections,
  forbiddenSourceCapabilities,
} from "./check-capabilities";

describe("WP-G2-A01 simulation-only capability boundary", () => {
  test("forbids process, filesystem, network, environment and unreviewed dependencies", async () => {
    expect(await checkAgentOrchestratorCapabilityBoundary()).toEqual([]);
  });

  test.each([
    "[build-dependencies]",
    "[dev-dependencies] # test escape",
    "[dependencies.reqwest]",
    "[target.'cfg(unix)'.dependencies]",
  ])("detects forbidden dependency section: %s", (manifest) => {
    expect(forbiddenDependencySections(manifest).length).toBeGreaterThan(0);
  });

  test.each([
    "use std::process::Command;",
    "use std :: fs :: File;",
    "use std::{collections::BTreeMap, net::TcpStream};",
    "use std as platform;",
    "use std::thread;",
    "let now = Utc::now();",
    'extern "C" { fn effect(); }',
    "unsafe { effect(); }",
  ])("detects forbidden source capability: %s", (source) => {
    expect(forbiddenSourceCapabilities("fixture.rs", source).length).toBeGreaterThan(0);
  });
});
