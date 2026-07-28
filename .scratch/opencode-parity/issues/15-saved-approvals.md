# 15 — 持久化 always-allow（`~/.komo/permissions.json`）

Status: done (2026-07-28) — `cargo test --workspace` 615 + 71 passed
Phase: 3 权限 · 依赖: 01

## 落地记录

新 `src/infra/permissions_store.rs`（JSON，schema 与 `[[policy.rule]]` 同构）+
`domain/policy.rs` 的 saved 层。与 issue 设计的三处偏差：

1. **写入方只有一个：`PolicyApprover`**。issue 的涉及文件列表让三个交互 approver
   （cli / chat / tui）各自写盘 —— 那样"always"的语义会在三处漂移。改成
   `Decision` 新增 `AllowAlways` 变体：交互层只*报告*用户按了 `a`，
   `PolicyApprover`（唯一持有 store 的地方）负责合成最窄规则并落盘，然后向上返回
   普通 `Allow`。代价是 `Decision` 多一个变体，实际只有 3 处穷尽 match 需要跟改
   （cron.rs + fs_common.rs ×2）。
2. **saved 列表与 Policy 共享 `Arc<RwLock<Vec<Rule>>>`**，不是每次决策读文件。
   否则「按 `a` 之后同类操作立刻不再问」要么做不到（Policy 是 wiring 时 clone 的），
   要么每次决策一次文件 I/O。文件是记录，Arc 是活视图。
3. **规则渲染收敛到 `Rule::describe()`**（domain）。原来 `cli/policy.rs` 有一份
   `rule_str`；审批提示要显示"将要保存的规则"，saved list 也要显示 —— 三处各写一份
   必然漂移，所以删掉 cli 的那份，全部走 domain。

优先级（写进 `Policy::decide` 的注释）：
`工具 hardline > config deny > saved > config allow/default > ask`。
三条硬约束全部在**引擎里**实现，不靠调用方自觉：saved 不越过 deny；saved 永不覆盖
`Risk::Dangerous`；无 channel（无人值守）不读 saved。第三条另有一道 wiring 层的
保险 —— cron / briefing 的 approver 用 `wrap`（不带 store），只有交互路径用
`wrap_with_store`。

最窄规则（`Rule::narrowest_for`）：shell → 命令首个 token（`cargo build` → prefix
`cargo `，带参数时保留尾空格，`cargo ` 不会匹配 `cargonaut`）；file → 父目录 prefix
（带尾 `/`，`/src` 不会覆盖 `/src-old`）+ read/write 区分；network → host 后缀；
HA → `domain.service` exact。一律带当前 channel scope。无 `ActionRef` 或无 session
时**不保存**（会退化成"本次允许"并记一条 info），因为写一条无 channel 的规则等于
到处生效，与"最窄"相反。

提示侧：CLI `[y]es / [s]ession / [a]lways (saves: <规则文本>) / [N]o`；chat 多一行
`/approve always ... 将保存规则：<规则文本>`；TUI 模态多一行 `[a]`。**危险操作与
无可保存对象时不出现 `a`** —— 引擎本就拒绝为危险动作读 saved，给出这个键就是骗人。

Operator：`komo policy saved list` / `saved forget <n>|--all`；`policy list` 把
saved 单独分节列在 config 规则之后（即求值顺序）；`policy check` 命中 saved 时输出
`matched: saved #0 …` 并提示可以 forget；`komo doctor` 的 policy 段加一行条数。

### 顺带发现的既存缺陷（已修）

`cargo test` 从仓库根只测根 package —— komo-core 的 71 个测试从来没被跑过，
它的 `RunStep` 测试 fixture 自 `elapsed_ms` 那次改动起就编译不过了。加了三个字段
把它修好，并在 AGENTS.md 的 Testing 段写明动 `crates/komo-core` 要跑
`cargo test --workspace`。

验证：domain 6 个纯函数单测（saved 命中/channel 隔离/config deny 压过 saved/
危险不覆盖/无人值守不读/中途新增立即生效）+ `narrowest_for` 逐类断言；store 6 个
（重载存活、去重、forget 单条与全部、坏文件与未来版本忽略、手改文件不能自我放宽、
坏条目不带走好条目）；approver 2 个（落盘后重载不再问；无 session 只允许一次不落盘）。
另外用真临时 KOMO_HOME 跑通了 `saved list` → `check`（命中 / 换 channel 问 /
危险问 / 无人值守问）→ `forget` → 再问 的完整回路，以及 `policy list` 与 `doctor`
的输出。

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
