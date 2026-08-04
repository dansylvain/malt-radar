//! Malt-Radar TUI (Phase 4): reads the aggregator's `GET /fiches` over
//! Tailscale and renders it. No local state beyond what's fetched — the
//! client machine never touches the DB or the filesystem on the VPS/Pi5.
//!
//! Deliberately thin for now: `Market Metrics` and `Top Skills` are
//! computed client-side from the raw fiche list (`malt_radar_core::compute_metrics`)
//! rather than read from a weekly aggregation, because Phase 3 (the service
//! that would produce that aggregation) doesn't exist yet. The digest panel
//! is a placeholder for the same reason.
//!
//! Layering: `tasks` talks to the network, `app` holds state, `events`
//! turns keys into state changes, `ui` renders. Only this file knows about
//! the terminal.

mod app;
mod browser;
mod events;
mod tasks;
mod ui;

use std::io;
use std::time::Instant;

use crossterm::event::{self, Event, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use malt_radar_client::{AggregatorClient, AggregatorConfig};
use ratatui::backend::CrosstermBackend;
use ratatui::{Frame, Terminal};
use tokio::sync::mpsc;

use crate::app::App;
use crate::events::{handle_key, Commands, Flow};
use crate::tasks::{alias_write_loop, fetch_loop, AliasCommand, FetchOutcome, POLL_INTERVAL};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    // Config is validated *before* the terminal is put into raw mode, so a
    // missing env var surfaces as a plain readable error rather than as a
    // message painted into an alternate screen that's about to be torn down.
    let config = AggregatorConfig::from_env().map_err(|err| {
        anyhow::anyhow!("{err}\n\nRenseigne AGGREGATOR_URL et AGGREGATOR_API_TOKEN dans tui/.env (voir tui/.env.example).")
    })?;
    let client = AggregatorClient::new(config);

    let (refresh_tx, refresh_rx) = mpsc::unbounded_channel::<()>();
    let (outcome_tx, mut outcome_rx) = mpsc::unbounded_channel::<FetchOutcome>();
    let (alias_cmd_tx, alias_cmd_rx) = mpsc::unbounded_channel::<AliasCommand>();
    let fetcher = tokio::spawn(fetch_loop(client.clone(), refresh_rx, outcome_tx.clone()));
    let alias_writer = tokio::spawn(alias_write_loop(client, alias_cmd_rx, outcome_tx));

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new();
    let commands = Commands {
        refresh_tx: &refresh_tx,
        alias_cmd_tx: &alias_cmd_tx,
    };
    let run_result = run(&mut terminal, &mut app, &commands, &mut outcome_rx).await;

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    drop(refresh_tx);
    drop(alias_cmd_tx);
    let _ = fetcher.await;
    let _ = alias_writer.await;

    run_result
}

async fn run(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    commands: &Commands<'_>,
    outcome_rx: &mut mpsc::UnboundedReceiver<FetchOutcome>,
) -> anyhow::Result<()> {
    loop {
        terminal.draw(|frame: &mut Frame| ui::draw(frame, app))?;

        if event::poll(POLL_INTERVAL)? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press && handle_key(app, key, commands) == Flow::Quit {
                    return Ok(());
                }
            }
        }

        while let Ok(outcome) = outcome_rx.try_recv() {
            app.apply(outcome);
            app.last_refreshed = Some(Instant::now());
        }
    }
}
