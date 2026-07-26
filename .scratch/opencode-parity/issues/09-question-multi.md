# 09 — `ask_user` 对齐 v2 `question`：多问题 / 多选 / 自定义答案

Status: ready-for-agent
Phase: 1 工具集（小项）· 依赖: 无

## 目标

[src/tools/ask_user.rs](../../../src/tools/ask_user.rs) 现在：单问题、字符串选项列表、
单选、无「自己输入」。对照 `opencode/packages/core/src/tool/question.ts`。

## 设计

```
questions: [{
  question: string,
  header?:  string,          短标签
  options:  [string],
  multiple?: bool,           默认 false
  custom?:   bool,           默认 true —— 自动追加「自己输入」项
}]
```

- **一次多问**：一轮里问完 2-3 个相关问题，而不是连着调三次（每次都吃一轮往返 +
  一次 clarify 预算）。
- **clarify 预算按「调用」计，不按「问题数」计**：现有
  `ClarifyState::try_claim_budget`（2 次/turn）语义保持 —— 一次多问仍算一次。
  这正是多问的收益所在。
- **多选**：答案回 `Vec<String>`；渲染上提示可多选（如「可多选，用逗号分隔」）。
- **custom**：默认在选项末尾追加「其它（直接输入）」，所以 SKILL/prompt 侧不该再
  自己写「其它」选项 —— 在工具 description 里写明这条（v2 的 usage notes 也这么写）。
- 输出文本沿用 v2 形状：`User has answered your questions: "Q"="A", … You can now
  continue with the user's answers in mind.`
- 非交互降级（sweeps / aux / api / detached）不变：回 `NO_ANSWER` 引导文本。

## 涉及文件

`src/tools/ask_user.rs` · `src/services/clarify.rs`（答案载荷从 `String` 改结构化，
按问题序号回填）· `src/agent/interaction.rs`（下一条普通消息路由进 clarify 的解析：
多问时按行/序号切分）· `src/tui/mod.rs`（本地模式同路）

## 验收

- 一次问 2 个问题，用户一条回复答完两个 → 工具输出含两组 Q=A。
- 多选题回「1,3」→ 两个答案都进结果。
- 未答的问题渲染成 `Unanswered`，不阻塞其余答案。
- clarify 预算仍是 2 次/turn（多问不多扣）。
