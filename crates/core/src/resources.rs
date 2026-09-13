// The data directory uses the same layout as the official launcher (https://github.com/openmultiplayer/launcher):
// omp/omp-client.dll, samp/<version>/samp.dll and samp/shared/.

use md5::{Digest, Md5};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum SampVersion {
    #[serde(rename = "037R1")]
    R1,
    #[serde(rename = "037R2")]
    R2,
    #[serde(rename = "037R3")]
    R3,
    #[serde(rename = "037R31")]
    R31,
    #[serde(rename = "037R4")]
    R4,
    #[serde(rename = "037R5")]
    #[default]
    R5,
    #[serde(rename = "03DL")]
    DL,
    // Uses the samp.dll that is already in the game folder.
    #[serde(rename = "custom")]
    Custom,
}

impl SampVersion {
    pub const ALL: [SampVersion; 8] = [
        SampVersion::R1,
        SampVersion::R2,
        SampVersion::R3,
        SampVersion::R31,
        SampVersion::R4,
        SampVersion::R5,
        SampVersion::DL,
        SampVersion::Custom,
    ];

    // Same identifiers as the official launcher uses in its settings.
    pub fn id(self) -> &'static str {
        match self {
            SampVersion::R1 => "037R1",
            SampVersion::R2 => "037R2",
            SampVersion::R3 => "037R3",
            SampVersion::R31 => "037R31",
            SampVersion::R4 => "037R4",
            SampVersion::R5 => "037R5",
            SampVersion::DL => "03DL",
            SampVersion::Custom => "custom",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|v| v.id().eq_ignore_ascii_case(id))
    }

    pub fn label(self) -> &'static str {
        match self {
            SampVersion::R1 => "0.3.7-R1",
            SampVersion::R2 => "0.3.7-R2",
            SampVersion::R3 => "0.3.7-R3",
            SampVersion::R31 => "0.3.7-R3-1",
            SampVersion::R4 => "0.3.7-R4",
            SampVersion::R5 => "0.3.7-R5",
            SampVersion::DL => "0.3.DL",
            SampVersion::Custom => "custom (samp.dll in game folder)",
        }
    }

    pub fn dir_name(self) -> Option<&'static str> {
        match self {
            SampVersion::Custom => None,
            other => Some(other.label()),
        }
    }

    // R1 to R4 come from the official launcher (https://github.com/openmultiplayer/launcher/blob/master/src/constants/app.ts).
    // R5 and DL were hashed from the files that launcher downloads.
    pub fn dll_md5(self) -> Option<&'static str> {
        Some(match self {
            SampVersion::R1 => "1d22eaa2605717ddf215f68e861de378",
            SampVersion::R2 => "074241172174f9f2f93afce3261f97ad",
            SampVersion::R3 => "61dfd96e0bb01e2fd8cd27e0df18e653",
            SampVersion::R31 => "08cf4166d916e314ed3ee8cff2f13cca",
            SampVersion::R4 => "7b3a5b379848eda9f9e26f633515a77d",
            SampVersion::R5 => "5ba5f0be7af99dfd03fb39e88a970a2b",
            SampVersion::DL => "449e4f985215ffb5bffadf23551c0d50",
            SampVersion::Custom => return None,
        })
    }

    pub fn next(self) -> Self {
        let i = Self::ALL.iter().position(|v| *v == self).unwrap_or(0);
        Self::ALL[(i + 1) % Self::ALL.len()]
    }

    pub fn prev(self) -> Self {
        let i = Self::ALL.iter().position(|v| *v == self).unwrap_or(0);
        Self::ALL[(i + Self::ALL.len() - 1) % Self::ALL.len()]
    }
}

#[derive(Debug, Clone, Copy)]
pub struct SharedFile {
    pub rel: &'static str,
    pub md5: Option<&'static str>,
}

// Contents of samp_clients.7z from assets.open.mp, hashed after extraction.
pub const SHARED_FILES: &[SharedFile] = &[
    SharedFile { rel: "bass.dll", md5: Some("8f5b9b73d33e8c99202b5058cb6dce51") },
    SharedFile { rel: "gtaweap3.ttf", md5: Some("59cbae9fd42a9a4eea90af7f81e5e734") },
    SharedFile { rel: "mouse.png", md5: Some("337ddcbe53be7dd8032fb8f6fe1b607b") },
    SharedFile { rel: "rcon.exe", md5: Some("3f4821cda1de6d7d10654e5537b4df6e") },
    SharedFile { rel: "samp.saa", md5: Some("833af65bc94eea6f8503900ef597ad51") },
    SharedFile { rel: "sampaux3.ttf", md5: Some("6a03a32076e76f6c1720cad6c6ea6915") },
    SharedFile { rel: "sampgui.png", md5: Some("1423c18dfa2064d967b397227960b93d") },
    SharedFile { rel: "samp_debug.exe", md5: Some("2c00c60a5511c3a41a70296fd1879067") },
    SharedFile { rel: "SAMP/blanktex.txd", md5: Some("00dc42d499f5ca6059e4683fd761f032") },
    SharedFile { rel: "SAMP/CUSTOM.ide", md5: Some("d41d8cd98f00b204e9800998ecf8427e") },
    SharedFile { rel: "SAMP/custom.img", md5: Some("8fc7f2ec79402a952d5b896b710b3a41") },
    SharedFile { rel: "SAMP/samaps.txd", md5: Some("e0fdfd9fbe272baa9284e275fb426610") },
    SharedFile { rel: "SAMP/SAMP.ide", md5: Some("9fc8a6769f18d3daceabbbed8632c68e") },
    SharedFile { rel: "SAMP/SAMP.img", md5: Some("c85eb523407583f602a2f48df572081f") },
    SharedFile { rel: "SAMP/SAMP.ipl", md5: Some("f5fc70efa49b43fc48fc71e3c680b50e") },
    SharedFile { rel: "SAMP/SAMPCOL.img", md5: Some("eb690e98b644fa584be6917d48ee6cbc") },
];

// gta_sa.exe 1.0 US is the only build SA-MP and open.mp run on.
pub const GTA_SA_10US_SIZE: u64 = 14_383_616;
pub const GTA_SA_10US_MD5: &str = "170b3a9108687b26da2d8901c6948a18";

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn md5_file(path: &Path) -> io::Result<String> {
    let mut f = fs::File::open(path)?;
    let mut h = Md5::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(hex(&h.finalize()))
}

pub fn md5_bytes(bytes: &[u8]) -> String {
    hex(&Md5::digest(bytes))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileState {
    Ok,
    Missing,
    Mismatch { actual: String },
    Unverified,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileStatus {
    pub label: String,
    pub path: PathBuf,
    pub state: FileState,
}

impl FileStatus {
    pub fn is_ok(&self) -> bool {
        matches!(self.state, FileState::Ok | FileState::Unverified)
    }
}

fn status_of(label: impl Into<String>, path: PathBuf, expected_md5: Option<&str>) -> FileStatus {
    let state = if !path.is_file() {
        FileState::Missing
    } else {
        match (expected_md5, md5_file(&path)) {
            (Some(exp), Ok(actual)) if actual == exp => FileState::Ok,
            (Some(_), Ok(actual)) => FileState::Mismatch { actual },
            (Some(_), Err(_)) => FileState::Missing,
            (None, _) => FileState::Unverified,
        }
    };
    FileStatus { label: label.into(), path, state }
}

#[derive(Debug, Clone)]
pub struct ClientFiles {
    pub data_dir: PathBuf,
}

impl ClientFiles {
    pub fn new(data_dir: impl Into<PathBuf>) -> Self {
        Self { data_dir: data_dir.into() }
    }

    pub fn omp_client_dll(&self) -> PathBuf {
        self.data_dir.join("omp").join("omp-client.dll")
    }

    pub fn samp_dll(&self, v: SampVersion) -> Option<PathBuf> {
        v.dir_name().map(|d| self.data_dir.join("samp").join(d).join("samp.dll"))
    }

    pub fn shared_dir(&self) -> PathBuf {
        self.data_dir.join("samp").join("shared")
    }

    pub fn helper_dir(&self) -> PathBuf {
        self.data_dir.join("bin")
    }

    pub fn check(&self, version: SampVersion, omp: bool) -> Vec<FileStatus> {
        let mut out = Vec::new();
        if let Some(p) = self.samp_dll(version) {
            out.push(status_of(format!("samp.dll {}", version.label()), p, version.dll_md5()));
        }
        if omp {
            out.push(status_of("omp-client.dll", self.omp_client_dll(), None));
        }
        if version != SampVersion::Custom {
            for f in SHARED_FILES {
                out.push(status_of(format!("shared/{}", f.rel), self.shared_dir().join(f.rel), f.md5));
            }
        }
        out
    }

    pub fn available_versions(&self) -> Vec<SampVersion> {
        SampVersion::ALL
            .into_iter()
            .filter(|v| match self.samp_dll(*v) {
                Some(p) => status_of("", p, v.dll_md5()).state == FileState::Ok,
                None => true,
            })
            .collect()
    }

    // src is a mp.open.launcher directory from the official launcher, or another omp-tui data dir.
    pub fn import_from(&self, src: &Path) -> io::Result<ImportReport> {
        let mut report = ImportReport::default();
        let src = if src.join("mp.open.launcher").is_dir() { src.join("mp.open.launcher") } else { src.to_path_buf() };
        let omp_src = src.join("omp").join("omp-client.dll");
        if omp_src.is_file() {
            copy_file(&omp_src, &self.omp_client_dll())?;
            report.copied.push("omp/omp-client.dll".into());
        } else {
            report.missing.push("omp/omp-client.dll".into());
        }
        for v in SampVersion::ALL {
            let Some(dir) = v.dir_name() else { continue };
            let s = src.join("samp").join(dir).join("samp.dll");
            if s.is_file() {
                let dst = self.samp_dll(v).unwrap();
                copy_file(&s, &dst)?;
                let st = status_of("", dst, v.dll_md5());
                match st.state {
                    FileState::Ok => report.copied.push(format!("samp/{dir}/samp.dll")),
                    FileState::Mismatch { actual } => {
                        report
                            .copied
                            .push(format!("samp/{dir}/samp.dll (checksum {actual} differs from the known one)"));
                    }
                    _ => {}
                }
            } else {
                report.missing.push(format!("samp/{dir}/samp.dll"));
            }
        }
        for f in SHARED_FILES {
            let s = src.join("samp").join("shared").join(f.rel);
            if s.is_file() {
                copy_file(&s, &self.shared_dir().join(f.rel))?;
                report.copied.push(format!("samp/shared/{}", f.rel));
            } else {
                report.missing.push(format!("samp/shared/{}", f.rel));
            }
        }
        Ok(report)
    }

    // Existing files are not overwritten, the official launcher behaves the same way.
    pub fn ensure_shared_in_game_dir(&self, game_dir: &Path) -> io::Result<Vec<String>> {
        let mut copied = Vec::new();
        for f in SHARED_FILES {
            let dst = game_dir.join(f.rel);
            if dst.is_file() {
                continue;
            }
            let src = self.shared_dir().join(f.rel);
            if !src.is_file() {
                return Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("{} is missing from both the game folder and {}", f.rel, self.shared_dir().display()),
                ));
            }
            copy_file(&src, &dst)?;
            copied.push(f.rel.to_string());
        }
        Ok(copied)
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ImportReport {
    pub copied: Vec<String>,
    pub missing: Vec<String>,
}

fn copy_file(src: &Path, dst: &Path) -> io::Result<()> {
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = dst.with_extension("tmp-import");
    fs::copy(src, &tmp)?;
    fs::rename(&tmp, dst)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameExeInfo {
    pub path: PathBuf,
    pub size: u64,
    pub md5: String,
    pub is_10_us: bool,
}

pub fn inspect_game_exe(path: &Path) -> io::Result<GameExeInfo> {
    let size = fs::metadata(path)?.len();
    let md5 = md5_file(path)?;
    let is_10_us = size == GTA_SA_10US_SIZE && md5 == GTA_SA_10US_MD5;
    Ok(GameExeInfo { path: path.to_path_buf(), size, md5, is_10_us })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_ids_roundtrip() {
        for v in SampVersion::ALL {
            assert_eq!(SampVersion::from_id(v.id()), Some(v));
            let json = serde_json::to_string(&v).unwrap();
            assert_eq!(json, format!("\"{}\"", v.id()));
            assert_eq!(serde_json::from_str::<SampVersion>(&json).unwrap(), v);
        }
        assert_eq!(SampVersion::Custom.next(), SampVersion::R1);
        assert_eq!(SampVersion::R1.prev(), SampVersion::Custom);
        assert!(SampVersion::Custom.dll_md5().is_none());
    }

    #[test]
    fn check_reports_missing_then_ok_after_import() {
        let src = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();
        let cf = ClientFiles::new(data.path());
        let st = cf.check(SampVersion::R5, true);
        assert!(st.iter().all(|s| s.state == FileState::Missing));
        assert_eq!(st.len(), 2 + SHARED_FILES.len());
        assert_eq!(cf.available_versions(), vec![SampVersion::Custom]);

        let s = src.path().join("mp.open.launcher");
        fs::create_dir_all(s.join("samp/0.3.7-R5")).unwrap();
        fs::write(s.join("samp/0.3.7-R5/samp.dll"), b"fake").unwrap();
        fs::create_dir_all(s.join("omp")).unwrap();
        fs::write(s.join("omp/omp-client.dll"), b"fake omp").unwrap();
        for f in SHARED_FILES {
            let p = s.join("samp/shared").join(f.rel);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(&p, b"").unwrap(); // only CUSTOM.ide is really empty, the rest will mismatch
        }
        let rep = cf.import_from(src.path()).unwrap();
        assert!(rep.copied.iter().any(|c| c.starts_with("samp/0.3.7-R5/samp.dll (checksum")));
        assert!(rep.copied.contains(&"omp/omp-client.dll".to_string()));
        assert!(rep.missing.contains(&"samp/0.3.7-R1/samp.dll".to_string()));

        let st = cf.check(SampVersion::R5, true);
        let by_label = |l: &str| st.iter().find(|s| s.label == l).unwrap().clone();
        assert!(matches!(by_label("samp.dll 0.3.7-R5").state, FileState::Mismatch { .. }));
        assert_eq!(by_label("omp-client.dll").state, FileState::Unverified);
        assert_eq!(by_label("shared/SAMP/CUSTOM.ide").state, FileState::Ok);
        assert!(matches!(by_label("shared/bass.dll").state, FileState::Mismatch { .. }));

        let st = cf.check(SampVersion::Custom, true);
        assert_eq!(st.len(), 1);
        let st = cf.check(SampVersion::Custom, false);
        assert!(st.is_empty());
    }

    #[test]
    fn ensure_shared_copies_only_missing() {
        let data = tempfile::tempdir().unwrap();
        let game = tempfile::tempdir().unwrap();
        let cf = ClientFiles::new(data.path());
        for f in SHARED_FILES {
            let p = cf.shared_dir().join(f.rel);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(&p, f.rel).unwrap();
        }
        fs::write(game.path().join("bass.dll"), b"keep me").unwrap();
        let copied = cf.ensure_shared_in_game_dir(game.path()).unwrap();
        assert_eq!(copied.len(), SHARED_FILES.len() - 1);
        assert_eq!(fs::read(game.path().join("bass.dll")).unwrap(), b"keep me");
        assert_eq!(fs::read_to_string(game.path().join("SAMP/SAMP.img")).unwrap(), "SAMP/SAMP.img");
        assert!(cf.ensure_shared_in_game_dir(game.path()).unwrap().is_empty());
        fs::remove_file(game.path().join("SAMP/SAMP.ipl")).unwrap();
        fs::remove_file(cf.shared_dir().join("SAMP/SAMP.ipl")).unwrap();
        let e = cf.ensure_shared_in_game_dir(game.path()).unwrap_err();
        assert!(e.to_string().contains("SAMP/SAMP.ipl"));
    }

    #[test]
    fn game_exe_inspection() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("gta_sa.exe");
        fs::write(&p, b"not the real thing").unwrap();
        let i = inspect_game_exe(&p).unwrap();
        assert!(!i.is_10_us);
        assert_eq!(i.size, 18);
        assert_eq!(i.md5, md5_bytes(b"not the real thing"));
    }
}
