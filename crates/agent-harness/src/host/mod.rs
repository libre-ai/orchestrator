//! The host boundary — the only part of the harness holding OS capabilities,
//! and it holds exactly what ADR-0018 D2 opens: filesystem observation and
//! local process confinement. Everything it observes is judged by the pure
//! modules; nothing here decides policy.

mod binding;
mod fs;
mod process;
mod run;

pub use binding::RunBinding;

pub use fs::{WorkspaceObserver, canonical_workspace};
pub use process::{ConfinedOutcome, SpawnLimits, spawn_confined};
pub use run::{RunError, RunIdentity, run_confined_attested};
