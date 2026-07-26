# 12 — 按 policy 过滤工具目录（整只被 deny 的工具不进 schema）

Status: ready-for-agent
Phase: 2 管线 · 依赖: 无（改动小，收益直接）

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
