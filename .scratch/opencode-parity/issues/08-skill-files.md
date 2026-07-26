# 08 — `skill view` 补 base directory + 文件清单

Status: ready-for-agent
Phase: 1 工具集（小项，改动最小/收益最直接）· 依赖: 无

## 目标

[src/tools/skill.rs:134](../../../src/tools/skill.rs) 的 `view` 只回
`# Skill: <name>` + description + instructions。多文件 skill 的
`scripts/` / `references/` 模型**不知道在哪、有什么** —— 于是 SKILL.md 里
「运行 scripts/foo.py」这类指令必然落空。

对照 `opencode/packages/core/src/tool/skill.ts::toModelOutput`。

## 设计

`view` 的输出改成：

```
<skill_content name="<name>">
# Skill: <name>
<instructions>

Base directory for this skill: <dir>
Relative paths in this skill (e.g., scripts/, references/) are relative to this base directory.
Note: file list is sampled.

<skill_files>
<file>/abs/path/scripts/foo.py</file>
…
</skill_files>
</skill_content>
```

- `dir` = SKILL.md 所在目录（`SkillRegistry` 已知路径）
- 文件清单：递归 glob 该目录，排除 SKILL.md 自身，排序后**取前 10 个**
  （v2 的 `FILE_LIMIT`），并保留 "sampled" 措辞 —— 不要让模型以为这就是全部
- 单文件 skill（不是 `SKILL.md` 形态的）→ 空清单，不输出 `<skill_files>` 块
- `disabled` skill 的回复不变（回状态，不回 instructions）

## 涉及文件

`src/tools/skill.rs` · `src/infra/skills.rs`（若 `Skill` 未带 location 需补字段）

## 验收

- 一个带 `scripts/` 的 skill：`view` 输出含 base directory 与该脚本的绝对路径。
- 文件数 > 10 时只列 10 个且保留 sampled 说明。
- 单文件 skill 无 `<skill_files>` 块；disabled skill 行为不变。
