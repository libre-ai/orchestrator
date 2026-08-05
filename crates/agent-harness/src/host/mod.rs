//! The host boundary — the only part of the harness holding OS capabilities,
//! and it holds exactly what ADR-0018 D2 opens: filesystem observation and
//! local process confinement. Everything it observes is judged by the pure
//! modules; nothing here decides policy.

mod fs;
mod process;
mod run;

pub use fs::WorkspaceObserver;
pub use process::{ConfinedOutcome, ConfinementPlan, SpawnLimits, spawn_confined};
pub use run::{RunError, RunIdentity, run_confined_attested};
