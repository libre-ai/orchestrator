# Security boundary

All handoffs, CLI arguments and event streams are untrusted. JSON passes the embedded canonical schema registry before semantic use; generated types are not treated as sufficient for conditional rules.

Bounds: handoff 64 KiB, event 16 KiB, six input lines/96 KiB, three accepted events, duration 1–604800 seconds, tool limit 0–100000 with actual use always zero, and network `none` only. Time is explicit; there is no host clock or randomness.

Unknown data, context mismatch, sequence gaps, forged IDs, invalid transitions, progress regression, mutated replay, reference confusion and events beyond budget fail closed. Exact replay is a no-op. Errors expose only stable codes and never input content.

Missions owns approval, authorization, accepted handoff digest, durable cursor/idempotency and Proof/Artifact dereferencing. Adding execution, control, DB, network, tokens, tools, providers, harness or persistence requires a new approved package.
