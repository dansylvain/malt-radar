//! Malt-Radar TUI (Phase 4): reads the aggregator's `GET /fiches` over
//! Tailscale and renders it. No local state beyond what's fetched — the
//! client machine never touches the DB or the filesystem on the VPS/Pi5.
//!
//! Deliberately thin for now: `Market Metrics` and `Top Skills` are
//! computed client-side from the raw fiche list (`malt_radar_core::compute_metrics`)
//! rather than read from a weekly aggregation, because Phase 3 (the service
//! that would produce that aggregation) doesn't exist yet. The digest panel
//! is a placeholder for the same reason.

use std::io;
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use malt_radar_client::{AggregatorClient, AggregatorConfig};
use malt_radar_core::{compute_metrics, truncate_label, Fiche, MarketMetrics};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};
use ratatui::{Frame, Terminal};
use tokio::sync::mpsc;

const REFRESH_INTERVAL: Duration = Duration::from_secs(60);
const TOP_SKILLS: usize = 5;
const SKILL_BAR_WIDTH: usize = 24;
const SKILL_NAME_WIDTH: usize = 16;
const ACCENT_COLOR: Color = Color::Cyan;
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
        .constraints([
            Constraint::Length(9),
            Constraint::Fill(2),
            Constraint::Fill(1),
            Constraint::Length(1),
        ])
        .split(frame.area());

    let top = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(root[0]);

    draw_market_metrics(frame, top[0], &app.metrics);
    draw_top_skills(frame, top[1], &app.metrics);
    draw_live_feed(frame, root[1], &app.fiches);
    draw_digest(frame, root[2]);
    draw_footer(frame, root[3], app, last_refreshed);
}

fn draw_market_metrics(frame: &mut Frame, area: Rect, metrics: &MarketMetrics) {
    let avg_tjm = metrics
        .avg_tjm
        .map(|v| format!("{v:.0} EUR/j"))
        .unwrap_or_else(|| "n/a".to_string());
    let top_skill = metrics
        .top_skills
        .first()
        .map(|s| format!("{} ({}%)", s.name, s.percent_rounded))
        .unwrap_or_else(|| "n/a".to_string());
    let lines = vec![
        Line::from(format!("Fiches : {}", metrics.fiche_count)),
        Line::from(format!("TJM moyen : {avg_tjm}")),
        Line::from(format!("Top skill demandé : {top_skill}")),
    ];
    let paragraph = Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title("Market Metrics"));
    frame.render_widget(paragraph, area);
}

fn draw_top_skills(frame: &mut Frame, area: Rect, metrics: &MarketMetrics) {
    let lines: Vec<Line> = metrics
        .top_skills
        .iter()
        .map(|skill| {
            // Fraction absolue de 100%, pas relative au plus grand count
            // affiché — sinon "Autres" (souvent le plus gros poste) remplit
            // toujours sa barre à 100% quel que soit son vrai pourcentage.
            let filled = ((skill.percent / 100.0) * SKILL_BAR_WIDTH as f64).round() as usize;
            let bar = format!("{}{}", "█".repeat(filled), " ".repeat(SKILL_BAR_WIDTH - filled));
            let name = truncate_label(&skill.name, SKILL_NAME_WIDTH);
            Line::from(vec![
                Span::raw(format!("{name:<SKILL_NAME_WIDTH$}")),
                Span::raw("["),
                Span::styled(bar, Style::default().fg(ACCENT_COLOR)),
                Span::raw(format!("] {}%", skill.percent_rounded)),
            ])
        })
        .collect();
    let paragraph = Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title("Top Skills"));
    frame.render_widget(paragraph, area);
}

fn draw_digest(frame: &mut Frame, area: Rect) {
    let paragraph = Paragraph::new("Digest hebdomadaire : Phase 3 pas encore implémentée.")
        .block(Block::default().borders(Borders::ALL).title("DAILY AI STRATEGY DIGEST"));
    frame.render_widget(paragraph, area);
}

fn draw_live_feed(frame: &mut Frame, area: Rect, fiches: &[Fiche]) {
    let items: Vec<ListItem> = fiches
        .iter()
        .enumerate()
        .map(|(i, fiche)| {
            let title = fiche.title.as_deref().unwrap_or("(sans titre)");
            let company = fiche.company.as_deref().unwrap_or("?");
            let tjm = fiche
                .tjm_mid()
                .map(|v| format!("{v:.0} EUR/j"))
                .unwrap_or_else(|| "TJM n/a".to_string());
            let stack = if fiche.stack.is_empty() {
                "(stack non extraite)".to_string()
            } else {
                fiche.stack.join(", ")
            };

            // Sur deux lignes : la ligne stack débordait souvent la largeur
            // du panneau (List ne wrap pas comme Paragraph), d'où la coupure
            // sans retour à la ligne observée.
            let text = Text::from(vec![
                Line::from(format!("{title} — {company} — {tjm}")),
                Line::from(format!("  {stack}")),
            ]);

            // Une ligne sur deux en couleur d'accent (même teinte que les
            // barres de Top Skills) pour distinguer les annonces d'un coup
            // d'œil, sans toucher au fond.
            let style = if i % 2 == 1 {
                Style::default().fg(ACCENT_COLOR)
            } else {
                Style::default()
            };

            ListItem::new(text).style(style)
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
