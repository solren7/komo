# PRD: Tool trait v2 —— 借鉴 opencode v2 的类型化工具接口

Status: in-progress — 基座 + 7/15 工具已迁移，全量测试绿（461 passed）

进度（2026-07-21）：
- ✅ 基座：`ToolOutput`/`ToolError`/`parse_args`（`domain/tool.rs`）、`ToolContext`
  + `SessionContext`/`RunContext` 迁入 `domain/context.rs`（services 侧再导出保持
  路径稳定）、executor 持 approver + `call` 分发 + `ToolError` 分类、`DenyAllApprover`
  默认。
- ✅ 兼容桥：`Tool::call`（新）默认桥接到 `Tool::execute`（旧），未迁移工具零改动
  继续编译运行 —— 每迁一个工具都是可编译、可测的独立步骤。
- ✅ 已迁移（7）：`time` · `file` · `web_search` · `web_fetch` · `shell` · `todo`
  （读 `ctx.session`）· `ask_user`（读 `ctx.session`）。approver 注入已从
  file/shell/web_fetch 构造中移除，改走 `ctx.approve`。
- ⏳ 待迁移（8）：`session` · `reminder` · `task` · `memory`（读 `ctx.session`）·
  `skill`（`ctx.approve`@install）· `delegate` · `homeassistant`（`ctx.approve`）。
  这些仍走兼容桥，行为不变。
- ⏳ 收尾：全部迁移后删除 `Tool::execute` 默认 + `call` 桥接，`call` 变必需；
  D5（入口显式传 session、删 `with_session` 包裹）作为独立清理。

## 背景

opencode v2（`~/01-code/opencode/packages/core/src/tool/`）把工具抽象重写成
`Tool.make({ input, output, execute, toModelOutput, structured })`：

- `input`/`output` 是**类型化 schema**，`settle` 在边界处解码 input（失败即
  `"Invalid tool input"`）、并对 output 双向校验后才交出。
- `execute` 返回**类型化领域值**，不是字符串。
- `toModelOutput(output) → Content[]` 是**独立投影**，决定模型看到什么；`Content`
  是 `text | file{data,mime}`——`read` 借此返回图片、`bash` 返回 `[stdout, 状态行]`。
- `structured`（见 bash）是给程序/UI 的**第三个视图**，模型不必为它付 token。
- 权限在 `execute` **叶子内**断言（`permission.assert`），不是每个工具构造时注入。

komo 现状：`Tool::execute(String) -> anyhow::Result<String>`，approver 每个工具
构造注入，session 走 `SESSION` task-local，每个工具自己 `serde_json::from_str`
解析参数并返回各式各样的错误文本。AGENTS.md 已把该 task-local 标注为"内部兼容
seam"，是想收窄的。

## 目标（本次重构要拿到的）

1. **类型化返回**：`execute → ToolOutput { title, text, structured }`，把"领域结果
   / 模型文本 / 结构化数据"三者分开。
2. **参数解码集中化**：一个规范的 `InvalidInput` 错误——"invalid tool input: …；
   请按 schema 重写参数"，取代每个工具各写一遍的解析样板。
3. **显式上下文 + 叶子端 `ctx.ask`**：`execute(input, ctx)`，`ctx` 携带 session、
   run、approver。**删除每工具构造注入 approver**；**把 `SESSION`
   task-local 收窄到只服务于 approver**（工具不再读它）。

协作式取消（`ctx.cancel`）**推迟**：`tokio-util::CancellationToken` 不是现有依赖，
不在本次引入依赖；单列后续 issue。

**非目标（拆成后续 issue，见文末）**：图片/附件端到端（需动 rig driver 与各
channel）、`structured` 落库（改 `RunStep` = state.db 迁移）、把 `file` 拆成
read/write/edit、新增 grep/glob(rg)。本次只做 **trait 形态**，行为对模型保持等价。

## 与 opencode 的取舍（Rust 化的偏离，刻意为之）

| opencode v2 | komo v2 | 理由 |
|---|---|---|
| `input`/`output` 双 schema + `settle` 双向校验 | `execute` 直接返回 `ToolOutput`，参数用 serde 解码校验 | Rust 无 HKT/schema-runtime；serde 解码即校验，output 校验对 komo 收益低 |
| `toModelOutput` 独立投影 | 折进 `execute`：工具自己构造 `ToolOutput{text, structured}` | 无 schema 驱动，独立投影反而更绕；投影 = 构造 ToolOutput，等价且更简单 |
| `Content: text\|file` | 本次只 `text`；`file`/附件推迟 | rig `ToolOutcome.content` 现为 `String`，图片需 driver + channel 改造，单列一个 issue |
| `withPermission` 共享 `edit` action | 保留现有 `ActionRef`/policy | komo policy 已用 `ActionRef` 匹配，无需改 |
| `permission.assert` 叶子内、approver 由 Layer 提供 | `ctx.ask(req)`，approver 由 executor 持有 | 同构，且顺带删掉每工具构造注入 |

## 设计

### D1: 新类型（`domain/tool.rs`）

```rust
pub struct ToolOutput {
    pub title: Option<String>,   // 简短 UI/日志标题，如 "read src/x.rs (120 lines)"
    pub text: String,            // 模型看到的文本（等价于旧的返回 String）
    pub structured: serde_json::Value,  // 给 ledger/UI 的结构化视图；默认 Null
}
impl ToolOutput {
    pub fn text(s: impl Into<String>) -> Self { text=s, title=None, structured=Null }
    pub fn with_title / with_structured(..) -> Self
}

pub enum ToolError {
    InvalidInput(String),   // 参数不合 schema；executor 渲染成"重写参数"提示，不重试
    Denied(String),         // approval 拒绝 / policy 拦截；不重试
    Failed(anyhow::Error),  // 其它；保留 TransientError 供重试分类
}
impl From<anyhow::Error> for ToolError { -> Failed }   // 工具体内 `?` 仍顺手
```

### D2: Trait 形态（`domain/tool.rs`）

```rust
#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;
    fn parameters_schema(&self) -> Value { <no-args> }
    fn idempotent(&self) -> bool { false }
    fn redact_args(&self, args: &Value) -> Value { args.clone() }   // Value，非 &str
    async fn execute(&self, input: Value, ctx: &ToolContext<'_>) -> Result<ToolOutput, ToolError>;
}

// 参数解码辅助：工具体内标准第一行
pub fn parse_args<T: DeserializeOwned>(input: &Value) -> Result<T, ToolError>
```

保持 `Tool` object-safe（executor 持 `Arc<dyn Tool>`）。不引入第二个 `TypedTool`
trait——用 `parse_args` 辅助函数达成"集中化解码 + 规范错误"，比 blanket-impl 关联
类型的方案更轻、不改 object-safety。

### D3: `ToolContext`（`services/tool_execution/context.rs`）

```rust
pub struct ToolContext<'a> {
    pub session: &'a SessionContext,
    pub run: Option<&'a RunContext>,
    pub cancel: CancellationToken,
    approver: &'a dyn Approver,   // 私有
}
impl ToolContext<'_> {
    // 装 SESSION task-local **仅围绕 approver 调用**，让 ChatApprover/PolicyApprover
    // 继续 current_session() 读 sink/channel——approver 保持 domain-pure，不认识
    // SessionContext。工具本身不再碰 task-local。
    pub async fn ask(&self, req: ApprovalRequest) -> bool {
        with_session(self.session.clone(), self.approver.approve(&req)).await
    }
}
```

**关键判断**：`SESSION` task-local **不删除，收窄**。它现在的消费者：
- 工具 `memory`/`todo`/`ask_user` → 改读 `ctx.session`（不再用 task-local）。
- approver `ChatApprover`/`PolicyApprover` → `ctx.ask` 在其调用处装回 task-local，
  **零改动**。这样 task-local 只剩 approval 这一条私有通道（可辩护：Approver 是
  domain trait，不该知道 komo 的 SessionContext）。

### D4: Executor（`services/tool_execution/mod.rs`）

- `ToolExecutor::new(config, approver: Arc<dyn Approver>)`——approver 从**每工具注入**
  变为 **executor 持有一份**（wiring 传 policy 包装后的；briefing executor 传
  deny-all）。这反而简化 wiring：`FileTool::new(ws)` 等不再收 approver。
- `execute` 管线内：解析 `input: String → Value`（非法 JSON → InvalidInput，不重试）
  → 构造 `ToolContext{session:&ctx.session, run, cancel, approver:&*self.approver}`
  → 传给 `tool.execute(value, &ctx)`。
- `ToolError` 映射：`InvalidInput`/`Denied` 直接成 outcome 文本、**不重试**；`Failed(e)`
  走现有 `should_retry(e, idempotent)`（TransientError/文本分类不变）。
- ledger：`RunStep.result` 仍存 `ToolOutput.text`（本次不落 `structured`，避免
  state.db 迁移）；`tool ok` 日志优先用 `title`（有则）。
- 结果 cap：作用于 `ToolOutput.text`，同今。
- **删除** `execute_fallback` 里的 `current_session()`（rig fallback 路径构造一个
  detached ctx——不变）。

### D5: 入口显式传 session（收尾 task-local）

`runtime.rs:65-67/186`、`api.rs`、`tui/mod.rs` 现在用 `with_session` 装 task-local
让 runtime 事后 `current_session()` 取回。改为**显式把 `SessionContext` 传进
`handle_input`/`run_turn`**（runtime 已在 186 行组装 `ToolTurnContext.session`，只是
来源换成参数而非 task-local）。入口不再需要 `with_session` 包整个 turn。

### D6: rig adapter（`infra/rig_tool.rs`）

`RigTool::call` 走 `core.execute_fallback`——签名不变，内部构造 detached
`ToolContext`。`ToolOutcome.content` 仍取 `ToolOutput.text`，driver 不动。

## 迁移清单（15 个工具）

`time`(平凡) · `file`(approver+typed+read/write) · `shell`(approver) ·
`web_fetch`(approver) · `web_search` · `session` · `reminder` · `task` ·
`todo`(读 ctx.session) · `memory`(读 ctx.session) · `ask_user`(读 ctx.session+sink) ·
`skill`(approver@install) · `delegate` · `homeassistant`(approver) · `http`(内部helper，非Tool)

每个：`String→Value` 参数（`parse_args`）、返回 `ToolOutput`、approver 从
`self.approver` 改 `ctx.ask`、构造函数去掉 approver 入参、随迁的单测更新。

## 实施顺序

1. **基座**：`ToolOutput`/`ToolError`/`parse_args` + 新 `Tool::execute` 签名 +
   `ToolContext` + executor 改造 + rig_tool。（此时全仓不编译——trait 变了。）
2. **试点 2 个**：`time`（无参无 approver）+ `file`（有 approver + typed read/write），
   `cargo check` 通过 → **形态确认点**（本 PRD 提交时已到此）。
3. 其余 13 个工具批量迁移，每批 `cargo check`。
4. 入口显式传 session（D5）、删 `with_session` 包裹、`cargo test`。
5. 全量 `cargo test` + `cargo fmt`。

## 后续 issue（本次不做）

- `02-attachments`: `Content::File` 端到端（rig driver + channel 图片投递）——解锁
  `read` 返回图片、`web_fetch` 图片。
- `03-structured-ledger`: `ToolOutput.structured` 落 `RunStep`（新列，state.db 迁移）。
- `04-split-file`: `file` → `read`(分页/目录/图片) + `write` + `edit`(精确替换,保
  BOM/换行,stale 保护)。
- `05-grep-glob-rg`: 新增 ripgrep 支撑的 `grep`/`glob`。
- `06-bash-parity`: shell 加 `timeout` 参数 + 结构化 `{exit,truncated,timeout}`。
