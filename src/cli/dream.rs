//! `shion dream` — operator view over the usage-driven memory "dreaming"
//! consolidation (the OpenClaw-borrowed back-loop).
//!
//! By default this is a **dry run**: it shows which candidate memories would be
//! promoted (recalled often enough) or archived (old and never recalled), with
//! the dreaming score that drove each verdict — like OpenClaw's `rem-harness` /
//! `promote-explain`. Pass `--apply` to actually run one consolidation cycle
//! (the same `DreamSweep` the gateway runs on `dream_schedule`).
//!
//! The dry-run routes through a running gateway (which holds the db lock) when
//! one is up; `--apply` mutates the db, so it requires the gateway stopped.

use crate::agent::daemon::DreamSweep;
use crate::cli::gateway_client::GatewayClient;
use crate::domain::memory::MemoryRepository;
use crate::infra::memory::memory_db::MemoryDb;
use crate::services::operator_control::{DreamItem, DreamReport, actions::dream_classify};
use std::sync::Arc;

/// Run a dreaming cycle, or preview one. `apply = false` mutates nothing.
pub async fn run(url: &str, apply: bool) -> anyhow::Result<()> {
    let now = time::OffsetDateTime::now_utc().unix_timestamp();

    // Both preview and apply route through a running gateway (which holds the db
    // lock) when one is up, else open the db directly.
    let gw = GatewayClient::try_connect().await;
    let report = match &gw {
        Some(gw) => {
            let (promote, archive) = gw.dream_preview().await?;
            DreamReport { promote, archive }
        }
        None => dream_classify(&MemoryDb::connect(url).await?.list().await?, now),
    };

    if report.is_empty() {
        println!("Nothing to dream about — no candidate meets the promote or archive bar.");
        return Ok(());
    }

    report_bucket(
        "promote → active (well-recalled candidates)",
        &report.promote,
    );
    report_bucket("archive (old, never recalled)", &report.archive);

    if !apply {
        println!("\n(dry run — pass --apply to execute this cycle)");
        return Ok(());
    }

    let (promoted, archived) = match &gw {
        Some(gw) => gw.dream_apply().await?,
        None => {
            let db = Arc::new(MemoryDb::connect(url).await?);
            let summary = DreamSweep { memories: db }.apply().await?;
            (summary.memories_promoted, summary.memories_archived)
        }
    };
    println!("\nApplied: promoted {promoted}, archived {archived}.");
    Ok(())
}

fn report_bucket(label: &str, items: &[DreamItem]) {
    if items.is_empty() {
        return;
    }
    println!("\n{label}: {}", items.len());
    for m in items.iter().take(20) {
        println!(
            "  {}  [recalls={} queries={} score={:.2}]  {}",
            m.id, m.recall_count, m.unique_queries, m.score, m.content
        );
    }
    if items.len() > 20 {
        println!("  … and {} more", items.len() - 20);
    }
}
