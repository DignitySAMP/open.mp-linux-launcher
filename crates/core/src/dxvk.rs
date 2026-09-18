use crate::download::DownloadError;
use crate::wine::{Prefix, WineEnv};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const RELEASES_URL: &str = "https://api.github.com/repos/doitsujin/dxvk/releases/latest";
const OVERRIDES_KEY: &str = r"HKCU\Software\Wine\DllOverrides";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum D3d9Dll {
    Missing,
    Wine,
    Dxvk,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct D3dStack {
    pub dll: D3d9Dll,
    pub dll_override: Option<String>,
}

impl D3dStack {
    pub fn uses_dxvk(&self) -> bool {
        self.dll == D3d9Dll::Dxvk && self.dll_override.as_deref().is_some_and(|o| o.starts_with("native"))
    }

    pub fn describe(&self) -> String {
        let over = self.dll_override.as_deref().unwrap_or("builtin");
        match self.dll {
            D3d9Dll::Dxvk if self.uses_dxvk() => format!("DXVK (d3d9={over})"),
            D3d9Dll::Dxvk => format!("wined3d (DXVK d3d9.dll is there but the override is {over})"),
            D3d9Dll::Wine | D3d9Dll::Missing => format!("wined3d (d3d9={over})"),
            D3d9Dll::Other => format!("unknown d3d9.dll (d3d9={over})"),
        }
    }
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w.eq_ignore_ascii_case(needle))
}

impl Prefix {
    fn d3d9_path(&self) -> Option<PathBuf> {
        self.system_dirs().first().map(|d| d.join("d3d9.dll"))
    }

    // wine dlls carry "Wine builtin DLL" in the DOS stub
    pub fn d3d9_dll(&self) -> D3d9Dll {
        let Some(bytes) = self.d3d9_path().and_then(|p| fs::read(p).ok()) else { return D3d9Dll::Missing };
        if contains(&bytes[..bytes.len().min(0x100)], b"Wine ") {
            D3d9Dll::Wine
        } else if contains(&bytes, b"dxvk") {
            D3d9Dll::Dxvk
        } else {
            D3d9Dll::Other
        }
    }

    // NOTE: lutris writes *d3d9
    pub fn d3d9_override(&self) -> Option<String> {
        let reg = fs::read(self.path.join("user.reg")).ok()?;
        let reg = String::from_utf8_lossy(&reg);
        let mut in_section = false;
        for line in reg.lines() {
            if line.starts_with('[') {
                in_section = line.to_ascii_lowercase().starts_with(r"[software\\wine\\dlloverrides]");
            } else if in_section
                && let Some((name, value)) = line.split_once('=')
                && matches!(name.trim_matches('"').to_ascii_lowercase().as_str(), "d3d9" | "*d3d9")
            {
                return Some(value.trim().trim_matches('"').to_owned());
            }
        }
        None
    }

    pub fn d3d_stack(&self) -> D3dStack {
        D3dStack { dll: self.d3d9_dll(), dll_override: self.d3d9_override() }
    }
}

// NOTE: dxvk without a vulkan driver = black window
pub fn vulkan_available() -> bool {
    let home = directories::BaseDirs::new().map(|b| b.home_dir().to_path_buf());
    vulkan_available_in(Path::new("/"), home.as_deref())
}

pub fn vulkan_available_in(root: &Path, home: Option<&Path>) -> bool {
    let has = |dir: PathBuf, want: &dyn Fn(&str) -> bool| {
        fs::read_dir(dir).is_ok_and(|rd| rd.flatten().any(|e| want(&e.file_name().to_string_lossy())))
    };
    let mut icd_dirs: Vec<PathBuf> =
        ["usr/share/vulkan/icd.d", "usr/local/share/vulkan/icd.d", "etc/vulkan/icd.d"].map(|d| root.join(d)).into();
    icd_dirs.extend(home.map(|h| h.join(".local/share/vulkan/icd.d")));
    let loader_dirs = ["usr/lib", "usr/lib64", "usr/lib32", "usr/lib/x86_64-linux-gnu", "usr/lib/i386-linux-gnu"];
    icd_dirs.into_iter().any(|d| has(d, &|n| n.ends_with(".json")))
        && loader_dirs.iter().any(|d| has(root.join(d), &|n| n.starts_with("libvulkan.so")))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DxvkRelease {
    pub tag: String,
    pub url: String,
    pub sha256: Option<String>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
    digest: Option<String>,
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    assets: Vec<Asset>,
}

fn http() -> Result<reqwest::Client, DownloadError> {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .user_agent(concat!("omp-tui/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| DownloadError::Http { url: String::new(), msg: e.to_string() })
}

// skips dxvk-native, linux build
pub async fn latest_release(releases_url: &str) -> Result<DxvkRelease, DownloadError> {
    let err = |msg: String| DownloadError::Http { url: releases_url.to_owned(), msg };
    let resp = http()?.get(releases_url).send().await.map_err(|e| err(e.to_string()))?;
    if !resp.status().is_success() {
        return Err(err(format!("HTTP {}", resp.status())));
    }
    let r: Release = resp.json().await.map_err(|e| err(e.to_string()))?;
    let asset = r
        .assets
        .into_iter()
        .find(|a| a.name.starts_with("dxvk-") && !a.name.contains("native") && a.name.ends_with(".tar.gz"))
        .ok_or_else(|| err(format!("release {} has no dxvk-*.tar.gz", r.tag_name)))?;
    Ok(DxvkRelease {
        tag: r.tag_name,
        url: asset.browser_download_url,
        sha256: asset.digest.and_then(|d| d.strip_prefix("sha256:").map(str::to_lowercase)),
    })
}

fn x32_d3d9(tarball: &[u8]) -> Result<Vec<u8>, DownloadError> {
    let bad = |e: String| DownloadError::Archive("dxvk tarball".into(), e);
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(tarball));
    for entry in archive.entries().map_err(|e| bad(e.to_string()))? {
        let mut entry = entry.map_err(|e| bad(e.to_string()))?;
        let path = entry.path().map_err(|e| bad(e.to_string()))?.into_owned();
        if path.ends_with("x32/d3d9.dll") {
            let mut dll = Vec::new();
            entry.read_to_end(&mut dll).map_err(|e| bad(e.to_string()))?;
            return Ok(dll);
        }
    }
    Err(bad("no x32/d3d9.dll inside".into()))
}

pub async fn download_d3d9(release: &DxvkRelease) -> Result<Vec<u8>, DownloadError> {
    let err = |msg: String| DownloadError::Http { url: release.url.clone(), msg };
    let resp = http()?.get(&release.url).send().await.map_err(|e| err(e.to_string()))?;
    if !resp.status().is_success() {
        return Err(err(format!("HTTP {}", resp.status())));
    }
    let tarball = resp.bytes().await.map_err(|e| err(e.to_string()))?;
    if let Some(expected) = &release.sha256 {
        let actual: String = Sha256::digest(&tarball).iter().map(|b| format!("{b:02x}")).collect();
        if &actual != expected {
            return Err(DownloadError::Checksum {
                file: "dxvk tarball".into(),
                url: release.url.clone(),
                actual,
                expected: expected.clone(),
            });
        }
    }
    x32_d3d9(&tarball)
}

// gta_sa is 32-bit so only x32/d3d9.dll gets placed. keeps wine's dll as d3d9.dll.wine
pub fn place_d3d9(prefix: &Prefix, dll: &[u8]) -> Result<PathBuf, String> {
    let target = prefix.d3d9_path().ok_or("the prefix has no windows/system32 folder")?;
    let backup = target.with_extension("dll.wine");
    if prefix.d3d9_dll() == D3d9Dll::Wine && !backup.exists() {
        fs::rename(&target, &backup).map_err(|e| format!("{}: {e}", target.display()))?;
    }
    fs::write(&target, dll).map_err(|e| format!("{}: {e}", target.display()))?;
    Ok(target)
}

impl WineEnv {
    // NOTE: goes through reg.exe, a running wineserver overwrites user.reg edits
    pub async fn set_d3d9_native(&self) -> Result<(), String> {
        let out = self
            .command()
            .args(["reg", "add", OVERRIDES_KEY, "/v", "d3d9", "/d", "native", "/f"])
            .output()
            .await
            .map_err(|e| format!("could not run {}: {e}", self.wine.display()))?;
        if !out.status.success() {
            return Err(format!("reg add failed: {}", String::from_utf8_lossy(&out.stderr).trim()));
        }
        // user.reg gets written when wineserver exits
        let server = self.wine.with_file_name("wineserver");
        if server.is_file() {
            let _ = tokio::process::Command::new(server).arg("-w").env("WINEPREFIX", &self.prefix).output().await;
        }
        Ok(())
    }
}

pub async fn install(env: &WineEnv, releases_url: &str) -> Result<Vec<String>, String> {
    let prefix = env.prefix();
    if !prefix.exists() {
        return Err(format!("{} is not an initialised prefix, create it first", env.prefix.display()));
    }
    let release = latest_release(releases_url).await.map_err(|e| e.to_string())?;
    let dll = download_d3d9(&release).await.map_err(|e| e.to_string())?;
    let target = place_d3d9(&prefix, &dll)?;
    env.set_d3d9_native().await?;
    Ok(vec![format!("DXVK {} d3d9.dll installed to {}", release.tag, target.display()), "d3d9 set to native".into()])
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const WINE_DLL: &[u8] =
        b"MZ\x90\0 padding padding padding padding padding padding padding  Wine builtin DLL\0 rest";
    const DXVK_DLL: &[u8] = b"MZ\x90\0 This program cannot be run in DOS mode. ... DXVK: v3.1.1 ...";

    fn prefix(user_reg: &str, d3d9: Option<&[u8]>) -> (tempfile::TempDir, Prefix) {
        let d = tempfile::tempdir().unwrap();
        let sys = d.path().join("drive_c/windows/syswow64");
        fs::create_dir_all(&sys).unwrap();
        fs::create_dir_all(d.path().join("drive_c/windows/system32")).unwrap();
        fs::write(d.path().join("system.reg"), "WINE REGISTRY Version 2\n#arch=win64\n").unwrap();
        fs::write(d.path().join("user.reg"), user_reg).unwrap();
        if let Some(bytes) = d3d9 {
            fs::write(sys.join("d3d9.dll"), bytes).unwrap();
        }
        let p = Prefix::new(d.path());
        (d, p)
    }

    const LUTRIS_REG: &str = "WINE REGISTRY Version 2\n\n[Software\\\\Wine\\\\DllOverrides] 1789301194\n#time=1dd4378524f0622\n\"*d3d11\"=\"native\"\n\"*d3d9\"=\"native\"\n\n[Software\\\\Wine\\\\Other] 1\n\"d3d9\"=\"nope\"\n";
    const FRESH_REG: &str = "WINE REGISTRY Version 2\n\n[Software\\\\Wine\\\\DllOverrides] 1789756031\n\"api-ms-win-crt-heap-l1-1-0\"=\"native,builtin\"\n";

    #[test]
    fn tells_wined3d_from_dxvk() {
        let (_d, fresh) = prefix(FRESH_REG, Some(WINE_DLL));
        assert_eq!(fresh.d3d_stack(), D3dStack { dll: D3d9Dll::Wine, dll_override: None });
        assert!(!fresh.d3d_stack().uses_dxvk());
        assert_eq!(fresh.d3d_stack().describe(), "wined3d (d3d9=builtin)");

        let (_d, lutris) = prefix(LUTRIS_REG, Some(DXVK_DLL));
        assert!(lutris.d3d_stack().uses_dxvk());
        assert_eq!(lutris.d3d_stack().describe(), "DXVK (d3d9=native)");

        // dll copied by hand, no override
        let (_d, half) = prefix(FRESH_REG, Some(DXVK_DLL));
        assert!(!half.d3d_stack().uses_dxvk());
        assert!(half.d3d_stack().describe().starts_with("wined3d (DXVK d3d9.dll is there"));

        let (_d, none) = prefix("", None);
        assert_eq!(none.d3d9_dll(), D3d9Dll::Missing);
    }

    #[test]
    fn placing_keeps_wines_dll() {
        let (d, p) = prefix(FRESH_REG, Some(WINE_DLL));
        let target = place_d3d9(&p, DXVK_DLL).unwrap();
        assert_eq!(target, d.path().join("drive_c/windows/syswow64/d3d9.dll"));
        assert_eq!(p.d3d9_dll(), D3d9Dll::Dxvk);
        assert_eq!(fs::read(d.path().join("drive_c/windows/syswow64/d3d9.dll.wine")).unwrap(), WINE_DLL);
        // second install keeps the first backup
        place_d3d9(&p, DXVK_DLL).unwrap();
        assert_eq!(fs::read(d.path().join("drive_c/windows/syswow64/d3d9.dll.wine")).unwrap(), WINE_DLL);
    }

    #[test]
    fn vulkan_needs_a_driver_and_the_loader() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path();
        assert!(!vulkan_available_in(root, None));
        fs::create_dir_all(root.join("usr/share/vulkan/icd.d")).unwrap();
        fs::write(root.join("usr/share/vulkan/icd.d/radeon_icd.json"), "{}").unwrap();
        assert!(!vulkan_available_in(root, None));
        fs::create_dir_all(root.join("usr/lib")).unwrap();
        fs::write(root.join("usr/lib/libvulkan.so.1"), "").unwrap();
        assert!(vulkan_available_in(root, None));
    }

    pub(crate) fn tarball(dll: &[u8]) -> Vec<u8> {
        let mut b = tar::Builder::new(flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast()));
        for (name, data) in [("dxvk-9.9/x64/d3d9.dll", &b"sixty-four"[..]), ("dxvk-9.9/x32/d3d9.dll", dll)] {
            let mut h = tar::Header::new_gnu();
            h.set_size(data.len() as u64);
            h.set_mode(0o644);
            h.set_cksum();
            b.append_data(&mut h, name, data).unwrap();
        }
        b.into_inner().unwrap().finish().unwrap()
    }

    async fn github(tar: &[u8], digest: &str) -> MockServer {
        let mock = MockServer::start().await;
        let body = serde_json::json!({
            "tag_name": "v9.9",
            "assets": [
                { "name": "dxvk-native-9.9-steamrt-sniper.tar.gz", "browser_download_url": format!("{}/native.tar.gz", mock.uri()) },
                { "name": "dxvk-9.9.tar.gz", "browser_download_url": format!("{}/dxvk-9.9.tar.gz", mock.uri()), "digest": digest },
            ],
        });
        Mock::given(method("GET"))
            .and(path("/releases/latest"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(&mock)
            .await;
        Mock::given(method("GET"))
            .and(path("/dxvk-9.9.tar.gz"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(tar.to_vec()))
            .mount(&mock)
            .await;
        mock
    }

    #[tokio::test]
    async fn downloads_the_32bit_dll_and_checks_the_digest() {
        let tar = tarball(DXVK_DLL);
        let sum: String = Sha256::digest(&tar).iter().map(|b| format!("{b:02x}")).collect();
        let mock = github(&tar, &format!("sha256:{sum}")).await;
        let release = latest_release(&format!("{}/releases/latest", mock.uri())).await.unwrap();
        assert_eq!(release.tag, "v9.9");
        assert!(release.url.ends_with("/dxvk-9.9.tar.gz"));
        assert_eq!(download_d3d9(&release).await.unwrap(), DXVK_DLL);

        let mock = github(&tar, "sha256:00ff").await;
        let release = latest_release(&format!("{}/releases/latest", mock.uri())).await.unwrap();
        assert!(matches!(download_d3d9(&release).await, Err(DownloadError::Checksum { .. })));
    }

    // cargo test -p omptui-core -- --ignored real_dxvk
    #[tokio::test]
    #[ignore]
    async fn real_dxvk_release_has_a_32bit_d3d9() {
        let release = latest_release(RELEASES_URL).await.unwrap();
        let dll = download_d3d9(&release).await.unwrap();
        assert!(dll.starts_with(b"MZ") && contains(&dll, b"dxvk"), "{} bytes", dll.len());
    }
}
