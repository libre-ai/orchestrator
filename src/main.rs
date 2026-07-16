#![forbid(unsafe_code)]

use chrono::{DateTime, Utc};
use libre_ai_agent_orchestrator::{
    CanonicalReference, ErrorCode, MAX_EVENT_STREAM_BYTES, MAX_HANDOFF_BYTES, MissionBudgets,
    MissionContext, NetworkBudget, OrchestratorError, Scenario, SimulationRequest,
    encode_ndjson_v1, simulate_v1, validate_event_stream_v1,
};
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{self, Read, Write};

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(error.exit_status());
    }
}

fn run() -> Result<(), OrchestratorError> {
    let mut args = std::env::args().skip(1);
    let command = args
        .next()
        .ok_or_else(|| OrchestratorError::new(ErrorCode::UsageInvalid))?;
    let values = args.collect::<Vec<_>>();
    let mut flags = flags(&values)?;
    match command.as_str() {
        "simulate-v1" => simulate(&mut flags),
        "validate-events-v1" => validate(&mut flags),
        _ => Err(OrchestratorError::new(ErrorCode::UsageInvalid)),
    }
}

fn simulate(flags: &mut BTreeMap<String, String>) -> Result<(), OrchestratorError> {
    let context = context(flags)?;
    let scenario = Scenario::parse(&take(flags, "--scenario")?)
        .ok_or_else(|| OrchestratorError::new(ErrorCode::UsageInvalid))?;
    let artifact = reference(flags, "artifact")?;
    let evidence = reference(flags, "evidence")?;
    empty(flags)?;
    let handoff = read_bounded(io::stdin().lock(), MAX_HANDOFF_BYTES)?;
    let events = simulate_v1(
        &handoff,
        &SimulationRequest {
            context,
            scenario,
            artifact,
            evidence,
        },
    )?;
    io::stdout()
        .lock()
        .write_all(&encode_ndjson_v1(&events)?)
        .map_err(|_| OrchestratorError::new(ErrorCode::InputUnavailable))
}

fn validate(flags: &mut BTreeMap<String, String>) -> Result<(), OrchestratorError> {
    let context = context(flags)?;
    let handoff_path = take(flags, "--handoff")?;
    empty(flags)?;
    let handoff = read_bounded(
        File::open(handoff_path)
            .map_err(|_| OrchestratorError::new(ErrorCode::InputUnavailable))?,
        MAX_HANDOFF_BYTES,
    )?;
    let stream = read_bounded(io::stdin().lock(), MAX_EVENT_STREAM_BYTES)?;
    validate_event_stream_v1(&handoff, &context, &stream)?;
    Ok(())
}

fn flags(args: &[String]) -> Result<BTreeMap<String, String>, OrchestratorError> {
    if !args.len().is_multiple_of(2) {
        return Err(OrchestratorError::new(ErrorCode::UsageInvalid));
    }
    let mut output = BTreeMap::new();
    for pair in args.chunks_exact(2) {
        if !pair[0].starts_with("--") || output.insert(pair[0].clone(), pair[1].clone()).is_some() {
            return Err(OrchestratorError::new(ErrorCode::UsageInvalid));
        }
    }
    Ok(output)
}

fn context(flags: &mut BTreeMap<String, String>) -> Result<MissionContext, OrchestratorError> {
    let mission_id = take(flags, "--mission-id")?;
    let started_at = DateTime::parse_from_rfc3339(&take(flags, "--started-at")?)
        .map(|value| value.with_timezone(&Utc))
        .map_err(|_| OrchestratorError::new(ErrorCode::UsageInvalid))?;
    let max_duration_seconds = number(flags, "--max-duration-seconds")?;
    let max_tool_calls = number(flags, "--max-tool-calls")?;
    let network = NetworkBudget::parse(&take(flags, "--network")?)
        .ok_or_else(|| OrchestratorError::new(ErrorCode::UsageInvalid))?;
    Ok(MissionContext {
        mission_id,
        started_at,
        budgets: MissionBudgets {
            max_duration_seconds,
            max_tool_calls,
            network,
        },
    })
}

fn reference(
    flags: &mut BTreeMap<String, String>,
    prefix: &str,
) -> Result<Option<CanonicalReference>, OrchestratorError> {
    let names = [
        format!("--{prefix}-id"),
        format!("--{prefix}-digest"),
        format!("--{prefix}-media-type"),
    ];
    let present = names
        .iter()
        .filter(|name| flags.contains_key(name.as_str()))
        .count();
    if present == 0 {
        return Ok(None);
    }
    if present != 3 {
        return Err(OrchestratorError::new(ErrorCode::UsageInvalid));
    }
    Ok(Some(CanonicalReference {
        id: take(flags, &names[0])?,
        digest: take(flags, &names[1])?,
        media_type: take(flags, &names[2])?,
    }))
}

fn number(flags: &mut BTreeMap<String, String>, name: &str) -> Result<u64, OrchestratorError> {
    take(flags, name)?
        .parse()
        .map_err(|_| OrchestratorError::new(ErrorCode::UsageInvalid))
}

fn take(flags: &mut BTreeMap<String, String>, name: &str) -> Result<String, OrchestratorError> {
    flags
        .remove(name)
        .ok_or_else(|| OrchestratorError::new(ErrorCode::UsageInvalid))
}

fn empty(flags: &BTreeMap<String, String>) -> Result<(), OrchestratorError> {
    if flags.is_empty() {
        Ok(())
    } else {
        Err(OrchestratorError::new(ErrorCode::UsageInvalid))
    }
}

fn read_bounded(mut reader: impl Read, maximum: usize) -> Result<Vec<u8>, OrchestratorError> {
    let mut bytes = Vec::new();
    reader
        .by_ref()
        .take(maximum as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| OrchestratorError::new(ErrorCode::InputUnavailable))?;
    if bytes.len() > maximum {
        Err(OrchestratorError::new(ErrorCode::InputTooLarge))
    } else {
        Ok(bytes)
    }
}
