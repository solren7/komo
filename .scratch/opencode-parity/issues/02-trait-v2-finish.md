# 02 — Tool trait v2 收尾：迁移剩余 8 只工具，删掉兼容桥

Status: done (2026-07-26) — `cargo test` 471 passed，`Tool::execute` 已删除
Phase: 0 地基 · 依赖: 无（与 01 可并行，建议 01 先落地以免签名改两遍）

## 落地记录

- 8 只工具全部迁到 `call(Value, &ToolContext)`：`session · reminder · task ·
  memory · delegate · skill · cron · homeassistant`。
- `Tool::execute` 与 `call` 的桥接默认实现**已删除**，`call` 成为必需方法。
  连带迁移了 `agent/runtime.rs` 与 `services/tool_execution/mod.rs` 里的 8 个
  测试 double。
- approver 从构造注入改为 `ctx.decide`/`ctx.approve`：`SkillTool::new` /
  `CronTool::new` / `HomeAssistantTool::new` 都少了一个参数（wiring 同步）。
  `cron` 的管理类审批抽成自由函数 `approve_manage(ctx, …)`。
- `memory` 是最后一个读 `current_session()` 的工具，改读 `ctx.session`
  （`memory_context(session_id)` 纯函数）。**至此没有任何工具读 task-local**，
  `SESSION` 只服务 approver —— 已同步 `context.rs` 两处文档。
- 错误分类顺手做实：缺参 / 未知 action / 未知 id / 坏 cron 表达式一律
  `ToolError::InvalidInput`（不重试、模型自己改），仅真实执行失败留 `Failed`。
- 新增 `src/tools/test_support.rs`：`detached_ctx` / `approving_ctx` +
  `DenyAll`/`AllowAll`，取代各测试模块各写一遍的 approver double。
- 两处偏离，刻意为之：
  1. `homeassistant` 的 20 个返回点保留为私有 `run(args, ctx) -> Result<String>`
     helper，`call` 只做一层 `ToolOutput::text` 包装 —— 它的每个分支本来就是
     模型可读的散文（拒绝文本也是终态答案而非错误），当前没有 structured 可暴露。
  2. `Approver::approve -> bool` 作为 `decide` 的投影保留（见 01），tools 侧
     不需要反馈时仍可直接用。

## 目标

`.scratch/tool-trait-v2/PRD.md` 留下的收尾项。现在 `Tool` 上并存两个入口：
`call(Value, &ToolContext)`（v2）和 `execute(String)`（v1 桥），新工具作者要判断
该实现哪个。1 期要加 5 只新工具，先把这个岔路关掉。

## 现状

已迁移（7）：`time · file · shell · web_fetch · web_search · todo · ask_user`
未迁移（8）：`cron · delegate · memory · reminder · session · skill · task · homeassistant`

（`homeassistant` 是混合状态：内部有 `call` 形态的 helper，但 `Tool` impl 仍走
`execute` 桥 —— 迁移时一并收干净。）

## 设计

每只工具的固定动作：
1. `async fn execute(&self, input: String)` → `async fn call(&self, input: Value, ctx: &ToolContext)`
2. 手写的 `serde_json::from_str` + 各式错误文本 → `parse_args::<Args>(&input)?`
3. 返回 `String` → `ToolOutput::text(..)`，列表/查询类顺手 `.with_title(..)`
4. 构造函数里注入的 `Arc<dyn Approver>` 删掉，改走 `ctx.approve` / `ctx.decide`
   —— 影响 `skill`（install）、`homeassistant`（call_service）、`cron`（三处 gate），
   并同步改 `cli/wiring.rs` 的构造调用
5. 读 `current_session()` 的改读 `ctx.session` —— 影响 `memory`
6. 随迁测试：测试 double 从构造注入改为 `ToolContext::new(.., approver)`

收尾（**全部迁完后**，一次提交）：
- 删 `Tool::execute` 默认实现 + `Tool::call` 的桥接默认，`call` 变必需
- `infra/rig_tool.rs` 的 fallback 路径确认只走 `call`

## 涉及文件

`src/tools/{cron,delegate,memory,reminder,session,skill,task,homeassistant}.rs` ·
`crates/komo-core/src/domain/tool.rs`（删桥）· `src/cli/wiring.rs`（去掉 approver 入参）

## 验收

- `grep -c "async fn execute" src/tools/*.rs` 全为 0
- `domain/tool.rs` 里不再有 `execute`
- `cargo test` 全绿；每只工具的行为对模型保持等价（本 issue 不改任何输出文本）
