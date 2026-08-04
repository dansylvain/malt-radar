//! Malt-Radar TUI (Phase 4): reads the aggregator's `GET /fiches` over
//! Tailscale and renders it. No local state beyond what's fetched — the
//! client machine never touches the DB or the filesystem on the VPS/Pi5.
//!
//! Deliberately thin for now: `Market Metrics` and `Skills Frequency` are
//! computed client-side from the raw fiche list (`malt_radar_core::compute_metrics`)
//! rather than read from a weekly aggregation, because Phase 3 (the service
//! that would produce that aggregation) doesn't exist yet. `Digest` is a
//! placeholder for the same reason.

use std::io;
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use malt_radar_client::{AggregatorClient, AggregatorConfig};
use malt_radar_core::{compute_metrics, Fiche, MarketMetrics};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};
use ratatui::{Frame, Terminal};
use tokio::sync::mpsc;

const REFRESH_INTERVAL: Duration = Duration::from_secs(60);
const TOP_SKILLS: usize = 8;
const FICHES_LIMIT: u32 = 100;
const POLL_INTERVAL: Duration = Duration::from_millis(150);

enum FetchOutcome {
    Ok(Vec<Fiche>),
    Err(String),
}

struct App {
    fiches: Vec<Fiche>,
    metrics: MarketMetrics,
    status_line: String,
    loading: bool,
}

impl App {
    fn new() -> Self {
        Self {
            fiches: Vec::new(),
            metrics: MarketMetrics::default(),
            status_line: "chargement des fiches...".to_string(),
            loading: true,
        }
    }

    fn apply(&mut self, outcome: FetchOutcome) {
        self.loading = false;
        match outcome {
            FetchOutcome::Ok(fiches) => {
                self.metrics = compute_metrics(&fiches, TOP_SKILLS);
                self.fiches = fiches;
                self.status_line = format!("{} fiches chargées", self.fiches.len());
            }
            FetchOutcome::Err(message) => {
                self.status_line = format!("erreur: {message}");
            }
        }
    }
}

async fn fetch_loop(
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
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    let config = AggregatorConfig::from_env().map_err(|err| {
        anyhow::anyhow!("{err}\n\nRenseigne AGGREGATOR_URL et AGGREGATOR_API_TOKEN dans tui/.env (voir tui/.env.example).")
    })?;
    let client = AggregatorClient::new(config);

    let (refresh_tx, refresh_rx) = mpsc::unbounded_channel::<()>();
    let (outcome_tx, mut outcome_rx) = mpsc::unbounded_channel::<FetchOutcome>();
    let fetcher = tokio::spawn(fetch_loop(client, refresh_rx, outcome_tx));

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new();
    let run_result = run(&mut terminal, &mut app, &refresh_tx, &mut outcome_rx).await;

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    drop(refresh_tx);
    let _ = fetcher.await;

    run_result
}

async fn run(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    refresh_tx: &mpsc::UnboundedSender<()>,
    outcome_rx: &mut mpsc::UnboundedReceiver<FetchOutcome>,
) -> anyhow::Result<()> {
    let mut last_refreshed: Option<Instant> = None;

    loop {
        terminal.draw(|frame| draw(frame, app, last_refreshed))?;

        if event::poll(POLL_INTERVAL)? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
                        KeyCode::Char('r') => {
                            app.loading = true;
                            app.status_line = "refresh...".to_string();
                            let _ = refresh_tx.send(());
                        }
                        _ => {}
                    }
                }
            }
        }

        while let Ok(outcome) = outcome_rx.try_recv() {
            app.apply(outcome);
            last_refreshed = Some(Instant::now());
        }
    }
}

fn draw(frame: &mut Frame, app: &App, last_refreshed: Option<Instant>) {
    let root = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(9), Constraint::Min(5), Constraint::Length(1)])
        .split(frame.area());

    let top = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(34),
            Constraint::Percentage(33),
            Constraint::Percentage(33),
        ])
        .split(root[0]);

    draw_market_metrics(frame, top[0], &app.metrics);
    draw_skills_frequency(frame, top[1], &app.metrics);
    draw_digest_placeholder(frame, top[2]);
    draw_live_feed(frame, root[1], &app.fiches);
    draw_footer(frame, root[2], app, last_refreshed);
}

fn draw_market_metrics(frame: &mut Frame, area: Rect, metrics: &MarketMetrics) {
    let avg_tjm = metrics
        .avg_tjm
        .map(|v| format!("{v:.0} EUR/j"))
        .unwrap_or_else(|| "n/a".to_string());
    let lines = vec![
        Line::from(format!("Fiches : {}", metrics.fiche_count)),
        Line::from(format!("TJM moyen : {avg_tjm}")),
    ];
    let paragraph = Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title("Market Metrics"));
    frame.render_widget(paragraph, area);
}

fn draw_skills_frequency(frame: &mut Frame, area: Rect, metrics: &MarketMetrics) {
    let items: Vec<ListItem> = metrics
        .top_skills
        .iter()
        .map(|(skill, count)| ListItem::new(format!("{skill} — {count}")))
        .collect();
    let list = List::new(items).block(Block::default().borders(Borders::ALL).title("Skills Frequency"));
    frame.render_widget(list, area);
}

fn draw_digest_placeholder(frame: &mut Frame, area: Rect) {
    let paragraph = Paragraph::new("Digest hebdomadaire : Phase 3 pas encore implémentée.")
        .block(Block::default().borders(Borders::ALL).title("Digest"));
    frame.render_widget(paragraph, area);
}

fn draw_live_feed(frame: &mut Frame, area: Rect, fiches: &[Fiche]) {
    let items: Vec<ListItem> = fiches
        .iter()
        .map(|fiche| {
            let title = fiche.title.as_deref().unwrap_or("(sans titre)");
            let company = fiche.company.as_deref().unwrap_or("?");
            let tjm = fiche
                .tjm_mid()
                .map(|v| format!("{v:.0} EUR/j"))
                .unwrap_or_else(|| "TJM n/a".to_string());
            let stack = if fiche.stack.is_empty() {
                String::new()
            } else {
                format!(" [{}]", fiche.stack.join(", "))
            };
            ListItem::new(format!("{title} — {company} — {tjm}{stack}"))
        })
        .collect();
    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!("Live Feed ({})", fiches.len())),
    );
    frame.render_widget(list, area);
}

fn draw_footer(frame: &mut Frame, area: Rect, app: &App, last_refreshed: Option<Instant>) {
    let age = last_refreshed
        .map(|t| format!("{}s", t.elapsed().as_secs()))
        .unwrap_or_else(|| "jamais".to_string());
    let loading_marker = if app.loading { " (chargement...)" } else { "" };
    let line = Line::from(vec![
        Span::styled("[q]", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" quitter  "),
        Span::styled("[r]", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" refresh  "),
        Span::styled(&app.status_line, Style::default().fg(Color::Yellow)),
        Span::raw(format!("  (dernier refresh il y a {age}){loading_marker}")),
    ]);
    frame.render_widget(Paragraph::new(line), area);
}
