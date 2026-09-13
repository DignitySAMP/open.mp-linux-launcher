#![allow(dead_code)]

use omp_tui::app::App;
use omp_tui::app::tasks::{AppEvent, Services};
use omptui_core::api::ApiClient;
use omptui_core::query::{Querier, QueryConfig};
use omptui_core::store::{Lists, Paths, Settings};
use omptui_core::{Server, ServerAddr, ServerInfo};
use std::net::{Ipv4Addr, SocketAddrV4};
use std::time::Duration;
use tokio::sync::mpsc;

pub struct Harness {
    pub app: App,
    pub rx: mpsc::UnboundedReceiver<AppEvent>,
    pub root: tempfile::TempDir,
}

pub async fn harness(settings: Settings, lists: Lists) -> Harness {
    let root = tempfile::tempdir().unwrap();
    let paths = Paths::under(root.path());
    let (tx, rx) = mpsc::unbounded_channel();
    let querier = Querier::bind(QueryConfig {
        timeout: Duration::from_millis(300),
        bind: SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0),
        ..Default::default()
    })
    .await
    .unwrap();
    let svc = Services { api: ApiClient::new("http://127.0.0.1:9"), querier, tx, helper: b"" };
    Harness { app: App::new(svc, paths, settings, lists), rx, root }
}

pub fn server(port: u16, name: &str, players: u16, omp: bool, password: bool) -> Server {
    let mut s = Server::with_addr(ServerAddr::new(Ipv4Addr::LOCALHOST, port));
    s.info = ServerInfo {
        hostname: name.into(),
        gamemode: format!("{name} mode"),
        language: "English".into(),
        players,
        max_players: 100,
        password,
        version: if omp { "omp 1.5.8.3079".into() } else { "0.3.7-R2".into() },
        omp,
        partner: port.is_multiple_of(2),
    };
    s
}

pub fn fixture_servers() -> Vec<Server> {
    vec![
        server(7001, "Alpha Freeroam", 12, true, false),
        server(7002, "Bravo Roleplay", 0, false, true),
        server(7003, "Charlie Deathmatch", 55, true, false),
    ]
}
