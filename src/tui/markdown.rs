//! Markdown → styled ratatui lines for agent replies in the transcript.
//! Produces *logical* (unwrapped) lines; `ui::render_transcript` wraps them to
//! the terminal width with the same CJK-aware rules as plain text. Soft breaks
//! are kept as line breaks (chat replies use single newlines meaningfully), so
//! plain-text output renders exactly as before.

use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::LazyLock;

use pulldown_cmark::{Alignment, CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use syntect::easy::HighlightLines;
use syntect::highlighting::{FontStyle, Theme};
use syntect::parsing::SyntaxSet;

use super::ui::display_width;

/// Memoized front for [`markdown_lines`]. The transcript re-renders on every
/// tick (~120ms) and syntect highlighting is far too slow to re-run per frame,
/// so settled messages hit the cache and only the entry currently streaming
/// re-parses.
pub(super) fn markdown_lines_cached(text: &str) -> Vec<Line<'static>> {
    thread_local! {
        static CACHE: RefCell<HashMap<u64, Vec<Line<'static>>>> = RefCell::new(HashMap::new());
    }
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    let key = hasher.finish();
    CACHE.with(|cache| {
        if let Some(hit) = cache.borrow().get(&key) {
            return hit.clone();
        }
        let lines = markdown_lines(text);
        let mut map = cache.borrow_mut();
        if map.len() >= 256 {
            map.clear();
        }
        map.insert(key, lines.clone());
        lines
    })
}

pub(super) fn markdown_lines(text: &str) -> Vec<Line<'static>> {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_TASKLISTS);
    let mut renderer = Renderer::default();
    for event in Parser::new_ext(text, opts) {
        renderer.on_event(event);
    }
    renderer.finish()
}

/// Loaded once on first use (deserializing the syntax dump is slow). Bat's
/// `ansi` theme resolves to the terminal's own palette — its colors encode
/// "default fg" as alpha 0 and "ANSI index in `r`" as alpha 1 — so highlighted
/// code stays readable on light and dark terminals alike.
struct Highlighting {
    syntaxes: SyntaxSet,
    theme: Theme,
}

static HIGHLIGHTING: LazyLock<Highlighting> = LazyLock::new(|| Highlighting {
    syntaxes: two_face::syntax::extra_newlines(),
    theme: two_face::theme::extra()
        .get(two_face::theme::EmbeddedThemeName::Ansi)
        .clone(),
});

fn syntect_style(style: syntect::highlighting::Style) -> Style {
    let mut s = Style::new();
    s = match style.foreground.a {
        0 => s,
        1 => s.fg(Color::Indexed(style.foreground.r)),
        _ => s.fg(Color::Rgb(
            style.foreground.r,
            style.foreground.g,
            style.foreground.b,
        )),
    };
    if style.font_style.contains(FontStyle::BOLD) {
        s = s.add_modifier(Modifier::BOLD);
    }
    if style.font_style.contains(FontStyle::ITALIC) {
        s = s.add_modifier(Modifier::ITALIC);
    }
    if style.font_style.contains(FontStyle::UNDERLINE) {
        s = s.add_modifier(Modifier::UNDERLINED);
    }
    s
}

/// A table being collected; rendered as an aligned box once it closes.
#[derive(Default)]
struct TableBuf {
    aligns: Vec<Alignment>,
    rows: Vec<Vec<Vec<Span<'static>>>>,
    row: Vec<Vec<Span<'static>>>,
    in_head: bool,
    has_header: bool,
}

#[derive(Default)]
struct Renderer {
    lines: Vec<Line<'static>>,
    current: Vec<Span<'static>>,
    bold: u32,
    italic: u32,
    strike: u32,
    heading: Option<HeadingLevel>,
    code_block: bool,
    highlighter: Option<HighlightLines<'static>>,
    quote: u32,
    /// One entry per open list: `None` = bullet, `Some(n)` = next ordered index.
    lists: Vec<Option<u64>>,
    /// Open link: (url, span index where its text started).
    link: Option<(String, usize)>,
    table: Option<TableBuf>,
}

impl Renderer {
    fn on_event(&mut self, event: Event) {
        match event {
            Event::Start(tag) => self.on_start(tag),
            Event::End(tag) => self.on_end(tag),
            Event::Text(t) if self.code_block => {
                // Code block text arrives with embedded newlines; flushing even
                // empty parts preserves blank lines inside the block.
                for (i, part) in t.split('\n').enumerate() {
                    if i > 0 {
                        self.flush_line();
                    }
                    if !part.is_empty() {
                        self.push_code(part);
                    }
                }
            }
            Event::Text(t) => {
                let style = self.style();
                self.current.push(Span::styled(t.into_string(), style));
            }
            Event::Code(t) => {
                let style = self.style().fg(Color::Yellow);
                self.current.push(Span::styled(t.into_string(), style));
            }
            Event::SoftBreak | Event::HardBreak => self.flush_if_content(),
            Event::Rule => {
                self.block_sep();
                self.current.push(Span::styled(
                    "─".repeat(24),
                    Style::new().fg(Color::DarkGray),
                ));
                self.flush_line();
            }
            Event::TaskListMarker(checked) => {
                let mark = if checked { "[x] " } else { "[ ] " };
                self.current
                    .push(Span::styled(mark, Style::new().fg(Color::DarkGray)));
            }
            // Raw HTML has no terminal rendering — show it verbatim rather
            // than lose content.
            Event::Html(t) | Event::InlineHtml(t) => {
                let style = self.style();
                self.current.push(Span::styled(t.into_string(), style));
            }
            _ => {}
        }
    }

    fn on_start(&mut self, tag: Tag) {
        match tag {
            Tag::Paragraph => self.block_sep(),
            Tag::Heading { level, .. } => {
                self.block_sep();
                self.heading = Some(level);
            }
            Tag::BlockQuote(_) => {
                self.block_sep();
                self.quote += 1;
            }
            Tag::CodeBlock(kind) => {
                self.block_sep();
                self.code_block = true;
                if let CodeBlockKind::Fenced(info) = kind {
                    let lang = info.split([' ', ',']).next().unwrap_or("").trim();
                    if !lang.is_empty() {
                        self.current.push(Span::styled(
                            format!("· {lang}"),
                            Style::new().fg(Color::DarkGray),
                        ));
                        self.flush_line();
                        let hl = &*HIGHLIGHTING;
                        self.highlighter = hl
                            .syntaxes
                            .find_syntax_by_token(lang)
                            .map(|syntax| HighlightLines::new(syntax, &hl.theme));
                    }
                }
            }
            Tag::List(start) => {
                if self.lists.is_empty() {
                    self.block_sep();
                }
                self.lists.push(start);
            }
            Tag::Item => {
                self.flush_if_content();
                let depth = self.lists.len().saturating_sub(1);
                let marker = match self.lists.last_mut() {
                    Some(Some(n)) => {
                        let m = format!("{n}. ");
                        *n += 1;
                        m
                    }
                    _ => "• ".to_string(),
                };
                self.current
                    .push(Span::raw(format!("{}{marker}", "  ".repeat(depth))));
            }
            Tag::Emphasis => self.italic += 1,
            Tag::Strong => self.bold += 1,
            Tag::Strikethrough => self.strike += 1,
            Tag::Link { dest_url, .. } | Tag::Image { dest_url, .. } => {
                self.link = Some((dest_url.to_string(), self.current.len()));
            }
            Tag::Table(aligns) => {
                self.block_sep();
                self.table = Some(TableBuf {
                    aligns,
                    ..TableBuf::default()
                });
            }
            Tag::TableHead => {
                if let Some(t) = &mut self.table {
                    t.in_head = true;
                    t.has_header = true;
                }
            }
            _ => {}
        }
    }

    fn on_end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph | TagEnd::Item => self.flush_if_content(),
            TagEnd::Heading(_) => {
                self.flush_if_content();
                self.heading = None;
            }
            TagEnd::BlockQuote(_) => {
                self.flush_if_content();
                self.quote = self.quote.saturating_sub(1);
            }
            TagEnd::CodeBlock => {
                self.flush_if_content();
                self.code_block = false;
                self.highlighter = None;
            }
            TagEnd::List(_) => {
                self.lists.pop();
            }
            TagEnd::Emphasis => self.italic = self.italic.saturating_sub(1),
            TagEnd::Strong => self.bold = self.bold.saturating_sub(1),
            TagEnd::Strikethrough => self.strike = self.strike.saturating_sub(1),
            TagEnd::Link | TagEnd::Image => {
                if let Some((url, start)) = self.link.take() {
                    // Autolinks repeat the URL as their text — append the
                    // target only when it adds information.
                    let text: String = self.current[start..]
                        .iter()
                        .map(|s| s.content.as_ref())
                        .collect();
                    if !url.is_empty() && text != url {
                        self.current.push(Span::styled(
                            format!(" ({url})"),
                            Style::new().fg(Color::DarkGray),
                        ));
                    }
                }
            }
            TagEnd::TableCell => {
                let cell = std::mem::take(&mut self.current);
                if let Some(t) = &mut self.table {
                    t.row.push(cell);
                }
            }
            TagEnd::TableHead => {
                if let Some(t) = &mut self.table {
                    t.rows.push(std::mem::take(&mut t.row));
                    t.in_head = false;
                }
            }
            TagEnd::TableRow => {
                if let Some(t) = &mut self.table {
                    t.rows.push(std::mem::take(&mut t.row));
                }
            }
            TagEnd::Table => {
                if let Some(t) = self.table.take() {
                    self.render_table(t);
                }
            }
            _ => {}
        }
    }

    fn push_code(&mut self, part: &str) {
        if let Some(hl) = self.highlighter.as_mut() {
            // Highlight with the newline the parser stripped — syntect's
            // grammars (extra_newlines) expect it for correct state.
            let with_newline = format!("{part}\n");
            if let Ok(regions) = hl.highlight_line(&with_newline, &HIGHLIGHTING.syntaxes) {
                for (style, piece) in regions {
                    let piece = piece.strip_suffix('\n').unwrap_or(piece);
                    if !piece.is_empty() {
                        self.current
                            .push(Span::styled(piece.to_string(), syntect_style(style)));
                    }
                }
                return;
            }
        }
        let style = self.style();
        self.current.push(Span::styled(part.to_string(), style));
    }

    fn render_table(&mut self, t: TableBuf) {
        let cols = t.rows.iter().map(|r| r.len()).max().unwrap_or(0);
        if cols == 0 {
            return;
        }
        let mut widths = vec![1usize; cols];
        for row in &t.rows {
            for (i, cell) in row.iter().enumerate() {
                let w: usize = cell.iter().map(|s| display_width(&s.content)).sum();
                widths[i] = widths[i].max(w);
            }
        }
        let border = Style::new().fg(Color::DarkGray);
        let rule = |left: char, mid: char, right: char| {
            let mut s = String::from(left);
            for (i, w) in widths.iter().enumerate() {
                if i > 0 {
                    s.push(mid);
                }
                s.push_str(&"─".repeat(w + 2));
            }
            s.push(right);
            Line::from(Span::styled(s, border))
        };
        self.lines.push(rule('┌', '┬', '┐'));
        for (ri, row) in t.rows.into_iter().enumerate() {
            if ri == 1 && t.has_header {
                self.lines.push(rule('├', '┼', '┤'));
            }
            let mut row = row;
            row.resize_with(cols, Vec::new);
            let mut spans = Vec::new();
            for (ci, cell) in row.into_iter().enumerate() {
                spans.push(Span::styled("│ ", border));
                let content: usize = cell.iter().map(|s| display_width(&s.content)).sum();
                let pad = widths[ci].saturating_sub(content);
                let (before, after) = match t.aligns.get(ci) {
                    Some(Alignment::Right) => (pad, 0),
                    Some(Alignment::Center) => (pad / 2, pad - pad / 2),
                    _ => (0, pad),
                };
                spans.push(Span::raw(" ".repeat(before)));
                spans.extend(cell);
                spans.push(Span::raw(format!("{} ", " ".repeat(after))));
            }
            spans.push(Span::styled("│", border));
            self.lines.push(Line::from(spans));
        }
        self.lines.push(rule('└', '┴', '┘'));
    }

    fn style(&self) -> Style {
        let mut s = Style::new();
        if self.code_block {
            s = s.fg(Color::Yellow);
        }
        s = match self.heading {
            None => s,
            Some(HeadingLevel::H1) => s
                .fg(Color::Magenta)
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
            Some(HeadingLevel::H2) => s.fg(Color::Magenta).add_modifier(Modifier::BOLD),
            Some(HeadingLevel::H3) => s.fg(Color::Cyan).add_modifier(Modifier::BOLD),
            Some(_) => s.add_modifier(Modifier::BOLD),
        };
        if self.quote > 0 {
            s = s.fg(Color::DarkGray);
        }
        if self.link.is_some() {
            s = s.fg(Color::Blue).add_modifier(Modifier::UNDERLINED);
        }
        if self.bold > 0 || self.table.as_ref().is_some_and(|t| t.in_head) {
            s = s.add_modifier(Modifier::BOLD);
        }
        if self.italic > 0 {
            s = s.add_modifier(Modifier::ITALIC);
        }
        if self.strike > 0 {
            s = s.add_modifier(Modifier::CROSSED_OUT);
        }
        s
    }

    /// Blank separator before a new block — only between blocks, never leading,
    /// and never while a line is being built (a list bullet awaiting its text).
    fn block_sep(&mut self) {
        if !self.current.is_empty() {
            return;
        }
        if self.lines.last().is_some_and(|l| !l.spans.is_empty()) {
            self.lines.push(Line::default());
        }
    }

    fn flush_if_content(&mut self) {
        if !self.current.is_empty() {
            self.flush_line();
        }
    }

    fn flush_line(&mut self) {
        let mut spans = std::mem::take(&mut self.current);
        if self.quote > 0 {
            spans.insert(0, Span::styled("▎ ", Style::new().fg(Color::DarkGray)));
        }
        self.lines.push(Line::from(spans));
    }

    fn finish(mut self) -> Vec<Line<'static>> {
        self.flush_if_content();
        self.lines
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(line: &Line) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn plain_text_keeps_its_line_breaks() {
        let lines = markdown_lines("第一行\n第二行");
        assert_eq!(
            lines.iter().map(plain).collect::<Vec<_>>(),
            vec!["第一行", "第二行"]
        );
    }

    #[test]
    fn heading_is_bold_and_bullets_get_markers() {
        let lines = markdown_lines("# 标题\n\n- one\n- two\n\n1. first");
        let texts: Vec<String> = lines.iter().map(plain).collect();
        assert_eq!(texts[0], "标题");
        assert!(
            lines[0].spans[0]
                .style
                .add_modifier
                .contains(Modifier::BOLD),
            "heading rendered bold"
        );
        assert!(texts.contains(&"• one".to_string()), "{texts:?}");
        assert!(texts.contains(&"1. first".to_string()), "{texts:?}");
    }

    #[test]
    fn heading_levels_are_visually_distinct() {
        let h1 = markdown_lines("# 一");
        let h2 = markdown_lines("## 二");
        assert!(
            h1[0].spans[0]
                .style
                .add_modifier
                .contains(Modifier::UNDERLINED),
            "h1 underlined"
        );
        assert!(
            !h2[0].spans[0]
                .style
                .add_modifier
                .contains(Modifier::UNDERLINED),
            "h2 not underlined"
        );
    }

    #[test]
    fn inline_styles_split_into_styled_spans() {
        let lines = markdown_lines("a **bold** and `code`");
        assert_eq!(lines.len(), 1);
        let spans = &lines[0].spans;
        let bold = spans.iter().find(|s| s.content == "bold").unwrap();
        assert!(bold.style.add_modifier.contains(Modifier::BOLD));
        let code = spans.iter().find(|s| s.content == "code").unwrap();
        assert_eq!(code.style.fg, Some(Color::Yellow));
    }

    #[test]
    fn code_block_preserves_lines_and_blank_lines() {
        let lines = markdown_lines("```\nlet a = 1;\n\nlet b = 2;\n```");
        let texts: Vec<String> = lines.iter().map(plain).collect();
        assert_eq!(texts, vec!["let a = 1;", "", "let b = 2;"]);
    }

    #[test]
    fn fenced_code_gets_language_label_and_highlighting() {
        let lines = markdown_lines("```rust\nfn main() {}\n```");
        assert_eq!(plain(&lines[0]), "· rust");
        assert_eq!(plain(&lines[1]), "fn main() {}");
        assert!(
            lines[1].spans.len() > 1,
            "keyword split into styled spans: {:?}",
            lines[1]
        );
        assert!(
            lines[1].spans.iter().any(|s| s.style.fg.is_some()),
            "ansi theme decoded into terminal colors: {:?}",
            lines[1]
        );
    }

    #[test]
    fn unknown_language_falls_back_to_plain_code_style() {
        let lines = markdown_lines("```nosuchlang\nhello world\n```");
        assert_eq!(plain(&lines[0]), "· nosuchlang");
        assert_eq!(plain(&lines[1]), "hello world");
        assert_eq!(lines[1].spans[0].style.fg, Some(Color::Yellow));
    }

    #[test]
    fn table_renders_as_aligned_box() {
        let lines = markdown_lines("| a | bb |\n|---|----|\n| cc | d |");
        let texts: Vec<String> = lines.iter().map(plain).collect();
        assert_eq!(
            texts,
            vec![
                "┌────┬────┐",
                "│ a  │ bb │",
                "├────┼────┤",
                "│ cc │ d  │",
                "└────┴────┘",
            ]
        );
    }

    #[test]
    fn table_columns_align_with_cjk_cells() {
        let lines = markdown_lines("| 名称 | v |\n|------|---|\n| a | 值 |");
        let texts: Vec<String> = lines.iter().map(plain).collect();
        assert_eq!(
            texts,
            vec![
                "┌──────┬────┐",
                "│ 名称 │ v  │",
                "├──────┼────┤",
                "│ a    │ 值 │",
                "└──────┴────┘",
            ]
        );
    }

    #[test]
    fn link_shows_target_unless_it_is_the_text() {
        let lines = markdown_lines("see [docs](https://example.com)");
        assert!(plain(&lines[0]).contains("docs (https://example.com)"));
        let lines = markdown_lines("see <https://example.com>");
        assert_eq!(plain(&lines[0]), "see https://example.com");
    }

    #[test]
    fn blockquote_lines_are_prefixed() {
        let lines = markdown_lines("> 引用内容");
        assert_eq!(plain(&lines[0]), "▎ 引用内容");
    }

    #[test]
    fn blocks_are_separated_by_one_blank_line() {
        let lines = markdown_lines("para one\n\npara two");
        let texts: Vec<String> = lines.iter().map(plain).collect();
        assert_eq!(texts, vec!["para one", "", "para two"]);
    }

    #[test]
    fn cached_render_matches_uncached() {
        let text = "# t\n\n```rust\nlet x = 1;\n```";
        assert_eq!(markdown_lines_cached(text), markdown_lines(text));
        assert_eq!(markdown_lines_cached(text), markdown_lines(text));
    }
}
