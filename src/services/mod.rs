//! What is left of `services/` after the rest moved to `komo-services`:
//! `operator_control` reaches up into `agent::daemon` (for a cron job's next
//! occurrence) and out through `infra::gateway_client`, so it cannot sit below
//! either. Both of its transports still run the same `OperatorActions`.
pub mod operator_control;
