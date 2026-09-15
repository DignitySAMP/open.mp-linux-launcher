use omptui_core::query::fake_server::{FakeServer, FakeServerConfig};
use omptui_core::store::{Lists, Paths, Settings};
use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

struct Tui {
    parser: Arc<Mutex<vt100::Parser>>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn portable_pty::Child + Send + Sync>,
}

impl Tui {
    fn spawn(env: &[(&str, String)], args: &[String]) -> Tui {
        let pty = native_pty_system();
        let pair = pty.openpty(PtySize { rows: 32, cols: 120, pixel_width: 0, pixel_height: 0 }).unwrap();
        let mut cmd = CommandBuilder::new(env!("CARGO_BIN_EXE_omp-tui"));
        cmd.env("TERM", "xterm-256color");
        cmd.env("OMPTUI_QUERY_TIMEOUT_MS", "500");
        for (k, v) in env {
            cmd.env(k, v);
        }
        cmd.args(args);
        let child = pair.slave.spawn_command(cmd).unwrap();
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader().unwrap();
        let writer = pair.master.take_writer().unwrap();
        let parser = Arc::new(Mutex::new(vt100::Parser::new(32, 120, 0)));
        let p2 = parser.clone();
        std::thread::spawn(move || {
            let mut buf = [0u8; 4096];
            while let Ok(n) = reader.read(&mut buf) {
                if n == 0 {
                    break;
                }
                p2.lock().unwrap().process(&buf[..n]);
            }
        });
        Tui { parser, writer, child }
    }

    fn screen(&self) -> String {
        self.parser.lock().unwrap().screen().contents()
    }

    fn wait_for(&self, what: &str) -> String {
        let start = Instant::now();
        loop {
            let s = self.screen();
            if s.contains(what) {
                return s;
            }
            assert!(start.elapsed() < Duration::from_secs(15), "timed out waiting for {what:?}; screen:\n{s}");
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    fn wait_gone(&self, what: &str) {
        let start = Instant::now();
        while self.screen().contains(what) {
            assert!(start.elapsed() < Duration::from_secs(15), "timed out waiting for {what:?} to disappear");
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    fn send(&mut self, bytes: &[u8]) {
        self.writer.write_all(bytes).unwrap();
        self.writer.flush().unwrap();
        std::thread::sleep(Duration::from_millis(80));
    }

    fn finish(mut self) -> portable_pty::ExitStatus {
        let start = Instant::now();
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                return status;
            }
            assert!(start.elapsed() < Duration::from_secs(10), "binary did not exit");
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

struct World {
    _mock: MockServer,
    _fakes: Vec<FakeServer>,
    root: tempfile::TempDir,
    env: Vec<(&'static str, String)>,
}

async fn world() -> World {
    let alpha = FakeServer::start(FakeServerConfig::sample("Alpha Freeroam")).await.unwrap();
    let mut bravo_cfg = FakeServerConfig::sample("Bravo Roleplay");
    bravo_cfg.info.password = true;
    bravo_cfg.info.players = 0;
    let bravo = FakeServer::start(bravo_cfg).await.unwrap();
    let json = format!(
        r#"[{{"ip":"{}","hn":"Alpha Freeroam","pc":2,"pm":100,"gm":"Freeroam","la":"English","pa":false,"vn":"omp 1.5.8.3079","omp":true,"pr":true}},
            {{"ip":"{}","hn":"Bravo Roleplay","pc":0,"pm":100,"gm":"Roleplay","la":"Russian","pa":true,"vn":"0.3.7-R2","omp":false,"pr":false}}]"#,
        alpha.addr(),
        bravo.addr()
    );
    let mock = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/servers"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(json, "application/json"))
        .mount(&mock)
        .await;
    let root = tempfile::tempdir().unwrap();
    let paths = Paths::under(root.path());
    let game = root.path().join("game");
    std::fs::create_dir_all(&game).unwrap();
    Settings { nickname: "Tester".into(), game_dir: Some(game), ..Default::default() }.save(&paths).unwrap();
    let env = vec![
        ("OMPTUI_API_URL", mock.uri()),
        ("OMPTUI_CONFIG_DIR", paths.config_dir.to_string_lossy().into_owned()),
        ("OMPTUI_DATA_DIR", paths.data_dir.to_string_lossy().into_owned()),
        ("OMPTUI_STATE_DIR", paths.state_dir.to_string_lossy().into_owned()),
    ];
    World { _mock: mock, _fakes: vec![alpha, bravo], root, env }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn browse_search_favorite_join_prompt_and_persist() {
    let w = world().await;
    let mut tui = Tui::spawn(&w.env, &[]);
    let screen = tui.wait_for("Bravo Roleplay");
    assert!(screen.contains("2 Internet (2)"), "{screen}");
    assert!(screen.contains("Alpha Freeroam"));
    tui.wait_for("ms");
    let screen = tui.wait_for("johnny");
    assert!(screen.contains("bigsmoke"), "player list missing:\n{screen}");
    assert!(screen.contains("mapname=San Andreas"), "rules missing:\n{screen}");

    tui.send(b"/bravo");
    let screen = tui.wait_for("1/2");
    assert!(!screen.contains("Alpha Freeroam"), "{screen}");
    tui.send(b"\x1b");
    tui.wait_for("2/2");
    tui.send(b"g");

    tui.send(b"F");
    tui.wait_for("added to favorites");
    tui.send(b"1");
    let screen = tui.wait_for("1 Favorites (1)");
    assert!(screen.contains("Alpha Freeroam"), "{screen}");

    tui.send(b"\r");
    let screen = tui.wait_for("Join server");
    assert!(screen.contains("Tester"), "nickname not prefilled:\n{screen}");
    tui.send(b"\x1b");
    tui.wait_gone("Join server");

    tui.send(b"p");
    tui.wait_for("Server settings");
    tui.send(b"no");
    tui.send(b"\r");
    tui.wait_for("nickname must be 3-24");
    tui.send(b"body_here");
    tui.send(b"\t\t");
    tui.send(b"\x1b[C");
    tui.send(b"\r");
    tui.wait_for("saved settings for");
    tui.wait_for("overrides nickname nobody_here, SA-MP 0.3.7-R1");

    tui.send(b",");
    tui.wait_for("Settings");
    tui.send(b"\x1b");
    tui.wait_gone("Extra environment");

    tui.send(b"?");
    tui.wait_for("Keys");
    tui.send(b"\x1b");

    tui.send(b"q");
    let status = tui.finish();
    assert!(status.success(), "{status:?}");

    let lists = Lists::load(&Paths::under(w.root.path())).unwrap();
    assert_eq!(lists.favorites.len(), 1);
    assert_eq!(lists.favorites[0].info.hostname, "Alpha Freeroam");
    assert!(lists.favorites[0].rules.is_empty());
    let per = lists.settings_for(w._fakes[0].addr());
    assert_eq!(per.nickname.as_deref(), Some("nobody_here"));
    assert_eq!(per.samp_version, Some(omptui_core::resources::SampVersion::R1));

    let mut tui = Tui::spawn(&w.env, &[]);
    let screen = tui.wait_for("Alpha Freeroam");
    assert!(screen.contains("1 Favorites (1)"), "{screen}");
    tui.send(b"q");
    assert!(tui.finish().success());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn deep_link_opens_join_prompt() {
    let w = world().await;
    let alpha = w._fakes[0].addr();
    let mut tui = Tui::spawn(&w.env, &[format!("omp://{alpha}/?password=hunter2")]);
    let screen = tui.wait_for("Join server");
    assert!(screen.contains("Alpha Freeroam"), "{screen}");
    assert!(screen.contains("•••••••"), "password not prefilled:\n{screen}");
    tui.send(b"\x1b");
    tui.send(b"q");
    assert!(tui.finish().success());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn remembered_password_is_encrypted_on_disk_and_restored() {
    let w = world().await;
    let mut tui = Tui::spawn(&w.env, &[]);
    tui.wait_for("Alpha Freeroam");
    tui.send(b"\r");
    tui.wait_for("Join server");
    tui.send(b"\t");
    tui.send(b"hunter2");
    tui.send(b"\t");
    tui.send(b" ");
    tui.send(b"\t\t");
    tui.send(b"\r");
    tui.wait_for("cannot launch: game executable not found");
    tui.send(b"\x1b");
    tui.send(b"q");
    assert!(tui.finish().success());

    let paths = Paths::under(w.root.path());
    let text = std::fs::read_to_string(paths.lists_file()).unwrap();
    assert!(!text.contains("hunter2"), "plaintext on disk:\n{text}");
    assert!(text.contains("password = \"enc1:"), "{text}");
    assert!(paths.data_dir.join("secret.key").is_file());
    assert_eq!(Lists::load(&paths).unwrap().settings_for(w._fakes[0].addr()).password.as_deref(), Some("hunter2"));

    let mut tui = Tui::spawn(&w.env, &[]);
    tui.wait_for("Alpha Freeroam");
    tui.send(b"\r");
    let screen = tui.wait_for("Join server");
    assert!(screen.contains("•••••••"), "password not restored:\n{screen}");
    assert!(screen.contains("[x] remember password"), "{screen}");
    tui.send(b"\x1b");
    tui.send(b"q");
    assert!(tui.finish().success());
}
