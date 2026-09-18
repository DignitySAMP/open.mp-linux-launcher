use clap::Parser;
use omp_tui::app::App;
use omp_tui::cli::Cli;
use omptui_core::launch::{self, LaunchRequest};
use omptui_core::resources::{ClientFiles, SampVersion};
use omptui_core::store::{Lists, Paths, Settings};
use omptui_core::validation::resolve_host;
use omptui_core::wine::WineEnv;
use std::process::ExitCode;
use tokio::sync::mpsc;

fn main() -> ExitCode {
    let cli = Cli::parse();
    let paths = Paths::detect();
    omp_tui::init_logging(&paths);
    let rt = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("could not start async runtime: {e}");
            return ExitCode::FAILURE;
        }
    };
    match rt.block_on(run(cli, paths)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::FAILURE
        }
    }
}

async fn run(cli: Cli, paths: Paths) -> anyhow::Result<()> {
    let first_run = !paths.settings_file().exists();
    let mut settings =
        Settings::load(&paths).map_err(|e| anyhow::anyhow!("{}: {e}", paths.settings_file().display()))?;
    let lists = Lists::load(&paths).map_err(|e| anyhow::anyhow!("{}: {e}", paths.lists_file().display()))?;
    let files = ClientFiles::new(&paths.data_dir);
    apply_cli_overrides(&mut settings, &cli);

    if cli.install_desktop {
        let bin = omp_tui::desktop::install_binary()?;
        println!("installed {}", bin.display());
        for line in omp_tui::desktop::install_desktop_entries(&bin, settings.terminal.as_deref())? {
            println!("{line}");
        }
        return Ok(());
    }

    let api_base = omp_tui::api_url(&settings, &cli);
    let (tx, rx) = mpsc::unbounded_channel();
    let svc = omp_tui::build_services(&api_base, tx).await?;

    if cli.dump {
        let servers = svc.api.servers().await?;
        use std::io::Write;
        let mut out = std::io::stdout().lock();
        let _ = writeln!(out, "{}", serde_json::to_string_pretty(&servers)?);
        return Ok(());
    }

    let mut report = Vec::new();
    if first_run {
        report = omp_tui::setup::auto_detect(&mut settings, &files);
        settings.save(&paths)?;
        report.extend(first_run_downloads(&settings, &files, &svc.api).await);
    }

    if cli.is_direct_launch() {
        return direct_launch(&cli, &settings, &files, &svc.api).await;
    }

    let mut app = App::new(svc, paths, settings, lists);
    if cli.check {
        for line in app.check_report() {
            println!("{line}");
        }
        return Ok(());
    }
    if !report.is_empty() {
        let mut lines = vec!["First run: settings were filled in automatically.".to_string(), String::new()];
        lines.extend(report);
        lines.push(String::new());
        lines.push("Review them under Settings (,) and press ? for the key list.".into());
        app.message("Welcome", lines, false);
    }
    if let Some(link) = &cli.link {
        match omptui_core::deeplink::parse(link) {
            Ok(l) => app.pending_link = Some(l),
            Err(_) if !link.contains("://") => {
                app.pending_link = Some(omptui_core::deeplink::DeepLink {
                    scheme: "omp".into(),
                    host: link.rsplit_once(':').map(|(h, _)| h).unwrap_or(link).to_string(),
                    port: link.rsplit_once(':').and_then(|(_, p)| p.parse().ok()).unwrap_or(7777),
                    password: cli.password.clone(),
                })
            }
            Err(e) => anyhow::bail!("{link}: {e}"),
        }
    }
    omp_tui::run_tui(app, rx).await?;
    Ok(())
}

fn apply_cli_overrides(settings: &mut Settings, cli: &Cli) {
    if let Some(p) = &cli.wine {
        settings.wine_binary = Some(p.clone());
    }
    if let Some(p) = &cli.prefix {
        settings.wine_prefix = Some(p.clone());
    }
    if let Some(p) = &cli.gamepath {
        settings.game_dir = Some(p.clone());
    }
    if cli.no_omp {
        settings.omp_inject = false;
    }
    if let Some(v) = cli.samp_version.as_deref().and_then(SampVersion::from_id) {
        settings.samp_version = v;
    }
}

async fn direct_launch(
    cli: &Cli,
    settings: &Settings,
    files: &ClientFiles,
    api: &omptui_core::api::ApiClient,
) -> anyhow::Result<()> {
    let host = cli.host.clone().unwrap();
    let port = cli.port.unwrap();
    let (addr, _) = tokio::task::spawn_blocking(move || resolve_host(&format!("{host}:{port}"))).await??;
    let wine = settings
        .wine_binary
        .clone()
        .or_else(|| omptui_core::wine::discover_wine().into_iter().next().map(|w| w.path))
        .ok_or_else(|| anyhow::anyhow!("no Wine binary found; pass --wine"))?;
    let game_dir = cli.gamepath.clone().unwrap();
    let prefix = settings.wine_prefix.clone().or_else(|| prefix_containing(&game_dir)).ok_or_else(|| {
        anyhow::anyhow!("no Wine prefix configured and {} is not inside one; pass --prefix", game_dir.display())
    })?;
    // The official launcher uses the samp.dll from the game folder in CLI mode, do the same.
    let mut samp_version = settings.samp_version;
    if files.samp_dll(samp_version).is_some_and(|p| !p.is_file()) && game_dir.join("samp.dll").is_file() {
        eprintln!("samp.dll {} is not installed, using the one in the game folder", samp_version.label());
        samp_version = SampVersion::Custom;
    }
    let req = LaunchRequest {
        addr,
        nickname: cli.name.clone().unwrap(),
        password: cli.password.clone(),
        game_dir,
        game_exe: settings.game_exe.clone(),
        samp_version,
        omp_inject: settings.omp_inject,
        wine: WineEnv { wine, prefix, extra_env: settings.env.clone() },
        create_suspended: settings.create_suspended,
        wait_for_module: settings.wait_for_module.clone(),
    };
    if omptui_core::download::missing_for(files, req.samp_version, req.omp_inject) {
        println!("Client files missing, fetching them from open.mp…");
        let mut progress = |line: String| println!("  {line}");
        omptui_core::download::download_client_files(files, api, &omp_tui::assets_url(), &mut progress).await?;
    }
    let prepared = launch::prepare(&req, files, omp_tui::HELPER_EXE)?;
    for w in &prepared.warnings {
        eprintln!("warning: {w}");
    }
    println!("{}", prepared.preview());
    let code = launch::run_printing(&prepared).await?;
    if code != Some(0) {
        anyhow::bail!("helper exited with {code:?}");
    }
    Ok(())
}

fn prefix_containing(path: &std::path::Path) -> Option<std::path::PathBuf> {
    path.ancestors().find(|p| p.join("drive_c").is_dir() && p.join("system.reg").is_file()).map(Into::into)
}

// Runs before the UI is up, so progress goes to stdout.
async fn first_run_downloads(
    settings: &Settings,
    files: &ClientFiles,
    api: &omptui_core::api::ApiClient,
) -> Vec<String> {
    let mut report = Vec::new();
    let have_client =
        files.omp_client_dll().is_file() && files.samp_dll(settings.samp_version).is_some_and(|p| p.is_file());
    if !have_client {
        println!("Fetching the SA-MP and open.mp client files from open.mp (one time)…");
        let mut progress = |line: String| println!("  {line}");
        match omptui_core::download::download_client_files(files, api, &omp_tui::assets_url(), &mut progress).await {
            Ok(lines) => report.extend(lines),
            Err(e) => report.push(format!("Client files could not be downloaded: {e}. Retry from Settings.")),
        }
    }
    if let Some(prefix) = &settings.wine_prefix
        && let Some(wine) = &settings.wine_binary
    {
        let pfx = omptui_core::wine::Prefix::new(prefix);
        if pfx.exists() && !pfx.has_arial() {
            println!("Installing Arial into the Wine prefix with winetricks (one time)…");
            let env = WineEnv { wine: wine.clone(), prefix: prefix.clone(), extra_env: settings.env.clone() };
            match omp_tui::install_arial(&env).await {
                Ok(()) => report.push("Arial installed into the prefix with winetricks".into()),
                Err(e) => report.push(format!("Arial (SA-MP crashes on pause without it) is missing: {e}")),
            }
        }
    }
    if settings.omp_inject
        && let Some(prefix) = &settings.wine_prefix
        && let Some(wine) = &settings.wine_binary
    {
        let pfx = omptui_core::wine::Prefix::new(prefix);
        let has_d3dx = pfx.system_dirs().iter().any(|d| d.join("d3dx9_25.dll").is_file());
        if pfx.exists() && !has_d3dx {
            let env = WineEnv { wine: wine.clone(), prefix: prefix.clone(), extra_env: settings.env.clone() };
            match omp_tui::install_d3dx9(&env).await {
                Ok(()) => report.push("d3dx9 installed into the prefix with winetricks".into()),
                Err(e) => report.push(format!("d3dx9 (needed by the open.mp client) is missing: {e}")),
            }
        }
    }
    report
}
