//! Background tasks and the messages they exchange with the UI thread.
//!
//! Two long-lived tokio tasks, both talking to the UI over unbounded mpsc
//! channels: `fetch_loop` (periodic + on-demand reads) and
//! `alias_write_loop` (serialised writes). Neither ever blocks the render
//! loop, and neither knows anything about `App`.

use std::time::Duration;

use malt_radar_client::AggregatorClient;
use malt_radar_core::{Fiche, SkillAlias};
use tokio::sync::mpsc;

pub const REFRESH_INTERVAL: Duration = Duration::from_secs(60);
/// Raised from 100 to cover the trend's 14-day two-window comparison
/// (`tjm_trend`): if the aggregator ever returns exactly this many fiches,
/// the older window may be truncated and the delta would be silently wrong
/// but plausible-looking — `App` checks for that (see `recompute`) and
/// withholds the delta rather than trust a possibly-cut sample.
pub const FICHES_LIMIT: u32 = 500;
pub const POLL_INTERVAL: Duration = Duration::from_millis(150);

pub enum FetchOutcome {
    Ok(Vec<Fiche>),
    Err(String),
    AliasesOk(Vec<SkillAlias>),
    AliasWriteErr(String),
}

/// Sent to `alias_write_loop`. Both variants use the *cascade* pattern:
/// a merge/rename or a "defuse" (undo) touches every alias row pointing
/// at the same canonical name, not just one — see `App::submit_alias_edit`
/// and `App::defuse_selected_skill`.
pub enum AliasCommand {
    Upsert { alias: String, canonical: String },
    Delete { alias: String },
}

pub async fn fetch_loop(
    client: AggregatorClient,
    mut refresh_rx: mpsc::UnboundedReceiver<()>,
    outcome_tx: mpsc::UnboundedSender<FetchOutcome>,
) {
    let mut ticker = tokio::time::interval(REFRESH_INTERVAL);
    loop {
        tokio::select! {
            _ = ticker.tick() => {}
            received = refresh_rx.recv() => {
                if received.is_none() {
                    break;
                }
            }
        }

        let outcome = match client.list_fiches(FICHES_LIMIT).await {
            Ok(fiches) => FetchOutcome::Ok(fiches),
            Err(err) => FetchOutcome::Err(err.to_string()),
        };
        if outcome_tx.send(outcome).is_err() {
            break;
        }

        // Fetched independently and best-effort, deliberately not combined
        // with the fiches fetch above (e.g. via try_join!): the aggregator
        // might not have redeployed the /skill-aliases endpoint yet even
        // after this TUI binary is updated (deploys across the three
        // machines are manual), and a combined fetch would otherwise turn
        // a missing endpoint into a fully blank dashboard.
        if let Ok(aliases) = client.list_skill_aliases().await {
            if outcome_tx.send(FetchOutcome::AliasesOk(aliases)).is_err() {
                break;
            }
        }
    }
}

pub async fn alias_write_loop(
    client: AggregatorClient,
    mut cmd_rx: mpsc::UnboundedReceiver<AliasCommand>,
    outcome_tx: mpsc::UnboundedSender<FetchOutcome>,
) {
    while let Some(cmd) = cmd_rx.recv().await {
        let result = match cmd {
            AliasCommand::Upsert { alias, canonical } => {
                client.upsert_skill_alias(&alias, &canonical).await.map(|_| ())
            }
            AliasCommand::Delete { alias } => client.delete_skill_alias(&alias).await.map(|_| ()),
        };
        if let Err(err) = result {
            if outcome_tx.send(FetchOutcome::AliasWriteErr(err.to_string())).is_err() {
                break;
            }
        }
    }
}
