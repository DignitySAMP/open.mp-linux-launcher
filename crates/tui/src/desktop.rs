use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const APP_DESKTOP: &str = "omp-tui.desktop";
pub const URL_DESKTOP: &str = "omp-tui-url.desktop";

const TERMINALS: &[(&str, &[&str])] = &[
    ("foot", &[]),
    ("kitty", &[]),
    ("alacritty", &["-e"]),
    ("wezterm", &["start", "--"]),
    ("ghostty", &["-e"]),
    ("gnome-terminal", &["--"]),
    ("konsole", &["-e"]),
    ("xterm", &["-e"]),
];

fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).map(|d| d.join(name)).find(|p| p.is_file())
}

pub fn terminal_command(preferred: Option<&str>) -> Option<String> {
    if let Some(p) = preferred.map(str::trim).filter(|p| !p.is_empty()) {
        let name = p.split_whitespace().next().unwrap_or(p);
        if p.split_whitespace().count() > 1 {
            return Some(p.to_owned());
        }
        if let Some((_, args)) = TERMINALS.iter().find(|(n, _)| *n == name) {
            return Some(std::iter::once(name).chain(args.iter().copied()).collect::<Vec<_>>().join(" "));
        }
        return Some(format!("{name} -e"));
    }
    TERMINALS
        .iter()
        .find(|(n, _)| which(n).is_some())
        .map(|(n, args)| std::iter::once(*n).chain(args.iter().copied()).collect::<Vec<_>>().join(" "))
}

pub fn applications_dir() -> PathBuf {
    directories::BaseDirs::new()
        .map(|b| b.data_local_dir().join("applications"))
        .unwrap_or_else(|| PathBuf::from("~/.local/share/applications"))
}

pub fn local_bin_dir() -> PathBuf {
    directories::BaseDirs::new()
        .and_then(|b| b.executable_dir().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("~/.local/bin"))
}

pub fn install_binary() -> io::Result<PathBuf> {
    let src = std::env::current_exe()?;
    let dir = local_bin_dir();
    fs::create_dir_all(&dir)?;
    let dst = dir.join("omp-tui");
    if fs::canonicalize(&src).ok() == fs::canonicalize(&dst).ok() {
        return Ok(dst);
    }
    let tmp = dir.join(".omp-tui.tmp");
    fs::copy(&src, &tmp)?;
    fs::rename(&tmp, &dst)?;
    Ok(dst)
}

fn quote(s: &str) -> String {
    if s.chars().any(|c| c.is_whitespace() || c == '"') {
        format!("\"{}\"", s.replace('"', "\\\""))
    } else {
        s.to_owned()
    }
}

pub fn install_desktop_entries(bin: &Path, terminal: Option<&str>) -> io::Result<Vec<String>> {
    let mut report = Vec::new();
    let term = terminal_command(terminal)
        .ok_or_else(|| io::Error::other("no terminal emulator found (set one in Settings)"))?;
    let dir = applications_dir();
    fs::create_dir_all(&dir)?;
    let bin = quote(&bin.to_string_lossy());
    let app = format!(
        "[Desktop Entry]\nType=Application\nName=omp-tui\nGenericName=GTA San Andreas multiplayer launcher\n\
         Comment=Browse and join open.mp / SA-MP servers\nExec={term} {bin}\nTerminal=false\n\
         Icon=applications-games\nCategories=Game;\nKeywords=samp;openmp;gta;\nStartupNotify=false\n"
    );
    fs::write(dir.join(APP_DESKTOP), app)?;
    report.push(format!("wrote {}", dir.join(APP_DESKTOP).display()));
    let url = format!(
        "[Desktop Entry]\nType=Application\nName=omp-tui (join server)\nNoDisplay=true\n\
         Exec={term} {bin} %u\nTerminal=false\nIcon=applications-games\n\
         MimeType=x-scheme-handler/omp;x-scheme-handler/samp;\nCategories=Game;\n"
    );
    fs::write(dir.join(URL_DESKTOP), url)?;
    report.push(format!("wrote {}", dir.join(URL_DESKTOP).display()));
    if which("update-desktop-database").is_some() {
        let ok = Command::new("update-desktop-database").arg(&dir).status().map(|s| s.success()).unwrap_or(false);
        report.push(format!("update-desktop-database: {}", if ok { "ok" } else { "failed" }));
    }
    if which("xdg-mime").is_some() {
        for scheme in ["x-scheme-handler/omp", "x-scheme-handler/samp"] {
            let ok = Command::new("xdg-mime")
                .args(["default", URL_DESKTOP, scheme])
                .status()
                .map(|s| s.success())
                .unwrap_or(false);
            report.push(format!("xdg-mime default {scheme}: {}", if ok { "ok" } else { "failed" }));
        }
    } else {
        report.push("xdg-mime not found: register the omp:// and samp:// handlers manually".into());
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_selection() {
        assert_eq!(terminal_command(Some("foot")).as_deref(), Some("foot"));
        assert_eq!(terminal_command(Some("alacritty")).as_deref(), Some("alacritty -e"));
        assert_eq!(terminal_command(Some("myterm --run")).as_deref(), Some("myterm --run"));
        assert_eq!(terminal_command(Some("unknownterm")).as_deref(), Some("unknownterm -e"));
        let _ = terminal_command(None);
    }

    #[test]
    fn quoting() {
        assert_eq!(quote("/usr/bin/omp-tui"), "/usr/bin/omp-tui");
        assert_eq!(quote("/my dir/omp-tui"), "\"/my dir/omp-tui\"");
    }
}
