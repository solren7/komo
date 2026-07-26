# 04 — `edit` 工具：精确替换 + stale 保护 + BOM/换行保留 + diff

Status: ready-for-agent
Phase: 1 工具集 · 依赖: 01 03 · 阻塞: 06

## 目标

**全程序最大的缺口**。komo 现在改一行要整文件 `write`：token 浪费、且
`tokio::fs::write` 会静默覆盖并发修改。对照 `opencode/packages/core/src/tool/edit.ts`。

## 设计

```
path: string
oldString: string       必须精确匹配（含空白与缩进）
newString: string       必须与 oldString 不同
replaceAll?: bool       默认 false
```

按 v2 的顺序与错误文本（这些文本本身就是给模型的教学）：

1. `oldString == newString` → `"No changes to apply: oldString and newString are identical."`
2. `oldString == ""` → `"oldString must not be empty. Use write to create or overwrite a file."`
3. 解析路径 → `ActionRef::File{write:true}` 审批（`ctx.decide`，走 01 的反馈通道）
4. 读原文件 → **探测换行风格**（含 `\r\n` 即 CRLF）并把 `oldString`/`newString`
   转成同风格；**探测 BOM** 并在写回时恢复
5. 数精确出现次数：
   - 0 → `"Could not find oldString in the file. It must match exactly, including whitespace and indentation."`
   - \>1 且 `replaceAll != true` → `"Found multiple exact matches for oldString. Provide more surrounding context or set replaceAll to true."`
6. 替换 → **`write_if_unchanged{expected: 步骤 4 读到的 bytes}`**：写前重读比对，
   不一致 → `"File changed after permission approval. Read it again before editing."`
7. 返回 `ToolOutput`：
   - `text` = `Edited file successfully: <path>` + `Replacements: <n>` + 首 6 行
     `-`/`+` 预览（每行截 240 字符）
   - `structured` = `{file, additions, deletions, patch}`（unified diff，落 11 的 ledger 列）

**不做 fuzzy 匹配** —— v2 刻意不做（见 PRD 非目标）。

## `FileMutation` 原语

新 `src/services/file_mutation.rs`，两个函数供 `write`/`edit`/`apply_patch` 共用：
- `write_text_preserving_bom(path, content)`
- `write_if_unchanged(path, expected: &[u8], content) -> Result<_, StaleContent>`

diff 用 `similar` crate（unified diff + 行级增删计数），比手写稳。

## 涉及文件

新 `src/tools/edit.rs` · 新 `src/services/file_mutation.rs` ·
`src/tools/write.rs`（改用同原语）· `src/tools/mod.rs` · `src/cli/wiring.rs` ·
`Cargo.toml`（`similar`）

## 验收

- 单元测试覆盖上述 6 类错误文本各一条。
- CRLF 文件用 LF 的 `oldString` 能命中，写回仍是 CRLF；BOM 文件写回仍带 BOM。
- stale 测试：审批回调里篡改文件 → 报 stale，文件内容不被覆盖。
- `replaceAll` 计数正确；`structured.patch` 是可 `git apply` 的 unified diff。
