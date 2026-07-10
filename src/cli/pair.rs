//! Pairing operator commands (`shion pair list/approve/revoke`).
//!
//! Approval deliberately lives here and nowhere else: typing
//! `shion pair approve <code>` in a shell on this machine is the proof of
//! ownership that lets a new chat-platform sender talk to the agent. The
//! gateway reads the same SQLite db, so approval takes effect on the
//! sender's next message — no restart.

use crate::{
    cli::gateway_client::{GatewayClient, PairApprove},
    domain::pairing::{ApproveOutcome, PairingRepository},
    infra::persistence::db::Db,
    services::operator_control::{PairingView, actions::pairing_views},
};

fn local_time(unix: i64) -> String {
    chrono::DateTime::from_timestamp(unix, 0)
        .map(|dt| dt.with_timezone(&chrono::Local).to_rfc3339())
        .unwrap_or_else(|| unix.to_string())
}

/// List all pairings: pending requests and approved senders. The code itself is
/// stored only as a salted hash — get it from the sender and run
/// `shion pair approve <code>` (or `/pair approve` in chat while the gateway runs).
pub async fn list(db_url: &str) -> anyhow::Result<()> {
    let pairings: Vec<PairingView> = match GatewayClient::try_connect().await {
        Some(gw) => gw.pairings().await?,
        None => {
            let db = Db::connect(db_url).await?;
            let now = time::OffsetDateTime::now_utc().unix_timestamp();
            pairing_views(PairingRepository::list(&db).await?, now)
        }
    };

    if pairings.is_empty() {
        println!("No pairings. Unknown senders get a code on first contact.");
        return Ok(());
    }
    for p in pairings {
        if p.status == "approved" {
            println!("{}  [approved]  since {}", p.id, local_time(p.created_at));
        } else {
            println!(
                "{}  [{}]  requested {}",
                p.id,
                p.status,
                local_time(p.created_at)
            );
        }
    }
    Ok(())
}

/// Approve the pending request bearing `code`. Routes through a running gateway
/// (which holds the db lock) when one is up, else opens the db directly. (The
/// `/pair approve` chat command is the other in-gateway path.)
pub async fn approve(db_url: &str, code: &str) -> anyhow::Result<()> {
    let code = code.trim().to_uppercase();
    if let Some(gw) = GatewayClient::try_connect().await {
        return match gw.pair_approve(&code).await? {
            PairApprove::Approved(id) => {
                println!("Paired {id} — they can chat now.");
                Ok(())
            }
            PairApprove::NotFound => anyhow::bail!(
                "no approvable pairing with code {code} — unknown or expired (see `shion pair list`)"
            ),
            PairApprove::Locked(retry_after_secs) => anyhow::bail!(
                "too many failed attempts — approve is locked for {} more minutes",
                (retry_after_secs + 59) / 60
            ),
        };
    }
    let db = Db::connect(db_url).await?;
    match PairingRepository::approve_code(&db, &code).await? {
        ApproveOutcome::Approved(request) => {
            println!("Paired {} — they can chat now.", request.id);
            Ok(())
        }
        ApproveOutcome::NotFound => anyhow::bail!(
            "no approvable pairing with code {code} — unknown or expired (see `shion pair list`)"
        ),
        ApproveOutcome::Locked { retry_after_secs } => anyhow::bail!(
            "too many failed attempts — approve is locked for {} more minutes",
            (retry_after_secs + 59) / 60
        ),
    }
}

/// Remove a pairing (`{platform}:{sender_id}`, as printed by `pair list`).
pub async fn revoke(db_url: &str, id: &str) -> anyhow::Result<()> {
    let revoked = match GatewayClient::try_connect().await {
        Some(gw) => gw.pair_revoke(id).await?,
        None => {
            let db = Db::connect(db_url).await?;
            PairingRepository::revoke(&db, id).await?
        }
    };
    if revoked {
        println!("Revoked {id}.");
    } else {
        println!("No pairing {id} (see `shion pair list`).");
    }
    Ok(())
}
