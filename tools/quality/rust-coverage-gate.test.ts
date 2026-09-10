// SPDX-FileCopyrightText: 2026 Libre AI contributors
// SPDX-License-Identifier: EUPL-1.2

import { describe, expect, test } from "bun:test";

const workflowPath = ".github/workflows/ci.yml";

describe("Rust coverage gate", () => {
  test("pins the coverage tool and blocks below the measured baseline", async () => {
    const workflow = await Bun.file(workflowPath).text();

    expect(workflow).toContain('CARGO_LLVM_COV_VERSION: "0.9.1"');
    expect(workflow).toContain(
      'CARGO_LLVM_COV_SHA256: "3fca950394a3c49457657c158b1619cec8dfd2647ae5b48746734c0ab969a522"',
    );
    expect(workflow).toContain("rustup component add llvm-tools-preview");
    expect(workflow).toContain("mkdir -p coverage");
    expect(workflow).toContain("cargo llvm-cov --locked --all-features --lcov");
    expect(workflow).toContain("--output-path coverage/lcov.info");
    expect(workflow).toContain("--fail-under-lines 87");
    expect(workflow).toContain("--fail-under-functions 90");
    expect(workflow).toContain("cargo llvm-cov report --summary-only");
  });
});
