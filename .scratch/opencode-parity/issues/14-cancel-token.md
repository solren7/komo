# 14 — `ctx.cancel`：协作式取消能打断正在执行的工具

Status: ready-for-agent
Phase: 2 管线 · 依赖: 13（shell 是最需要被打断的工具）

## 目标

AGENTS.md 已明确记录这个限制：取消一个 turn「不停止已经在执行的工具调用 —— executor
spawn 了每次调用，`Tool` 没有 abort 钩子，所以取消只意味着『不再有后续轮次、
不再有后续工具调用』」。用户中断一个跑了 5 分钟的 `cargo build`，它会继续跑完。

tool-trait-v2 PRD 当时把 `ctx.cancel` 推迟了，理由是「`tokio-util` 不是现有依赖」。
现在值得付这个依赖。

## 设计

- `Cargo.toml` 加 `tokio-util = { version = "0.7", default-features = false }`
  （只要 `CancellationToken`，无额外传递依赖）。
- `SessionContext` 已经挂着 `CancelSignal`（`agent/interaction.rs::CancelState`，
  一个 per-session `watch`）。**不新造机制**：从这个 watch 派生
  `CancellationToken`，塞进 `ToolContext.cancel`。
- `ToolContext` 暴露：
  - `cancel: CancellationToken`（工具自己 select）
  - `async fn cancelled(&self)`（等待取消，方便 `tokio::select!`）
- executor：spawn 每次调用时，把 `tool.call(..)` 与 `token.cancelled()` race；
  取消胜出 → outcome 记为「被用户中断」，`RunStep` 标 `ok=false` +
  `error="cancelled by user"`（与现有 turn 级 `cancelled by user` 措辞一致）。
- 认领取消的工具（收益从高到低）：
  1. `shell` —— select `child.wait()`，取消时 kill 进程组（复用 13 的 kill 逻辑）
  2. `web_fetch` / `web_search` —— reqwest future 被 drop 即断连
  3. `apply_patch` —— 在 hunk 之间检查 token（**不在**单个文件写入中途中断），
     取消时按 06 的「部分应用」语义报告已落盘的文件
  4. 其余工具不认领（默认在调用**结束后**才被观察到取消，行为等价现状）

**不做**的事：不强杀不认领的工具（Rust 没有安全的任务强杀）；`edit`/`write` 的单次
写入不可中断（半个文件比慢一点糟得多）。

## 涉及文件

`Cargo.toml` · `crates/komo-core/src/domain/context.rs` ·
`src/services/tool_execution/mod.rs` · `src/agent/interaction.rs`（watch → token 桥）·
`src/tools/{shell,web_fetch,web_search,apply_patch}.rs` · `crates/komo-core/src/domain/cancel.rs`

## 验收

- `shell {command:"sleep 60"}` 执行中调 `POST /api/interactions/{s}/cancel`
  → 1 秒内 turn 结束，且 `sleep` 进程消失（现在会继续跑 60 秒）。
- `RunStep` 记录 `cancelled by user`，run 状态与现有取消语义一致
  （`Failed` / 非 `recoverable`）。
- 无取消信号的上下文（cron / sweeps / aux）行为完全不变。
- 更新 AGENTS.md 里「不停止已在执行的工具调用」那段描述。
