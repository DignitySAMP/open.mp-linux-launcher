// The injector helper prints one JSON object per line, parsed into HelperEvent below.

use crate::model::ServerAddr;
use crate::resources::{ClientFiles, SampVersion, inspect_game_exe, md5_bytes, md5_file};
use crate::store::write_atomic;
use crate::validation::{ValidationError, sanitize_password, validate_game, validate_nickname};
use crate::wine::WineEnv;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::mpsc;

pub const HELPER_NAME: &str = "omp-injector.exe";
pub const INJECT_RETRIES: u32 = 5;
pub const INJECT_RETRY_DELAY_MS: u32 = 500;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchRequest {
    pub addr: ServerAddr,
    pub nickname: String,
    pub password: Option<String>,
    pub game_dir: PathBuf,
    pub game_exe: String,
    pub samp_version: SampVersion,
    pub omp_inject: bool,
    pub wine: WineEnv,
    pub create_suspended: bool,
    pub wait_for_module: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum LaunchError {
    #[error("{0}")]
    Validation(#[from] ValidationError),
    #[error("{0}")]
    MissingFile(String),
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("cannot translate {0} to a Windows path inside prefix {1}")]
    Path(PathBuf, PathBuf),
    #[error("no Windows helper embedded in this build (set OMPTUI_INJECTOR_EXE when building)")]
    NoHelper,
    #[error("Wine binary not found: {0}")]
    NoWine(PathBuf),
    #[error("Wine prefix not initialised: {0}")]
    NoPrefix(PathBuf),
}

#[derive(Debug, Clone)]
pub struct Prepared {
    pub request: LaunchRequest,
    pub helper_path: PathBuf,
    pub helper_args: Vec<String>,
    pub warnings: Vec<String>,
    pub copied: Vec<String>,
}

impl Prepared {
    pub fn preview(&self) -> String {
        let mut s = format!(
            "WINEPREFIX={} {} {}",
            self.request.wine.prefix.display(),
            self.request.wine.wine.display(),
            self.helper_path.display()
        );
        for a in &self.helper_args {
            s.push(' ');
            if a.contains(' ') { s.push_str(&format!("\"{a}\"")) } else { s.push_str(a) }
        }
        s
    }
}

pub fn extract_helper(files: &ClientFiles, helper: &[u8]) -> Result<PathBuf, LaunchError> {
    if helper.is_empty() {
        return Err(LaunchError::NoHelper);
    }
    let path = files.helper_dir().join(HELPER_NAME);
    let want = md5_bytes(helper);
    if md5_file(&path).ok().as_deref() != Some(want.as_str()) {
        write_atomic(&path, helper)?;
    }
    Ok(path)
}

pub fn prepare(req: &LaunchRequest, files: &ClientFiles, helper: &[u8]) -> Result<Prepared, LaunchError> {
    let mut warnings = Vec::new();
    let nickname = validate_nickname(&req.nickname)?;
    let exe = validate_game(&req.game_dir, &req.game_exe)?;
    if !req.wine.wine.is_file() {
        return Err(LaunchError::NoWine(req.wine.wine.clone()));
    }
    let prefix = req.wine.prefix();
    if !prefix.exists() {
        return Err(LaunchError::NoPrefix(req.wine.prefix.clone()));
    }
    match inspect_game_exe(&exe) {
        Ok(i) if !i.is_10_us => warnings.push(format!(
            "{} is not the 1.0 US executable ({} bytes, md5 {}); open.mp/SA-MP require gta_sa.exe 1.0 US",
            req.game_exe, i.size, i.md5
        )),
        Ok(_) => {}
        Err(e) => warnings.push(format!("could not inspect {}: {e}", exe.display())),
    }

    // samp.dll has to be loaded before omp-client.dll, which hooks into it.
    let mut dlls: Vec<PathBuf> = Vec::new();
    let mut copied = Vec::new();
    match req.samp_version {
        SampVersion::Custom => {
            let p = req.game_dir.join("samp.dll");
            if !p.is_file() {
                return Err(LaunchError::MissingFile(format!(
                    "SA-MP version is 'custom' but {} does not exist",
                    p.display()
                )));
            }
            dlls.push(p);
        }
        v => {
            let p = files.samp_dll(v).expect("non-custom version has a path");
            if !p.is_file() {
                return Err(LaunchError::MissingFile(format!(
                    "samp.dll {} is not installed ({}); download or import client files in Settings",
                    v.label(),
                    p.display()
                )));
            }
            if let Some(exp) = v.dll_md5()
                && let Ok(actual) = md5_file(&p)
                && actual != exp
            {
                warnings.push(format!("samp.dll {} has an unexpected checksum ({actual})", v.label()));
            }
            copied =
                files.ensure_shared_in_game_dir(&req.game_dir).map_err(|e| LaunchError::MissingFile(e.to_string()))?;
            dlls.push(p);
        }
    }
    if req.omp_inject {
        let p = files.omp_client_dll();
        if !p.is_file() {
            return Err(LaunchError::MissingFile(format!(
                "omp-client.dll is not installed ({}); download it in Settings or disable open.mp injection",
                p.display()
            )));
        }
        dlls.push(p);
        let has_d3dx = prefix.system_dirs().iter().any(|d| d.join("d3dx9_25.dll").is_file());
        if !has_d3dx {
            warnings.push(
                "d3dx9_25.dll is missing from the prefix's system folder; the open.mp client needs it (run `winetricks d3dx9` for this prefix or use Settings > Install d3dx9)"
                    .into(),
            );
        }
    }

    if !prefix.has_arial() {
        warnings.push(
            "arial.ttf is missing from the prefix; SA-MP crashes when the game is paused without it (run `winetricks arial` for this prefix or use Settings > Install Arial)"
                .into(),
        );
    }
    if prefix.d3d_stack().uses_dxvk() && !crate::dxvk::vulkan_available() {
        warnings.push("the prefix uses DXVK but no Vulkan driver was found; expect a black window".into());
    }

    let helper_path = extract_helper(files, helper)?;
    let win =
        |p: &Path| prefix.to_windows_path(p).ok_or_else(|| LaunchError::Path(p.to_path_buf(), req.wine.prefix.clone()));
    let mut args = vec![
        "--exe".to_string(),
        win(&exe)?,
        "--cwd".to_string(),
        win(&req.game_dir)?,
        "--retries".to_string(),
        INJECT_RETRIES.to_string(),
        "--delay".to_string(),
        INJECT_RETRY_DELAY_MS.to_string(),
        "--wait".to_string(),
    ];
    for d in &dlls {
        args.push("--dll".into());
        args.push(win(d)?);
    }
    if req.create_suspended {
        args.push("--suspended".into());
    } else if let Some(m) = &req.wait_for_module {
        args.push("--wait-module".into());
        args.push(m.clone());
    }
    args.push("--".into());
    args.extend([
        "-c".to_string(),
        "-n".to_string(),
        nickname,
        "-h".to_string(),
        req.addr.ip.to_string(),
        "-p".to_string(),
        req.addr.port.to_string(),
    ]);
    if let Some(pw) = req.password.as_deref().map(sanitize_password).filter(|p| !p.is_empty()) {
        args.push("-z".into());
        args.push(pw);
    }
    Ok(Prepared { request: req.clone(), helper_path, helper_args: args, warnings, copied })
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "event", rename_all = "lowercase")]
pub enum HelperEvent {
    Spawned {
        pid: u32,
    },
    Injected {
        dll: String,
        attempt: u32,
    },
    Retry {
        dll: String,
        attempt: u32,
        code: u32,
        message: String,
    },
    Waiting {
        module: String,
    },
    Resumed,
    Error {
        stage: String,
        code: u32,
        message: String,
    },
    Exit {
        code: u32,
    },
    // not from the helper, stderr line
    #[serde(skip)]
    Log(String),
    // not from the helper, wine exited
    #[serde(skip)]
    Finished(Option<i32>),
}

impl HelperEvent {
    pub fn parse_line(line: &str) -> Option<Self> {
        let line = line.trim();
        if !line.starts_with('{') {
            return None;
        }
        serde_json::from_str(line).ok()
    }

    pub fn describe(&self) -> String {
        match self {
            HelperEvent::Spawned { pid } => format!("game process started (pid {pid})"),
            HelperEvent::Injected { dll, attempt } => {
                let name = dll.rsplit(['\\', '/']).next().unwrap_or(dll);
                if *attempt > 1 { format!("injected {name} (attempt {attempt})") } else { format!("injected {name}") }
            }
            HelperEvent::Retry { dll, attempt, code, message } => {
                let name = dll.rsplit(['\\', '/']).next().unwrap_or(dll);
                format!("retrying {name} (attempt {attempt}): {message} (code {code})")
            }
            HelperEvent::Waiting { module } => format!("waiting for {module} to load"),
            HelperEvent::Resumed => "game resumed".into(),
            HelperEvent::Error { stage, code, message } => format!("error during {stage}: {message} (code {code})"),
            HelperEvent::Exit { code } => format!("game exited with code {code}"),
            HelperEvent::Log(l) => l.clone(),
            HelperEvent::Finished(Some(c)) => format!("helper finished (exit {c})"),
            HelperEvent::Finished(None) => "helper finished".into(),
        }
    }
}

// The helper is started with --wait, so it and this future run until the game exits.
pub async fn run(prepared: &Prepared, tx: mpsc::Sender<HelperEvent>) -> Result<Option<i32>, LaunchError> {
    let mut cmd = prepared.request.wine.command();
    cmd.arg(&prepared.helper_path).args(&prepared.helper_args);
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    cmd.kill_on_drop(false);
    tracing::info!("launch: {}", prepared.preview());
    let mut child = cmd.spawn()?;
    let stdout = child.stdout.take().expect("piped");
    let stderr = child.stderr.take().expect("piped");
    let tx2 = tx.clone();
    let err_task = tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(l)) = lines.next_line().await {
            let l = l.trim().to_string();
            if l.is_empty() {
                continue;
            }
            tracing::debug!("wine stderr: {l}");
            let _ = tx2.send(HelperEvent::Log(l)).await;
        }
    });
    let mut lines = BufReader::new(stdout).lines();
    while let Ok(Some(l)) = lines.next_line().await {
        tracing::debug!("helper: {l}");
        match HelperEvent::parse_line(&l) {
            Some(ev) => {
                let _ = tx.send(ev).await;
            }
            None if !l.trim().is_empty() => {
                let _ = tx.send(HelperEvent::Log(l)).await;
            }
            None => {}
        }
    }
    let status = child.wait().await?;
    let _ = err_task.await;
    let code = status.code();
    let _ = tx.send(HelperEvent::Finished(code)).await;
    Ok(code)
}

pub async fn run_printing(prepared: &Prepared) -> Result<Option<i32>, LaunchError> {
    let (tx, mut rx) = mpsc::channel::<HelperEvent>(64);
    let printer = tokio::spawn(async move {
        while let Some(ev) = rx.recv().await {
            println!("{}", ev.describe());
        }
    });
    let r = run(prepared, tx).await;
    let _ = printer.await;
    r
}

pub fn parse_env_pairs(pairs: &[String]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .filter_map(|p| p.split_once('=').map(|(k, v)| (k.trim().to_owned(), v.to_owned())))
        .filter(|(k, _)| !k.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resources::SHARED_FILES;
    use std::fs;
    use std::os::unix::fs::symlink;

    struct Fixture {
        _root: tempfile::TempDir,
        files: ClientFiles,
        req: LaunchRequest,
    }

    fn fixture() -> Fixture {
        let root = tempfile::tempdir().unwrap();
        let p = root.path();
        let prefix = p.join("prefix");
        let game = prefix.join("drive_c/Program Files (x86)/GTA San Andreas");
        fs::create_dir_all(&game).unwrap();
        fs::create_dir_all(prefix.join("drive_c/windows/syswow64")).unwrap();
        fs::create_dir_all(prefix.join("dosdevices")).unwrap();
        symlink("../drive_c", prefix.join("dosdevices/c:")).unwrap();
        symlink("/", prefix.join("dosdevices/z:")).unwrap();
        fs::write(prefix.join("system.reg"), "#arch=win64\n").unwrap();
        fs::write(game.join("gta_sa.exe"), b"MZ fake").unwrap();
        let wine = p.join("wine");
        fs::write(&wine, "#!/bin/sh\n").unwrap();
        let data = p.join("data");
        let files = ClientFiles::new(&data);
        fs::create_dir_all(files.samp_dll(SampVersion::R5).unwrap().parent().unwrap()).unwrap();
        fs::write(files.samp_dll(SampVersion::R5).unwrap(), b"samp").unwrap();
        fs::create_dir_all(files.omp_client_dll().parent().unwrap()).unwrap();
        fs::write(files.omp_client_dll(), b"omp").unwrap();
        for f in SHARED_FILES {
            let s = files.shared_dir().join(f.rel);
            fs::create_dir_all(s.parent().unwrap()).unwrap();
            fs::write(s, f.rel).unwrap();
        }
        let req = LaunchRequest {
            addr: "1.2.3.4:7777".parse().unwrap(),
            nickname: "Carl_Johnson".into(),
            password: Some(" p w \x01".into()),
            game_dir: game,
            game_exe: "gta_sa.exe".into(),
            samp_version: SampVersion::R5,
            omp_inject: true,
            wine: WineEnv { wine, prefix, extra_env: BTreeMap::new() },
            create_suspended: true,
            wait_for_module: None,
        };
        Fixture { _root: root, files, req }
    }

    #[test]
    fn prepare_builds_expected_command() {
        let f = fixture();
        let prepared = prepare(&f.req, &f.files, b"fake helper exe").unwrap();
        assert!(prepared.helper_path.ends_with("bin/omp-injector.exe"));
        assert_eq!(fs::read(&prepared.helper_path).unwrap(), b"fake helper exe");
        let a = &prepared.helper_args;
        let pos = |s: &str| a.iter().position(|x| x == s).unwrap();
        assert_eq!(a[pos("--exe") + 1], "C:\\Program Files (x86)\\GTA San Andreas\\gta_sa.exe");
        assert_eq!(a[pos("--cwd") + 1], "C:\\Program Files (x86)\\GTA San Andreas");
        let dlls: Vec<&String> =
            a.iter().enumerate().filter(|(i, x)| *x == "--dll" && *i + 1 < a.len()).map(|(i, _)| &a[i + 1]).collect();
        assert_eq!(dlls.len(), 2);
        assert!(dlls[0].ends_with("\\samp\\0.3.7-R5\\samp.dll"), "{}", dlls[0]);
        assert!(dlls[0].starts_with("Z:\\"));
        assert!(dlls[1].ends_with("\\omp\\omp-client.dll"));
        assert!(a.contains(&"--suspended".to_string()));
        let dd = pos("--");
        assert_eq!(&a[dd + 1..], &["-c", "-n", "Carl_Johnson", "-h", "1.2.3.4", "-p", "7777", "-z", "pw"]);
        assert_eq!(prepared.copied.len(), SHARED_FILES.len());
        assert!(f.req.game_dir.join("SAMP/SAMP.img").is_file());
        assert!(prepared.warnings.iter().any(|w| w.contains("not the 1.0 US")));
        assert!(prepared.warnings.iter().any(|w| w.contains("unexpected checksum")));
        assert!(prepared.warnings.iter().any(|w| w.contains("d3dx9_25.dll")));
        assert!(prepared.preview().contains("omp-injector.exe --exe"));
        let m1 = fs::metadata(&prepared.helper_path).unwrap().modified().unwrap();
        let p2 = prepare(&f.req, &f.files, b"fake helper exe").unwrap();
        assert_eq!(fs::metadata(&p2.helper_path).unwrap().modified().unwrap(), m1);
    }

    #[test]
    fn prepare_errors() {
        let f = fixture();
        let mut r = f.req.clone();
        r.nickname = "x".into();
        assert!(matches!(prepare(&r, &f.files, b"h"), Err(LaunchError::Validation(ValidationError::Nickname))));
        let mut r = f.req.clone();
        r.game_exe = "nope.exe".into();
        assert!(matches!(prepare(&r, &f.files, b"h"), Err(LaunchError::Validation(ValidationError::ExeMissing(_)))));
        let mut r = f.req.clone();
        r.samp_version = SampVersion::R1;
        assert!(matches!(prepare(&r, &f.files, b"h"), Err(LaunchError::MissingFile(_))));
        let mut r = f.req.clone();
        r.samp_version = SampVersion::Custom;
        assert!(matches!(prepare(&r, &f.files, b"h"), Err(LaunchError::MissingFile(_))));
        fs::write(r.game_dir.join("samp.dll"), b"custom").unwrap();
        let p = prepare(&r, &f.files, b"h").unwrap();
        assert!(p.helper_args.iter().any(|a| a.ends_with("GTA San Andreas\\samp.dll")));
        assert!(p.copied.is_empty());
        assert!(matches!(prepare(&f.req, &f.files, b""), Err(LaunchError::NoHelper)));
        let mut r = f.req.clone();
        r.wine.prefix = PathBuf::from("/nonexistent/prefix");
        assert!(matches!(prepare(&r, &f.files, b"h"), Err(LaunchError::NoPrefix(_))));
        let mut r = f.req.clone();
        r.wine.wine = PathBuf::from("/nonexistent/wine");
        assert!(matches!(prepare(&r, &f.files, b"h"), Err(LaunchError::NoWine(_))));
        let mut r = f.req.clone();
        r.omp_inject = false;
        r.create_suspended = false;
        r.wait_for_module = Some("vorbisFile.dll".into());
        let p = prepare(&r, &f.files, b"h").unwrap();
        assert!(!p.helper_args.contains(&"--suspended".to_string()));
        assert!(p.helper_args.contains(&"vorbisFile.dll".to_string()));
        assert_eq!(p.helper_args.iter().filter(|a| *a == "--dll").count(), 1);
    }

    #[test]
    fn parses_helper_events() {
        assert_eq!(HelperEvent::parse_line(r#"{"event":"spawned","pid":42}"#), Some(HelperEvent::Spawned { pid: 42 }));
        assert_eq!(
            HelperEvent::parse_line(r#"{"event":"injected","dll":"C:\\x\\samp.dll","attempt":2}"#),
            Some(HelperEvent::Injected { dll: "C:\\x\\samp.dll".into(), attempt: 2 })
        );
        assert_eq!(
            HelperEvent::parse_line(r#"{"event":"error","stage":"inject","code":5,"message":"Access denied"}"#)
                .unwrap()
                .describe(),
            "error during inject: Access denied (code 5)"
        );
        assert_eq!(HelperEvent::parse_line(r#"{"event":"exit","code":0}"#), Some(HelperEvent::Exit { code: 0 }));
        assert_eq!(HelperEvent::parse_line("wine: something"), None);
        assert_eq!(HelperEvent::parse_line(r#"{"event":"bogus"}"#), None);
        assert_eq!(HelperEvent::Injected { dll: "C:\\a\\b.dll".into(), attempt: 1 }.describe(), "injected b.dll");
    }

    #[tokio::test]
    async fn run_streams_events_from_a_fake_wine() {
        let f = fixture();
        fs::write(
            &f.req.wine.wine,
            "#!/bin/sh\necho '{\"event\":\"spawned\",\"pid\":7}'\necho 'plain text'\necho 'oops' >&2\necho '{\"event\":\"exit\",\"code\":3}'\nexit 0\n",
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&f.req.wine.wine, fs::Permissions::from_mode(0o755)).unwrap();
        let prepared = prepare(&f.req, &f.files, b"helper").unwrap();
        let (tx, mut rx) = mpsc::channel(16);
        let code = run(&prepared, tx).await.unwrap();
        assert_eq!(code, Some(0));
        let mut events = Vec::new();
        while let Some(e) = rx.recv().await {
            events.push(e);
        }
        assert!(events.contains(&HelperEvent::Spawned { pid: 7 }));
        assert!(events.contains(&HelperEvent::Log("plain text".into())));
        assert!(events.contains(&HelperEvent::Log("oops".into())));
        assert!(events.contains(&HelperEvent::Exit { code: 3 }));
        assert_eq!(events.last(), Some(&HelperEvent::Finished(Some(0))));
    }

    #[test]
    fn env_pairs() {
        let m = parse_env_pairs(&["A=1".into(), "bad".into(), " B = x=y".into(), "=v".into()]);
        assert_eq!(m.len(), 2);
        assert_eq!(m["A"], "1");
        assert_eq!(m["B"], " x=y");
    }
}
