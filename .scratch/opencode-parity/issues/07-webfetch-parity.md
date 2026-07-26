# 07 — `web_fetch` 对齐：format 参数 + content-type 白名单 + 真 Markdown

Status: ready-for-agent
Phase: 1 工具集（小项）· 依赖: 无

## 目标

[src/tools/web_fetch.rs](../../../src/tools/web_fetch.rs) 现在：8KB 上限、手写
`strip_html`、无 `format` 参数、**不检查 content-type**（拿到 PDF/图片会
`resp.text()` 糊成乱码进上下文）。对照 `opencode/packages/core/src/tool/webfetch.ts`。

## 设计

新增 `format: "text" | "markdown" | "html"`，默认 `markdown`，并按 format 发不同的
`Accept` 头（v2 的 `acceptHeader`，让服务端优先给我们想要的表示）。

**content-type 白名单**（最实质的一项）：
- 图片类（`image/*` 除 svg）→ 明确报 `"Unsupported fetched image content type: …"`
- 非文本类（不是 `text/*` / `+json` / `+xml` / js）→ 明确报 `"Unsupported fetched file content type: …"`
- 现在的行为是无差别 `text()`，必须改掉

**大小上限**：`Content-Length` 预检 + 流式累积双保险（v2 的 `collectBoundedResponseBody`），
上限从 8KB 提到 **256KB**（超限的部分交给 10 的 output store，而不是在工具里就丢）。

**HTML→Markdown**：手写 `strip_html` 换成真转换（标题/列表/代码块/链接保留）。
Rust 侧候选 `html2md`；若依赖不合意，退而在 `strip_html` 基础上保留
`h1-h6`/`li`/`pre`/`a` 的结构标记 —— 但不要停在现在这个「纯裸文本」状态。

~~顺手修文档漂移：`ActionRef::Network` 的注释与 `#[allow(dead_code)]`~~ ——
**已在 03 里做掉**（同一个文件正好在改）。

## 涉及文件

`src/tools/web_fetch.rs` · `crates/komo-core/src/domain/approval.rs`（注释 + allow）·
`Cargo.toml`（`html2md`，如采用）

## 验收

- `format:"html"` 返回原始 HTML；`markdown` 保留标题/列表/代码块结构。
- 请求一个 PDF/PNG URL → 明确的 unsupported 错误，上下文里没有乱码。
- `Content-Length` 超限的响应不被下载完（提前失败）。
- ~~`ActionRef::Network` 注释~~（已在 03 完成）。
