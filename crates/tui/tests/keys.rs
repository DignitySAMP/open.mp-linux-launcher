mod common;

use common::{fixture_servers, harness, server};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use omp_tui::app::App;
use omp_tui::app::popup::Popup;
use omp_tui::app::tasks::AppEvent;
use omptui_core::ListKind;
use omptui_core::filter::{SortDir, SortKey};
use omptui_core::launch::HelperEvent;
use omptui_core::query::BasicResult;
use omptui_core::resources::{ClientFiles, SHARED_FILES, SampVersion};
use omptui_core::store::{Lists, Settings};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use std::fs;
use std::time::Duration;

fn key(app: &mut App, code: KeyCode) {
    app.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
}

fn ctrl(app: &mut App, c: char) {
    app.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL));
}

fn type_str(app: &mut App, s: &str) {
    for c in s.chars() {
        key(app, KeyCode::Char(c));
    }
}

fn render(app: &mut App, w: u16, h: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
    terminal.draw(|f| omp_tui::ui::draw(f, app)).unwrap();
    terminal.backend().to_string()
}

fn selected_name(app: &App) -> String {
    app.selected_server().map(|s| s.info.hostname.clone()).unwrap_or_default()
}

async fn next_event(h: &mut common::Harness) -> AppEvent {
    tokio::time::timeout(Duration::from_secs(10), h.rx.recv()).await.expect("event in time").expect("channel open")
}

async fn pump_until(h: &mut common::Harness, pred: impl Fn(&AppEvent) -> bool) {
    loop {
        let ev = next_event(h).await;
        let done = pred(&ev);
        h.app.handle_event(ev);
        if done {
            return;
        }
    }
}

async fn app_with_list() -> common::Harness {
    let mut h = harness(Settings { query_lists: false, ..Default::default() }, Lists::default()).await;
    let mut servers = fixture_servers();
    for n in 0..30 {
        servers.push(server(7100 + n, &format!("Filler {n:02}"), n, n % 2 == 0, false));
    }
    h.app.handle_event(AppEvent::ApiLoaded(Ok(servers)));
    h
}

#[tokio::test]
async fn tabs_and_navigation() {
    let mut h = app_with_list().await;
    assert_eq!(h.app.tab, ListKind::Internet);
    key(&mut h.app, KeyCode::Char('3'));
    assert_eq!(h.app.tab, ListKind::Partners);
    assert!(h.app.view.iter().all(|&i| h.app.list()[i].info.partner));
    key(&mut h.app, KeyCode::Tab);
    assert_eq!(h.app.tab, ListKind::Recent);
    key(&mut h.app, KeyCode::Tab);
    assert_eq!(h.app.tab, ListKind::Favorites);
    key(&mut h.app, KeyCode::BackTab);
    assert_eq!(h.app.tab, ListKind::Recent);
    key(&mut h.app, KeyCode::Char('2'));
    assert_eq!(h.app.tab, ListKind::Internet);
    assert_eq!(h.app.table.selected(), Some(0));

    key(&mut h.app, KeyCode::Char('j'));
    key(&mut h.app, KeyCode::Down);
    assert_eq!(h.app.table.selected(), Some(2));
    key(&mut h.app, KeyCode::Char('k'));
    key(&mut h.app, KeyCode::Up);
    key(&mut h.app, KeyCode::Up);
    assert_eq!(h.app.table.selected(), Some(0));
    key(&mut h.app, KeyCode::PageDown);
    assert_eq!(h.app.table.selected(), Some(15));
    ctrl(&mut h.app, 'd');
    assert_eq!(h.app.table.selected(), Some(30));
    key(&mut h.app, KeyCode::PageUp);
    assert_eq!(h.app.table.selected(), Some(15));
    ctrl(&mut h.app, 'u');
    assert_eq!(h.app.table.selected(), Some(0));
    key(&mut h.app, KeyCode::Char('G'));
    assert_eq!(h.app.table.selected(), Some(32));
    key(&mut h.app, KeyCode::Char('g'));
    assert_eq!(h.app.table.selected(), Some(0));
    key(&mut h.app, KeyCode::End);
    assert_eq!(h.app.table.selected(), Some(32));
    key(&mut h.app, KeyCode::Home);
    assert_eq!(h.app.table.selected(), Some(0));
    key(&mut h.app, KeyCode::Down);
    key(&mut h.app, KeyCode::Down);
    key(&mut h.app, KeyCode::Down);
    assert_eq!(h.app.table.selected(), Some(3));
    let name = selected_name(&h.app);
    key(&mut h.app, KeyCode::Down);
    assert_ne!(selected_name(&h.app), name);
    render(&mut h.app, 100, 30);
}

#[tokio::test]
async fn quick_filters_sorting_and_search_keys() {
    let mut h = app_with_list().await;
    let total = h.app.view.len();
    key(&mut h.app, KeyCode::Char('e'));
    assert!(h.app.filters.non_empty);
    assert!(h.app.view.len() < total);
    key(&mut h.app, KeyCode::Char('e'));
    assert_eq!(h.app.view.len(), total);
    key(&mut h.app, KeyCode::Char('n'));
    assert!(h.app.filters.unpassworded);
    assert!(h.app.view.iter().all(|&i| !h.app.list()[i].info.password));
    key(&mut h.app, KeyCode::Char('n'));
    key(&mut h.app, KeyCode::Char('o'));
    assert!(h.app.view.iter().all(|&i| h.app.list()[i].info.omp));
    key(&mut h.app, KeyCode::Char('o'));

    key(&mut h.app, KeyCode::Char('s'));
    assert_eq!(h.app.filters.sort, SortKey::Players);
    let players: Vec<u16> = h.app.view.iter().map(|&i| h.app.list()[i].info.players).collect();
    assert!(players.windows(2).all(|w| w[0] >= w[1]));
    key(&mut h.app, KeyCode::Char('S'));
    assert_eq!(h.app.filters.dir, SortDir::Asc);
    let players: Vec<u16> = h.app.view.iter().map(|&i| h.app.list()[i].info.players).collect();
    assert!(players.windows(2).all(|w| w[0] <= w[1]));
    for _ in 0..4 {
        key(&mut h.app, KeyCode::Char('s'));
    }
    assert_eq!(h.app.filters.sort, SortKey::None);

    key(&mut h.app, KeyCode::Char('/'));
    assert!(h.app.search_editing);
    type_str(&mut h.app, "filler 0");
    assert_eq!(h.app.view.len(), 10);
    key(&mut h.app, KeyCode::Backspace);
    key(&mut h.app, KeyCode::Backspace);
    assert_eq!(h.app.view.len(), 30);
    key(&mut h.app, KeyCode::Enter);
    assert!(!h.app.search_editing);
    assert_eq!(h.app.filters.query, "filler");
    key(&mut h.app, KeyCode::Char('/'));
    key(&mut h.app, KeyCode::Esc);
    assert_eq!(h.app.filters.query, "");
    assert_eq!(h.app.view.len(), total);

    key(&mut h.app, KeyCode::Char('q'));
    assert!(h.app.should_quit);
    h.app.should_quit = false;
    ctrl(&mut h.app, 'c');
    assert!(h.app.should_quit);
    h.app.save_all();
    let saved = Settings::load(&h.app.paths).unwrap();
    assert_eq!(saved.filters, h.app.filters);
}

#[tokio::test]
async fn filters_popup_keys() {
    let mut h = app_with_list().await;
    key(&mut h.app, KeyCode::Char('f'));
    key(&mut h.app, KeyCode::Char(' '));
    assert!(h.app.filters.omp_only);
    key(&mut h.app, KeyCode::Char('j'));
    key(&mut h.app, KeyCode::Enter);
    assert!(h.app.filters.non_empty);
    key(&mut h.app, KeyCode::Down);
    key(&mut h.app, KeyCode::Right);
    assert!(h.app.filters.unpassworded);
    key(&mut h.app, KeyCode::Down);
    key(&mut h.app, KeyCode::Right);
    assert_eq!(h.app.filters.sort, SortKey::Players);
    key(&mut h.app, KeyCode::Left);
    assert_eq!(h.app.filters.sort, SortKey::None);
    key(&mut h.app, KeyCode::Left);
    assert_eq!(h.app.filters.sort, SortKey::Gamemode);
    key(&mut h.app, KeyCode::Down);
    key(&mut h.app, KeyCode::Enter);
    assert_eq!(h.app.filters.dir, SortDir::Asc);
    key(&mut h.app, KeyCode::Down);
    key(&mut h.app, KeyCode::Char(' '));
    assert_eq!(h.app.filters.languages.len(), 1);
    assert!(h.app.view.iter().all(|&i| h.app.list()[i].info.language == "English"));
    key(&mut h.app, KeyCode::Char('k'));
    key(&mut h.app, KeyCode::Up);
    key(&mut h.app, KeyCode::Char('c'));
    assert_eq!(h.app.filters.active_count(), 0);
    assert_eq!(h.app.filters.sort, SortKey::None);
    render(&mut h.app, 100, 30);
    key(&mut h.app, KeyCode::Char('q'));
    assert!(h.app.popup.is_none());
    assert!(!h.app.should_quit);
}

#[tokio::test]
async fn favorites_keys_including_reorder_and_delete() {
    let mut h = app_with_list().await;
    key(&mut h.app, KeyCode::Char(' '));
    key(&mut h.app, KeyCode::Down);
    key(&mut h.app, KeyCode::Char('F'));
    key(&mut h.app, KeyCode::Down);
    key(&mut h.app, KeyCode::Char('d'));
    assert_eq!(h.app.lists.favorites.len(), 3);
    key(&mut h.app, KeyCode::Char('1'));
    let names = |app: &App| app.lists.favorites.iter().map(|s| s.info.hostname.clone()).collect::<Vec<_>>();
    assert_eq!(names(&h.app), vec!["Alpha Freeroam", "Bravo Roleplay", "Charlie Deathmatch"]);
    key(&mut h.app, KeyCode::Char('J'));
    assert_eq!(names(&h.app), vec!["Bravo Roleplay", "Alpha Freeroam", "Charlie Deathmatch"]);
    assert_eq!(selected_name(&h.app), "Alpha Freeroam");
    key(&mut h.app, KeyCode::Char('J'));
    key(&mut h.app, KeyCode::Char('J'));
    assert_eq!(names(&h.app), vec!["Bravo Roleplay", "Charlie Deathmatch", "Alpha Freeroam"]);
    key(&mut h.app, KeyCode::Char('K'));
    assert_eq!(names(&h.app), vec!["Bravo Roleplay", "Alpha Freeroam", "Charlie Deathmatch"]);
    assert_eq!(Lists::load(&h.app.paths).unwrap().favorites.len(), 3);

    key(&mut h.app, KeyCode::Delete);
    assert!(matches!(h.app.popup, Some(Popup::Confirm { .. })));
    key(&mut h.app, KeyCode::Char('n'));
    assert!(h.app.popup.is_none());
    assert_eq!(h.app.lists.favorites.len(), 3);
    key(&mut h.app, KeyCode::Char('d'));
    key(&mut h.app, KeyCode::Esc);
    assert_eq!(h.app.lists.favorites.len(), 3);
    key(&mut h.app, KeyCode::Char('d'));
    key(&mut h.app, KeyCode::Enter);
    assert_eq!(names(&h.app), vec!["Bravo Roleplay", "Charlie Deathmatch"]);
    assert_eq!(selected_name(&h.app), "Charlie Deathmatch");
    key(&mut h.app, KeyCode::Char(' '));
    assert_eq!(names(&h.app), vec!["Bravo Roleplay"]);
    assert_eq!(Lists::load(&h.app.paths).unwrap().favorites.len(), 1);
}

#[tokio::test]
async fn recent_keys() {
    let mut h = app_with_list().await;
    h.app.settings.game_dir = Some(h.root.path().to_path_buf());
    for _ in 0..2 {
        key(&mut h.app, KeyCode::Enter);
        type_str(&mut h.app, "Carl_Johnson");
        key(&mut h.app, KeyCode::Enter);
        key(&mut h.app, KeyCode::Esc);
        key(&mut h.app, KeyCode::Down);
    }
    assert_eq!(h.app.lists.recent.len(), 2);
    key(&mut h.app, KeyCode::Char('4'));
    assert_eq!(h.app.view.len(), 2);
    key(&mut h.app, KeyCode::Char('d'));
    assert_eq!(h.app.lists.recent.len(), 1);
    assert!(h.app.popup.is_none());
    key(&mut h.app, KeyCode::Char('x'));
    assert!(matches!(h.app.popup, Some(Popup::Confirm { .. })));
    key(&mut h.app, KeyCode::Char('y'));
    assert!(h.app.lists.recent.is_empty());
    assert!(Lists::load(&h.app.paths).unwrap().recent.is_empty());
    key(&mut h.app, KeyCode::Char('x'));
    assert!(h.app.popup.is_none());
}

#[tokio::test]
async fn refresh_and_requery_keys_send_requests() {
    let mut h = app_with_list().await;
    h.app.settings.query_lists = true;
    key(&mut h.app, KeyCode::Char('r'));
    assert!(h.app.loading);
    let ev = next_event(&mut h).await;
    assert!(matches!(ev, AppEvent::ApiLoaded(Err(_))), "{ev:?}");
    h.app.handle_event(ev);
    assert!(!h.app.loading);
    assert!(h.app.api_error.is_some());
    key(&mut h.app, KeyCode::Char('R'));
    let want = h.app.view.len();
    let (mut basics, mut fulls) = (0, 0);
    while basics < want {
        match next_event(&mut h).await {
            AppEvent::Basic { .. } => basics += 1,
            AppEvent::Full { .. } => fulls += 1,
            other => panic!("unexpected {other:?}"),
        }
    }
    assert!(fulls >= 1);
    h.app.handle_event(AppEvent::Tick);
    let ev = next_event(&mut h).await;
    assert!(matches!(ev, AppEvent::Ping { .. } | AppEvent::Full { .. }), "{ev:?}");
}

#[tokio::test]
async fn join_popup_keys_and_remembered_password() {
    let mut h = app_with_list().await;
    h.app.settings.game_dir = Some(h.root.path().to_path_buf());
    key(&mut h.app, KeyCode::Enter);
    type_str(&mut h.app, "Carl_Johnson");
    key(&mut h.app, KeyCode::Down);
    type_str(&mut h.app, "secret");
    key(&mut h.app, KeyCode::Tab);
    key(&mut h.app, KeyCode::Char(' '));
    key(&mut h.app, KeyCode::BackTab);
    key(&mut h.app, KeyCode::Up);
    key(&mut h.app, KeyCode::Up);
    match &h.app.popup {
        Some(Popup::Join(f)) => {
            assert_eq!(f.field, 3);
            assert!(f.remember_password);
        }
        other => panic!("{other:?}"),
    }
    key(&mut h.app, KeyCode::Enter);
    assert!(matches!(h.app.popup, Some(Popup::Launch(_))));
    let addr = "127.0.0.1:7001".parse().unwrap();
    assert_eq!(h.app.lists.settings_for(addr).password.as_deref(), Some("secret"));
    assert_eq!(Lists::load(&h.app.paths).unwrap().settings_for(addr).password.as_deref(), Some("secret"));
    h.app.handle_event(AppEvent::Launch(HelperEvent::Log("wine: noise".into())));
    key(&mut h.app, KeyCode::Char('c'));
    if let Some(Popup::Launch(st)) = &h.app.popup {
        assert!(st.lines.is_empty());
    }
    key(&mut h.app, KeyCode::Esc);
    assert!(h.app.popup.is_none());
    h.app.handle_event(AppEvent::LaunchPrepared(Err("no wine".into())));
    key(&mut h.app, KeyCode::Char('l'));
    match &h.app.popup {
        Some(Popup::Launch(st)) => assert!(st.lines.iter().any(|(l, _)| l.contains("no wine"))),
        other => panic!("{other:?}"),
    }
    key(&mut h.app, KeyCode::Enter);
    assert!(h.app.popup.is_none());
    key(&mut h.app, KeyCode::Enter);
    match &h.app.popup {
        Some(Popup::Join(f)) => {
            assert_eq!(f.password.value(), "secret");
            assert!(f.remember_password);
        }
        other => panic!("{other:?}"),
    }
    key(&mut h.app, KeyCode::Esc);
    assert_eq!(h.app.settings.nickname, "Carl_Johnson");
    assert_eq!(Settings::load(&h.app.paths).unwrap().recent_nicknames, vec!["Carl_Johnson"]);
}

#[tokio::test]
async fn settings_popup_text_editing_toggles_and_actions() {
    let mut h = app_with_list().await;
    key(&mut h.app, KeyCode::Char(','));
    key(&mut h.app, KeyCode::Enter);
    type_str(&mut h.app, "Carl");
    key(&mut h.app, KeyCode::Enter);
    assert_eq!(h.app.settings.nickname, "Carl");
    assert_eq!(Settings::load(&h.app.paths).unwrap().nickname, "Carl");
    key(&mut h.app, KeyCode::Enter);
    type_str(&mut h.app, "!!");
    key(&mut h.app, KeyCode::Enter);
    match &h.app.popup {
        Some(Popup::Settings(f)) => {
            assert!(f.editing.is_some());
            assert!(f.message.as_deref().unwrap().contains("nickname"));
        }
        other => panic!("{other:?}"),
    }
    key(&mut h.app, KeyCode::Esc);
    assert_eq!(h.app.settings.nickname, "Carl");
    render(&mut h.app, 110, 32);

    key(&mut h.app, KeyCode::Down);
    key(&mut h.app, KeyCode::Enter);
    type_str(&mut h.app, "/nonexistent/dir");
    key(&mut h.app, KeyCode::Enter);
    assert!(h.app.settings.game_dir.is_none());
    ctrl(&mut h.app, 'u');
    let game = h.root.path().join("game");
    fs::create_dir_all(&game).unwrap();
    type_str(&mut h.app, game.to_str().unwrap());
    key(&mut h.app, KeyCode::Enter);
    assert!(h.app.settings.game_dir.is_none(), "exe missing should be rejected");
    fs::write(game.join("gta_sa.exe"), b"MZ").unwrap();
    key(&mut h.app, KeyCode::Enter);
    assert_eq!(h.app.settings.game_dir.as_deref(), Some(game.as_path()));

    key(&mut h.app, KeyCode::Down);
    key(&mut h.app, KeyCode::Down);
    key(&mut h.app, KeyCode::Left);
    assert_eq!(h.app.settings.samp_version, SampVersion::R4);
    key(&mut h.app, KeyCode::Right);
    key(&mut h.app, KeyCode::Right);
    assert_eq!(h.app.settings.samp_version, SampVersion::DL);
    key(&mut h.app, KeyCode::Down);
    key(&mut h.app, KeyCode::Char(' '));
    assert!(!h.app.settings.omp_inject);
    key(&mut h.app, KeyCode::Tab);
    key(&mut h.app, KeyCode::Tab);
    key(&mut h.app, KeyCode::Tab);
    key(&mut h.app, KeyCode::Enter);
    type_str(&mut h.app, "DXVK_HUD=fps WINEDEBUG=-all");
    key(&mut h.app, KeyCode::Enter);
    assert_eq!(h.app.settings.env["DXVK_HUD"], "fps");
    key(&mut h.app, KeyCode::Enter);
    type_str(&mut h.app, " broken");
    key(&mut h.app, KeyCode::Enter);
    assert!(matches!(&h.app.popup, Some(Popup::Settings(f)) if f.message.is_some()));
    key(&mut h.app, KeyCode::Esc);
    key(&mut h.app, KeyCode::Down);
    key(&mut h.app, KeyCode::Enter);
    assert!(!h.app.settings.create_suspended);
    key(&mut h.app, KeyCode::Down);
    key(&mut h.app, KeyCode::Enter);
    assert!(h.app.settings.quit_after_launch);
    key(&mut h.app, KeyCode::Down);
    key(&mut h.app, KeyCode::Enter);
    assert!(h.app.settings.query_lists);
    key(&mut h.app, KeyCode::Down);
    key(&mut h.app, KeyCode::Enter);
    type_str(&mut h.app, "kitty");
    key(&mut h.app, KeyCode::Enter);
    assert_eq!(h.app.settings.terminal.as_deref(), Some("kitty"));

    key(&mut h.app, KeyCode::Down);
    key(&mut h.app, KeyCode::Enter);
    match &h.app.popup {
        Some(Popup::Message { title, lines, .. }) => {
            assert_eq!(title, "Check");
            assert!(lines.iter().any(|l| l.starts_with("Game:")));
        }
        other => panic!("{other:?}"),
    }
    key(&mut h.app, KeyCode::Esc);
    assert!(matches!(h.app.popup, Some(Popup::Settings(_))));
    key(&mut h.app, KeyCode::Down);
    key(&mut h.app, KeyCode::Down);
    key(&mut h.app, KeyCode::Enter);
    assert!(matches!(h.app.popup, Some(Popup::PathPrompt(_))));
    key(&mut h.app, KeyCode::Esc);
    assert!(h.app.popup.is_none());

    key(&mut h.app, KeyCode::Char(','));
    key(&mut h.app, KeyCode::Up);
    key(&mut h.app, KeyCode::Up);
    key(&mut h.app, KeyCode::Enter);
    match &h.app.popup {
        Some(Popup::PathPrompt(p)) => assert!(p.title.contains("favorites")),
        other => panic!("{other:?}"),
    }
    key(&mut h.app, KeyCode::Esc);
    key(&mut h.app, KeyCode::Char(','));
    key(&mut h.app, KeyCode::Up);
    key(&mut h.app, KeyCode::Up);
    key(&mut h.app, KeyCode::Up);
    key(&mut h.app, KeyCode::Up);
    key(&mut h.app, KeyCode::Up);
    key(&mut h.app, KeyCode::Enter);
    match &h.app.popup {
        Some(Popup::Message { title, .. }) => assert_eq!(title, "Auto-detect"),
        other => panic!("{other:?}"),
    }
    key(&mut h.app, KeyCode::Enter);
    assert!(matches!(h.app.popup, Some(Popup::Settings(_))), "back in settings after an action");
    key(&mut h.app, KeyCode::Char(','));
    assert!(h.app.popup.is_none());
    let saved = Settings::load(&h.app.paths).unwrap();
    assert_eq!(saved.samp_version, SampVersion::DL);
    assert!(!saved.omp_inject);
    assert!(saved.quit_after_launch);
}

#[tokio::test]
async fn import_client_files_and_userdata_through_the_ui() {
    let mut h = app_with_list().await;
    let src = h.root.path().join("mp.open.launcher");
    fs::create_dir_all(src.join("omp")).unwrap();
    fs::write(src.join("omp/omp-client.dll"), b"omp").unwrap();
    fs::create_dir_all(src.join("samp/0.3.7-R5")).unwrap();
    fs::write(src.join("samp/0.3.7-R5/samp.dll"), b"samp").unwrap();
    for f in SHARED_FILES {
        let p = src.join("samp/shared").join(f.rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, b"x").unwrap();
    }
    key(&mut h.app, KeyCode::Char('i'));
    key(&mut h.app, KeyCode::Char('u')); // prompt input: ctrl-u would clear, plain u is typed
    ctrl(&mut h.app, 'u');
    type_str(&mut h.app, src.to_str().unwrap());
    key(&mut h.app, KeyCode::Enter);
    pump_until(&mut h, |e| matches!(e, AppEvent::TaskDone { .. })).await;
    match &h.app.popup {
        Some(Popup::Message { title, lines, error }) => {
            assert_eq!(title, "Import client files");
            assert!(!error);
            assert!(lines[0].starts_with("copied 18 files"), "{lines:?}");
        }
        other => panic!("{other:?}"),
    }
    let files = ClientFiles::new(&h.app.paths.data_dir);
    assert!(files.omp_client_dll().is_file());
    assert!(files.samp_dll(SampVersion::R5).unwrap().is_file());
    key(&mut h.app, KeyCode::Esc);

    let mut userdata = b"SAMP".to_vec();
    userdata.extend_from_slice(&1u32.to_le_bytes());
    userdata.extend_from_slice(&2u32.to_le_bytes());
    for (host, port, name) in [("127.0.0.1", 7201u32, "Imported One"), ("localhost", 7202, "Imported Two")] {
        for s in [host.as_bytes(), name.as_bytes()] {
            if s == host.as_bytes() {
                userdata.extend_from_slice(&(s.len() as u32).to_le_bytes());
                userdata.extend_from_slice(s);
                userdata.extend_from_slice(&port.to_le_bytes());
            } else {
                userdata.extend_from_slice(&(s.len() as u32).to_le_bytes());
                userdata.extend_from_slice(s);
            }
        }
        userdata.extend_from_slice(&0u32.to_le_bytes());
        userdata.extend_from_slice(&0u32.to_le_bytes());
    }
    let path = h.root.path().join("USERDATA.DAT");
    fs::write(&path, userdata).unwrap();
    key(&mut h.app, KeyCode::Char(','));
    key(&mut h.app, KeyCode::Up);
    key(&mut h.app, KeyCode::Up);
    key(&mut h.app, KeyCode::Enter);
    type_str(&mut h.app, path.to_str().unwrap());
    key(&mut h.app, KeyCode::Enter);
    pump_until(&mut h, |e| matches!(e, AppEvent::ImportedFavorites(_))).await;
    assert_eq!(h.app.tab, ListKind::Favorites);
    assert_eq!(h.app.lists.favorites.len(), 2);
    assert_eq!(h.app.lists.favorites[1].host_label.as_deref(), Some("localhost:7202"));
    assert_eq!(Lists::load(&h.app.paths).unwrap().favorites.len(), 2);
    match &h.app.popup {
        Some(Popup::Message { lines, .. }) => assert!(lines[0].starts_with("2 new favorites")),
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn add_server_keys_and_resolution() {
    let mut h = app_with_list().await;
    key(&mut h.app, KeyCode::Char('a'));
    key(&mut h.app, KeyCode::Enter);
    assert!(matches!(h.app.popup, Some(Popup::AddServer(_))), "empty input keeps the prompt");
    type_str(&mut h.app, "bad host!");
    key(&mut h.app, KeyCode::Enter);
    pump_until(&mut h, |e| matches!(e, AppEvent::Resolved { .. })).await;
    assert!(matches!(&h.app.popup, Some(Popup::Message { error: true, .. })));
    key(&mut h.app, KeyCode::Enter);
    key(&mut h.app, KeyCode::Char('a'));
    type_str(&mut h.app, "localhost:7300");
    key(&mut h.app, KeyCode::Enter);
    pump_until(&mut h, |e| matches!(e, AppEvent::Resolved { .. })).await;
    assert_eq!(h.app.tab, ListKind::Favorites);
    assert_eq!(selected_name(&h.app), "localhost:7300");
    key(&mut h.app, KeyCode::Char('a'));
    key(&mut h.app, KeyCode::Esc);
    assert!(h.app.popup.is_none());
    h.app.handle_event(AppEvent::Basic {
        addr: "127.0.0.1:7300".parse().unwrap(),
        result: BasicResult { info: None, ping: Some(3) },
    });
    assert_eq!(h.app.selected_server().unwrap().ping, Some(3));
}

#[tokio::test]
async fn help_and_message_close_keys() {
    let mut h = app_with_list().await;
    for close in [KeyCode::Esc, KeyCode::Char('q'), KeyCode::Char('?'), KeyCode::Enter] {
        key(&mut h.app, KeyCode::Char('?'));
        assert!(matches!(h.app.popup, Some(Popup::Help)));
        key(&mut h.app, KeyCode::Char('x'));
        assert!(matches!(h.app.popup, Some(Popup::Help)));
        key(&mut h.app, close);
        assert!(h.app.popup.is_none());
    }
    h.app.message("t", vec!["m".into()], false);
    key(&mut h.app, KeyCode::Char('z'));
    assert!(h.app.popup.is_some());
    key(&mut h.app, KeyCode::Char('q'));
    assert!(h.app.popup.is_none());
    assert!(!h.app.should_quit);
}

#[tokio::test]
async fn tiny_terminals_do_not_panic() {
    let mut h = app_with_list().await;
    for (w, hgt) in [(5, 3), (12, 6), (30, 10), (60, 5), (200, 60)] {
        render(&mut h.app, w, hgt);
        key(&mut h.app, KeyCode::Char('?'));
        render(&mut h.app, w, hgt);
        key(&mut h.app, KeyCode::Esc);
        key(&mut h.app, KeyCode::Char(','));
        render(&mut h.app, w, hgt);
        key(&mut h.app, KeyCode::Esc);
        key(&mut h.app, KeyCode::Char('/'));
        type_str(&mut h.app, "alpha");
        render(&mut h.app, w, hgt);
        key(&mut h.app, KeyCode::Esc);
        key(&mut h.app, KeyCode::Enter);
        render(&mut h.app, w, hgt);
        key(&mut h.app, KeyCode::Esc);
    }
}

#[tokio::test]
async fn added_server_shows_up_once_its_query_answers_with_filters_on() {
    let mut h = app_with_list().await;
    key(&mut h.app, KeyCode::Char('e'));
    key(&mut h.app, KeyCode::Char('a'));
    type_str(&mut h.app, "127.0.0.1:7400");
    key(&mut h.app, KeyCode::Enter);
    pump_until(&mut h, |e| matches!(e, AppEvent::Resolved { .. })).await;
    assert_eq!(h.app.tab, ListKind::Favorites);
    assert_eq!(h.app.lists.favorites.len(), 1);
    assert!(h.app.view.is_empty(), "no player count yet, hidden by the non-empty filter");
    let addr = "127.0.0.1:7400".parse().unwrap();
    let info = omptui_core::query::packet::InfoPacket {
        players: 3,
        max_players: 50,
        hostname: "Late".into(),
        ..Default::default()
    };
    h.app.handle_event(AppEvent::Basic { addr, result: BasicResult { info: Some(info), ping: Some(9) } });
    assert_eq!(h.app.view.len(), 1);
    assert_eq!(selected_name(&h.app), "Late");
    let screen = render(&mut h.app, 100, 24);
    assert!(screen.contains("Late"), "{screen}");
}

#[tokio::test]
async fn status_line_clears_quickly_and_on_esc() {
    let mut h = app_with_list().await;
    h.app.status("2 servers loaded", false);
    h.app.handle_event(AppEvent::Tick);
    assert!(h.app.status.is_some());
    h.app.status.as_mut().unwrap().at = std::time::Instant::now() - Duration::from_secs(3);
    h.app.handle_event(AppEvent::Tick);
    assert!(h.app.status.is_none());
    h.app.status("boom", true);
    h.app.status.as_mut().unwrap().at = std::time::Instant::now() - Duration::from_secs(3);
    h.app.handle_event(AppEvent::Tick);
    assert!(h.app.status.is_some(), "errors stay longer");
    key(&mut h.app, KeyCode::Esc);
    assert!(h.app.status.is_none());
}
