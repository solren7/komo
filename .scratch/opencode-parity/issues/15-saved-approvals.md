# 15 — 持久化 always-allow（`~/.komo/permissions.json`）

Status: ready-for-agent
Phase: 3 权限 · 依赖: 01

## 目标

komo 的审批记忆只有**会话级**（`/approve session` + `scope_key` 缓存）：重启即忘。
想让某类操作长期免打扰，只能手写 `config.toml` 的 `[[policy.rule]]`。

opencode 的 `PermissionSaved`（`packages/core/src/permission/saved.ts`）把「以后都允许」
按 project 写盘，在评估时**合并进 ruleset**（`savedRules()` → `effect: "allow"`）。

## 设计

### 存储

`~/.komo/permissions.json`，**不进 state.db**（disposable，删库不该丢掉这类
durable 个人数据 —— 与 memory.db / kanban.db / cron.db 的定位一致）。选 JSON 而非
第四个 db：条目少，operator 应当能直接看和删。

```json
{
  "version": 1,
  "entries": [
    { "category": "file", "access": "write", "match": "prefix",
      "value": "/Users/x/01-code/komo/", "channel": "cli",
      "created_at": "2026-07-25T10:00:00Z", "source": "approval" }
  ]
}
```

字段刻意与 `[[policy.rule]]` **同构** —— 这样 saved 条目就是「运行时累积出来的
policy allow 规则」，共用一套匹配代码，也能被 `komo policy check` 解释。

### 判定优先级（写进 `domain/policy.rs` 的文档注释）

```
工具 hardline floor  >  config [policy] deny  >  saved allow
                     >  config [policy] allow / default  >  交互 ask
```

三条硬约束：
1. saved allow **永不**越过 deny 规则或工具 hardline（`rm -rf /`、HA 的
   `BLOCKED_DOMAINS` 依旧无解）。
2. saved allow **不覆盖 `Risk::Dangerous`** —— 那仍需 `include_dangerous`；
   即「记住这个允许」不能把危险动作变成静默执行。
3. 无 session 的**无人值守**上下文（cron / sweeps / briefing）**不读 saved**：
   现有语义是「只有显式 `unattended = true` 的规则能授权」，saved 是交互式积累的，
   不该泄漏到无人值守路径。

### 交互侧

审批提示从 `y / s / n` 变成 `y / s / a / n`：
- `y` 本次
- `s` 本会话（现状）
- **`a` 以后都允许** → 写入 permissions.json
- `n` 拒绝（01 之后可带理由）

写入时按 `ActionRef` 生成最窄的条目：`File{path}` → 该文件所在**目录** prefix；
`Shell{command}` → 命令的第一个 token（`cargo` 而非整条命令行）；
`Network{url}` → host suffix。提示里必须**明示将要保存的规则文本**，让用户看清
自己授了多宽的权。chat 侧对应 `/approve always`。

### Operator surface

- `komo policy saved list` / `komo policy saved forget <n>|--all`
- `komo policy list` 把 saved 条目与 config 规则**分节**列出（来源可辨）
- `komo policy check` 的判定里纳入 saved，输出注明命中的是 saved 还是 config
- `komo doctor` 的 `policy:` 段加一行 saved 条目数

## 涉及文件

新 `src/infra/permissions_store.rs` · `crates/komo-core/src/domain/policy.rs`（合并 + 优先级）·
`src/agent/policy_approver.rs` · `src/cli/approver.rs` · `src/tui/approver.rs` ·
`src/agent/interaction.rs`（`/approve always`）· `src/cli/policy.rs` · `src/cli/doctor.rs`

## 验收

- 审批时按 `a` → permissions.json 出现最窄条目；重启 gateway 后同类操作不再问。
- 一条 config deny 规则能压过 saved allow（单测）。
- `Risk::Dangerous` 的动作按 `a` 之后**仍然会问**（单测）。
- 无人值守的 cron turn 不读 saved（单测）。
- `komo policy saved forget` 之后恢复询问。
