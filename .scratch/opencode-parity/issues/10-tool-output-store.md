# 10 — `ToolOutputStore`：超限输出落盘 + 双端预览 + 可回搜

Status: done (2026-07-28) — `cargo test` 604 passed
Phase: 2 管线 · 依赖: 03（read 要能读 managed 路径）· 建议与 11 同批

## 落地记录

新 `src/services/tool_output_store.rs`，与 issue 设计的三处偏差：

1. **`output_paths` 不进 `ToolOutput`**。issue 写「output_paths 进 ToolOutput 新
   字段」，但产出方是 executor（`bound()` 在工具返回**之后**跑），工具永远不会
   自己填它。所以 `bound()` 直接返回 `Bounded { text, output_paths }`，
   `ToolOutput` 没动。
2. **落盘的门槛是「有 ledger seq」**，不是「有 session」。aux 子代理 / sweeps 既没有
   run 可供 operator 回查，也没有后续 turn 会去 `read`，落一个没人打开的文件是垃圾。
   `bound()` 的 call id 因此是 `<run_id>-<seq:04>`。
3. **只读 managed 根挂在 `Workspace` 上**，不在 `fs_common` 里判定：
   `Workspace::with_readonly()` + `resolve_readable()`，`fs_common::resolve_readable`
   给 `read`/`grep` 用，`resolve`（写路径）不变 —— 于是 write/edit/apply_patch
   指向 managed 路径**自动**被拒，不需要额外分支。session 自选 workspace 时
   派生的临时 Workspace 会继承 readonly 根（`fs_common::effective`）。
   `glob` 仍走 `resolve`：模型拿到的是精确文件路径，不需要列目录。

管线顺序：执行 → 从**原始**结果算 ledger 字段 → `bound()`（可能写盘）→ 记 step
（带 output_paths）→ turn budget admit。ledger 记原始、模型看预览，这一点不变。

预览：marker 之外的预算对半分给首尾，先按行采样（`MAX_PREVIEW_LINES` 2000 对半），
无换行的单行 blob 退回 char boundary 字节切分。写盘失败 → 退回原来的单向截断
（磁盘满不该把一次成功的工具调用变成失败）。

清理：7 天，gateway 启动扫一次 + store 内 1 小时去抖，无新 cron。

验证：8 个 store 单测 + 2 个 executor 集成测试（500 行输出 → 首尾都在、中段在文件里、
step 带路径；无 ledger → 不落盘）+ read/write 各一个（managed 路径可读 / 不可写，
且 managed 根的兄弟目录仍被拒）；另外真实启动 gateway，27 天前的文件与空目录被清掉、
当天的保留（日志 `expired stored tool outputs removed=1`）。

## 目标

[src/services/tool_execution/result.rs](../../../src/services/tool_execution/result.rs)
是**单向硬截断**：超过 `max_result_bytes` 就砍掉后半段，永久丢失。而编译错误、
测试失败摘要、堆栈的关键信息**通常正好在尾部**。这是每天都在损失信息的问题。

opencode 的 `packages/core/src/tool-output-store.ts` 的做法：完整输出落盘，
模型看 head+tail 双端预览，并把文件路径交回去让模型按需回读/回搜。

## 设计

新 `src/services/tool_output_store.rs`：

```rust
pub struct Bounded { pub text: String, pub output_paths: Vec<PathBuf> }
pub fn bound(session_id: &str, call_id: &str, output: String) -> Bounded
```

- 未超限 → 原样返回，`output_paths` 为空（零额外 I/O）。
- 超限 → 写 `~/.komo/tool-output/<session-id>/<call-id>.txt`（完整内容），
  返回预览：

```
<head：前 maxLines/2 行>

…[输出超出 <N> KB 限制。完整内容：/Users/…/tool-output/<sid>/<cid>.txt
  （用 read 翻页或 grep 搜索它）]…

<tail：后 maxLines/2 行>
```

- 双端采样先按**行**（`MAX_LINES` 对半），行数够了再按**字节**对半（照抄 v2 的
  `preview()` 两级逻辑，保证 UTF-8 边界）。
- 预算上限沿用 executor 现有的 `max_result_bytes`（实例配置，不引入新常量），
  行数上限取 v2 的 2000。

### 让模型能回读

`read` / `grep` 需要接受 managed 目录下的绝对路径：在 `Workspace` 之外单开一个
**只读 managed 根**（`fs_common` 里判定：`path.starts_with(tool_output_root())` →
允许读，永不允许写）。不这么做，模型拿到路径也打不开。

### 保留与清理

7 天保留。清理时机：**gateway 启动一次 + store 内 1 小时去抖**，
不新增 cron schedule（`agent/daemon.rs` 的 sweep 列表已经很长，且这件事不需要准点）。

### 接线

`ToolExecutor` 管线里，`cap_tool_result` 的位置换成 `bound(..)`：
- ledger 记原始（现在也是先记原始再 cap，顺序不变）
- `output_paths` 进 `ToolOutput` 新字段 → 随 11 落 `RunStep.output_paths`
- 无 session/无 call_id 的路径（aux 子代理、sweeps）退回纯截断，不落盘

## 涉及文件

新 `src/services/tool_output_store.rs` · `src/services/tool_execution/{mod,result}.rs` ·
`crates/komo-core/src/domain/tool.rs`（`ToolOutput.output_paths`）·
`src/tools/fs_common.rs`（managed 只读根）· `src/cli/gateway.rs`（启动清理）

## 验收

- 500KB 的 `shell` 输出：模型看到首尾预览 + 路径；`read` 能翻到中段；
  `grep` 能在该文件里搜到中段的字符串（**今天中段直接消失**）。
- 未超限的输出不产生任何文件（`ls ~/.komo/tool-output` 为空）。
- 8 天前的文件在下次启动后被清掉。
- managed 路径只读：`write`/`edit` 指向它 → 拒绝。
