# PRD: opencode v2 工具体系对齐（工具集 + 执行管线 + 权限模型）

Status: ready-for-agent — 分 5 期、16 个 issue，见 `issues/`

## 背景

`.scratch/tool-trait-v2/PRD.md` 已经完成了 **trait 形态**的对齐（`ToolOutput{title,text,structured}`、
`ToolError`、`parse_args`、`ToolContext`、executor 持 approver），7/15 个工具已迁移。
那次刻意把「行为」留在原地，只动形状。

本 PRD 处理剩下的三层差距（对照 `~/01-code/opencode/packages/core/src/tool/`）：

1. **工具集**：v2 有 `read/write/edit/apply_patch/glob/grep`，komo 只有一只
   整文件读写的 `file`，没有任何搜索工具。
2. **执行管线**：v2 的 `ToolOutputStore`（超限输出落盘 + 双端预览 + 可回搜）、
   `structured` 落库、按权限过滤工具目录、协作式取消。
3. **权限模型**：持久化 always-allow、带反馈的拒绝、工具目录级 deny。

## 目标

- 让 komo 具备**改代码的完整闭环**：定位（grep/glob）→ 读（分页）→ 精确改（edit/apply_patch），
  而不是「整文件重写 + 让模型自己拼 shell 命令」。
- 让**超限输出不再丢尾部**：编译错误、测试失败摘要通常正好在尾部，现在被硬截断吃掉。
- 让**权限决策可积累、可解释**：以后都允许写盘，拒绝时能带一句话反馈回模型。

## 非目标（刻意不做）

| 不做 | 理由 |
|---|---|
| `lsp` / `task`（子代理）/ `plan mode` / `code-mode` | opencode v2 自己都还没从 v1 搬（`builtins.ts` 的 TODO 明列）。komo 的 `delegate` 弱于 v1 `task`，但那是独立议题 |
| `edit` 的 fuzzy 匹配（line-trimmed / block-anchor / 缩进纠正） | v2 **刻意**只做精确替换 + 明确报错，把 fuzzy 列为「exact 稳定之后再说」。komo 直接跟这个取舍 |
| Location + `external_directory` 越界审批 | komo 的 `Workspace` 白名单硬拒绝是刻意选择（个人 agent，不跨仓库）。将来要跨仓库再单开 |
| `input`/`output` 双 schema + 双向校验 | 已在 tool-trait-v2 PRD 里论证过：Rust 侧 serde 解码即校验，output 校验收益低 |

## 关键设计决策

### D1 — `file` 拆成三只独立工具

`file{action}` → `read` / `write` / `edit`（+ 新增 `apply_patch` / `grep` / `glob`）。
独立工具的 schema 更精确，模型不会在 action 枚举上犯错，也才能给 `read` 加
`offset/limit` 而不污染 `write`。

**策略层零破坏**：`ActionRef::File{path, write}` 与 config 的
`category = "file"` / `access = "read"|"write"` 规则**不变**，用户现有
`~/.komo/config.toml` 无需改动。`shell` 的 hardline floor 同理不动。

代价：工具数 15 → 20，system prompt 的工具名列表变长。可接受（`SystemPromptBuilder`
只列名字，不列 schema）。

### D2 — 搜索用 ripgrep 的**库**，不依赖外部 `rg` 二进制

opencode 走 `Ripgrep.Service` 调外部二进制（它自己管下载）。komo 是单二进制分发，
不能假设宿主/容器里有 `rg`。改用 ripgrep 自身的组件库：`ignore`（尊重
`.gitignore` 的并行遍历）+ `globset` + `grep-searcher`/`grep-regex`。`regex` 和
`walkdir` 已在 `Cargo.lock` 里（tracing-subscriber 带的），增量可控。

### D3 — stale 保护只做「同调用内」，不做「必须先 Read」

v2 `edit`/`write` 的 `writeIfUnchanged({expected})` 是**同一次调用内**读入原内容 →
审批 → 写前比对 bytes，防的是「审批期间文件被改」这个 TOCTOU 窗口。

komo 照抄这个范围。**不**引入 Claude Code 式「未 Read 过的文件不许 edit」的跨 turn
状态（那需要 session 级已读集合，是另一套机制）。

### D4 — 超限输出落盘（`ToolOutputStore`）

- 位置 `~/.komo/tool-output/<session-id>/<call-id>.txt`，保留 7 天。
- 模型看到的是 **head + tail 双端预览** + 中间一行 marker + 完整文件的绝对路径
  （现在是单向硬截断，尾部永久丢失 —— 见 `services/tool_execution/result.rs`）。
- `read` / `grep` 显式接受这个目录下的绝对路径（在 `Workspace` 白名单之外单开一个
  **只读** managed 根），否则模型拿到路径也读不了。
- 清理挂在 **gateway 启动 + store 内 1 小时去抖**，不新增 cron schedule。

### D5 — Approver 从 `bool` 改成 `Decision`

```rust
pub enum Decision {
    Allow,
    Deny { feedback: Option<String> },
}
```

`ToolContext::approve(&req) -> bool` **保留**（现有工具零改动，`Allow => true`），
新增 `ToolContext::decide(&req) -> Decision`；关心反馈的工具（shell/write/edit）用
后者，把 feedback 作为 `ToolError::Denied(msg)` 交回模型 —— 用户就能说
「别用 `rm`，用 `trash`」而不是干巴巴一个 denied。

交互侧：chat 支持 `/deny <理由>`，TUI 在按 `n` 后可选输入一行。
影响面：6 个真实 `Approver` impl + ~12 个测试 double。

### D6 — 持久化审批写 `~/.komo/permissions.json`，不写 state.db

state.db 是 disposable（AGENTS.md：删了就重置）。「以后都允许」是 durable 个人数据，
和 memory.db / kanban.db / cron.db 同类，所以进自己的文件。选 JSON 而非第四个 db：
条目少、operator 应当能直接读改。

判定优先级（从强到弱）：
**工具 hardline floor** > config `[policy]` deny > saved allow > config `[policy]`
allow/default > 交互 ask。saved allow **永远不能**越过 deny 规则或 hardline，
也**不覆盖** `Risk::Dangerous`（那个仍需 `include_dangerous`）。

### D7 — `structured` + `output_paths` 落 `RunStep`

`run_step_records` 加两个**additive**列（`ensure_columns` 已经给 `STEP_COLUMNS` 铺好路，
见 `infra/persistence/db.rs:228`）：`structured TEXT NOT NULL DEFAULT ''`、
`output_paths TEXT NOT NULL DEFAULT ''`。老 state.db 不用删。
消费方：`komo run inspect` + `apps/app` 的 tool-call 渲染。

### D8 — 按 policy 过滤工具目录

executor 持一份 `Policy`，`definitions()` → `definitions_for(channel)`：被
`deny *` 的工具**整只不进 schema、也不进 prompt 的工具名列表**（对齐 v2
`ToolRegistry.materialize` 的 `whollyDisabled`）。现在这类工具模型照样看得见、
照样调、照样被拒 —— 白烧 token 和轮次。

注意 prompt 缓存：工具名列表会因 channel 而异，但同一 channel 内稳定，
按会话仍然命中缓存。

### D9 — `Content::file`（图片/附件）单列最后

要动 `TurnDriver`/`ToolOutcome`（rig 侧 content 现在是 `String`）+ 每个 channel 的
投递路径，是全程序里唯一跨层的高风险项，与其余 15 项解耦，放最后。

## 分期与依赖

| 期 | issue | 依赖 | 规模 |
|---|---|---|---|
| **0 地基** | `01-approval-decision` · `02-trait-v2-finish` | — | 中·中 |
| **1 工具集** | `03-read-write-split` → `04-edit-tool` → `06-apply-patch` · `05-grep-glob` · `07-webfetch-parity` · `08-skill-files` · `09-question-multi` | 01 | 大 |
| **2 管线** | `10-tool-output-store` · `11-structured-ledger` · `12-catalog-filtering` · `13-shell-parity` · `14-cancel-token` | 03/04（read 回搜 managed 输出）、01 | 大 |
| **3 权限** | `15-saved-approvals` | 01 | 中 |
| **4 附件** | `16-content-file` | 03 | 大·高风险 |

**为什么 0 期先做**：01（Decision）和 02（删 trait 兼容桥）都是**广度改动**。
1 期要新增 5 只工具，如果先写工具再改 approver 签名和 trait 形态，等于把同样的
文件改两遍。先收敛地基，新工具一次写对。

**为什么 grep/glob 可以和 edit 并行**：它们只读、不碰 `FileMutation` 原语，
唯一的交集是 `Workspace` 解析，冲突面小。

## 验收（程序级）

- 一次「定位 → 读 → 精确改」的真实任务，全程不经 `shell`：
  `grep` 找到符号 → `read` 带 offset 翻到那一段 → `edit` 精确替换 → 返回 diff。
- 一条产生 500KB 输出的命令：模型看到首尾预览 + 文件路径，`grep` 能在该文件里
  搜到中段内容（今天中段直接消失）。
- `komo policy saved list` 能列出运行中累积的 always-allow；删掉它，下次重新问。
- 拒绝一次 shell 调用并附「用 trash 代替 rm」，模型下一轮据此改写命令。
- `cargo test` 全绿；`komo run inspect` 显示 structured + 输出文件路径。
