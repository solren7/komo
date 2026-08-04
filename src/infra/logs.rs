//! komo's own log files: where they live, which one this process writes, and a
//! bounded tail of one.
//!
//! Shared by `komo logs` (operator-facing, `cli/logs.rs`) and the `logs` tool
//! (`tools/logs.rs`, so the agent can read its own diagnostics mid-conversation)
//! — one definition of "which file is the live one", so the two can never
//! disagree.

use std::{
    collections::VecDeque,
    fs::File,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    sync::OnceLock,
};

/// Where `main.rs::init_tracing` routed *this* process's tracing output, when it
/// went to a single file at all — that is the chat TUI, which cannot log to
/// stderr without tearing the alternate screen. The gateway's file rotates
/// daily, so it is resolved by name ([`latest_gateway_log`]) instead of pinned
/// here, and every other subcommand logs to stderr.
static ACTIVE: OnceLock<PathBuf> = OnceLock::new();

pub fn dir() -> PathBuf {
    komo_config::komo_home().join("logs")
}

/// The chat TUI's append-mode log (`~/.komo/logs/chat-tui.log`).
pub fn chat_log() -> PathBuf {
    dir().join("chat-tui.log")
}

/// Record the file this process logs to. Called once from `init_tracing`.
pub fn set_active(path: PathBuf) {
    let _ = ACTIVE.set(path);
}

pub fn active() -> Option<&'static Path> {
    ACTIVE.get().map(PathBuf::as_path)
}

/// The newest `gateway.YYYY-MM-DD.log`, if any. Date-stamped names sort
/// lexicographically in time order, so the max name is the newest day.
pub fn latest_gateway_log(dir: &Path) -> Option<PathBuf> {
    let entries = std::fs::read_dir(dir).ok()?;
    entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(is_daily_log_name)
        })
        .max()
}

/// `gateway.<10-char date>.log` — excludes the launchd captures
/// (`gateway.log` / `gateway.err.log`) and the TUI log.
pub fn is_daily_log_name(name: &str) -> bool {
    name.strip_prefix("gateway.")
        .and_then(|rest| rest.strip_suffix(".log"))
        .is_some_and(|date| {
            date.len() == 10 && date.chars().all(|c| c.is_ascii_digit() || c == '-')
        })
}

/// The tail of a log file: the last `lines` lines that pass the filter.
pub struct Tail {
    /// Kept lines, oldest first, each with its trailing newline.
    pub lines: Vec<String>,
    /// Byte offset one past the last line read — where `komo logs -f` starts
    /// following, so a line can be neither skipped nor printed twice.
    pub end: u64,
    /// How many lines passed the filter in the whole file, which can exceed
    /// `lines.len()` when the tail was capped.
    pub matched: usize,
}

/// Read the last `lines` lines of `path`, keeping only those containing
/// `contains` (case-insensitive) when it is given. Holds at most `lines` lines
/// in memory regardless of file size.
pub fn tail(path: &Path, lines: usize, contains: Option<&str>) -> std::io::Result<Tail> {
    let needle = contains.map(str::to_lowercase);
    let mut reader = BufReader::new(File::open(path)?);
    let mut kept: VecDeque<String> = VecDeque::with_capacity(lines.min(1024) + 1);
    let mut matched = 0usize;
    let mut end = 0u64;
    let mut line = String::new();
    loop {
        line.clear();
        // Log files can hold non-UTF8 bytes (a tool result echoed verbatim);
        // read lossily rather than aborting the whole tail on one bad line.
        let read = read_line_lossy(&mut reader, &mut line)?;
        if read == 0 {
            break;
        }
        end += read as u64;
        // Older log files carry ANSI color codes (tracing wrote them before the
        // file writers turned color off); strip them so the tail is clean text
        // and the filter can't be defeated by an escape sequence mid-token.
        if line.contains('\u{1b}') {
            line = strip_ansi(&line);
        }
        if let Some(needle) = &needle
            && !line.to_lowercase().contains(needle)
        {
            continue;
        }
        matched += 1;
        if lines == 0 {
            continue;
        }
        if kept.len() == lines {
            kept.pop_front();
        }
        kept.push_back(line.clone());
    }
    Ok(Tail {
        lines: kept.into(),
        end,
        matched,
    })
}

/// Remove ANSI escape sequences: CSI (`ESC [ … final`), OSC (`ESC ] … BEL`),
/// and two-byte escapes. Anything malformed just loses its ESC.
fn strip_ansi(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('[') => {
                // CSI: skip until the final byte (0x40..=0x7e).
                for n in chars.by_ref() {
                    if ('\u{40}'..='\u{7e}').contains(&n) {
                        break;
                    }
                }
            }
            Some(']') => {
                // OSC: terminated by BEL (or ST, whose ESC restarts this loop).
                for n in chars.by_ref() {
                    if n == '\u{7}' {
                        break;
                    }
                }
            }
            _ => {} // two-byte escape: the consumed char was the whole sequence
        }
    }
    out
}

/// `BufRead::read_line` that tolerates invalid UTF-8 by replacing it, returning
/// the number of **bytes** consumed (0 at EOF).
fn read_line_lossy<R: BufRead>(reader: &mut R, out: &mut String) -> std::io::Result<usize> {
    let mut buf = Vec::new();
    let read = reader.read_until(b'\n', &mut buf)?;
    out.push_str(&String::from_utf8_lossy(&buf));
    Ok(read)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(name);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn daily_log_names_are_recognized() {
        assert!(is_daily_log_name("gateway.2026-07-18.log"));
        assert!(!is_daily_log_name("gateway.log"));
        assert!(!is_daily_log_name("gateway.err.log"));
        assert!(!is_daily_log_name("chat-tui.log"));
    }

    #[test]
    fn newest_daily_file_wins() {
        let dir = temp_dir("komo_logs_test_newest");
        for name in [
            "gateway.2026-07-16.log",
            "gateway.2026-07-18.log",
            "gateway.2026-07-17.log",
            "gateway.err.log",
        ] {
            std::fs::write(dir.join(name), "x").unwrap();
        }
        let newest = latest_gateway_log(&dir).unwrap();
        assert_eq!(
            newest.file_name().unwrap().to_str().unwrap(),
            "gateway.2026-07-18.log"
        );
    }

    #[test]
    fn tail_keeps_the_last_lines_and_reports_the_end_offset() {
        let dir = temp_dir("komo_logs_test_tail");
        let path = dir.join("gateway.2026-07-18.log");
        let body = "a\nb\nc\nd\n";
        std::fs::write(&path, body).unwrap();

        let t = tail(&path, 2, None).unwrap();
        assert_eq!(t.lines, vec!["c\n", "d\n"]);
        assert_eq!(t.matched, 4, "counts every line, not just the kept ones");
        assert_eq!(
            t.end,
            body.len() as u64,
            "follow resumes exactly at end of file"
        );
    }

    #[test]
    fn tail_strips_ansi_from_old_log_files() {
        let dir = temp_dir("komo_logs_test_ansi");
        let path = dir.join("gateway.2026-08-04.log");
        // The shape tracing wrote before file writers turned color off.
        std::fs::write(
            &path,
            "\u{1b}[2m2026-08-04T03:02:04Z\u{1b}[0m \u{1b}[33m WARN\u{1b}[0m run failed\n",
        )
        .unwrap();

        let t = tail(&path, 10, Some("warn")).unwrap();
        assert_eq!(t.lines, vec!["2026-08-04T03:02:04Z  WARN run failed\n"]);
    }

    #[test]
    fn strip_ansi_handles_csi_osc_and_bare_escapes() {
        assert_eq!(strip_ansi("plain"), "plain");
        assert_eq!(strip_ansi("\u{1b}[1;33mbold\u{1b}[0m"), "bold");
        assert_eq!(strip_ansi("\u{1b}]0;title\u{7}rest"), "rest");
        assert_eq!(strip_ansi("a\u{1b}Mb"), "ab");
        assert_eq!(strip_ansi("dangling\u{1b}"), "dangling");
    }

    #[test]
    fn tail_filter_is_case_insensitive() {
        let dir = temp_dir("komo_logs_test_filter");
        let path = dir.join("chat-tui.log");
        std::fs::write(&path, "INFO ok\nERROR boom\ninfo fine\nerror again\n").unwrap();

        let t = tail(&path, 10, Some("Error")).unwrap();
        assert_eq!(t.lines, vec!["ERROR boom\n", "error again\n"]);
        assert_eq!(t.matched, 2);
    }
}
