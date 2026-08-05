use libre_ai_agent_harness::{OutputLedger, OutputScan, admit_scan};

const PER_TOOL: u64 = 1_000;
const TOTAL: u64 = 2_500;

#[test]
fn outputs_inside_both_bounds_accumulate() {
    let mut ledger = OutputLedger::new(PER_TOOL, TOTAL);
    ledger
        .admit(900, false)
        .expect("a first output inside both bounds passes");
    ledger
        .admit(900, false)
        .expect("a second output inside both bounds passes");
    assert_eq!(ledger.total_consumed(), 1_800);
}

#[test]
fn a_truncated_capture_is_a_refusal_not_a_warning() {
    let mut ledger = OutputLedger::new(PER_TOOL, TOTAL);
    let refusal = ledger
        .admit(PER_TOOL, true)
        .expect_err("a producer that hit the cap exceeded its bound");
    assert_eq!(refusal.code(), "harness.output_limit_exceeded");
}

#[test]
fn a_single_tool_output_over_its_bound_is_refused() {
    let mut ledger = OutputLedger::new(PER_TOOL, TOTAL);
    let refusal = ledger
        .admit(PER_TOOL + 1, false)
        .expect_err("per-tool bytes over the bound are refused");
    assert_eq!(refusal.code(), "harness.output_limit_exceeded");
}

#[test]
fn the_total_bound_holds_across_tools() {
    let mut ledger = OutputLedger::new(PER_TOOL, TOTAL);
    ledger.admit(1_000, false).expect("first tool passes");
    ledger.admit(1_000, false).expect("second tool passes");
    let refusal = ledger
        .admit(1_000, false)
        .expect_err("the third tool pushes the run over its total bound");
    assert_eq!(refusal.code(), "harness.output_limit_exceeded");
    assert_eq!(
        ledger.total_consumed(),
        2_000,
        "a refused output is not consumed"
    );
}

#[test]
fn an_interrupted_scan_fails_closed() {
    admit_scan(OutputScan::Complete).expect("a completed scan admits the result");
    let refusal = admit_scan(OutputScan::Interrupted)
        .expect_err("an unscanned output is a failed output, never a clean one");
    assert_eq!(refusal.code(), "harness.output_scan_incomplete");
}
