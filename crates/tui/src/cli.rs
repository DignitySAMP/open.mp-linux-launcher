//! Same flags as the official launcher (-h host, -p port, -P password, -n name, -g gamepath,
//! --no-omp) plus the Linux bits. -h is taken by host, so help is --help only.

use clap::{ArgAction, Parser};
use std::path::PathBuf;

#[derive(Debug, Parser, Clone, Default)]
#[command(
    name = "omp-tui",
    version,
    about = "Terminal launcher for open.mp / SA-MP servers (Linux + Wine)",
    disable_help_flag = true,
    disable_version_flag = true
)]
pub struct Cli {
    /// Print help.
    #[arg(long, action = ArgAction::Help)]
    pub help: Option<bool>,

    /// Print version.
    #[arg(short = 'V', long, action = ArgAction::Version)]
    pub version: Option<bool>,

    /// Server host or IP.
    #[arg(short = 'h', long)]
    pub host: Option<String>,

    /// Server port.
    #[arg(short = 'p', long)]
    pub port: Option<u16>,

    /// Server password.
    #[arg(short = 'P', long)]
    pub password: Option<String>,

    /// Nickname.
    #[arg(short = 'n', long)]
    pub name: Option<String>,

    /// Game folder (Linux path to the folder containing gta_sa.exe).
    #[arg(short = 'g', long)]
    pub gamepath: Option<PathBuf>,

    /// Do not inject the open.mp client (plain SA-MP).
    #[arg(long)]
    pub no_omp: bool,

    /// SA-MP client version: 037R1 037R2 037R3 037R31 037R4 037R5 03DL custom.
    #[arg(long)]
    pub samp_version: Option<String>,

    /// Wine binary.
    #[arg(long)]
    pub wine: Option<PathBuf>,

    /// Wine prefix.
    #[arg(long)]
    pub prefix: Option<PathBuf>,

    /// Master list base URL (default https://api.open.mp).
    #[arg(long)]
    pub api_url: Option<String>,

    /// Install the .desktop entries (app + omp:// / samp:// handler) and a copy of this binary in
    /// ~/.local/bin, then exit.
    #[arg(long)]
    pub install_desktop: bool,

    /// Check game path, Wine, prefix and client files, print a report and exit.
    #[arg(long)]
    pub check: bool,

    /// Do not touch the terminal; print the server list as JSON and exit.
    #[arg(long)]
    pub dump: bool,

    /// An omp:// or samp:// link, or ip:port, to join.
    #[arg(value_name = "LINK")]
    pub link: Option<String>,
}

impl Cli {
    /// the official launcher skips its UI when all four are given
    pub fn is_direct_launch(&self) -> bool {
        self.host.is_some() && self.port.is_some() && self.name.is_some() && self.gamepath.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_official_style_flags() {
        let c = Cli::parse_from([
            "omp-tui",
            "-h",
            "1.2.3.4",
            "-p",
            "7777",
            "-n",
            "Carl",
            "-g",
            "/games/gta",
            "--no-omp",
            "-P",
            "pw",
        ]);
        assert!(c.is_direct_launch());
        assert_eq!(c.host.as_deref(), Some("1.2.3.4"));
        assert_eq!(c.port, Some(7777));
        assert!(c.no_omp);
        assert_eq!(c.password.as_deref(), Some("pw"));
        let c = Cli::parse_from(["omp-tui", "omp://1.2.3.4:7777"]);
        assert!(!c.is_direct_launch());
        assert_eq!(c.link.as_deref(), Some("omp://1.2.3.4:7777"));
    }

    #[test]
    fn help_is_long_only() {
        let r = Cli::try_parse_from(["omp-tui", "--help"]);
        assert!(r.is_err());
        assert_eq!(r.unwrap_err().kind(), clap::error::ErrorKind::DisplayHelp);
    }
}
