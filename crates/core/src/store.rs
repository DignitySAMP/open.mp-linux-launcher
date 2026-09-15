// settings.toml lives in XDG_CONFIG_HOME/omp-tui, lists.toml in XDG_DATA_HOME/omp-tui.
// OMPTUI_CONFIG_DIR, OMPTUI_DATA_DIR and OMPTUI_STATE_DIR override the locations.

use crate::filter::Filters;
use crate::model::{Server, ServerAddr};
use crate::resources::SampVersion;
use crate::secrets::Keyring;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub const APP_NAME: &str = "omp-tui";
pub const MAX_RECENT_NICKNAMES: usize = 5;
pub const MAX_RECENT_SERVERS: usize = 50;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    pub config_dir: PathBuf,
    pub data_dir: PathBuf,
    pub state_dir: PathBuf,
}

impl Paths {
    pub fn detect() -> Self {
        let dirs = directories::ProjectDirs::from("", "", APP_NAME);
        let env = |k: &str| std::env::var_os(k).map(PathBuf::from);
        let home_fallback = |sub: &str| {
            directories::BaseDirs::new()
                .map(|b| b.home_dir().join(sub).join(APP_NAME))
                .unwrap_or_else(|| PathBuf::from(sub).join(APP_NAME))
        };
        Self {
            config_dir: env("OMPTUI_CONFIG_DIR")
                .or_else(|| dirs.as_ref().map(|d| d.config_dir().to_path_buf()))
                .unwrap_or_else(|| home_fallback(".config")),
            data_dir: env("OMPTUI_DATA_DIR")
                .or_else(|| dirs.as_ref().map(|d| d.data_dir().to_path_buf()))
                .unwrap_or_else(|| home_fallback(".local/share")),
            state_dir: env("OMPTUI_STATE_DIR")
                .or_else(|| dirs.as_ref().and_then(|d| d.state_dir().map(Path::to_path_buf)))
                .unwrap_or_else(|| home_fallback(".local/state")),
        }
    }

    pub fn under(root: &Path) -> Self {
        Self { config_dir: root.join("config"), data_dir: root.join("data"), state_dir: root.join("state") }
    }

    pub fn settings_file(&self) -> PathBuf {
        self.config_dir.join("settings.toml")
    }

    pub fn lists_file(&self) -> PathBuf {
        self.data_dir.join("lists.toml")
    }

    pub fn log_file(&self) -> PathBuf {
        self.state_dir.join("omp-tui.log")
    }
}

pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let dir = path.parent().ok_or_else(|| io::Error::other("path has no parent"))?;
    fs::create_dir_all(dir)?;
    let tmp = dir.join(format!(
        ".{}.tmp-{}",
        path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
        std::process::id()
    ));
    fs::write(&tmp, bytes)?;
    fs::rename(&tmp, path)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub nickname: String,
    pub recent_nicknames: Vec<String>,
    pub game_dir: Option<PathBuf>,
    pub game_exe: String,
    pub samp_version: SampVersion,
    pub omp_inject: bool,
    pub wine_binary: Option<PathBuf>,
    pub wine_prefix: Option<PathBuf>,
    pub env: BTreeMap<String, String>,
    // Start the game suspended, inject, then resume. This is what samp.exe does.
    pub create_suspended: bool,
    pub wait_for_module: Option<String>,
    pub quit_after_launch: bool,
    pub api_url: Option<String>,
    pub terminal: Option<String>,
    pub filters: Filters,
    pub query_lists: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            nickname: String::new(),
            recent_nicknames: Vec::new(),
            game_dir: None,
            game_exe: "gta_sa.exe".into(),
            samp_version: SampVersion::R5,
            omp_inject: true,
            wine_binary: None,
            wine_prefix: None,
            env: BTreeMap::new(),
            create_suspended: true,
            wait_for_module: None,
            quit_after_launch: false,
            api_url: None,
            terminal: None,
            filters: Filters::default(),
            query_lists: true,
        }
    }
}

impl Settings {
    pub fn load(paths: &Paths) -> io::Result<Self> {
        load_toml(&paths.settings_file())
    }

    pub fn save(&self, paths: &Paths) -> io::Result<()> {
        save_toml(&paths.settings_file(), self)
    }

    pub fn use_nickname(&mut self, name: &str) {
        let name = name.trim();
        if name.is_empty() {
            return;
        }
        self.recent_nicknames.retain(|n| !n.eq_ignore_ascii_case(name));
        self.recent_nicknames.insert(0, name.to_owned());
        self.recent_nicknames.truncate(MAX_RECENT_NICKNAMES);
        self.nickname = name.to_owned();
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ServerSettings {
    pub nickname: Option<String>,
    pub samp_version: Option<SampVersion>,
    pub password: Option<String>,
}

impl ServerSettings {
    pub fn is_empty(&self) -> bool {
        self.nickname.is_none() && self.samp_version.is_none() && self.password.is_none()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Lists {
    pub favorites: Vec<Server>,
    pub recent: Vec<Server>,
    pub server_settings: BTreeMap<String, ServerSettings>,
}

impl Lists {
    pub fn load(paths: &Paths) -> io::Result<Self> {
        let mut lists: Lists = load_toml(&paths.lists_file())?;
        let stored: Vec<String> = lists.server_settings.values().filter_map(|s| s.password.clone()).collect();
        if stored.iter().any(|p| Keyring::is_encrypted(p)) {
            let keyring = Keyring::open(&paths.data_dir)?;
            for s in lists.server_settings.values_mut() {
                if let Some(p) = &s.password {
                    // a password that no longer decrypts is dropped rather than used as-is
                    s.password = keyring.decrypt(p);
                }
            }
        }
        Ok(lists)
    }

    pub fn save(&self, paths: &Paths) -> io::Result<()> {
        let mut server_settings = self.server_settings.clone();
        if server_settings.values().any(|s| s.password.is_some()) {
            let keyring = Keyring::open(&paths.data_dir)?;
            for s in server_settings.values_mut() {
                if let Some(p) = &s.password {
                    s.password = Some(keyring.encrypt(p));
                }
            }
        }
        let stripped = Lists {
            favorites: self.favorites.iter().map(strip_volatile).collect(),
            recent: self.recent.iter().map(strip_volatile).collect(),
            server_settings,
        };
        save_toml(&paths.lists_file(), &stripped)
    }

    pub fn is_favorite(&self, addr: ServerAddr) -> bool {
        self.favorites.iter().any(|s| s.addr == Some(addr))
    }

    pub fn toggle_favorite(&mut self, server: &Server) -> bool {
        let Some(addr) = server.addr else { return false };
        if let Some(i) = self.favorites.iter().position(|s| s.addr == Some(addr)) {
            self.favorites.remove(i);
            false
        } else {
            self.favorites.push(strip_volatile(server));
            true
        }
    }

    pub fn move_favorite(&mut self, index: usize, delta: isize) -> Option<usize> {
        let len = self.favorites.len();
        if index >= len {
            return None;
        }
        let target = index as isize + delta;
        if target < 0 || target >= len as isize {
            return None;
        }
        self.favorites.swap(index, target as usize);
        Some(target as usize)
    }

    pub fn push_recent(&mut self, server: &Server) {
        let Some(addr) = server.addr else { return };
        self.recent.retain(|s| s.addr != Some(addr));
        self.recent.insert(0, strip_volatile(server));
        self.recent.truncate(MAX_RECENT_SERVERS);
    }

    pub fn settings_for(&self, addr: ServerAddr) -> ServerSettings {
        self.server_settings.get(&addr.to_string()).cloned().unwrap_or_default()
    }

    pub fn set_settings_for(&mut self, addr: ServerAddr, s: ServerSettings) {
        if s.is_empty() {
            self.server_settings.remove(&addr.to_string());
        } else {
            self.server_settings.insert(addr.to_string(), s);
        }
    }
}

fn strip_volatile(s: &Server) -> Server {
    Server {
        addr: s.addr,
        host_label: s.host_label.clone(),
        info: s.info.clone(),
        rules: BTreeMap::new(),
        player_list: Vec::new(),
        extra: None,
        ping: None,
        queried: false,
    }
}

fn load_toml<T: Default + serde::de::DeserializeOwned>(path: &Path) -> io::Result<T> {
    match fs::read_to_string(path) {
        Ok(text) => toml::from_str(&text).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string())),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(T::default()),
        Err(e) => Err(e),
    }
}

fn save_toml<T: Serialize>(path: &Path, value: &T) -> io::Result<()> {
    let text = toml::to_string_pretty(value).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
    write_atomic(path, text.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ServerInfo;

    fn srv(ip: &str, name: &str) -> Server {
        let mut s = Server::with_addr(format!("{ip}:7777").parse().unwrap());
        s.info = ServerInfo { hostname: name.into(), ..Default::default() };
        s.ping = Some(12);
        s.rules.insert("k".into(), "v".into());
        s
    }

    #[test]
    fn settings_roundtrip_and_defaults() {
        let d = tempfile::tempdir().unwrap();
        let paths = Paths::under(d.path());
        let s = Settings::load(&paths).unwrap();
        assert_eq!(s, Settings::default());
        let mut s = s;
        s.use_nickname("Carl");
        s.use_nickname("Ryder");
        s.use_nickname("carl");
        s.game_dir = Some(PathBuf::from("/games/gta"));
        s.env.insert("DXVK_HUD".into(), "1".into());
        s.save(&paths).unwrap();
        let back = Settings::load(&paths).unwrap();
        assert_eq!(back, s);
        assert_eq!(back.recent_nicknames, vec!["carl", "Ryder"]);
        assert_eq!(back.nickname, "carl");
        for n in 0..10 {
            s.use_nickname(&format!("n{n}"));
        }
        assert_eq!(s.recent_nicknames.len(), MAX_RECENT_NICKNAMES);
        assert_eq!(s.recent_nicknames[0], "n9");
    }

    #[test]
    fn corrupt_settings_is_an_error_not_a_reset() {
        let d = tempfile::tempdir().unwrap();
        let paths = Paths::under(d.path());
        fs::create_dir_all(&paths.config_dir).unwrap();
        fs::write(paths.settings_file(), "nickname = [broken").unwrap();
        assert!(Settings::load(&paths).is_err());
    }

    #[test]
    fn lists_roundtrip_and_ops() {
        let d = tempfile::tempdir().unwrap();
        let paths = Paths::under(d.path());
        let mut l = Lists::load(&paths).unwrap();
        let a = srv("1.1.1.1", "A");
        let b = srv("2.2.2.2", "B");
        assert!(l.toggle_favorite(&a));
        assert!(l.toggle_favorite(&b));
        assert!(l.is_favorite(a.addr.unwrap()));
        assert_eq!(l.move_favorite(1, -1), Some(0));
        assert_eq!(l.favorites[0].info.hostname, "B");
        assert_eq!(l.move_favorite(0, -1), None);
        l.push_recent(&a);
        l.push_recent(&b);
        l.push_recent(&a);
        assert_eq!(l.recent.iter().map(|s| s.info.hostname.as_str()).collect::<Vec<_>>(), vec!["A", "B"]);
        l.set_settings_for(a.addr.unwrap(), ServerSettings { nickname: Some("x".into()), ..Default::default() });
        l.save(&paths).unwrap();
        let back = Lists::load(&paths).unwrap();
        assert_eq!(back, l);
        l.favorites[0].ping = Some(50);
        l.favorites[0].rules.insert("x".into(), "y".into());
        l.save(&paths).unwrap();
        let back = Lists::load(&paths).unwrap();
        assert!(back.favorites[0].rules.is_empty());
        assert_eq!(back.favorites[0].ping, None);
        assert_eq!(back.settings_for(a.addr.unwrap()).nickname.as_deref(), Some("x"));
        let mut l = back;
        l.set_settings_for(a.addr.unwrap(), ServerSettings::default());
        assert!(l.server_settings.is_empty());
        assert!(!l.toggle_favorite(&a));
        assert_eq!(l.favorites.len(), 1);
    }

    #[test]
    fn passwords_are_encrypted_on_disk() {
        let d = tempfile::tempdir().unwrap();
        let paths = Paths::under(d.path());
        let mut l = Lists::default();
        let addr = srv("1.1.1.1", "A").addr.unwrap();
        l.set_settings_for(addr, ServerSettings { password: Some("hunter2".into()), ..Default::default() });
        l.save(&paths).unwrap();
        let text = fs::read_to_string(paths.lists_file()).unwrap();
        assert!(!text.contains("hunter2"), "{text}");
        assert!(text.contains("enc1:"));
        assert!(paths.data_dir.join("secret.key").is_file());
        let back = Lists::load(&paths).unwrap();
        assert_eq!(back.settings_for(addr).password.as_deref(), Some("hunter2"));

        // files written before encryption existed still load, and get encrypted on the next save
        fs::write(paths.lists_file(), "[server_settings.\"2.2.2.2:7777\"]\npassword = \"plain\"\n").unwrap();
        let legacy = Lists::load(&paths).unwrap();
        assert_eq!(legacy.settings_for("2.2.2.2:7777".parse().unwrap()).password.as_deref(), Some("plain"));
        legacy.save(&paths).unwrap();
        assert!(!fs::read_to_string(paths.lists_file()).unwrap().contains("plain"));

        // a value encrypted with another key is dropped instead of being passed to the game
        fs::remove_file(paths.data_dir.join("secret.key")).unwrap();
        let other = Lists::load(&paths).unwrap();
        assert_eq!(other.settings_for("2.2.2.2:7777".parse().unwrap()).password, None);
    }

    #[test]
    fn atomic_write_leaves_no_temp_files() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("sub").join("f.txt");
        write_atomic(&p, b"hello").unwrap();
        write_atomic(&p, b"world").unwrap();
        assert_eq!(fs::read_to_string(&p).unwrap(), "world");
        assert_eq!(fs::read_dir(d.path().join("sub")).unwrap().count(), 1);
    }

    #[test]
    fn env_overrides_paths() {
        let d = tempfile::tempdir().unwrap();
        unsafe {
            std::env::set_var("OMPTUI_CONFIG_DIR", d.path().join("c"));
            std::env::set_var("OMPTUI_DATA_DIR", d.path().join("d"));
            std::env::set_var("OMPTUI_STATE_DIR", d.path().join("s"));
        }
        let p = Paths::detect();
        unsafe {
            std::env::remove_var("OMPTUI_CONFIG_DIR");
            std::env::remove_var("OMPTUI_DATA_DIR");
            std::env::remove_var("OMPTUI_STATE_DIR");
        }
        assert_eq!(p.config_dir, d.path().join("c"));
        assert_eq!(p.lists_file(), d.path().join("d").join("lists.toml"));
        assert_eq!(p.log_file(), d.path().join("s").join("omp-tui.log"));
    }
}
