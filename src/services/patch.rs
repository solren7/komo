//! The `apply_patch` envelope format, ported from opencode v2's `patch.ts`.
//!
//! Not unified diff: this grammar carries no line numbers, so a model doesn't
//! have to count. Chunks are located by their context lines instead, which is
//! why `seek` below tries progressively looser comparisons — a model that
//! reflowed trailing whitespace or smart-quoted a string should still land its
//! patch, since the *replacement* text is taken verbatim from the patch either
//! way.
//!
//! ```text
//! *** Begin Patch
//! *** Add File: src/new.rs
//! +fn hello() {}
//! *** Update File: src/main.rs
//! @@ fn main()
//! -    old();
//! +    new();
//! *** Delete File: src/stale.rs
//! *** End Patch
//! ```

const BEGIN: &str = "*** Begin Patch";
const END: &str = "*** End Patch";
const ADD: &str = "*** Add File:";
const DELETE: &str = "*** Delete File:";
const UPDATE: &str = "*** Update File:";
const MOVE: &str = "*** Move to:";
const EOF_MARKER: &str = "*** End of File";

/// One file operation.
#[derive(Debug, PartialEq, Eq)]
pub enum Hunk {
    Add { path: String, contents: String },
    Delete { path: String },
    Update { path: String, chunks: Vec<Chunk> },
}

impl Hunk {
    pub fn path(&self) -> &str {
        match self {
            Hunk::Add { path, .. } | Hunk::Delete { path } | Hunk::Update { path, .. } => path,
        }
    }
}

/// One `@@` chunk of an update: the lines to find, and what to put there.
#[derive(Debug, PartialEq, Eq, Default)]
pub struct Chunk {
    pub old_lines: Vec<String>,
    pub new_lines: Vec<String>,
    /// Text after `@@`: a landmark to seek past before matching `old_lines`.
    pub change_context: Option<String>,
    /// The chunk was marked `*** End of File`, anchoring it to the tail.
    pub end_of_file: bool,
}

/// Parse a patch. `Err` carries a model-facing explanation.
pub fn parse(text: &str) -> Result<Vec<Hunk>, String> {
    let text = strip_heredoc(text.trim());
    let lines: Vec<&str> = text.split('\n').collect();
    let begin = lines.iter().position(|l| l.trim() == BEGIN);
    let end = lines.iter().position(|l| l.trim() == END);
    let (begin, end) = match (begin, end) {
        (Some(b), Some(e)) if b < e => (b, e),
        _ => {
            return Err(format!(
                "invalid patch: it must start with `{BEGIN}` and end with `{END}`"
            ));
        }
    };

    let mut hunks = Vec::new();
    let mut i = begin + 1;
    while i < end {
        let line = lines[i];
        if let Some(rest) = line.strip_prefix(ADD) {
            let path = require_path(rest, "add")?;
            let (contents, next) = parse_add(&lines, i + 1)?;
            hunks.push(Hunk::Add { path, contents });
            i = next;
        } else if let Some(rest) = line.strip_prefix(DELETE) {
            hunks.push(Hunk::Delete {
                path: require_path(rest, "delete")?,
            });
            i += 1;
        } else if let Some(rest) = line.strip_prefix(UPDATE) {
            let path = require_path(rest, "update")?;
            let mut next = i + 1;
            if next < lines.len() && lines[next].starts_with(MOVE) {
                // v2 doesn't implement moves either; refusing beats pretending.
                return Err("apply_patch does not support `*** Move to:` yet — \
                     write the new file and delete the old one instead."
                    .to_string());
            }
            let (chunks, after) = parse_update(&lines, next)?;
            if chunks.is_empty() {
                return Err(format!(
                    "invalid update for {path}: expected at least one `@@` chunk"
                ));
            }
            next = after;
            hunks.push(Hunk::Update { path, chunks });
            i = next;
        } else {
            return Err(format!("invalid patch line: {line}"));
        }
    }
    Ok(hunks)
}

fn require_path(rest: &str, verb: &str) -> Result<String, String> {
    let path = rest.trim();
    if path.is_empty() {
        return Err(format!("invalid {verb} hunk: missing a file path"));
    }
    Ok(path.to_string())
}

fn parse_add(lines: &[&str], start: usize) -> Result<(String, usize), String> {
    let mut content: Vec<&str> = Vec::new();
    let mut i = start;
    while i < lines.len() && !lines[i].starts_with("***") {
        let line = lines[i].strip_prefix('+').ok_or_else(|| {
            format!(
                "invalid add-file line (expected a `+` prefix): {}",
                lines[i]
            )
        })?;
        content.push(line);
        i += 1;
    }
    Ok((content.join("\n"), i))
}

fn parse_update(lines: &[&str], start: usize) -> Result<(Vec<Chunk>, usize), String> {
    let mut chunks: Vec<Chunk> = Vec::new();
    let mut i = start;
    while i < lines.len() && !lines[i].starts_with("***") {
        let header = lines[i]
            .strip_prefix("@@")
            .ok_or_else(|| format!("invalid update line (expected `@@`): {}", lines[i]))?;
        let mut chunk = Chunk {
            change_context: Some(header.trim().to_string()).filter(|s| !s.is_empty()),
            ..Default::default()
        };
        i += 1;
        while i < lines.len() && !lines[i].starts_with("@@") {
            let line = lines[i];
            if line == EOF_MARKER {
                chunk.end_of_file = true;
                i += 1;
                break;
            }
            if line.starts_with("***") {
                break;
            }
            if let Some(body) = line.strip_prefix(' ') {
                chunk.old_lines.push(body.to_string());
                chunk.new_lines.push(body.to_string());
            } else if let Some(body) = line.strip_prefix('-') {
                chunk.old_lines.push(body.to_string());
            } else if let Some(body) = line.strip_prefix('+') {
                chunk.new_lines.push(body.to_string());
            } else if line.is_empty() {
                // A bare empty line reads as unchanged blank context; models emit
                // these constantly and rejecting them fails an otherwise fine patch.
                chunk.old_lines.push(String::new());
                chunk.new_lines.push(String::new());
            } else {
                return Err(format!(
                    "invalid chunk line (expected ` `, `-` or `+`): {line}"
                ));
            }
            i += 1;
        }
        chunks.push(chunk);
    }
    Ok((chunks, i))
}

/// Apply `chunks` to `original`, returning the new content.
///
/// `label` only names the file in error messages. The result is always
/// newline-terminated, like the input conventionally is.
pub fn apply(label: &str, original: &str, chunks: &[Chunk]) -> Result<String, String> {
    let mut lines: Vec<String> = original.split('\n').map(str::to_string).collect();
    // `split` leaves a trailing "" for a newline-terminated file; work without it.
    if lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }

    // Locate every chunk first (against the *original* lines), then splice from
    // the bottom up so earlier indices stay valid.
    let mut edits: Vec<(usize, usize, &Vec<String>)> = Vec::new();
    let mut cursor = 0usize;
    for chunk in chunks {
        if let Some(context) = &chunk.change_context {
            let at = seek(&lines, std::slice::from_ref(context), cursor, false)
                .ok_or_else(|| format!("failed to find context `{context}` in {label}"))?;
            cursor = at + 1;
        }
        // No old lines: pure append at the end of the file.
        if chunk.old_lines.is_empty() {
            edits.push((lines.len(), 0, &chunk.new_lines));
            continue;
        }
        let at = seek(&lines, &chunk.old_lines, cursor, chunk.end_of_file).ok_or_else(|| {
            format!(
                "failed to find these lines in {label}:\n{}",
                chunk.old_lines.join("\n")
            )
        })?;
        edits.push((at, chunk.old_lines.len(), &chunk.new_lines));
        cursor = at + chunk.old_lines.len();
    }

    edits.sort_by_key(|(start, _, _)| *start);
    for (start, remove, insert) in edits.into_iter().rev() {
        lines.splice(start..start + remove, insert.iter().cloned());
    }

    let mut out = lines.join("\n");
    if !out.is_empty() || !original.is_empty() {
        out.push('\n');
    }
    Ok(out)
}

/// Find `pattern` in `lines` at or after `start`, trying stricter comparisons
/// first. `eof` prefers the tail (`*** End of File`).
fn seek(lines: &[String], pattern: &[String], start: usize, eof: bool) -> Option<usize> {
    if pattern.is_empty() || pattern.len() > lines.len() {
        return None;
    }
    type Cmp = fn(&str, &str) -> bool;
    let ladder: [Cmp; 4] = [
        |a, b| a == b,
        |a, b| a.trim_end() == b.trim_end(),
        |a, b| a.trim() == b.trim(),
        |a, b| normalize(a.trim()) == normalize(b.trim()),
    ];
    for compare in ladder {
        let matches_at = |offset: usize| {
            pattern
                .iter()
                .enumerate()
                .all(|(i, p)| compare(&lines[offset + i], p))
        };
        if eof {
            let offset = lines.len() - pattern.len();
            if offset >= start && matches_at(offset) {
                return Some(offset);
            }
        }
        for offset in start..=lines.len() - pattern.len() {
            if matches_at(offset) {
                return Some(offset);
            }
        }
    }
    None
}

/// Fold the punctuation an LLM tends to prettify back to ASCII, so a smart-quoted
/// context line still matches the file it came from.
fn normalize(value: &str) -> String {
    value
        .chars()
        .map(|c| match c {
            '\u{2018}' | '\u{2019}' | '\u{201a}' | '\u{201b}' => '\'',
            '\u{201c}' | '\u{201d}' | '\u{201e}' | '\u{201f}' => '"',
            '\u{2010}'..='\u{2015}' => '-',
            '\u{00a0}' => ' ',
            other => other,
        })
        .collect()
}

/// Unwrap a patch a model wrapped in a heredoc (`cat <<'EOF' … EOF`).
fn strip_heredoc(input: &str) -> &str {
    let rest = input.strip_prefix("cat ").unwrap_or(input);
    let Some(after) = rest.strip_prefix("<<") else {
        return input;
    };
    let (tag_line, body) = match after.split_once('\n') {
        Some(parts) => parts,
        None => return input,
    };
    let tag = tag_line.trim().trim_matches(['\'', '"']);
    if tag.is_empty() {
        return input;
    }
    match body.trim_end().strip_suffix(tag) {
        Some(inner) => inner.trim_end_matches('\n'),
        None => input,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_add_update_and_delete() {
        let patch = "\
*** Begin Patch
*** Add File: src/new.rs
+fn hello() {}
+
*** Update File: src/main.rs
@@ fn main()
-    old();
+    new();
*** Delete File: src/stale.rs
*** End Patch";
        let hunks = parse(patch).unwrap();
        assert_eq!(hunks.len(), 3);
        assert_eq!(
            hunks[0],
            Hunk::Add {
                path: "src/new.rs".into(),
                contents: "fn hello() {}\n".into()
            }
        );
        let Hunk::Update { path, chunks } = &hunks[1] else {
            panic!("expected an update, got {:?}", hunks[1]);
        };
        assert_eq!(path, "src/main.rs");
        assert_eq!(chunks[0].change_context.as_deref(), Some("fn main()"));
        assert_eq!(chunks[0].old_lines, vec!["    old();"]);
        assert_eq!(chunks[0].new_lines, vec!["    new();"]);
        assert_eq!(
            hunks[2],
            Hunk::Delete {
                path: "src/stale.rs".into()
            }
        );
    }

    #[test]
    fn missing_markers_are_rejected() {
        assert!(parse("*** Add File: a\n+x").is_err());
        assert!(parse("").is_err());
        // End before begin.
        assert!(parse("*** End Patch\n*** Begin Patch").is_err());
    }

    #[test]
    fn malformed_bodies_are_rejected() {
        // An add line without `+`.
        assert!(parse("*** Begin Patch\n*** Add File: a\nplain\n*** End Patch").is_err());
        // An update without any `@@` chunk.
        assert!(parse("*** Begin Patch\n*** Update File: a\n-x\n*** End Patch").is_err());
        // A path-less hunk.
        assert!(parse("*** Begin Patch\n*** Add File:\n+x\n*** End Patch").is_err());
        // An unknown directive.
        assert!(parse("*** Begin Patch\n*** Rename File: a\n*** End Patch").is_err());
    }

    #[test]
    fn moves_are_refused_explicitly() {
        let err =
            parse("*** Begin Patch\n*** Update File: a\n*** Move to: b\n@@\n-x\n+y\n*** End Patch")
                .unwrap_err();
        assert!(err.contains("Move to"), "{err}");
    }

    #[test]
    fn a_heredoc_wrapper_is_unwrapped() {
        let wrapped = "cat <<'EOF'\n*** Begin Patch\n*** Delete File: a\n*** End Patch\nEOF";
        let hunks = parse(wrapped).unwrap();
        assert_eq!(hunks.len(), 1);
    }

    #[test]
    fn applies_a_chunk_by_context() {
        let original = "fn main() {\n    old();\n}\n";
        let chunks = vec![Chunk {
            old_lines: vec!["    old();".into()],
            new_lines: vec!["    new();".into()],
            change_context: Some("fn main() {".into()),
            end_of_file: false,
        }];
        assert_eq!(
            apply("a.rs", original, &chunks).unwrap(),
            "fn main() {\n    new();\n}\n"
        );
    }

    #[test]
    fn applies_several_chunks_in_one_file() {
        let original = "a\nb\nc\nd\n";
        let chunks = vec![
            Chunk {
                old_lines: vec!["a".into()],
                new_lines: vec!["A".into()],
                ..Default::default()
            },
            Chunk {
                old_lines: vec!["d".into()],
                new_lines: vec!["D".into()],
                ..Default::default()
            },
        ];
        assert_eq!(apply("f", original, &chunks).unwrap(), "A\nb\nc\nD\n");
    }

    #[test]
    fn an_empty_old_side_appends() {
        let chunks = vec![Chunk {
            old_lines: vec![],
            new_lines: vec!["tail".into()],
            ..Default::default()
        }];
        assert_eq!(apply("f", "head\n", &chunks).unwrap(), "head\ntail\n");
    }

    /// The looser rungs of the ladder: trailing whitespace and smart quotes.
    #[test]
    fn locating_tolerates_whitespace_and_smart_quotes() {
        let original = "let s = \"hi\";   \n";
        let chunks = vec![Chunk {
            // Model sent curly quotes and no trailing spaces.
            old_lines: vec!["let s = \u{201c}hi\u{201d};".into()],
            new_lines: vec!["let s = \"bye\";".into()],
            ..Default::default()
        }];
        assert_eq!(apply("f", original, &chunks).unwrap(), "let s = \"bye\";\n");
    }

    #[test]
    fn a_chunk_that_does_not_match_reports_the_lines() {
        let chunks = vec![Chunk {
            old_lines: vec!["nowhere".into()],
            new_lines: vec!["x".into()],
            ..Default::default()
        }];
        let err = apply("a.rs", "something else\n", &chunks).unwrap_err();
        assert!(err.contains("failed to find these lines in a.rs"), "{err}");
        assert!(err.contains("nowhere"), "{err}");
    }

    #[test]
    fn a_missing_context_landmark_is_reported() {
        let chunks = vec![Chunk {
            old_lines: vec!["x".into()],
            new_lines: vec!["y".into()],
            change_context: Some("fn absent()".into()),
            end_of_file: false,
        }];
        let err = apply("a.rs", "x\n", &chunks).unwrap_err();
        assert!(err.contains("failed to find context"), "{err}");
    }

    #[test]
    fn end_of_file_anchors_to_the_tail() {
        // "x" appears twice; the marker picks the last one.
        let chunks = vec![Chunk {
            old_lines: vec!["x".into()],
            new_lines: vec!["LAST".into()],
            change_context: None,
            end_of_file: true,
        }];
        assert_eq!(apply("f", "x\ny\nx\n", &chunks).unwrap(), "x\ny\nLAST\n");
    }
}
