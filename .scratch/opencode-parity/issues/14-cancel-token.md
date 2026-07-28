# 14 — `ctx.cancel`：协作式取消能打断正在执行的工具

Status: done (2026-07-28) — `cargo test` 607 passed
Phase: 2 管线 · 依赖: 13（shell 是最需要被打断的工具）

## 落地记录

与 issue 设计的三处偏差，都是为了不产生半成品文件：

1. **executor 不做全局 race**。issue 写「executor spawn 时把 `tool.call()` 与
   `token.cancelled()` race」—— 但那会作用于**所有**工具，包括 `apply_patch`
   （多文件顺序写，文件之间有 await 点）。主动在两个文件之间中断，等于把一个本会
   完成的 patch 变成半应用的仓库：超时导致的部分应用是不得已，取消导致的是自找。
   改成 executor 只把信号暴露给工具（`ToolContext::cancelled()`），认领的工具
   自己在安全点 `select!`。
2. **`apply_patch` 不认领**（issue 原本列为第 3 优先）。理由同上；patch 是毫秒级
   本地写入，没人需要中断它。`write`/`edit` 也不认领 —— 顺带核实过它们其实**安全**：
   `file_mutation::write_if_unchanged` 走 `tokio::fs::write`，内部是一次
   `spawn_blocking(std::fs::write)`，取消只取消*等待*，syscall 在 blocking 线程里
   跑完，不会写半个文件。但没有收益，所以不改。
3. **不加 `tokio-util` 依赖**。`CancelSignal::cancelled()` 本身就是 `async fn`，
   直接 `select!` 即可；`CancellationToken` 是多余的一层。`ToolContext::cancelled()`
   在无信号时 `std::future::pending()`，所以 sweeps/cron/aux 的那条 select 分支
   是惰性的，不会立刻胜出。

认领范围：`shell`（`killpg` 整个进程组，复用 13 的 kill 逻辑）+ `web_fetch` /
`web_search`（drop 请求即断连，纯读无副作用）。

`shell` 的 race 现在是三路：命令自己的 `timeout`（报给模型，可加大重试）、取消
（返回 `ToolError::Failed(Cancelled)`）、正常结束。取消的措辞与 run 级一致
（`CANCELLED_ERROR`），且重试分类器把它判为 terminal —— 这点很关键，因为
`web_fetch` 是 `idempotent`，不然一次取消会变成三次尝试。

**被杀的命令自己可能在写文件**（`cargo build` 写 target/）—— 这和用户按 Ctrl-C
完全一样，是杀进程固有的，也是用户主动要求的。

顺带发现一个**既存**行为（未改）：executor 的超时路径本来就 `abort()`，所以一个
超时的 `apply_patch` 现在就可能在文件之间被打断。

验证：`shell` 两个测试（取消 → 5s 内返回、错误措辞是 `cancelled by user`、
backgrounded 子进程的 marker 文件不出现；无信号的 turn 行为不变）+ executor 一个
（认领取消的工具只被调用一次、step 记 `ok=false` / `error=cancelled by user`）。
另外确认 `infra/messaging/api.rs:598` 给**每个** api turn 都挂了信号，所以这条路
在 GUI / `komo chat` over gateway 上真的可达。AGENTS.md 与 `domain/cancel.rs`
的「不停止已在执行的工具调用」段落已改写。

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
