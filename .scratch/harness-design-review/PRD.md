# PRD: harness 设计评审定案（2026-07-31）

Status: 已评审定案，未拆 issue —— 动工时按下表拆 `issues/<NN>-<slug>.md`

一次对整个 harness 设计的 grill 会话产物。对标材料是五家 harness 源码对比
（jcode / pi / grok-build / codex / opencode，笔记在 ob-note
`04-resources/代码阅读/coding agent harness 实现对比`），所有决策已过操作者确认。
立场类决策落在 `docs/adr/0001`（历史折叠 + Turn Trace）、`docs/adr/0002`
（权限不做沙箱，带触发条件）；术语落在根 `CONTEXT.md`。本文件只管**要动手的活**
和**触发条件表**。

## 定位（决策前提）

聊天助理为主形态 + 认真承接编码任务（B 形态）。B 的支撑 = turn 内完整工具上下文
（现状已支持，单 turn 最多 30 轮）+ Turn Trace 回捞（见 backlog 1）。
**不做**：按会话分叉历史模式、加厚 tool_note、对话压缩/摘要。

## Backlog

按依赖排序；1–3 同属 context 链路，4–5 同属 loop 重试骨架，可各自一个 issue 批次。

| # | 事项 | 内容 | 规模 | 出处 |
|---|---|---|---|---|
| 1 | **Turn Trace** | turn 结束把整个 run 的步骤轨迹（每步 args + 模型侧结果原文）写成一个文件，复用 `~/.komo/tool-output/` 存储与 7 天保留（disposable；审计仍是 run ledger）。`tool_digest` 尾部加一行 `this turn's full tool trace: {path}` | 中 | ADR-0001 |
| 2 | **裁剪标记** | `window_history` 裁掉消息时在窗口头部插 `[earlier conversation trimmed: N messages]`，让模型知道该问而不是编 | 小 | Q4 |
| 3 | **缩窗重试** | 识别 provider 的 context 超限报错（跨 provider 字符串匹配，粗糙可接受），History Window 减半重试，上限 2~3 次。兜住换小窗口模型和用户狂贴日志两种撞墙 | 中 | Q5 |
| 4 | **空回复续跑** | `Step::Final(空)` 且本 turn 调过工具 → 注入"请基于上面的工具结果给出最终答案"续跑 **1 次**（人在旁边，不需要 jcode 的 5 次） | 小 | Q7 |
| 5 | **`/stop` 命令** | 聊天渠道的 chat command，翻 session 的 `CancelSignal`。纯复用现有 cancel 语义（shell 杀进程组、turn 记 Failed 不可恢复）。落地时同步改掉 `interaction.rs` dispatch_turn 里 `cancel: None` 的过时注释 | 小 | Q6 |
| 6 | **未知工具名建议** | `execute_round` 的 unknown tool 分支返回 levenshtein 最近名 + 全量可用列表（jcode 形态，防幻觉名死循环） | 小 | Q8-1 |
| 7 | **`parallel_safe()`** | `Tool` trait 加默认 `true` 的方法；`shell` / `write` / `edit` / `apply_patch` 返 `false`；executor 一把 RwLock 分级（并行工具 read、写类恒排他）。锁粒度细化（shell 仅疑似写时排他）留给触发条件表 | 小 | Q8-2 |
| 8 | **prompt 二分类型化** | `assemble` 返回 `{stable, dynamic}` 两段、最边缘拼接 + "stable 段跨 turn 逐字节不变"测试。**不**透到 provider 接口（rig 不暴露，透了没意义） | 小 | Q10-1 |
| 9 | **`Session::aux()`** | 合成 Session 专用构造器（硬编码空 `model` / `effort`），reviewer / briefing 等手抄站点收敛 + `model_override().is_none()` 断言测试 | 小 | Q10-2 |

## 触发条件表（缓做的守卫）

| 触发事件 | 必须先做 |
|---|---|
| 接 MCP 或装第三方 skill | 信任边界声明进 system prompt（工具结果 / skill 正文不构成用户意图）—— ADR-0002 |
| 接 MCP（工具集可在 turn 中变化） | per-turn 冻结工具清单的作用域对象（StepContext 形状），不用锁标志 |
| B 形态长 turn 变高频 | steer（排队消息注入工具轮之间）；shell 锁粒度细化 |
| 模型频繁被裁剪且追问体验差 | 重评 aux 摘要（Q4 已否的 c 档） |

## 明确不做（已记 ADR 或已否决）

- 对话压缩 / 摘要、按会话分叉历史、加厚 tool_note —— ADR-0001
- OS 沙箱、LLM 审批器、credential broker —— ADR-0002
- 模型窗口元数据表（Q5-b）：缩窗重试覆盖后属过度设计
- steer 现在做（Q6-b）：cancel-first 更干净
