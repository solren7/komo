//! `komo logs` — print (and optionally follow) the gateway log. File lookup and
//! the bounded tail live in `infra::logs`, shared with the `logs` tool so the
//! operator command and the agent read the same files the same way.
//!
//! The gateway writes its tracing output to a daily-rotated file
//! (`~/.komo/logs/gateway.YYYY-MM-DD.log`, a month kept — see
//! `main.rs::open_gateway_log`), teed with stderr. This command reads the
//! newest daily file; the pre-rotation launchd capture
//! (`gateway.err.log`) is the fallback for logs from older builds.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::time::Duration;

use komo_infra::logs;

/// How often follow mode polls the file for appended bytes.
const FOLLOW_POLL: Duration = Duration::from_millis(500);

pub fn run(lines: usize, follow: bool, stdout: bool) -> anyhow::Result<()> {
    let dir = logs::dir();
    let path = if stdout {
        dir.join("gateway.log")
    } else {
        // Newest daily file, else the legacy launchd stderr capture.
        logs::latest_gateway_log(&dir).unwrap_or_else(|| dir.join("gateway.err.log"))
    };
    if !path.exists() {
        anyhow::bail!(
            "no log file at {} — has the gateway run yet? (check `komo gateway status`)",
            path.display()
        );
    }

    let out = std::io::stdout();
    let mut out = out.lock();

    // Print the last `lines` lines, keeping only that many in memory.
    let tail = logs::tail(&path, lines, None)?;
    for l in &tail.lines {
        out.write_all(l.as_bytes())?;
    }
    out.flush()?;

    if !follow {
        return Ok(());
    }

    // Follow: stream bytes appended after the point we've already printed.
    let mut pos = tail.end;
    loop {
        std::thread::sleep(FOLLOW_POLL);
        let mut file = File::open(&path)?;
        let len = file.metadata()?.len();
        if len < pos {
            // File was truncated or rotated — restart from the top.
            pos = 0;
        }
        if len > pos {
            file.seek(SeekFrom::Start(pos))?;
            let mut buf = Vec::new();
            let read = file.read_to_end(&mut buf)?;
            out.write_all(&buf)?;
            out.flush()?;
            pos += read as u64;
        }
    }
}
