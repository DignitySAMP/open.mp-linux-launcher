use crate::input::Input;
use omptui_core::filter::{SortDir, SortKey};
use omptui_core::resources::SampVersion;
use omptui_core::{Server, ServerAddr};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Popup {
    Join(Box<JoinForm>),
    AddServer(Input),
    Filters(FilterForm),
    Settings(SettingsForm),
    ServerSettings(ServerSettingsForm),
    PathPrompt(PathPrompt),
    Help,
    Message { title: String, lines: Vec<String>, error: bool },
    Confirm { title: String, text: String, action: ConfirmAction },
    Launch(LaunchState),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfirmAction {
    ClearRecent,
    RemoveFavorite(ServerAddr),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JoinForm {
    pub server: Server,
    pub nickname: Input,
    pub password: Input,
    pub remember_password: bool,
    pub samp_version: SampVersion,
    // Field order: nickname, password, remember password, SA-MP version, join button.
    pub field: usize,
    pub error: Option<String>,
}

impl JoinForm {
    pub const FIELDS: usize = 5;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilterForm {
    // Rows: three toggles, gamemode, sort key, direction, then the versions and languages.
    pub cursor: usize,
    pub editing: Option<Input>,
    pub versions: Vec<(String, usize)>,
    pub languages: Vec<(String, usize)>,
}

impl FilterForm {
    pub const FIXED: usize = 6;

    pub fn rows(&self) -> usize {
        Self::FIXED + self.versions.len() + self.languages.len()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsRow {
    Nickname,
    GameDir,
    GameExe,
    SampVersion,
    OmpInject,
    WineBinary,
    WinePrefix,
    Env,
    Suspended,
    QuitAfterLaunch,
    QueryLists,
    AutoRefresh,
    Terminal,
    ActionCheckFiles,
    ActionDownloadClient,
    ActionImportClient,
    ActionDetect,
    ActionInitPrefix,
    ActionInstallD3dx9,
    ActionImportUserdata,
    ActionInstallDesktop,
}

impl SettingsRow {
    pub const ALL: [SettingsRow; 21] = [
        SettingsRow::Nickname,
        SettingsRow::GameDir,
        SettingsRow::GameExe,
        SettingsRow::SampVersion,
        SettingsRow::OmpInject,
        SettingsRow::WineBinary,
        SettingsRow::WinePrefix,
        SettingsRow::Env,
        SettingsRow::Suspended,
        SettingsRow::QuitAfterLaunch,
        SettingsRow::QueryLists,
        SettingsRow::AutoRefresh,
        SettingsRow::Terminal,
        SettingsRow::ActionCheckFiles,
        SettingsRow::ActionDownloadClient,
        SettingsRow::ActionImportClient,
        SettingsRow::ActionDetect,
        SettingsRow::ActionInitPrefix,
        SettingsRow::ActionInstallD3dx9,
        SettingsRow::ActionImportUserdata,
        SettingsRow::ActionInstallDesktop,
    ];

    pub fn label(self) -> &'static str {
        match self {
            SettingsRow::Nickname => "Nickname",
            SettingsRow::GameDir => "Game folder",
            SettingsRow::GameExe => "Game executable",
            SettingsRow::SampVersion => "SA-MP client version",
            SettingsRow::OmpInject => "Inject open.mp client",
            SettingsRow::WineBinary => "Wine binary",
            SettingsRow::WinePrefix => "Wine prefix",
            SettingsRow::Env => "Extra environment (K=V ...)",
            SettingsRow::Suspended => "Start game suspended, inject, resume",
            SettingsRow::QuitAfterLaunch => "Quit after launching",
            SettingsRow::QueryLists => "Query servers in lists (ping/players)",
            SettingsRow::AutoRefresh => "Reload the master list every",
            SettingsRow::Terminal => "Terminal for desktop entries",
            SettingsRow::ActionCheckFiles => "▶ Check client files and game",
            SettingsRow::ActionDownloadClient => "▶ Download client files from open.mp",
            SettingsRow::ActionImportClient => "▶ Import client files from a launcher data folder…",
            SettingsRow::ActionDetect => "▶ Auto-detect Wine, prefix, game and client files",
            SettingsRow::ActionInitPrefix => "▶ Create / update the Wine prefix (wineboot)",
            SettingsRow::ActionInstallD3dx9 => "▶ Install d3dx9 into the prefix (winetricks)",
            SettingsRow::ActionImportUserdata => "▶ Import SA-MP favorites (USERDATA.DAT)…",
            SettingsRow::ActionInstallDesktop => "▶ Install desktop entry + omp:// handler",
        }
    }

    pub fn is_text(self) -> bool {
        matches!(
            self,
            SettingsRow::Nickname
                | SettingsRow::GameDir
                | SettingsRow::GameExe
                | SettingsRow::WineBinary
                | SettingsRow::WinePrefix
                | SettingsRow::Env
                | SettingsRow::Terminal
        )
    }

    pub fn is_action(self) -> bool {
        matches!(
            self,
            SettingsRow::ActionCheckFiles
                | SettingsRow::ActionDownloadClient
                | SettingsRow::ActionImportClient
                | SettingsRow::ActionDetect
                | SettingsRow::ActionInitPrefix
                | SettingsRow::ActionInstallD3dx9
                | SettingsRow::ActionImportUserdata
                | SettingsRow::ActionInstallDesktop
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsForm {
    pub cursor: usize,
    pub editing: Option<Input>,
    pub message: Option<String>,
}

impl SettingsForm {
    pub fn row(&self) -> SettingsRow {
        SettingsRow::ALL[self.cursor.min(SettingsRow::ALL.len() - 1)]
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerSettingsForm {
    pub addr: ServerAddr,
    pub name: String,
    pub nickname: Input,
    pub password: Input,
    pub samp_version: Option<SampVersion>,
    pub field: usize,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathAction {
    ImportClientFiles,
    ImportUserdata,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathPrompt {
    pub title: String,
    pub hint: String,
    pub input: Input,
    pub action: PathAction,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LaunchState {
    pub server: String,
    pub lines: Vec<(String, bool)>,
    pub finished: bool,
    pub failed: bool,
    pub game_started: bool,
}

impl LaunchState {
    pub fn push(&mut self, line: impl Into<String>, error: bool) {
        self.lines.push((line.into(), error));
        if self.lines.len() > 200 {
            self.lines.remove(0);
        }
    }
}

pub fn sort_label(key: SortKey, dir: SortDir) -> String {
    if key == SortKey::None { "none".into() } else { format!("{} {}", key.label(), dir.arrow()) }
}

pub fn expand_tilde(s: &str) -> PathBuf {
    if let Some(rest) = s.strip_prefix("~/")
        && let Some(b) = directories::BaseDirs::new()
    {
        return b.home_dir().join(rest);
    }
    PathBuf::from(s)
}
