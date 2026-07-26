# 16 — `Content::file`：图片/附件端到端

Status: needs-triage — 跨层高风险，动手前先确认 rig 侧可行性
Phase: 4 · 依赖: 03（read 已能识别图片）

## 目标

v2 的 `toModelOutput → Content[]`，`Content = text | file{data, mime, name}`：
`read` 因此能把截图直接给模型看。komo 的 `ToolOutput` 只有 `text`，
`read` 遇到 PNG 只能报「不支持」。

这是 tool-trait-v2 PRD 的 issue 02，当时因「要动 rig driver 与各 channel」推迟。
判断不变：**它是全程序唯一跨层的高风险项**，所以排最后、单独验证。

## 先要回答的问题（动手前）

1. `rig` 0.40 的 `ToolOutcome`/`Message` 是否支持多模态 tool result？
   —— `infra/llm.rs` 现在用 `Message::tool_result[_with_call_id]`，content 是
   `String`。需要确认 rig 的 `UserContent::Image` 能不能出现在 tool result 里，
   还是只能出现在 user message 里。
2. 各 provider 的差异：Anthropic 支持 tool_result 里带 image block；OpenAI 系
   历史上**不支持** —— 可能要退化成「工具回文本 + 紧跟一条 user message 带图」。
3. komo 的目标 provider（deepseek / codex）是否根本不支持视觉？如果主用模型不支持，
   这个 issue 的实际收益要重新评估 —— **这是本 issue 保持 needs-triage 的原因**。

## 若可行的设计草案

- `ToolOutput` 加 `attachments: Vec<Attachment>`（`{data: Vec<u8>, mime, name}`），
  `text` 仍是必填（不支持视觉的 provider 只看 text，优雅降级）。
- `domain/llm.rs` 的 `ToolOutcome` 带上 attachments；`RigTurnDriver` 按 provider
  能力决定：塞进 tool result / 退化成后随 user message / 丢弃并在 text 里说明。
- 出站：channel 侧已有 `ReplySink::send_photo`（telegram 已实现，见 wechat QR 那条路），
  所以「agent 把图发给用户」这半边其实已经通了；缺的是「图进模型」这半边。
- `read` 的图片分支接上 `Image` 归一化（尺寸/体积上限，v2 有 `MAX_MEDIA_INGEST_BYTES`
  = 20MB 与 resize）。

## 涉及文件

`crates/komo-core/src/domain/tool.rs` · `crates/komo-core/src/domain/llm.rs` ·
`src/infra/llm.rs`（driver）· `src/tools/read.rs` · 各 channel 的投递路径

## 验收（若开工）

- `read` 一张 PNG，模型能描述图片内容（在支持视觉的 provider 上）。
- 不支持视觉的 provider 上：不报错，模型收到「这是一张图片，尺寸/格式为…」的文本。
- 超过体积上限的图片 → 明确报错，不静默截断。
