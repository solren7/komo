# 13 — `shell` 对齐 v2 `bash`：timeout / workdir / 结构化输出

Status: ready-for-agent
Phase: 2 管线 · 依赖: 01（反馈拒绝）· 11（structured 落库）

## 目标

[src/tools/shell.rs](../../../src/tools/shell.rs) 现在：固定 `sh -c`、cwd 固定
workspace 根、**模型无法为长命令申请更多时间**（只有 executor 侧的全局
`call_timeout_secs` wall clock）。对照 `opencode/packages/core/src/tool/bash.ts`。

## 设计

新增两个参数：

```
timeout?: int   毫秒，默认 120_000，上限 600_000（v2 的 2min/10min）
workdir?: string  相对 workspace 的子目录，默认 workspace 根
```

- `timeout` 由**工具内部**实现（`tokio::time::timeout` 包住 `child.wait()`），
  executor 的全局 wall clock 仍在外层兜底 —— 两层不冲突：内层给出可解释的
  「命令超时，可用更大的 timeout 重试」，外层只防挂死。
- 超时后必须 kill 进程组（`kill_on_drop` 已有，但 `sh -c` 的**子进程**要靠
  `detached` + 负 pid kill 才能收干净；v2 用 `detached: true` + `forceKillAfter`）。
  这条要真测：`shell {command:"sleep 30 & sleep 30", timeout:1000}` 之后不该有孤儿。
- `workdir` 走 workspace 校验；不是目录 → 明确报错。

**结构化输出**（配合 11）：

```
structured = { exit: i32|null, truncated: bool, timeout: bool }
```

text 部分保持现在的形状（`exit status:` + stdout/stderr 分段），但把「被截断」
和「超时」这两个状态从纯文本提升成结构化字段，UI 与 ledger 才能据此渲染。

**可配置 shell**：v2 从 config 读 `shell`。komo 加 `[runtime] shell = "/bin/zsh"`
（可选，默认 `/bin/sh`）—— 低优先，可留 TODO。

## 涉及文件

`src/tools/shell.rs` · `src/config/resolved.rs`（可选 shell 配置）·
`crates/komo-core/src/domain/tool.rs`（无需改，structured 已在）

## 验收

- `timeout:1000` + `sleep 5` → 1 秒后返回超时文本，`structured.timeout == true`。
- 超时后无孤儿进程（测试里检查 pid 已回收）。
- `workdir` 生效；越出 workspace → 拒绝。
- 输出超 `MAX_STREAM_BYTES` → `structured.truncated == true`（且 10 落地后
  完整输出在 managed 文件里）。
