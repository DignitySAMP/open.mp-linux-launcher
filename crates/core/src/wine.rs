use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use tokio::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WineBinary {
    pub path: PathBuf,
    pub label: String,
}

fn home() -> Option<PathBuf> {
    directories::BaseDirs::new().map(|b| b.home_dir().to_path_buf())
}

// proton keeps wine in files/bin, old proton in dist/bin
const RUNNER_BINS: [&str; 3] = ["bin/wine", "files/bin/wine", "dist/bin/wine"];

fn push_unique(out: &mut Vec<WineBinary>, path: PathBuf, label: String) {
    let canon = fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
    if !out.iter().any(|w| fs::canonicalize(&w.path).unwrap_or_else(|_| w.path.clone()) == canon) {
        out.push(WineBinary { path, label });
    }
}

fn glob_runners(base: &Path, label: &str, out: &mut Vec<WineBinary>) {
    let Ok(rd) = fs::read_dir(base) else { return };
    let mut entries: Vec<_> = rd.flatten().map(|e| e.path()).collect();
    entries.sort();
    entries.reverse(); // newest-looking versions first
    for dir in entries {
        if let Some(bin) = RUNNER_BINS.iter().map(|b| dir.join(b)).find(|b| b.is_file()) {
            let name = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            push_unique(out, bin, format!("{label} {name}"));
        }
    }
}

fn runner_dirs(home: &Path, root: &Path) -> Vec<(PathBuf, &'static str)> {
    vec![
        (home.join(".local/share/lutris/runners/wine"), "lutris"),
        (home.join(".local/share/bottles/runners"), "bottles"),
        (home.join(".var/app/com.usebottles.bottles/data/bottles/runners"), "bottles (flatpak)"),
        (home.join(".local/share/Steam/compatibilitytools.d"), "steam compat tool"),
        (home.join(".steam/root/compatibilitytools.d"), "steam compat tool"),
        (root.join("usr/share/steam/compatibilitytools.d"), "steam compat tool"),
    ]
}

pub fn discover_wine() -> Vec<WineBinary> {
    discover_wine_in(home().as_deref(), Path::new("/"), std::env::var_os("PATH").as_deref())
}

// NOTE: root is "/" unless a test passes a temp dir
pub fn discover_wine_in(home: Option<&Path>, root: &Path, path_var: Option<&std::ffi::OsStr>) -> Vec<WineBinary> {
    let mut out = Vec::new();
    for (p, label) in [
        ("opt/wine-cachyos/bin/wine", "wine-cachyos-opt"),
        ("opt/wine-staging/bin/wine", "wine-staging"),
        ("opt/wine-tkg/bin/wine", "wine-tkg"),
    ] {
        let p = root.join(p);
        if p.is_file() {
            out.push(WineBinary { path: p, label: label.into() });
        }
    }
    if let Some(path) = path_var {
        for dir in std::env::split_paths(path) {
            let p = dir.join("wine");
            if p.is_file() && !out.iter().any(|w| w.path == p) {
                out.push(WineBinary { path: p, label: "system wine".into() });
                break;
            }
        }
    }
    if let Some(h) = home {
        for (dir, label) in runner_dirs(h, root) {
            glob_runners(&dir, label, &mut out);
        }
    }
    out
}

pub async fn wine_version(wine: &Path) -> Option<String> {
    let out = Command::new(wine).arg("--version").env("WINEDEBUG", "-all").output().await.ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    if s.is_empty() { None } else { Some(s) }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prefix {
    pub path: PathBuf,
}

impl Prefix {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn exists(&self) -> bool {
        self.path.join("system.reg").is_file()
    }

    pub fn arch(&self) -> Option<String> {
        let f = fs::File::open(self.path.join("system.reg")).ok()?;
        use std::io::BufRead;
        for line in io::BufReader::new(f).lines().take(20).map_while(Result::ok) {
            if let Some(a) = line.strip_prefix("#arch=") {
                return Some(a.trim().to_owned());
            }
        }
        None
    }

    pub fn drive_c(&self) -> PathBuf {
        self.path.join("drive_c")
    }

    // syswow64 comes first because 32-bit DLLs go there on a 64-bit prefix.
    pub fn system_dirs(&self) -> Vec<PathBuf> {
        ["syswow64", "system32"].iter().map(|d| self.drive_c().join("windows").join(d)).filter(|p| p.is_dir()).collect()
    }

    // NOTE: samp crashes on esc (device reset) without the real arial.ttf, font replacements don't help
    pub fn has_arial(&self) -> bool {
        fs::read_dir(self.drive_c().join("windows").join("Fonts"))
            .is_ok_and(|rd| rd.flatten().any(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case("arial.ttf")))
    }

    fn drives(&self) -> Vec<(char, PathBuf)> {
        let dd = self.path.join("dosdevices");
        let mut out = Vec::new();
        let Ok(rd) = fs::read_dir(&dd) else { return out };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let mut chars = name.chars();
            let (Some(letter), Some(':'), None) = (chars.next(), chars.next(), chars.next()) else { continue };
            let Ok(target) = fs::read_link(e.path()) else { continue };
            let abs = if target.is_absolute() { target } else { dd.join(target) };
            if let Ok(canon) = fs::canonicalize(&abs) {
                out.push((letter.to_ascii_uppercase(), canon));
            }
        }
        out
    }

    // Resolved through the dosdevices symlinks, so winepath is not needed.
    pub fn to_windows_path(&self, linux: &Path) -> Option<String> {
        let (existing, rest) = split_existing(linux);
        let canon = fs::canonicalize(&existing).ok()?;
        let mut best: Option<(char, PathBuf)> = None;
        for (letter, root) in self.drives() {
            if canon.starts_with(&root)
                && best.as_ref().is_none_or(|(_, r)| root.components().count() > r.components().count())
            {
                best = Some((letter, root));
            }
        }
        let (letter, root) = best?;
        let mut tail: Vec<String> = canon
            .strip_prefix(&root)
            .ok()?
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect();
        tail.extend(rest.components().map(|c| c.as_os_str().to_string_lossy().into_owned()));
        let mut s = format!("{letter}:");
        for t in tail {
            s.push('\\');
            s.push_str(&t);
        }
        if s.len() == 2 {
            s.push('\\');
        }
        Some(s)
    }

    pub fn from_windows_path(&self, windows: &str) -> Option<PathBuf> {
        let mut chars = windows.chars();
        let letter = chars.next()?.to_ascii_uppercase();
        if chars.next() != Some(':') {
            return None;
        }
        let rest = chars.as_str().trim_start_matches(['\\', '/']);
        let root = self.drives().into_iter().find(|(l, _)| *l == letter)?.1;
        let mut p = root;
        for seg in rest.split(['\\', '/']).filter(|s| !s.is_empty()) {
            p = match_case_insensitive(&p, seg);
        }
        Some(p)
    }
}

fn split_existing(p: &Path) -> (PathBuf, PathBuf) {
    let mut cur = p.to_path_buf();
    let mut tail = Vec::new();
    while !cur.exists() {
        match (cur.file_name().map(|f| f.to_os_string()), cur.parent().map(Path::to_path_buf)) {
            (Some(name), Some(parent)) => {
                tail.push(name);
                cur = parent;
            }
            _ => break,
        }
    }
    tail.reverse();
    (cur, tail.iter().collect())
}

fn match_case_insensitive(dir: &Path, seg: &str) -> PathBuf {
    let exact = dir.join(seg);
    if exact.exists() {
        return exact;
    }
    if let Ok(rd) = fs::read_dir(dir) {
        for e in rd.flatten() {
            if e.file_name().to_string_lossy().eq_ignore_ascii_case(seg) {
                return e.path();
            }
        }
    }
    exact
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WineEnv {
    pub wine: PathBuf,
    pub prefix: PathBuf,
    pub extra_env: BTreeMap<String, String>,
}

impl WineEnv {
    pub fn prefix(&self) -> Prefix {
        Prefix::new(&self.prefix)
    }

    pub fn command(&self) -> Command {
        let mut c = Command::new(&self.wine);
        c.env("WINEPREFIX", &self.prefix);
        if !self.extra_env.contains_key("WINEDEBUG") {
            c.env("WINEDEBUG", "-all");
        }
        for (k, v) in &self.extra_env {
            c.env(k, v);
        }
        c
    }

    // mscoree and mshtml are disabled so wineboot does not ask about Mono and Gecko.
    pub async fn init_prefix(&self) -> Result<String, String> {
        fs::create_dir_all(&self.prefix).map_err(|e| e.to_string())?;
        let mut c = self.command();
        c.arg("wineboot").arg("-u");
        if !self.extra_env.contains_key("WINEDLLOVERRIDES") {
            c.env("WINEDLLOVERRIDES", "mscoree,mshtml=");
        }
        let out = c.output().await.map_err(|e| format!("could not run {}: {e}", self.wine.display()))?;
        let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        if out.status.success() && self.prefix().exists() { Ok(text) } else { Err(text) }
    }

    pub async fn winepath(&self, linux: &Path) -> Option<String> {
        let out = self.command().arg("winepath").arg("-w").arg(linux).output().await.ok()?;
        let s = String::from_utf8_lossy(&out.stdout).trim().to_owned();
        if out.status.success() && !s.is_empty() { Some(s) } else { None }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    fn fake_prefix() -> (tempfile::TempDir, Prefix) {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().to_path_buf();
        fs::create_dir_all(p.join("drive_c/Program Files (x86)/Rockstar Games/GTA San Andreas")).unwrap();
        fs::create_dir_all(p.join("drive_c/windows/syswow64")).unwrap();
        fs::create_dir_all(p.join("drive_c/windows/system32")).unwrap();
        fs::create_dir_all(p.join("dosdevices")).unwrap();
        symlink("../drive_c", p.join("dosdevices/c:")).unwrap();
        symlink("/", p.join("dosdevices/z:")).unwrap();
        fs::write(
            p.join("system.reg"),
            "WINE REGISTRY Version 2\n;; All keys relative to \\\\Machine\n\n#arch=win64\n",
        )
        .unwrap();
        (d, Prefix::new(&p))
    }

    #[test]
    fn prefix_basics() {
        let (_d, pfx) = fake_prefix();
        assert!(pfx.exists());
        assert_eq!(pfx.arch().as_deref(), Some("win64"));
        assert_eq!(pfx.system_dirs().len(), 2);
        assert!(pfx.system_dirs()[0].ends_with("syswow64"));
        assert!(!Prefix::new("/nonexistent").exists());
    }

    #[test]
    fn arial_lookup_ignores_case() {
        let (_d, pfx) = fake_prefix();
        assert!(!pfx.has_arial());
        let fonts = pfx.drive_c().join("windows/Fonts");
        fs::create_dir_all(&fonts).unwrap();
        fs::write(fonts.join("arialbd.ttf"), b"").unwrap();
        assert!(!pfx.has_arial());
        fs::write(fonts.join("Arial.TTF"), b"").unwrap();
        assert!(pfx.has_arial());
    }

    #[test]
    fn path_translation() {
        let (_d, pfx) = fake_prefix();
        let game = pfx.drive_c().join("Program Files (x86)/Rockstar Games/GTA San Andreas");
        assert_eq!(pfx.to_windows_path(&game).unwrap(), "C:\\Program Files (x86)\\Rockstar Games\\GTA San Andreas");
        assert_eq!(
            pfx.to_windows_path(&game.join("gta_sa.exe")).unwrap(),
            "C:\\Program Files (x86)\\Rockstar Games\\GTA San Andreas\\gta_sa.exe"
        );
        assert_eq!(pfx.to_windows_path(Path::new("/tmp")).unwrap(), "Z:\\tmp");
        assert_eq!(pfx.to_windows_path(Path::new("/")).unwrap(), "Z:\\");
        let back = pfx.from_windows_path("c:\\program files (x86)\\ROCKSTAR GAMES\\GTA San Andreas").unwrap();
        assert_eq!(fs::canonicalize(back).unwrap(), fs::canonicalize(&game).unwrap());
        assert_eq!(pfx.from_windows_path("Z:\\tmp").unwrap(), PathBuf::from("/tmp"));
        assert!(pfx.from_windows_path("nope").is_none());
    }

    #[test]
    fn command_sets_prefix_and_debug() {
        let env = WineEnv {
            wine: PathBuf::from("/usr/bin/wine"),
            prefix: PathBuf::from("/tmp/pfx"),
            extra_env: [("DXVK_HUD".to_string(), "fps".to_string())].into_iter().collect(),
        };
        let c = env.command();
        let std = c.as_std();
        let envs: BTreeMap<_, _> = std
            .get_envs()
            .map(|(k, v)| (k.to_string_lossy().into_owned(), v.map(|v| v.to_string_lossy().into_owned())))
            .collect();
        assert_eq!(envs["WINEPREFIX"].as_deref(), Some("/tmp/pfx"));
        assert_eq!(envs["WINEDEBUG"].as_deref(), Some("-all"));
        assert_eq!(envs["DXVK_HUD"].as_deref(), Some("fps"));
    }

    fn touch(p: &Path) {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, b"").unwrap();
    }

    #[test]
    fn discovers_proton_layouts_and_system_compat_tools() {
        let d = tempfile::tempdir().unwrap();
        let (home, root) = (d.path().join("home"), d.path().join("root"));
        touch(&home.join(".local/share/lutris/runners/wine/wine-ge-8-26/bin/wine"));
        touch(&home.join(".local/share/lutris/runners/wine/proton-cachyos/files/bin/wine"));
        touch(&home.join(".local/share/Steam/compatibilitytools.d/GE-Proton9-1/files/bin/wine"));
        touch(&home.join(".local/share/Steam/compatibilitytools.d/Proton-5.0-GE/dist/bin/wine"));
        touch(&home.join(".local/share/Steam/compatibilitytools.d/not-a-runner/readme.txt"));
        touch(&root.join("usr/share/steam/compatibilitytools.d/proton-cachyos-slr/files/bin/wine"));
        // ~/.steam/root is a symlink to ~/.local/share/Steam, no doubles
        fs::create_dir_all(home.join(".steam")).unwrap();
        symlink(home.join(".local/share/Steam"), home.join(".steam/root")).unwrap();

        let found = discover_wine_in(Some(&home), &root, None);
        let labels: Vec<&str> = found.iter().map(|w| w.label.as_str()).collect();
        assert_eq!(
            labels,
            [
                "lutris wine-ge-8-26",
                "lutris proton-cachyos",
                "steam compat tool Proton-5.0-GE",
                "steam compat tool GE-Proton9-1",
                "steam compat tool proton-cachyos-slr",
            ]
        );
        assert!(found[1].path.ends_with("proton-cachyos/files/bin/wine"));
        assert!(found[4].path.starts_with(&root));
    }

    #[test]
    fn opt_and_path_wine_come_first() {
        let d = tempfile::tempdir().unwrap();
        let (home, root) = (d.path().join("home"), d.path().join("root"));
        touch(&root.join("opt/wine-cachyos/bin/wine"));
        touch(&root.join("usr/bin/wine"));
        touch(&home.join(".local/share/lutris/runners/wine/wine-ge/bin/wine"));
        let path = std::env::join_paths([root.join("usr/local/bin"), root.join("usr/bin")]).unwrap();
        let found = discover_wine_in(Some(&home), &root, Some(&path));
        let labels: Vec<&str> = found.iter().map(|w| w.label.as_str()).collect();
        assert_eq!(labels, ["wine-cachyos-opt", "system wine", "lutris wine-ge"]);
    }

    #[test]
    fn discover_does_not_panic() {
        let _ = discover_wine();
    }
}
