# 11 — `structured` + `output_paths` 落 `RunStep`（additive 迁移）

Status: partly done (2026-07-28) — Rust 侧完成；web 渲染 + TurnEvent 字段仍未做
Phase: 2 管线 · 依赖: 10（output_paths 来源）· 建议与 10 同批提交

## 落地记录

`STEP_COLUMNS` 加 `structured` / `output_paths` 两个 additive 列，`RunStep` 加
对应字段（`structured: serde_json::Value`、`output_paths: Vec<String>`，都
`#[serde(default)]`）。db 侧映射：`Value::Null` ⇄ 空串（不存 `"null"` 四个字节，
这样"工具没有 structured"和"列存在之前写的行"读起来一样）、`Vec` ⇄ 换行分隔。
解析失败也读成 `Null`：ledger 是审计记录，一个坏 cell 不该让整次读取失败。

**cap 语义与 issue 不同**：structured 超过 `STEP_FIELD_CAP` 时**整体替换**成
`{"_elided": …, "bytes": N}`，不截断 —— 截断后的 JSON 解析不了，等于逼每个读侧把
它当损坏数据处理。

executor 里 `ToolOutput.structured` 原来在 `Ok(out) => Ok(out.text)` 处被丢掉，
现在捕获到局部变量再落 step；模型看到的仍然只有 text（第三视图的意义就是不烧 token）。

`komo run inspect` 渲染缩进 JSON + 输出文件路径。

### 未做（有意，需要一起落）

`TurnEvent::ToolFinished` **没有**加 `structured`，`apps/app` 的 tool-call 折叠区
也没渲染。原因：assistant-ui 的 tool-call part 形状里没有第三视图的位置，要么改
vendored kit 要么另开通道；而 issue 自己写明 live 与 ledger 必须一致 —— 只加 wire
字段不渲染是死字段，只渲染 ledger 不改 event 就会出现"运行中一种、刷新后另一种"的
跳变。所以两者作为一个单元留到 UI 侧一起做，现在 web 端行为完全未变（无跳变风险）。

验证：db roundtrip 断言两列往返 + 空值读成 `Null`；executor 3 个测试
（structured 到 ledger 不到模型 / 失败调用不记 structured / 超限替换）；
用**改动前的二进制**建的 state.db 跑 `komo skills audit`（`steps_by_tool` 会
SELECT 全部 step 列）成功返回 → additive 迁移在旧库上生效，无需删库。

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
