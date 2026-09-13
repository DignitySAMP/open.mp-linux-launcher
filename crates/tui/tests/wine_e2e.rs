// Needs Wine. Skipped when none is found or when OMPTUI_WINE_E2E=0.

use omptui_core::launch::{self, HelperEvent, LaunchRequest};
use omptui_core::resources::{ClientFiles, SHARED_FILES, SampVersion};
use omptui_core::wine::{WineEnv, discover_wine};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tokio::sync::mpsc;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().parent().unwrap().to_path_buf()
}

fn build_testpe() -> (PathBuf, PathBuf) {
    let dir = workspace_root().join("crates").join("testpe");
    let cargo = std::env::var_os("CARGO").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("cargo"));
    let mut cmd = Command::new(cargo);
    cmd.current_dir(&dir)
        .args(["build", "--release", "--target", "i686-pc-windows-msvc"])
        .arg("--target-dir")
        .arg(dir.join("target"));
    for k in ["RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "CARGO_TARGET_DIR", "CARGO_BUILD_TARGET"] {
        cmd.env_remove(k);
    }
    let status = cmd.status().expect("run cargo for crates/testpe");
    assert!(status.success(), "building crates/testpe failed");
    let out = dir.join("target/i686-pc-windows-msvc/release");
    (out.join("dummy-game.exe"), out.join("dummy.dll"))
}

async fn run_case(wine: &Path, root: &Path, game_exe: &Path, dll: &Path, suspended: bool) {
    let prefix = root.join(if suspended { "prefix-suspended" } else { "prefix-running" });
    let env = WineEnv { wine: wine.to_path_buf(), prefix: prefix.clone(), extra_env: Default::default() };
    env.init_prefix().await.expect("wineboot");
    let game_dir = prefix.join("drive_c").join("game");
    fs::create_dir_all(&game_dir).unwrap();
    fs::copy(game_exe, game_dir.join("gta_sa.exe")).unwrap();
    let files = ClientFiles::new(root.join("data"));
    let samp = files.samp_dll(SampVersion::R5).unwrap();
    fs::create_dir_all(samp.parent().unwrap()).unwrap();
    fs::copy(dll, &samp).unwrap();
    fs::create_dir_all(files.omp_client_dll().parent().unwrap()).unwrap();
    fs::copy(dll, files.omp_client_dll()).unwrap();
    for f in SHARED_FILES {
        let p = files.shared_dir().join(f.rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, b"stub").unwrap();
    }
    let req = LaunchRequest {
        addr: "127.0.0.1:7777".parse().unwrap(),
        nickname: "Tester".into(),
        password: Some("hunter2".into()),
        game_dir: game_dir.clone(),
        game_exe: "gta_sa.exe".into(),
        samp_version: SampVersion::R5,
        omp_inject: true,
        wine: env,
        create_suspended: suspended,
        wait_for_module: None,
    };
    let prepared = launch::prepare(&req, &files, omp_tui::HELPER_EXE).unwrap();
    assert!(prepared.warnings.iter().any(|w| w.contains("not the 1.0 US")));
    assert_eq!(prepared.copied.len(), SHARED_FILES.len());
    let (tx, mut rx) = mpsc::channel(64);
    let code = launch::run(&prepared, tx).await.unwrap();
    let mut events = Vec::new();
    while let Some(e) = rx.recv().await {
        events.push(e);
    }
    let names: Vec<String> = events.iter().map(|e| e.describe()).collect();
    assert_eq!(code, Some(0), "helper exit code; events:\n{}", names.join("\n"));
    assert!(events.iter().any(|e| matches!(e, HelperEvent::Spawned { .. })), "{names:?}");
    assert_eq!(events.iter().filter(|e| matches!(e, HelperEvent::Injected { .. })).count(), 2, "{names:?}");
    assert_eq!(events.iter().any(|e| matches!(e, HelperEvent::Resumed)), suspended, "{names:?}");
    assert!(events.contains(&HelperEvent::Exit { code: 7 }), "{names:?}");
    assert!(!events.iter().any(|e| matches!(e, HelperEvent::Error { .. } | HelperEvent::Retry { .. })), "{names:?}");

    let args = fs::read_to_string(game_dir.join("omp-tui-game-args.txt")).unwrap();
    assert!(args.ends_with("gta_sa.exe -c -n Tester -h 127.0.0.1 -p 7777 -z hunter2"), "{args}");
    let loaded = fs::read_to_string(game_dir.join("omp-tui-dll-loaded.txt")).unwrap();
    assert!(loaded.ends_with("omp-client.dll") || loaded.ends_with("samp.dll"), "{loaded}");
    assert!(game_dir.join("SAMP").join("SAMP.img").is_file());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn launches_dummy_game_and_injects_under_wine() {
    if std::env::var("OMPTUI_WINE_E2E").is_ok_and(|v| v == "0") {
        eprintln!("skipped: OMPTUI_WINE_E2E=0");
        return;
    }
    let wine = match std::env::var_os("OMPTUI_WINE_E2E")
        .map(PathBuf::from)
        .filter(|p| p.is_file())
        .or_else(|| discover_wine().into_iter().next().map(|w| w.path))
    {
        Some(w) => w,
        None => {
            eprintln!("skipped: no Wine binary found (set OMPTUI_WINE_E2E=/path/to/wine)");
            return;
        }
    };
    assert!(!omp_tui::HELPER_EXE.is_empty(), "helper not embedded (built with OMPTUI_SKIP_INJECTOR?)");
    let (game, dll) = build_testpe();
    let root = tempfile::tempdir().unwrap();
    run_case(&wine, root.path(), &game, &dll, true).await;
    run_case(&wine, root.path(), &game, &dll, false).await;
}
