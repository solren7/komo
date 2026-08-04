//! What is left of `tools/` after the rest moved to `komo-tools`: `delegate`
//! runs a sub-agent as a **real agent turn**, so it holds an `AgentRuntime` and
//! cannot sit below the agent. Recursion stays blocked structurally — the
//! sub-agent's tool set has `delegate: None`.
pub mod delegate;
