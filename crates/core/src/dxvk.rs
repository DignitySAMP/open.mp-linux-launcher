use crate::wine::Prefix;
use std::fs;
use std::path::{Path, PathBuf};

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

#[cfg(test)]
mod tests {
    use super::*;

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
}
