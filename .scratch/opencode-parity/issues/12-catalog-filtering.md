# 12 — 按 policy 过滤工具目录（整只被 deny 的工具不进 schema）

Status: done (2026-07-28) — `cargo test` 588 passed
Phase: 2 管线 · 依赖: 无（改动小，收益直接）

## 落地记录

与 issue 设计的**偏差（有意）**：没有做 `definitions_for(channel)`。

原因：rig 0.40 的工具 schema 在 `build_llm` 构造 agent 时就烘进去了，按 channel
变化要走它的 `RequestPatch::active_tools` hook 机制，同时 prompt 工厂也得接
channel 参数 —— 两处新增可变性，换来的只是"channel-scoped deny 规则也能摘工具"。
按 issue 自己写的保守原则（拿不准就保留），channel-scoped deny 本就该保留工具。

实际做法：`Policy::wholly_denied(category, access)` 纯函数 +
`ToolExecutor::drop_policy_denied(&policy)`，wiring 在注册完、读目录前调用一次。
prompt 的工具名单和 schema 都从同一份过滤后的 `definitions()` 出，**结构上**
不可能不一致；一次性过滤也不动 cache-stable 层。

配套修掉一个**前置缺陷**：`build_rule` 原来把 `value` 为空的规则整条丢弃，所以
issue 验收里写的 `category="shell" effect="deny"`（无 match）根本进不了 policy。
现在「同时省略 `match` 和 `value`」= `Matcher::Any` 通配；「有 match 但 value 空」
仍然无效（把 `prefix ""` 读成"全部"是最糟的发现 typo 的方式）。`komo policy list`
把通配规则显示成 `any (whole category)`。

名字→category 映射是 `tool_execution::policy_scope`；表里没有的工具永不过滤。

验证：policy 纯函数 3 个单测 + executor 3 个 + config 解析 2 个；另外用临时
`KOMO_HOME` 起真 gateway 确认日志 `tools withheld by a policy deny rule
tools=apply_patch, edit, shell, write`（同一份 config 里 network 是 value-scoped
deny，`web_fetch` 如预期保留）。

## 目标

现在 `ToolExecutor::definitions()` 是全量目录。被 policy 完全禁掉的工具，模型
**照样看得见、照样调、照样被拒** —— 白烧一轮往返和一段 schema 的 token。

opencode 的 `ToolRegistry.materialize` 用 `whollyDisabled(action, rules)` 判定
（`resource === "*" && effect === "deny"`），命中就把该工具从 definitions 里删掉。

## 设计

`ToolExecutor` 持一份 `Policy`（wiring 已经在构造 `PolicyApprover` 时有它）：

```rust
pub fn definitions_for(&self, channel: Option<&str>) -> Vec<&dyn ToolDefinition>
```

判定「整只禁掉」：该工具的所有可能动作在给定 channel 下**都**被 deny 规则命中且
规则未限定资源（即 config 里没写 `match`/`value`，或写的是通配）。
每只工具需要一个「代表性 category」映射：

| 工具 | category |
|---|---|
| `shell` | shell |
| `read`/`write`/`edit`/`apply_patch`/`grep`/`glob` | file |
| `web_fetch`/`web_search` | network |
| `homeassistant` | homeassistant |
| 其余（time/todo/task/memory/…） | 不参与过滤（无 policy category） |

`file` 类要区分 `access`：只 deny 了 `write` 时不能把 `read`/`grep` 也摘掉。

**保守原则**：拿不准就**保留**该工具。误摘一只工具（模型完全不知道自己有这个能力）
比多留一只（会被拒，但模型能收到解释）糟得多。

### 与 prompt 的一致性

`SystemPromptBuilder::tools(tool_names)` 的名单也要用同一份过滤结果，否则 prompt 说
「你有 shell」而 schema 里没有 —— 模型会去调一个不存在的工具。
`cli/wiring.rs:220` 附近的 `tool_names` 由此改成按 channel 求值 → prompt 工厂需要
接受 channel 参数（`assemble` 每轮重建 prompt，已具备这个时机）。

**缓存影响**：工具名单进的是 cache-stable 层。按 channel 变化意味着不同 channel 的
前缀不同，但同一 channel 内稳定 —— 按会话仍然命中。可接受，代码注释里写明。

## 涉及文件

`src/services/tool_execution/mod.rs`（definitions_for + 持 Policy）·
`crates/komo-core/src/domain/policy.rs`（`wholly_denied(category, access, channel)` 判定，纯函数）·
`src/cli/wiring.rs` · `src/domain/prompt.rs`

## 验收

- `[[policy.rule]] category="shell" effect="deny"`（无 match）→ 模型的 schema 里
  没有 `shell`，prompt 的工具名单里也没有。
- 只 deny `access="write"` → `write`/`edit` 摘掉，`read`/`grep` 仍在。
- 带 `match`/`value` 的部分 deny 规则 → 工具**保留**（只在调用时拒）。
- `domain::policy` 的判定有纯函数单测（不依赖 executor）。
