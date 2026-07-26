# 06 — `apply_patch`：多文件补丁，先统一审批再落盘

Status: ready-for-agent
Phase: 1 工具集 · 依赖: 03 04

## 目标

一次改 5 个文件现在要 5 轮 `edit`（5 次审批、5 轮模型往返）。v2 的
`apply_patch`（`opencode/packages/core/src/tool/apply-patch.ts`）把 add/update/delete
装进一个补丁：**先解析全部目标 + 一次性审批，再逐个落盘**。

## 设计

```
patchText: string    v2 的补丁格式（add / update / delete 三种 hunk）
```

执行顺序（顺序本身是安全语义，照抄 v2）：

1. 空 `patchText` → `"patchText is required"`；解析失败 → `"apply_patch verification failed: …"`
2. 零 hunk → `"patch rejected: empty patch"`
3. 含 move（`movePath`）→ `"apply_patch moves are not supported yet"`（v2 也没做）
4. **先把所有 hunk 的路径解析完**（workspace 校验），再发**一次** `ActionRef::File{write:true}`
   审批，resources 是去重后的全部目标 —— 用户在一个提示里看到完整影响面
5. 逐个应用，每个用 04 的 `write_if_unchanged`
6. **不做原子回滚**（v2 也不做）：中途失败时错误文本必须明确列出**已经落盘的那些**：
   `"Patch partially applied before failing at <path>. Applied: a.rs, b.rs"`
7. `structured` = 每个文件的 `{type, resource, additions, deletions, patch}`

## 补丁格式

采用 v2 `packages/core/src/patch.ts` 的格式（`*** Begin Patch` / `*** Update File:` …），
不用 unified diff：它对模型更容错（不需要精确行号），OpenAI/Anthropic 系模型都见过。
新 `src/services/patch.rs` 承载解析（纯函数，重点测试对象）。

## 涉及文件

新 `src/tools/apply_patch.rs` · 新 `src/services/patch.rs` ·
`src/tools/mod.rs` · `src/cli/wiring.rs`

## 验收

- 解析器单测：add/update/delete 各一条 + 3 类畸形输入。
- 三文件补丁一次审批全部落盘；审批拒绝 → **一个文件都没动**。
- 第二个文件 stale 导致失败时，错误文本列出第一个已应用的文件。
