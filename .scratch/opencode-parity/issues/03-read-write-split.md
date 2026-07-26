# 03 — `file` 拆成 `read` + `write`，`read` 补分页/目录/二进制

Status: done (2026-07-26) — `cargo test` 492 passed
Phase: 1 工具集 · 依赖: 01 · 阻塞: 04 06 10 16

## 落地记录

- 新 `tools/read.rs`：`offset`/`limit` 分页（≤2000 行或 50KB，先到者为准）+ 行号
  gutter + `Continue with offset=N` 续读提示 + `structured.next_offset`；目录列举
  （目录带 `/` 后缀，同样分页）；二进制拒读（扩展名表 + 图片 magic bytes + NUL /
  不可打印比例 >30%）；非法 UTF-8 明确报错（不 lossy）；单行 >2000 字符截断；
  空文件单独措辞；>20MB 直接拒绝并指路 `grep`/`shell`。
- 新 `tools/write.rs`：等价旧 `file{action:"write"}`，加拒绝理由转达 +
  **stale 保护**。旧 `tools/file.rs` 已删除，不留 alias。
- 新 `tools/fs_common.rs`：路径解析（相对路径锚到 workspace 根）+ 读/写审批 +
  拒绝文本。**越界是 `ToolError::Denied` 而非 prompt** —— workspace 白名单是
  floor，和 shell 的 hardline 同层。为此给 `Workspace` 加了
  `resolve_contained`（原 `resolve` 是私有的）。
- 新 `services/file_mutation.rs`（04 的原语，本 issue 提前落地因为 `write` 的验收
  依赖它）：`snapshot` + `write_if_unchanged` + BOM 保留。范围限定在**审批窗口**：
  提示前快照、写前比对，不引入跨 turn 的「必须先 read」状态机。
- `test_support` 的默认 approver 从「拒绝一切」改成 `SafeOnly`（`Risk::Safe` 放行、
  其余拒绝）—— 真实 approver 都是这个语义，一律拒绝会让 read 类测试断言在一个
  生产上不存在的策略上。
- 顺带修的文档漂移：`ActionRef::File` 注释（`file` tool → `read`/`write`）、
  `ActionRef::Network` 的「没有工具构造它」注释与多余的 `#[allow(dead_code)]`
  （`web_fetch` 早就在构造它了 —— 这块 issue 07 可以划掉）、AGENTS.md 的
  redaction / deny-only / task-local 三处描述 + 新增 read/write/fs_common 段落。
- 新增 `READ_GUIDANCE` 系统提示（gated on `read`）：别用 `cat`/`ls` 走 shell，
  长文件要按 offset continue 读完再下结论。
- 手工验证：`komo doctor` 正常起（新 wiring 构造成功）；
  `komo policy check file …` 证明 `category="file" access="read"` 的 deny 规则
  对拆分后的工具照旧生效 —— **用户现有 config.toml 无需改动**。

## 与原计划的偏差

`read` 的图片分支只报「这是 PNG/JPEG，komo 还不能把图片给模型」，附件通道仍归 16。

## 目标

现在 `file{action:"read"}`（[src/tools/file.rs](../../../src/tools/file.rs)）整文件读、
64KB 硬截断、无分页 —— **大文件后半段不可达**，二进制被 `from_utf8_lossy` 糊进上下文。
对照 `opencode/packages/core/src/tool/read.ts` + `read-filesystem.ts`。

## 设计

### `read`

```
path: string            相对 workspace / workspace 内绝对路径
offset?: int (1-based)  起始行，或目录项序号
limit?:  int            最多行数（上限 2000）
```

- **分页**：返回 `TextPage{content, offset, truncated, next}`，`next` 是续读起点，
  文本里以 `…续读：offset=<next>` 提示模型。上限沿用 v2 的 `MAX_READ_LINES=2000`
  / `MAX_READ_BYTES=50KB`（两者先到先截）。
- **行号**：输出带 `<行号>│<内容>` 前缀（模型据此给 `edit` 定位；v2 靠 offset，
  komo 加行号成本低、对 `edit` 帮助大）。
- **目录**：`path` 是目录时列举条目（同样分页），不再报错。
- **二进制拒读**：扩展名表 + 魔数 + 不可打印字节比例 > 30%（照抄
  `read-filesystem.ts` 的 `binary()`），报明确错误而非糊进上下文。
- **单行截断**：超过 2000 字符的行截断并标注（minified JS 一行能吃掉整个预算）。
- **UTF-8 非法**：明确报错，不 lossy。
- 图片：本期**只识别并报「需要附件通道」**，真正返回图片见 16。

### `write`

行为等价现在的 `file{action:"write"}`，加两件：
- `Risk::Normal` 审批改走 `ctx.decide`（01 的反馈通道）
- 写前 stale 校验（见 04 的 `FileMutation` 原语）：文件在审批期间被改过 → 报
  「文件已变化，请重新 read 后再写」

### 共用

抽 `services/file_access.rs`（或 `tools/fs_common.rs`）承载：workspace 解析 +
`ActionRef::File` 审批 + BOM/换行探测，供 `read`/`write`/`edit`/`apply_patch` 共用。

**`file` 工具删除**，不保留 alias：komo 无外部工具契约，留两只只会让模型二选一。
policy 侧 `ActionRef::File` / `category="file"` 不变，用户 config 无需改动。

## 涉及文件

新 `src/tools/read.rs` · 新 `src/tools/write.rs` · 删 `src/tools/file.rs` ·
新 `src/tools/fs_common.rs` · `src/tools/mod.rs` · `src/cli/wiring.rs` ·
`src/domain/prompt.rs`（工具引导文案里 `file` 的提法）

## 验收

- 一个 5000 行文件：`read` 拿到 1-2000 行 + `next=2001`，`offset=2001` 拿到后续。
- 目录路径 → 条目列表；`.png` → 二进制拒读错误；非法 UTF-8 → 明确报错。
- 单元测试：分页边界（offset 超尾）、超长行截断、BOM 文件读回不带 BOM 污染。
- 现有 `file` 的测试等价迁移到 `read`/`write`。
