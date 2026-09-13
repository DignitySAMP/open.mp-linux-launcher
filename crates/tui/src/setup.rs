use omptui_core::import::read_prefix_registry;
use omptui_core::resources::ClientFiles;
use omptui_core::store::Settings;
use omptui_core::wine::{Prefix, discover_wine};
use std::fs;
use std::path::{Path, PathBuf};

const GAME_SUBDIRS: &[&str] = &[
    "Program Files (x86)/Rockstar Games/GTA San Andreas",
    "Program Files/Rockstar Games/GTA San Andreas",
    "Program Files (x86)/GTA San Andreas",
    "Program Files/GTA San Andreas",
    "GTA San Andreas",
    "Games/GTA San Andreas",
];

fn home() -> Option<PathBuf> {
    directories::BaseDirs::new().map(|b| b.home_dir().to_path_buf())
}

fn children(dir: &Path) -> Vec<PathBuf> {
    fs::read_dir(dir).map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect()).unwrap_or_default()
}

pub fn discover_prefixes() -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();
    if let Some(h) = home() {
        roots.push(h.join(".wine"));
        let share = h.join(".local/share");
        for c in children(&share) {
            roots.push(c.clone());
            roots.extend(children(&c));
        }
        roots.extend(children(&h.join(".local/share/wineprefixes")));
        roots.extend(children(&h.join("Games")));
        roots.extend(children(&h.join(".local/share/bottles/bottles")));
        roots.extend(children(&h.join(".var/app/com.usebottles.bottles/data/bottles/bottles")));
    }
    if let Some(p) = std::env::var_os("WINEPREFIX") {
        roots.insert(0, PathBuf::from(p));
    }
    let mut out = Vec::new();
    for r in roots {
        if r.join("drive_c").is_dir() && r.join("system.reg").is_file() && !out.contains(&r) {
            out.push(r);
        }
    }
    out
}

pub fn find_game_in_prefix(prefix: &Path, exe_name: &str) -> Option<PathBuf> {
    let pfx = Prefix::new(prefix);
    if let Ok(reg) = read_prefix_registry(prefix)
        && let Some(dir) = reg.game_dir_windows().and_then(|w| pfx.from_windows_path(&w))
        && dir.join(exe_name).is_file()
    {
        return Some(dir);
    }
    GAME_SUBDIRS.iter().map(|s| pfx.drive_c().join(s)).find(|d| d.join(exe_name).is_file())
}

pub fn find_launcher_data_in_prefix(prefix: &Path) -> Option<PathBuf> {
    for user in children(&prefix.join("drive_c").join("users")) {
        let p = user.join("AppData/Local/mp.open.launcher");
        if p.join("omp").join("omp-client.dll").is_file() || p.join("samp").is_dir() {
            return Some(p);
        }
    }
    None
}

pub fn auto_detect(settings: &mut Settings, files: &ClientFiles) -> Vec<String> {
    let mut report = Vec::new();
    if settings.wine_binary.is_none() {
        if let Some(w) = discover_wine().into_iter().next() {
            report.push(format!("Wine: {} ({})", w.path.display(), w.label));
            settings.wine_binary = Some(w.path);
        } else {
            report.push("Wine: not found (install wine or set the binary in Settings)".into());
        }
    }
    let exe = settings.game_exe.clone();
    if settings.game_dir.is_none() || settings.wine_prefix.is_none() {
        let candidates: Vec<PathBuf> = match &settings.wine_prefix {
            Some(p) => vec![p.clone()],
            None => discover_prefixes(),
        };
        let mut found = None;
        for p in &candidates {
            if let Some(dir) = find_game_in_prefix(p, &exe) {
                found = Some((p.clone(), dir));
                break;
            }
        }
        match found {
            Some((prefix, dir)) => {
                report.push(format!("Game: {}", dir.display()));
                report.push(format!("Prefix: {}", prefix.display()));
                settings.game_dir = Some(dir);
                settings.wine_prefix = Some(prefix);
            }
            None => report.push(format!("Game: {exe} not found in any Wine prefix (set the game folder in Settings)")),
        }
    }
    if let Some(prefix) = settings.wine_prefix.clone() {
        let prefix = prefix.as_path();
        if settings.nickname.is_empty()
            && let Ok(reg) = read_prefix_registry(prefix)
            && let Some(n) = reg.player_name.filter(|n| !n.is_empty())
        {
            report.push(format!("Nickname from SA-MP registry: {n}"));
            settings.use_nickname(&n);
        }
        if !files.omp_client_dll().is_file() {
            if let Some(src) = find_launcher_data_in_prefix(prefix) {
                match files.import_from(&src) {
                    Ok(rep) => {
                        report.push(format!("Imported {} client files from {}", rep.copied.len(), src.display()))
                    }
                    Err(e) => report.push(format!("Import from {} failed: {e}", src.display())),
                }
            } else {
                report.push(
                    "Client files (samp.dll, omp-client.dll): not found; use Settings > Import client files".into(),
                );
            }
        }
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    #[test]
    fn finds_game_in_prefix() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path();
        fs::create_dir_all(p.join("dosdevices")).unwrap();
        symlink("../drive_c", p.join("dosdevices/c:")).unwrap();
        fs::write(p.join("system.reg"), "#arch=win64\n").unwrap();
        let game = p.join("drive_c/Program Files (x86)/Rockstar Games/GTA San Andreas");
        fs::create_dir_all(&game).unwrap();
        assert!(find_game_in_prefix(p, "gta_sa.exe").is_none());
        fs::write(game.join("gta_sa.exe"), b"MZ").unwrap();
        assert_eq!(find_game_in_prefix(p, "gta_sa.exe").unwrap(), game);
        let other = p.join("drive_c/Elsewhere");
        fs::create_dir_all(&other).unwrap();
        fs::write(other.join("gta_sa.exe"), b"MZ").unwrap();
        fs::write(
            p.join("user.reg"),
            "[Software\\\\SAMP] 1\n\"gta_sa_exe\"=\"C:\\\\Elsewhere\\\\gta_sa.exe\"\n\"PlayerName\"=\"Sweet\"\n",
        )
        .unwrap();
        assert_eq!(find_game_in_prefix(p, "gta_sa.exe").unwrap(), other);
        assert!(find_launcher_data_in_prefix(p).is_none());
        let data = p.join("drive_c/users/me/AppData/Local/mp.open.launcher/omp");
        fs::create_dir_all(&data).unwrap();
        fs::write(data.join("omp-client.dll"), b"x").unwrap();
        assert!(find_launcher_data_in_prefix(p).unwrap().ends_with("mp.open.launcher"));

        let files = ClientFiles::new(d.path().join("data"));
        let mut s = Settings {
            wine_prefix: Some(p.to_path_buf()),
            wine_binary: Some(PathBuf::from("/bin/true")),
            ..Default::default()
        };
        let rep = auto_detect(&mut s, &files);
        assert_eq!(s.game_dir.as_deref(), Some(other.as_path()));
        assert_eq!(s.nickname, "Sweet");
        assert!(files.omp_client_dll().is_file());
        assert!(rep.iter().any(|l| l.starts_with("Imported")), "{rep:?}");
    }
}
