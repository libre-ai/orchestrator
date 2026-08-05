use crate::refusal::HarnessRefusal;

/// Byte accounting for a run's captured outputs. Exceeding a bound truncates
/// at the host layer and refuses here — never buffering without limit, never
/// a warning followed by execution.
#[derive(Clone, Debug)]
pub struct OutputLedger {
    per_tool_limit: u64,
    total_limit: u64,
    total_consumed: u64,
}

impl OutputLedger {
    #[must_use]
    pub const fn new(per_tool_limit: u64, total_limit: u64) -> Self {
        Self {
            per_tool_limit,
            total_limit,
            total_consumed: 0,
        }
    }

    #[must_use]
    pub const fn total_consumed(&self) -> u64 {
        self.total_consumed
    }

    /// Admit one tool's captured output. `truncated` reports that the host
    /// capture hit its cap — the producer exceeded its bound even though the
    /// buffered bytes did not. A refused output is not consumed.
    pub fn admit(&mut self, bytes: u64, truncated: bool) -> Result<(), HarnessRefusal> {
        if truncated || bytes > self.per_tool_limit {
            return Err(HarnessRefusal::OutputLimitExceeded);
        }
        let Some(next_total) = self.total_consumed.checked_add(bytes) else {
            return Err(HarnessRefusal::OutputLimitExceeded);
        };
        if next_total > self.total_limit {
            return Err(HarnessRefusal::OutputLimitExceeded);
        }
        self.total_consumed = next_total;
        Ok(())
    }
}

/// The fail-closed output scan of journey 4: a scan that cannot complete
/// refuses the result — an unscanned output is treated as a failed one.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OutputScan {
    Complete,
    Interrupted,
}

pub fn admit_scan(scan: OutputScan) -> Result<(), HarnessRefusal> {
    match scan {
        OutputScan::Complete => Ok(()),
        OutputScan::Interrupted => Err(HarnessRefusal::OutputScanIncomplete),
    }
}
