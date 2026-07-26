# 11 — `structured` + `output_paths` 落 `RunStep`（additive 迁移）

Status: ready-for-agent
Phase: 2 管线 · 依赖: 10（output_paths 来源）· 建议与 10 同批提交

## 目标

`ToolOutput.structured` 字段在 tool-trait-v2 里就加好了，但**没有任何消费者** ——
`RunStep` 没有对应列（原 PRD 的 issue 03，当时怕 state.db 迁移）。
现在有了 `ensure_columns`，这个顾虑不成立了。

v2 把 `structured` 定位成「给程序/UI 的第三视图，模型不为它付 token」：
`bash` 的 `{exit, truncated, timeout}`、`edit` 的 diff 统计都在这里。

## 设计

`infra/persistence/db.rs` 的 `STEP_COLUMNS`（当前只有 `elapsed_ms`，见 `db.rs:228`）
追加两列：

```rust
("structured",   "\"structured\" text NOT NULL DEFAULT ''"),
("output_paths", "\"output_paths\" text NOT NULL DEFAULT ''"),
```

`structured` 存 `serde_json::to_string`（`Value::Null` → 空串，不占空间）；
`output_paths` 存换行分隔的路径列表（不引入 JSON 数组，读侧 `split('\n')` 即可）。

**additive 语义**：老 `state.db` 直接 `ALTER TABLE ADD COLUMN`，无需删库。
读侧必须把空串当「未知/无」而不是「空对象」—— 与 `elapsed_ms=0` 的既有约定一致
（AGENTS.md 已写明这条规则）。

`domain/run.rs`：`RunStep` 加两个字段，沿用 `STEP_FIELD_CAP` 截断
（structured 也要 cap —— 一个大 diff 能很大）。

### 消费方

- `komo run inspect <id>`（`cli/inspect.rs`）：有 structured 时以缩进 JSON 展示；
  有 output_paths 时列出路径（这是 operator 事后翻完整输出的入口）。
- `apps/app` 的 tool-call 渲染：`features/chat/ToolCalls.tsx` 的折叠区里加
  structured 展示。**注意**：live event 与 ledger 的字段要一致，否则会出现
  「运行时看到一种、刷新后看到另一种」的跳变（AGENTS.md 里对 `elapsed_ms` 已有
  同样的告警）。`TurnEvent::ToolFinished` 一并带上 structured。

## 涉及文件

`src/infra/persistence/db.rs`（STEP_COLUMNS + 模型 + 读写）·
`crates/komo-core/src/domain/run.rs` · `src/services/tool_execution/mod.rs`（记录）·
`src/cli/inspect.rs` · `apps/app/src/features/chat/ToolCalls.tsx` + wire 类型

## 验收

- 用**旧** state.db 启动：不报错，新列自动补上，旧 step 的 structured 读作「无」。
- `edit` 一次后 `komo run inspect` 能看到 diff 统计。
- `bun run check` 通过；web 端运行中与刷新后显示一致（无跳变）。
