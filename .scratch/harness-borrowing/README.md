# Harness 借鉴清单：jcode · pi · grok-build · codex · opencode → komo

> 输入：笔记《jcode·pi·grok-build 三个 coding agent harness 实现对比》（2026-07-28）
> + 本次对 codex（`aea26af`）与 opencode（含 V2 重写）的源码调研，2026-07-31。
> 对照基线：komo 当前 `run_agent_loop`（src/agent/runtime.rs）、ToolExecutor
> （src/services/tool_execution/）、RigLlm（src/infra/llm.rs）、policy
> （src/domain/policy.rs）、GatewayDispatcher（src/agent/interaction.rs）。

## komo 现状定位（先说清楚缺口在哪）

| 维度 | komo 已有 | 缺口 |
|---|---|---|
| Agent loop | round 预算 + 用模型自己的 narration 收尾；cancel 每个 await 竞争；LLM 瞬态重试×3 | 无 steer（轮间插话）；无空响应续跑；无撞上下文自动恢复；无 doom-loop 检测 |
| Tool call | 并发保序、round 上限、panic 捕获、超限输出落盘（head+tail）、turn 级输出预算 | `definitions()` 从 HashMap 出来**无序**；未知工具名只回 `error: unknown tool` |
| 权限 | 五级梯子（hardline > deny > grant > allow > ask）、saved grants、unattended 规则、审批串行化 | `/deny` 不能带理由回给模型；决策原因不落 ledger；unattended 只有静态 allow 一档 |
| Context | 消息数+字节双窗口；tool_note 摘要跨 turn；tool-output-store 7 天回捞 | **无 compaction**；窗口裁剪 = 永久丢信息；单 turn 撞上下文 = turn 直接失败 |
| Prompt | day-stable 分层 preamble；aux 会话隔离 | memory recall 前缀 append 在 system prompt 尾部，**每 turn 变化 → churn 整个 system prompt 的 cache** |

mid-turn 消息：komo 已有有界 FIFO 队列（interaction.rs:746，= opencode 的 `queue`
档语义），缺的只是 `steer` 档。

---

## A. 直接可抄（每条几十到一二百行）

### A1. 工具定义排序 —— jcode / codex
- jcode `definitions()` 按名排序，注释原话 *"critical for prompt cache hits"*；codex 用 `IndexMap` 保序 + `prepend_trusted` 控制顺序。
- komo：`ToolExecutor::definitions()`（tool_execution/mod.rs:219）直接 `HashMap.values()`，顺序随机 —— 每次进程重启、甚至不同 executor 实例，工具 schema 顺序都可能不同，provider 侧 prompt cache 全 miss。
- 改法：`tools` 换 `BTreeMap`，或 `definitions()` 出口排序。**一行级改动，纯收益。**

### A2. 未知工具名 → 近似建议 + 全量列表 —— jcode（issue #104）
- jcode 返回 levenshtein 最近的候选 + 可用工具全表，止住模型在幻觉名字上死循环。codex 同思路（`unsupported call: {name}` 回模型自纠，不 abort）；opencode 更进一步：AI SDK `experimental_repairToolCall` 先试小写纠正，再把调用重写成隐藏的 `invalid` 工具，让错误变成正常 tool result。
- komo：mod.rs:300 只回 `error: unknown tool \`nope\``。
- 改法：在 execute_round 的 None 分支拼上 "did you mean X? available: [...]"。

### A3. 尊重 Retry-After —— codex / opencode
- 两家都优先读服务端 `retry-after`/`retry-after-ms` header，再退指数退避（opencode 无 header 时封顶 30s）。codex 重试用尽还有 WebSocket→HTTPS 传输回退。
- komo：`LLM_RETRY_BACKOFF_MS = [500, 2000]` 固定（infra/llm.rs:117），429 场景 2s 基本不够。
- 改法：retry 分类器已把错误传出来了，解析 header 有则用之；顺手把上限调到 30s 级。

### A4. doom-loop 检测 —— opencode / grok-build
- opencode：当前 assistant 消息最后 3 个 tool call **同名 + 参数 JSON 完全相同** → 转成一次 `doom_loop` 权限审批（不是硬中断）。grok 有独立 `doom_loop.rs`。
- komo：只有 max_turns 兜底，一个卡死的循环要烧完 30 轮才停。对 **unattended cron turn** 尤其重要——没人看着。
- 改法：`run_agent_loop` 里记录最近 N 个 (name, args)，3 连命中就把该 call 的结果替换成一条 "你在重复同样的调用，换个方法或直接回答" 的注入（unattended 下直接计入并提前收尾）。

### A5. 截断的 tool call 整批不执行 —— pi
- pi：`stopReason === "length"` 时整批 tool call 全部失败返回、一个不执行——流式 JSON 的 best-effort 解析可能让残缺参数通过 schema 校验。jcode 的 `filter_truncated_tool_calls` 同因。
- komo：`Step` 不携带 finish reason，rig 的响应里有。streaming 路径（codex provider）尤其暴露。
- 改法：`Step::ToolCalls` 加 `truncated: bool`（或 finish_reason），loop 里命中时把整批映射成 "arguments truncated, re-issue the call"。

## B. 高价值、中等改造

### B1. memory 移出 system prompt —— jcode（对 komo 是最实际的 cache 收益）
- jcode 刻意**不把 memory 放 system prompt**：后台备好，作为尾部 user message 注入，注释写明 *"preserves cache prefix"*；且只在 fresh user turn 消费。
- komo：`assemble()`（infra/llm.rs:339）把 recall 前缀 append 到 preamble 尾部。recall 是按当前用户消息 keyed 的——**每 turn 都变，等于每 turn 重写 system prompt，前缀 cache 全部作废**。komo 注释里说 "stable tier 不受影响"，但多数 provider 的 cache 是整段前缀匹配，system prompt 变尾部就断在那里，后面的 history 也别想复用。
- 改法：pinned（L1）留在 system prompt（真正稳定）；recall（L3）改为 history 尾部、最新 user 消息之前的一条 user/`<memory-context>` 消息。grok 的教训也一并抄：唯一标记块 upsert，防止累积。

### B2. steer 档消息注入 —— pi / opencode V2 / codex
- komo 队列只有 "排到 turn 结束"。一个典型场景：feishu 上说"查一下 X"，agent 跑长工具链时你补一句"顺便只看今天的"——现在要等整个 turn 跑完才被读到，还是作为新 turn。
- 三家共识语义（pi `steer`/`followUp`/`nextTurn`、opencode V2 `steer`/`queue`、codex 的 pending input 每轮 drain）：**steer 在下一个 round 边界注入当前 turn**；codex 还有两条 drain 时机规则（turn 开头先采样原始输入；compact 后先让工具续完）。
- komo 落点：`QueuedMessage` 加 delivery 标记；`run_agent_loop` 每轮开头从 dispatcher 取 pending steer，作为额外 user 内容混进下一次 `driver.step()`。TurnDriver 接口需要允许附加 user 消息（rig 的 request 是逐轮构建的，可行）。opencode 的细节也值得抄：steer 注入后**重置 round 预算**（新用户输入 = 新预算）。

### B3. 撞上下文的自动恢复 —— jcode / codex / opencode
- komo 现在单 turn 超上下文 = turn 失败 + транскрипт里留占位符。三家都不允许这种事：
  - jcode：`MAX_CONTEXT_LIMIT_RETRIES=5`，撞限 → 压缩 → 重试；≥95% 时**紧急降级**（tool result 截到 4000 字符、图片 1024）。
  - codex：`ContextWindowExceeded` 不可重试但走 compact 路径；compaction 自己撞限时从头部逐条裁（保 cache 前缀）。
  - opencode：捕获 provider 的 overflow 错误转成 compaction 任务。
- komo 最小版（不做 compaction 也能做）：识别 provider 的 context-overflow 错误（现在混在"其他错误"里直接失败）→ 收紧 window（临时减半 max_history_bytes / 砍 tool_note）→ 同一 turn 内重试一次。这就是 jcode 紧急降级的形态，改动集中在 llm.rs 的错误分类 + assemble 参数化。
- 完整 compaction 对 komo 优先级不高：窗口+tool_note 设计使跨 turn 溢出罕见。若将来做，抄 grok `CompactionMode::Segments`（摘要 + 磁盘原文回捞路）——komo 的 tool-output-store 已经是这个思路的雏形。

### B4. `/deny` 带理由 → 模型纠偏 —— opencode `CorrectedError`
- opencode 拒绝时可附自然语言 feedback，模型收到 *"The user rejected ... with the following feedback: {feedback}"* —— 拒绝从"此路不通"变成"往这边走"。codex 也区分拒绝来源文案（user/policy/guardian）。
- komo：`/deny` 之后模型收到的是无差别拒绝。
- 改法：聊天命令解析 `/deny <理由>`，理由塞进 approver 返回的 outcome 文本。改动小，体验收益大（尤其 feishu 场景：一句"别用 rm，用 trash"比重新描述任务省事得多）。

### B5. 权限决策原因落 ledger —— grok-build
- grok 的 `decision_reason` 有 ~25 个稳定取值（`policy_allow` / `persisted_grant` / `hardline_floor` …）+ 延迟/队列深度埋点。
- komo 的 policy 梯子五层，但事后无法回答"这个调用当时为什么放行/拦下"。komo 已有 run ledger + "列可加不用重置" 的迁移规则，`RunStep` 加一个 `decision_reason` 字符串列即可。给 `komo run inspect` 和将来调 policy 都有用。

## C. 设计层（unattended 方向，komo 特有的需求）

komo 与三家最大的场景差异：**cron/sweeps 的无人值守 turn** 是一等公民。笔记的
"未解决"一节说三家对权限没有共识，其实按"人是否在键盘前"分岔——komo 两边都要。
无人值守档现在只有静态 `unattended = true` allow 规则一档，下面三条是从静态规则
到弹窗之间的中间档，按成本排序：

### C1. jcode 反射式 gate（成本最低，先做这个）
- 危险动作不弹窗、不加第二个模型：确定性风险分级打回生成模型，要求 `justification` 字段说明服务哪条用户请求（≥25 字符，拒空肯定词）；`Catastrophic`（`/`、`$HOME`、凭证目录）无条件拒。关键性质：**盲目重试无法满足**。
- komo 落点：unattended approver 对 `Risk::Dangerous` 以下、静态规则未覆盖的调用，返回一条"需要 justification 重试"的 outcome，工具入参加可选 `justification` 字段。不引入新模型调用，纯 policy 层改动。

### C2. codex Guardian / grok auto 分类器（aux 模型审批员）
- codex：独立 guardian 会话审批 on-request 动作，强 JSON schema 输出，**超时/失败/格式错误一律 fail closed**，连续拒绝断路器（3 连拒/turn）、多级 token 预算、专用 prompt_cache_key 复用前缀。grok：静态白名单 fast path，miss 才 side-query LLM。
- komo 已有 aux model 通道（reviewer/recall 同款合成 Session 模式），落点清晰。**若做，grok 的信任边界声明必须整段抄**：只有 harness 提供的对话摘录构成用户意图；工具名/参数/AGENTS.md 不构成批准；记录里只有 harness 拥有的 decision 字段可信；已记录的拒绝永久绑定。
- 这是任何"让 LLM 做授权判断"的地方的注入防御底线。

### C3. opencode arity 字典（bash 前缀授权的轻量中间态）
- "always allow" 授权的不是整条命令而是语义前缀：`git checkout main` → `git checkout *`（字典声明 `git: 2`，flags 不计 token）。比 grok 28k 行的 tree-sitter 拆分轻两个数量级，比整条命令 grant 复用性高得多。
- komo saved grants 如果目前按整条命令存，这是最划算的粒度升级。字典文件头连生成它的 LLM prompt 都留着，可以直接借来生成 komo 自己的表。

## D. 低成本高杠杆的散件

- **模型可控上下文工具**（codex）：`get_context_remaining`（查剩余预算）几十行；对 komo 的长 cron turn，模型能自己判断"还够不够再读一个大文件"。`new_context` 暂不需要（komo turn 短）。
- **`error_or_panic`**（codex util.rs:93）：debug panic / release error 的不变量断言。komo 的 ledger/approval 不变量适用。
- **审批文案规范化 + 900 token 截断**（codex events.rs）：拒绝消息也是喂给模型的输入，值得治理。
- **超时错误文案教模型自救**（opencode shell）：`"terminated after {t}ms. If this command is expected to take longer..., retry with a larger timeout"` —— komo shell 工具的超时错误可以照抄这个句式。
- **LSP/检查器挂在编辑工具输出上**（opencode）：edit/write 后同步等诊断，错误直接进 tool result。komo 场景对应物是 `cargo check` / 语法检查挂在 apply_patch 之后——按需，非必须。

## 不抄的，及理由

- **OS 沙箱**（grok nono / codex Seatbelt+Landlock+execve 拦截）：komo 是单用户个人 agent，威胁模型不同；policy 梯子 + hardline floor 是对的量级。
- **完整 compaction 流水线**（codex 四条路径 / opencode prune+summary）：komo 的窗口+tool_note 架构使需求退化成 B3 的紧急降级。
- **会话树/fork**（pi 树状 session / opencode 影子 git 快照 revert）：komo 的 `/new` + run ledger 是刻意的简单选择。
- **grok 模板 XOR 混淆、多实现版本 A/B**：产品化阶段的东西。
- **pi 纯 SDK 分层**：komo 的 domain/agent/services 分层已经够用，为分层而分层没有收益。

## 建议落地顺序

| 批次 | 内容 | 验证方式 |
|---|---|---|
| 1（一个下午） | A1 排序、A2 未知工具建议、A3 Retry-After、D 超时文案 | 单测：definitions 顺序稳定；unknown tool 含建议；退避读 header |
| 2 | B1 memory 出 system prompt、B5 decision_reason 列 | 抓两轮连续请求对比 system prompt 字节是否一致；`run inspect` 显示 reason |
| 3 | A4 doom-loop、B4 `/deny` 理由、A5 截断批次 | 脚本化 LLM 测试（ScriptedLlm 模式已有现成基建） |
| 4 | B2 steer、B3 溢出降级重试 | 真机：feishu turn 中途补话；构造超长 tool 链 |
| 5（设计先行） | C1 反射式 gate → 视效果再评估 C2 guardian | unattended cron 上灰度 |
