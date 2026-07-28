# 07 — `web_fetch` 对齐：format 参数 + content-type 白名单 + 真 Markdown

Status: done (2026-07-28) — `cargo test` 588 passed
Phase: 1 工具集（小项）· 依赖: 无

## 落地记录

`src/tools/web_fetch.rs` 重写 fetch 路径：

- `format`（`markdown` 默认 / `text` / `html`）驱动 `Accept` 头与 HTML 渲染。
- **content-type 白名单**：raster image（SVG 除外）与非文本 mime 直接报
  `Unsupported fetched {image,file} content type: …`，不再 lossy 解码进上下文。
- **大小**：`Content-Length` 预检超限 → 未下载即失败；chunked 无声明长度时
  流式累积到 256KB 并带 marker 保留首段（与 v2 的直接失败不同，故意的：
  已经付过的下载比空手更有用）。
- **不再自截断**：模型可见范围交给 executor 唯一的 `max_tool_result_bytes`
  choke point（原来 8KB 自截断，`result.rs` 的注释同步修正）。
- HTML→Markdown 用手写 `render_html`（标题 / `- ` 列表 / ``` fence / 反引号
  行内 code / `[text](href)` / 实体解码 / pre 外空白折叠），**没有**引入
  html5ever 系依赖 —— 单二进制分发下 8 个传递依赖换不来模型理解上的差别，
  替换点收敛在一个函数里。顺带修掉 `strip_html` 用 `to_lowercase()` 建索引的
  隐性字节漂移（改 `to_ascii_lowercase()`，长度恒等）。
- 新增 `http(s)` scheme 校验（`file://` 之类当场报错）。

验证：15 个单测，其中 3 个跑真实 loopback socket（PDF 拒绝 / HTML→markdown 与
raw / `Content-Length` 超限提前失败）。

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
