mod common;

use common::{fixture_servers, harness};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use omp_tui::app::App;
use omp_tui::app::tasks::AppEvent;
use omptui_core::launch::HelperEvent;
use omptui_core::query::packet::InfoPacket;
use omptui_core::query::{BasicResult, FullResult};
use omptui_core::store::{Lists, Settings};
use omptui_core::{Player, ServerAddr};
use ratatui::Terminal;
use ratatui::backend::TestBackend;

fn render(app: &mut App, w: u16, h: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
    terminal.draw(|f| omp_tui::ui::draw(f, app)).unwrap();
    terminal.backend().to_string()
}

fn key(app: &mut App, code: KeyCode) {
    app.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
}

fn type_str(app: &mut App, s: &str) {
    for c in s.chars() {
        key(app, KeyCode::Char(c));
    }
}

fn settings() -> Settings {
    Settings { query_lists: false, ..Default::default() }
}

async fn loaded_app() -> common::Harness {
    let mut h = harness(settings(), Lists::default()).await;
    h.app.handle_event(AppEvent::ApiLoaded(Ok(fixture_servers())));
    let addr: ServerAddr = "127.0.0.1:7001".parse().unwrap();
    h.app.handle_event(AppEvent::Full {
        addr,
        result: FullResult {
            basic: BasicResult {
                info: Some(InfoPacket {
                    password: false,
                    players: 12,
                    max_players: 100,
                    hostname: "Alpha Freeroam".into(),
                    gamemode: "Freeroam v2".into(),
                    language: "English".into(),
                }),
                ping: Some(34),
            },
            players: Some(vec![
                Player { name: "johnny".into(), score: 123 },
                Player { name: "bigsmoke".into(), score: 69 },
                Player { name: "ryder".into(), score: 420 },
            ]),
            rules: Some(vec![
                ("mapname".into(), "San Andreas".into()),
                ("weather".into(), "10".into()),
                ("weburl".into(), "www.example.com".into()),
                ("version".into(), "omp 1.5.8.3079".into()),
            ]),
            extra: Some(omptui_core::ExtraInfo { discord: "https://discord.gg/example".into(), ..Default::default() }),
        },
    });
    for ms in [30, 35, 40, 33, 31, 60, 45] {
        h.app.handle_event(AppEvent::Ping { addr, ms });
    }
    h.app.handle_event(AppEvent::Basic {
        addr: "127.0.0.1:7002".parse().unwrap(),
        result: BasicResult { info: None, ping: Some(omptui_core::UNREACHABLE_PING) },
    });
    h.app.handle_event(AppEvent::Basic {
        addr: "127.0.0.1:7003".parse().unwrap(),
        result: BasicResult { info: None, ping: Some(120) },
    });
    h
}

#[tokio::test]
async fn main_view() {
    let mut h = loaded_app().await;
    insta::assert_snapshot!(render(&mut h.app, 120, 32));
}

#[tokio::test]
async fn main_view_narrow() {
    let mut h = loaded_app().await;
    insta::assert_snapshot!(render(&mut h.app, 80, 24));
}

#[tokio::test]
async fn empty_favorites_and_recent() {
    let mut h = loaded_app().await;
    key(&mut h.app, KeyCode::Char('1'));
    insta::assert_snapshot!("favorites_empty", render(&mut h.app, 100, 20));
    key(&mut h.app, KeyCode::Char('4'));
    insta::assert_snapshot!("recent_empty", render(&mut h.app, 100, 20));
}

#[tokio::test]
async fn search_and_filters() {
    let mut h = loaded_app().await;
    key(&mut h.app, KeyCode::Char('/'));
    type_str(&mut h.app, "bravo");
    assert_eq!(h.app.view.len(), 1);
    insta::assert_snapshot!("search_bravo", render(&mut h.app, 100, 20));
    key(&mut h.app, KeyCode::Esc);
    assert_eq!(h.app.view.len(), 3);
    key(&mut h.app, KeyCode::Char('o'));
    assert_eq!(h.app.view.len(), 2);
    key(&mut h.app, KeyCode::Char('f'));
    insta::assert_snapshot!("filters_popup", render(&mut h.app, 100, 26));
    key(&mut h.app, KeyCode::Esc);
    key(&mut h.app, KeyCode::Char('s'));
    key(&mut h.app, KeyCode::Char('s'));
    let names: Vec<String> = h.app.view.iter().map(|&i| h.app.list()[i].info.hostname.clone()).collect();
    assert_eq!(names, vec!["Alpha Freeroam", "Charlie Deathmatch"]);
}

#[tokio::test]
async fn favorite_toggle_persists() {
    let mut h = loaded_app().await;
    key(&mut h.app, KeyCode::Char('F'));
    assert_eq!(h.app.lists.favorites.len(), 1);
    key(&mut h.app, KeyCode::Char('1'));
    insta::assert_snapshot!("favorites_one", render(&mut h.app, 100, 20));
    let lists = Lists::load(&h.app.paths).unwrap();
    assert_eq!(lists.favorites[0].info.hostname, "Alpha Freeroam");
    key(&mut h.app, KeyCode::Char('d'));
    insta::assert_snapshot!("confirm_remove", render(&mut h.app, 100, 20));
    key(&mut h.app, KeyCode::Char('y'));
    assert!(h.app.lists.favorites.is_empty());
}

#[tokio::test]
async fn join_popup_and_validation() {
    let mut h = loaded_app().await;
    key(&mut h.app, KeyCode::Enter);
    type_str(&mut h.app, "CJ");
    key(&mut h.app, KeyCode::Enter);
    insta::assert_snapshot!("join_invalid_nick", render(&mut h.app, 100, 24));
    type_str(&mut h.app, "_Johnson");
    key(&mut h.app, KeyCode::Tab);
    type_str(&mut h.app, "secret");
    insta::assert_snapshot!("join_filled", render(&mut h.app, 100, 24));
    key(&mut h.app, KeyCode::Enter);
    insta::assert_snapshot!("join_no_game_dir", render(&mut h.app, 100, 24));
}

#[tokio::test]
async fn launch_popup_shows_helper_events() {
    let mut h = loaded_app().await;
    h.app.settings.game_dir = Some(h.root.path().to_path_buf());
    key(&mut h.app, KeyCode::Enter);
    type_str(&mut h.app, "Carl_Johnson");
    key(&mut h.app, KeyCode::Enter);
    assert!(matches!(h.app.popup, Some(omp_tui::app::popup::Popup::Launch(_))));
    h.app.handle_event(AppEvent::Launch(HelperEvent::Spawned { pid: 1234 }));
    h.app.handle_event(AppEvent::Launch(HelperEvent::Injected { dll: "C:\\x\\samp.dll".into(), attempt: 1 }));
    h.app.handle_event(AppEvent::Launch(HelperEvent::Retry {
        dll: "C:\\x\\omp-client.dll".into(),
        attempt: 1,
        code: 5,
        message: "Access denied".into(),
    }));
    h.app.handle_event(AppEvent::Launch(HelperEvent::Injected { dll: "C:\\x\\omp-client.dll".into(), attempt: 2 }));
    h.app.handle_event(AppEvent::Launch(HelperEvent::Resumed));
    insta::assert_snapshot!("launch_running", render(&mut h.app, 110, 26));
    assert_eq!(h.app.lists.recent.len(), 1);
    assert_eq!(h.app.settings.recent_nicknames, vec!["Carl_Johnson"]);
    h.app.handle_event(AppEvent::Launch(HelperEvent::Exit { code: 0 }));
    h.app.handle_event(AppEvent::LaunchFinished(Ok(Some(0))));
    insta::assert_snapshot!("launch_finished", render(&mut h.app, 110, 26));
    key(&mut h.app, KeyCode::Esc);
    key(&mut h.app, KeyCode::Char('4'));
    insta::assert_snapshot!("recent_after_launch", render(&mut h.app, 110, 20));
}

#[tokio::test]
async fn settings_help_and_server_settings() {
    let mut h = loaded_app().await;
    key(&mut h.app, KeyCode::Char('?'));
    insta::assert_snapshot!("help", render(&mut h.app, 100, 30));
    key(&mut h.app, KeyCode::Esc);
    key(&mut h.app, KeyCode::Char(','));
    h.app.settings.wine_binary = Some("/opt/wine/bin/wine".into());
    h.app.settings.wine_prefix = Some("/home/user/.local/share/omp-tui/prefix".into());
    h.app.settings.terminal = Some("foot".into());
    insta::assert_snapshot!("settings", render(&mut h.app, 110, 32));
    key(&mut h.app, KeyCode::Down);
    key(&mut h.app, KeyCode::Down);
    key(&mut h.app, KeyCode::Down);
    key(&mut h.app, KeyCode::Right);
    assert_eq!(h.app.settings.samp_version, omptui_core::resources::SampVersion::DL);
    key(&mut h.app, KeyCode::Esc);
    key(&mut h.app, KeyCode::Char('p'));
    type_str(&mut h.app, "Alpha_Nick");
    key(&mut h.app, KeyCode::Tab);
    key(&mut h.app, KeyCode::Tab);
    key(&mut h.app, KeyCode::Right);
    insta::assert_snapshot!("server_settings", render(&mut h.app, 100, 20));
    key(&mut h.app, KeyCode::Enter);
    let per = h.app.lists.settings_for("127.0.0.1:7001".parse().unwrap());
    assert_eq!(per.nickname.as_deref(), Some("Alpha_Nick"));
    assert_eq!(per.samp_version, Some(omptui_core::resources::SampVersion::R1));
    let on_disk = Lists::load(&h.app.paths).unwrap();
    assert_eq!(on_disk.settings_for("127.0.0.1:7001".parse().unwrap()), per);
    insta::assert_snapshot!("details_with_overrides", render(&mut h.app, 110, 30));
}

#[tokio::test]
async fn settings_long_values_stay_readable() {
    let mut h = loaded_app().await;
    h.app.settings.wine_binary = Some("/usr/share/steam/compatibilitytools.d/proton-cachyos-slr/files/bin/wine".into());
    h.app.settings.wine_prefix = Some("/home/user/.local/share/omp-tui/prefix".into());
    h.app.settings.terminal = Some("foot".into());
    key(&mut h.app, KeyCode::Char(','));
    let shown = render(&mut h.app, 100, 32);
    assert!(shown.contains("…compatibilitytools.d/proton-cachyos-slr/files/bin/wine│"), "{shown}");
    for _ in 0..5 {
        key(&mut h.app, KeyCode::Down);
    }
    key(&mut h.app, KeyCode::Enter);
    insta::assert_snapshot!("settings_long_edit", render(&mut h.app, 100, 32));
    key(&mut h.app, KeyCode::Home);
    let shown = render(&mut h.app, 100, 32);
    assert!(shown.contains("▏/usr/share/steam/compatibilitytools.d/proton-cachyos-"), "{shown}");
}

#[tokio::test]
async fn api_error_and_message() {
    let mut h = harness(settings(), Lists::default()).await;
    h.app.handle_event(AppEvent::ApiLoaded(Err("connection refused".into())));
    insta::assert_snapshot!("api_error", render(&mut h.app, 100, 20));
    h.app.handle_event(AppEvent::TaskDone {
        title: "Import client files".into(),
        result: Err("nope is not a directory".into()),
    });
    insta::assert_snapshot!("message_error", render(&mut h.app, 100, 20));
}

#[tokio::test]
async fn add_server_prompt() {
    let mut h = loaded_app().await;
    key(&mut h.app, KeyCode::Char('a'));
    type_str(&mut h.app, "play.example.org:7777");
    insta::assert_snapshot!("add_server", render(&mut h.app, 100, 20));
    key(&mut h.app, KeyCode::Esc);
    h.app.handle_event(AppEvent::Resolved {
        result: Ok(("127.0.0.1:7010".parse().unwrap(), "play.example.org:7777".into())),
        join: false,
    });
    assert_eq!(h.app.tab, omptui_core::ListKind::Favorites);
    assert_eq!(h.app.lists.favorites[0].host_label.as_deref(), Some("play.example.org:7777"));
    insta::assert_snapshot!("added_third_party", render(&mut h.app, 100, 20));
}
