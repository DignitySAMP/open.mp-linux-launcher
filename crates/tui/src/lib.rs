pub mod app;
pub mod cli;
pub mod clipboard;
pub mod desktop;
pub mod input;
pub mod setup;
pub mod theme;
pub mod ui;

use app::App;
use app::tasks::{AppEvent, Services};
use crossterm::event::{Event, EventStream, KeyEventKind};
use futures::StreamExt;
use omptui_core::api::{ApiClient, DEFAULT_BASE_URL};
use omptui_core::query::{Querier, QueryConfig};
use omptui_core::store::{Paths, Settings};
use std::time::Duration;
use tokio::sync::mpsc;

// built by build.rs
pub const HELPER_EXE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/omp-injector.exe"));

pub fn init_logging(paths: &Paths) {
    use tracing_subscriber::EnvFilter;
    let _ = std::fs::create_dir_all(&paths.state_dir);
    let Ok(file) = std::fs::OpenOptions::new().create(true).append(true).open(paths.log_file()) else { return };
    let filter = EnvFilter::try_from_env("OMPTUI_LOG").unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt().with_env_filter(filter).with_writer(file).with_ansi(false).try_init();
}

pub fn api_url(settings: &Settings, cli: &cli::Cli) -> String {
    cli.api_url
        .clone()
        .or_else(|| std::env::var("OMPTUI_API_URL").ok())
        .or_else(|| settings.api_url.clone())
        .unwrap_or_else(|| DEFAULT_BASE_URL.to_string())
}

pub fn assets_url() -> String {
    std::env::var("OMPTUI_ASSETS_URL").unwrap_or_else(|_| omptui_core::download::DEFAULT_ASSETS_URL.to_string())
}

pub async fn build_services(api_base: &str, tx: mpsc::UnboundedSender<AppEvent>) -> std::io::Result<Services> {
    let mut cfg = QueryConfig::default();
    if let Some(ms) = std::env::var("OMPTUI_QUERY_TIMEOUT_MS").ok().and_then(|v| v.parse().ok()) {
        cfg.timeout = Duration::from_millis(ms);
    }
    let querier = Querier::bind(cfg).await?;
    Ok(Services { api: ApiClient::new(api_base), querier, tx, helper: HELPER_EXE })
}

pub async fn run_tui(mut app: App, rx: mpsc::UnboundedReceiver<AppEvent>) -> std::io::Result<()> {
    let mut terminal = ratatui::try_init()?;
    let result = event_loop(&mut terminal, &mut app, rx).await;
    ratatui::restore();
    app.save_all();
    result
}

async fn event_loop(
    terminal: &mut ratatui::DefaultTerminal,
    app: &mut App,
    mut rx: mpsc::UnboundedReceiver<AppEvent>,
) -> std::io::Result<()> {
    let mut keys = EventStream::new();
    let mut ticker = tokio::time::interval(Duration::from_secs(1));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    app.start();
    loop {
        terminal.draw(|f| ui::draw(f, app))?;
        tokio::select! {
            ev = keys.next() => match ev {
                Some(Ok(Event::Key(k))) if k.kind != KeyEventKind::Release => app.handle_key(k),
                Some(Ok(_)) => {}
                Some(Err(e)) => return Err(e),
                None => return Ok(()),
            },
            ev = rx.recv() => match ev {
                Some(ev) => {
                    app.handle_event(ev);
                    while let Ok(more) = rx.try_recv() {
                        app.handle_event(more);
                    }
                }
                None => return Ok(()),
            },
            _ = ticker.tick() => app.handle_event(AppEvent::Tick),
        }
        if app.should_quit {
            return Ok(());
        }
    }
}

pub async fn install_d3dx9(env: &omptui_core::wine::WineEnv) -> Result<(), String> {
    let mut cmd = tokio::process::Command::new("winetricks");
    cmd.args(["-q", "d3dx9"]).env("WINEPREFIX", &env.prefix).env("WINE", &env.wine).env("WINEDEBUG", "-all");
    for (k, v) in &env.extra_env {
        cmd.env(k, v);
    }
    println!(
        "Installing d3dx9 into the Wine prefix with winetricks (one time, this downloads the DirectX redistributable)…"
    );
    let out =
        cmd.output().await.map_err(|e| format!("could not run winetricks: {e} (install the winetricks package)"))?;
    let has =
        omptui_core::wine::Prefix::new(&env.prefix).system_dirs().iter().any(|d| d.join("d3dx9_25.dll").is_file());
    if out.status.success() && has {
        Ok(())
    } else {
        let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        let tail: Vec<&str> = text.lines().rev().take(10).collect::<Vec<_>>().into_iter().rev().collect();
        Err(format!("winetricks exited with {}\n{}", out.status, tail.join("\n")))
    }
}
