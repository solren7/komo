# 05 — `grep` / `glob`：ripgrep 库栈，不依赖外部二进制

Status: done (2026-07-26) — `cargo test` 556 passed
Phase: 1 工具集 · 依赖: 01（审批反馈；可与 03/04 并行）

## 落地记录

- 依赖按计划用 ripgrep 库栈：`ignore` + `globset` + `grep-searcher` + `grep-regex`，
  **不调外部 `rg`**。
- `services/search.rs`（新）：阻塞的 walk/match 层，工具在 `spawn_blocking` 里调。
  刻意拆成 `candidates`（哪些路径）+ `search_files`（内容），**这样权限策略能在
  读取内容之前过一遍路径** —— 有专门的测试证明 `file`/read deny 规则让 grep
  根本不打开那个文件（而不是读了再不显示）。
- 关键细节：`ignore` 默认只在 git 仓库里认 `.gitignore`，加了
  `require_git(false)` —— komo 的 workspace 不一定是 repo。
- `glob` 按 mtime 倒序；`grep` 输出照抄 v2 形状（`Found N matches` + `path:` 块 +
  `Line N: text`，保留缩进）。两者都 `Risk::Safe` + `idempotent`。

## 目标

komo **完全没有搜索工具**，模型只能让 `shell` 跑 `rg`/`grep`：每次经审批、输出形状
不固定、没有 `limit`、路径不归一。对照
`opencode/packages/core/src/tool/{grep,glob}.ts`。

## 设计

不调外部 `rg`（komo 单二进制分发，容器里没有），用 ripgrep 自身的库：

```toml
ignore = "0.4"          # 尊重 .gitignore 的并行遍历
globset = "0.4"         # glob 匹配
grep-searcher = "0.1"   # 行搜索
grep-regex = "0.1"      # 正则匹配器
```

（`regex` / `walkdir` 已在 `Cargo.lock` 里。）

### `glob`

```
pattern: string    必填
path?:   string    相对 workspace 的子目录，默认 workspace 根
limit?:  int       默认 100
```
输出：每行一个**相对 workspace 的路径**，按 mtime 倒序（新的在前，对齐 v2 的
`FileSystem.Entry`）；无命中 → `"No files found"`。

### `grep`

```
pattern:  string   正则
path?:    string   子目录或单文件
include?: string   文件 glob，如 `*.{ts,tsx}`
limit?:   int      默认 100 条匹配
```
输出照抄 v2 的形状（模型已被训练过这个格式）：

```
Found 3 matches
src/a.rs:
  Line 12: fn foo() {
  Line 40: foo();
```

单行预览截断（复用 03 的 2000 字符规则）；`limit` 截断时明确标注被截。

### 共同约束

- **只读**：`Risk::Safe` + `ActionRef::File{write:false}` → policy 的 `access="read"`
  deny 规则可以把敏感目录挡在搜索之外（不然 grep 就成了绕过 file deny 的旁路）。
- workspace 白名单照旧；10 落地后额外允许 managed tool-output 根（只读）。
- 默认跳过 `.git/`、二进制文件（`ignore` + `grep-searcher` 自带）。

## 涉及文件

新 `src/tools/grep.rs` · 新 `src/tools/glob.rs` · 新 `src/services/search.rs`
（遍历/匹配封装，两只工具共用）· `src/tools/mod.rs` · `src/cli/wiring.rs` ·
`Cargo.toml`

## 验收

- 在本仓库跑 `glob {pattern:"src/tools/*.rs"}` 返回相对路径清单。
- `grep {pattern:"fn call", include:"*.rs"}` 输出上述格式，`.git/` 与 `target/` 不出现。
- `limit` 生效且截断有标注；无命中回 `"No files found"`。
- 一条 `category="file", access="read", effect="deny"` 规则能让 `grep` 跳过该路径。
