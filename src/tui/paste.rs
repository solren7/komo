//! Paste handling shared by the composer and the event loop.
//!
//! Two mechanisms, both modeled on grok build
//! (`xai-grok-pager/src/views/prompt_widget` and `app/event_loop.rs`):
//!
//! * **Chips** — a paste past [`CHIP_MIN_LINES`] lines or [`CHIP_MAX_BYTES`]
//!   bytes is *displayed* as a one-line label while the draft keeps the text
//!   verbatim. The point is not tidiness: a 1 MB paste rendered as a million
//!   characters would be re-wrapped on every frame.
//! * **Coalescing** — terminals without bracketed paste deliver a paste as a
//!   burst of key events, so a multi-line paste would submit at its first
//!   newline. [`coalesce_rapid_keys`] folds such a burst back into one paste.

use std::time::Duration;

use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers};
use tokio::sync::mpsc::UnboundedReceiver;

/// Multi-line pastes at or above this many lines collapse into a chip.
pub const CHIP_MIN_LINES: usize = 4;
/// Pastes larger than this collapse into a chip regardless of line count.
pub const CHIP_MAX_BYTES: usize = 10_000;
/// Minimum key events in one burst before it can be read as a paste. Below this
/// it is someone typing.
pub const COALESCE_THRESHOLD: usize = 3;

/// How long to wait for a follow-up event before deciding a burst was just a
/// keystroke. Short enough that a real keypress is never delayed noticeably.
const DETECT_TIMEOUT: Duration = Duration::from_millis(2);
/// Window for each round after a paste has been detected — the rest of it is
/// still in transit from the terminal.
const CONTINUE_TIMEOUT: Duration = Duration::from_millis(10);
/// Safety cap on events accumulated in one extension pass, so a stuck key or a
/// flood can't hold the loop open indefinitely.
const EXTEND_MAX_EVENTS: usize = 5_000;

/// Whether `text` should be shown as a chip rather than inline.
pub fn is_chip_worthy(text: &str) -> bool {
    // `lines()` treats a trailing newline as optional, so "hello\n" is 1 line.
    text.lines().count() >= CHIP_MIN_LINES || text.len() > CHIP_MAX_BYTES
}

/// The chip's label. A large paste reads better as its size than as a line
/// count ("1.0 MB" tells you more than "1 line" for a minified bundle), so byte
/// size wins when it is what triggered the chip. Decimal (1000-based) units,
/// matching grok build's labels.
pub fn chip_label(text: &str) -> String {
    if text.len() > CHIP_MAX_BYTES {
        let len = text.len();
        let size = if len >= 1_000_000 {
            format!("{:.1} MB", len as f64 / 1_000_000.0)
        } else {
            format!("{} KB", len / 1000)
        };
        return format!("[Pasted: {size}]");
    }
    let lines = text.lines().count();
    format!(
        "[Pasted: {lines} line{}]",
        if lines == 1 { "" } else { "s" }
    )
}

/// Normalize a clipboard payload: CRLF and lone CR both become LF, so a paste
/// from a Windows editor doesn't leave stray carriage returns in the draft.
pub fn normalize_cr(text: &str) -> String {
    if text.contains('\r') {
        text.replace("\r\n", "\n").replace('\r', "\n")
    } else {
        text.to_string()
    }
}

/// A character/Enter/Tab press with no control modifiers — the events a paste
/// decomposes into on a terminal without bracketed paste. Release/Repeat are
/// excluded: repeats come from a held key, releases carry no content.
pub fn is_pasteable_key(event: &Event) -> bool {
    match event {
        Event::Key(key) if key.kind == KeyEventKind::Press => match key.code {
            KeyCode::Char(_) => key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT,
            KeyCode::Enter | KeyCode::Tab => key.modifiers.is_empty(),
            _ => false,
        },
        _ => false,
    }
}

/// Take every event already queued, without waiting.
pub fn drain_ready(batch: &mut Vec<Event>, rx: &mut UnboundedReceiver<Event>) {
    while let Ok(event) = rx.try_recv() {
        batch.push(event);
    }
}

/// Whether `batch` might be a paste that the terminal delivered as keystrokes:
/// it carries pasteable keys and no real [`Event::Paste`].
pub fn should_extend(batch: &[Event]) -> bool {
    !batch.iter().any(|e| matches!(e, Event::Paste(_))) && batch.iter().any(is_pasteable_key)
}

/// Keep collecting while the burst still looks like a paste in transit.
///
/// One short detection window first: a lone keystroke has no follow-up and
/// returns immediately, so ordinary typing pays at most [`DETECT_TIMEOUT`].
/// Once a second pasteable key shows up, collection continues on the longer
/// [`CONTINUE_TIMEOUT`] until the burst goes quiet or hits
/// [`EXTEND_MAX_EVENTS`].
pub async fn extend_for_paste(batch: &mut Vec<Event>, rx: &mut UnboundedReceiver<Event>) {
    let before = batch.len();
    match tokio::time::timeout(DETECT_TIMEOUT, rx.recv()).await {
        Ok(Some(event)) => {
            batch.push(event);
            drain_ready(batch, rx);
        }
        // Nothing followed (or the reader is gone): a keystroke, not a paste.
        _ => return,
    }
    if !batch[before..].iter().any(is_pasteable_key) {
        return;
    }
    let mut added = 0usize;
    while added < EXTEND_MAX_EVENTS {
        match tokio::time::timeout(CONTINUE_TIMEOUT, rx.recv()).await {
            Ok(Some(event)) => {
                batch.push(event);
                added += 1;
                drain_ready(batch, rx);
            }
            _ => break,
        }
    }
}

/// Fold each run of pasteable key events into a single [`Event::Paste`], so a
/// paste on a terminal without bracketed paste doesn't submit at its first
/// newline.
///
/// A run collapses only when it is at least [`COALESCE_THRESHOLD`] events **and
/// a character follows an Enter** — that is what separates "typed a line, hit
/// Enter" (which must still submit) from "pasted several lines". Everything
/// else, including any run that already carries a real `Event::Paste`, passes
/// through untouched.
pub fn coalesce_rapid_keys(events: Vec<Event>) -> Vec<Event> {
    if events.len() < COALESCE_THRESHOLD || events.iter().any(|e| matches!(e, Event::Paste(_))) {
        return events;
    }

    let mut out = Vec::with_capacity(events.len());
    let mut i = 0;
    while i < events.len() {
        if !is_pasteable_key(&events[i]) {
            out.push(events[i].clone());
            i += 1;
            continue;
        }
        let start = i;
        let mut text = String::new();
        let mut seen_enter = false;
        let mut char_after_enter = false;
        while i < events.len() && is_pasteable_key(&events[i]) {
            if let Event::Key(key) = &events[i] {
                match key.code {
                    KeyCode::Char(c) => {
                        text.push(c);
                        char_after_enter |= seen_enter;
                    }
                    KeyCode::Enter => {
                        text.push('\n');
                        seen_enter = true;
                    }
                    KeyCode::Tab => {
                        text.push('\t');
                        char_after_enter |= seen_enter;
                    }
                    _ => unreachable!("is_pasteable_key guards this"),
                }
            }
            i += 1;
        }
        if i - start >= COALESCE_THRESHOLD && char_after_enter {
            tracing::debug!(
                run_len = i - start,
                text_len = text.len(),
                "coalesced rapid key events into a paste"
            );
            out.push(Event::Paste(text));
        } else {
            out.extend_from_slice(&events[start..i]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyEvent;

    fn ch(c: char) -> Event {
        Event::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE))
    }

    fn enter() -> Event {
        Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
    }

    fn keys(s: &str) -> Vec<Event> {
        s.chars()
            .map(|c| if c == '\n' { enter() } else { ch(c) })
            .collect()
    }

    #[test]
    fn chip_threshold_is_lines_or_bytes() {
        assert!(!is_chip_worthy("one\ntwo\nthree"), "3 lines stays inline");
        assert!(is_chip_worthy("one\ntwo\nthree\nfour"));
        assert!(
            is_chip_worthy(&"x".repeat(CHIP_MAX_BYTES + 1)),
            "one long line"
        );
    }

    #[test]
    fn chip_label_prefers_size_for_large_pastes() {
        assert_eq!(chip_label("a\nb\nc\nd"), "[Pasted: 4 lines]");
        assert_eq!(chip_label(&"x".repeat(12_000)), "[Pasted: 12 KB]");
        assert_eq!(chip_label(&"x".repeat(1_500_000)), "[Pasted: 1.5 MB]");
    }

    #[test]
    fn typing_a_line_then_enter_still_submits() {
        // No character follows the Enter, so this must NOT become a paste —
        // otherwise every fast-typed message would stop submitting.
        let events = coalesce_rapid_keys(keys("hello\n"));
        assert!(
            events.iter().all(|e| !matches!(e, Event::Paste(_))),
            "typed line + Enter must stay keys"
        );
        assert_eq!(events.len(), 6);
    }

    #[test]
    fn a_multi_line_burst_becomes_one_paste() {
        let events = coalesce_rapid_keys(keys("one\ntwo\nthree"));
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], Event::Paste(t) if t == "one\ntwo\nthree"));
    }

    #[test]
    fn short_bursts_and_real_pastes_pass_through() {
        assert_eq!(coalesce_rapid_keys(keys("hi")).len(), 2, "below threshold");
        let mixed = vec![Event::Paste("x".into()), ch('a'), ch('b')];
        let out = coalesce_rapid_keys(mixed);
        assert_eq!(out.len(), 3, "a real bracketed paste is left alone");
    }

    #[test]
    fn non_key_events_keep_their_place_around_a_coalesced_run() {
        let mut events = keys("a\nb\nc");
        events.insert(0, Event::Resize(80, 24));
        events.push(Event::Resize(100, 30));
        let out = coalesce_rapid_keys(events);
        assert!(matches!(out[0], Event::Resize(80, 24)));
        assert!(matches!(&out[1], Event::Paste(t) if t == "a\nb\nc"));
        assert!(matches!(out[2], Event::Resize(100, 30)));
    }

    #[test]
    fn crlf_is_normalized() {
        assert_eq!(normalize_cr("a\r\nb\rc"), "a\nb\nc");
    }

    /// Time is paused, so the windows resolve deterministically: a queued event
    /// arrives at once, an empty channel trips the timeout immediately.
    #[tokio::test(start_paused = true)]
    async fn extend_collects_a_whole_keystroke_burst() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let mut burst = keys("one\ntwo\nthree").into_iter();
        let mut batch = vec![burst.next().unwrap()];
        for event in burst {
            tx.send(event).unwrap();
        }

        assert!(should_extend(&batch));
        extend_for_paste(&mut batch, &mut rx).await;
        assert_eq!(batch.len(), 13, "the whole burst was collected");

        let folded = coalesce_rapid_keys(batch);
        assert_eq!(folded.len(), 1);
        assert!(matches!(&folded[0], Event::Paste(t) if t == "one\ntwo\nthree"));
    }

    #[tokio::test(start_paused = true)]
    async fn extend_gives_up_on_a_lone_keystroke() {
        let (_tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let mut batch = vec![ch('a')];
        extend_for_paste(&mut batch, &mut rx).await;
        assert_eq!(batch.len(), 1, "typing is not held up waiting for a paste");
    }
}
