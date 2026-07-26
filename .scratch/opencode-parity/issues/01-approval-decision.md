# 01 — Approver 返回 `Decision`（带反馈的拒绝）

Status: done (2026-07-26) — `cargo test` 471 passed
Phase: 0 地基 · 依赖: 无 · 阻塞: 03 04 06 13 15

## 落地记录

- `domain::approval::Decision{Allow, Deny{feedback}}` + `From<bool>`；`Approver::decide`
  是唯一必须实现的方法，`approve -> bool` 作为**不可覆盖的投影**保留为 provided
  method（现有调用点零改动）。
- 命名冲突处理：原 `interaction::Decision{Once,Session,Deny}` 更名为
  `interaction::Answer`（它本就是「用户的回答」，与 CLI/TUI 的 `Answer` 同构），
  `Decision` 让给 domain。`policy::Decision` 不动。
- 反馈通道：chat `/deny <理由>` · CLI `n: <理由>` · TUI 按 `n` 进入单行输入
  （Esc 仍是零成本直接拒绝）· api `POST …/approve {decision, feedback?}`。
- 消费方：`shell` / `file` write 把理由写进 `ToolOutput`，并附「不要重试同样的调用」。
  `PolicyApprover` 的拒绝会带上命中规则号；`UnattendedDeny` / `DenyAllApprover` /
  审批超时都给出可行动的解释。
- 未做（留给后续）：`apps/app` 的审批模态还没有「填理由」输入框，wire 字段已就绪。

## 目标

`Approver::approve -> bool` 丢掉了用户拒绝时想说的话。opencode v2 的
`PermissionV2` 有 `CorrectedError{feedback}`：用户可以驳回**并附一句指导**，
模型收到的是可行动的反馈而不是一个 denied。

## 设计

`domain/approval.rs`：

```rust
pub enum Decision {
    Allow,
    Deny { feedback: Option<String> },
}

#[async_trait]
pub trait Approver: Send + Sync {
    async fn decide(&self, request: &ApprovalRequest) -> Decision;
}
```

`ToolContext`（`domain/context.rs`）：
- `approve(&req) -> bool` **保留**，`matches!(decision, Allow)` —— 现有 15 只工具零改动。
- 新增 `decide(&req) -> Decision`。

工具侧只有拒绝路径有输出差别：

```rust
match ctx.decide(&request).await {
    Decision::Allow => { /* … */ }
    Decision::Deny { feedback } => return Err(ToolError::Denied(match feedback {
        Some(f) => format!("用户拒绝了这次操作，并说明：{f}"),
        None => "Command rejected by user; nothing was run.".into(),
    })),
}
```

（`ToolError::Denied` 已经是 executor 的「不重试、直接成 outcome 文本」通道。）

## 交互侧

- `cli/approver.rs`：`n` 之后读一行可选理由（回车跳过）。
- `tui/approver.rs`：模态按 `n` 切到单行输入；`ApprovalRequest` 的 oneshot 载荷从
  `bool` 改 `Decision`；**丢弃 modal 仍读作 `Deny{None}`**（现有语义不变）。
- `agent/interaction.rs::ChatApprover`：`/deny 用 trash 代替 rm` —— `/deny` 后的
  剩余文本即 feedback。`/approve` 不变。超时（5min）→ `Deny{None}`。
- `PolicyApprover`：deny 规则产生 `Deny{feedback: Some("被 policy 规则 … 拦截")}`，
  比现在只回 false 更好定位。
- `UnattendedDeny` / `DenyAllApprover`：`Deny{Some("无人值守上下文，需要 unattended 允许规则")}`。

## 涉及文件

`crates/komo-core/src/domain/approval.rs`（Decision + trait）·
`domain/context.rs`（ToolContext::decide）·
`src/cli/approver.rs` · `src/tui/approver.rs` · `src/tui/app.rs`（模态状态机）·
`src/agent/interaction.rs`（ChatApprover + `/deny` 解析）·
`src/agent/policy_approver.rs` · `src/cli/wiring.rs`（UnattendedDeny）·
`src/services/tool_execution/mod.rs`（DenyAllApprover）· ~12 处测试 double。

## 验收

- 现有测试全绿（`approve -> bool` 语义不变）。
- 新测试：`ChatApprover` 收到 `/deny 用 trash` → 工具得到 `Denied` 且文本含 "trash"。
- 新测试：TUI 模态被丢弃 → `Deny{None}`。
- 手验：chat 里对一个 `rm` 命令回 `/deny 用 trash 代替`，模型下一轮改写命令。
