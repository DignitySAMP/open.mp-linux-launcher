// USERDATA.DAT is the favorites file of the SA-MP client: "SAMP", version u32, count u32, then
// per server ip, port u32, name, password and rcon password. Strings are u32 length + bytes.
// The nickname and game path are in HKCU\Software\SAMP, which Wine keeps in user.reg.

use crate::encoding::decode_text;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedServer {
    pub host: String,
    pub port: u16,
    pub name: String,
    pub password: String,
    pub rcon: String,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum UserdataError {
    #[error("not a USERDATA.DAT file (bad magic)")]
    BadMagic,
    #[error("file truncated while reading {0}")]
    Truncated(&'static str),
}

struct Cur<'a>(&'a [u8], usize);

impl<'a> Cur<'a> {
    fn u32(&mut self, what: &'static str) -> Result<u32, UserdataError> {
        let s = self.0.get(self.1..self.1 + 4).ok_or(UserdataError::Truncated(what))?;
        self.1 += 4;
        Ok(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
    }
    fn str(&mut self, what: &'static str) -> Result<String, UserdataError> {
        let n = self.u32(what)? as usize;
        let s = self.0.get(self.1..self.1.checked_add(n).ok_or(UserdataError::Truncated(what))?);
        let s = s.ok_or(UserdataError::Truncated(what))?;
        self.1 += n;
        Ok(decode_text(s))
    }
}

pub fn parse_userdata(bytes: &[u8]) -> Result<Vec<ImportedServer>, UserdataError> {
    if bytes.len() < 4 || &bytes[..4] != b"SAMP" {
        return Err(UserdataError::BadMagic);
    }
    let mut c = Cur(bytes, 4);
    let _version = c.u32("version")?;
    let count = c.u32("count")?;
    let mut out = Vec::new();
    for _ in 0..count {
        let host = c.str("ip")?;
        let port = c.u32("port")?;
        let name = c.str("name")?;
        let password = c.str("password")?;
        let rcon = c.str("rcon")?;
        if !(1..=65535).contains(&port) || host.is_empty() {
            continue;
        }
        out.push(ImportedServer { host, port: port as u16, name, password, rcon });
    }
    Ok(out)
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SampRegistry {
    pub player_name: Option<String>,
    pub gta_sa_exe: Option<String>,
}

impl SampRegistry {
    pub fn game_dir_windows(&self) -> Option<String> {
        let exe = self.gta_sa_exe.as_ref()?;
        let idx = exe.rfind('\\')?;
        Some(exe[..idx].to_owned())
    }
}

fn unescape_reg(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('\\') => out.push('\\'),
                Some('"') => out.push('"'),
                Some('n') => out.push('\n'),
                Some('r') => out.push('\r'),
                Some('0') => {}
                Some(o) => {
                    out.push('\\');
                    out.push(o);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

pub fn parse_user_reg(text: &str) -> SampRegistry {
    let mut reg = SampRegistry::default();
    let mut in_samp = false;
    for line in text.lines() {
        let line = line.trim_end();
        if let Some(rest) = line.strip_prefix('[') {
            let key = rest.split(']').next().unwrap_or("");
            in_samp = key.eq_ignore_ascii_case("Software\\\\SAMP") || key.eq_ignore_ascii_case("Software\\SAMP");
            continue;
        }
        if !in_samp || !line.starts_with('"') {
            continue;
        }
        let Some((name, value)) = line[1..].split_once("\"=") else { continue };
        // REG_EXPAND_SZ shows up as str(2):"..."
        let value = value.trim();
        let value = value.strip_prefix("str(2):").unwrap_or(value);
        let Some(value) = value.strip_prefix('"').and_then(|v| v.strip_suffix('"')) else { continue };
        let value = unescape_reg(value);
        match name {
            n if n.eq_ignore_ascii_case("PlayerName") => reg.player_name = Some(value),
            n if n.eq_ignore_ascii_case("gta_sa_exe") => reg.gta_sa_exe = Some(value),
            _ => {}
        }
    }
    reg
}

pub fn read_prefix_registry(prefix: &std::path::Path) -> std::io::Result<SampRegistry> {
    Ok(parse_user_reg(&std::fs::read_to_string(prefix.join("user.reg"))?))
}

pub fn userdata_candidates(prefix: &std::path::Path) -> Vec<PathBuf> {
    let users = prefix.join("drive_c").join("users");
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&users) {
        for e in rd.flatten() {
            for docs in ["Documents", "My Documents"] {
                let p = e.path().join(docs).join("GTA San Andreas User Files").join("SAMP").join("USERDATA.DAT");
                if p.is_file() {
                    out.push(p);
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn push_str(v: &mut Vec<u8>, s: &[u8]) {
        v.extend_from_slice(&(s.len() as u32).to_le_bytes());
        v.extend_from_slice(s);
    }

    type Entry<'a> = (&'a [u8], u32, &'a [u8], &'a [u8], &'a [u8]);

    fn build_userdata(entries: &[Entry]) -> Vec<u8> {
        let mut v = b"SAMP".to_vec();
        v.extend_from_slice(&1u32.to_le_bytes());
        v.extend_from_slice(&(entries.len() as u32).to_le_bytes());
        for (ip, port, name, pw, rcon) in entries {
            push_str(&mut v, ip);
            v.extend_from_slice(&port.to_le_bytes());
            push_str(&mut v, name);
            push_str(&mut v, pw);
            push_str(&mut v, rcon);
        }
        v
    }

    #[test]
    fn parses_userdata() {
        let data = build_userdata(&[
            (b"127.0.0.1", 7777, b"Local", b"", b""),
            (b"play.example.org", 7788, b"\xD0\xF3\xF1\xF1\xEA\xE8\xE9", b"secret", b"rc"),
            (b"1.2.3.4", 0, b"bad port", b"", b""),
        ]);
        let s = parse_userdata(&data).unwrap();
        assert_eq!(s.len(), 2);
        assert_eq!(
            s[0],
            ImportedServer {
                host: "127.0.0.1".into(),
                port: 7777,
                name: "Local".into(),
                password: "".into(),
                rcon: "".into()
            }
        );
        assert_eq!(s[1].name, "Русский");
        assert_eq!(s[1].password, "secret");
        assert_eq!(s[1].port, 7788);
    }

    #[test]
    fn userdata_errors() {
        assert_eq!(parse_userdata(b"NOPE"), Err(UserdataError::BadMagic));
        let data = build_userdata(&[(b"127.0.0.1", 7777, b"Local", b"", b"")]);
        for n in 4..data.len() {
            assert!(parse_userdata(&data[..n]).is_err(), "prefix {n}");
        }
        let mut bad = b"SAMP".to_vec();
        bad.extend_from_slice(&1u32.to_le_bytes());
        bad.extend_from_slice(&1u32.to_le_bytes());
        bad.extend_from_slice(&u32::MAX.to_le_bytes());
        assert!(parse_userdata(&bad).is_err());
    }

    #[test]
    fn parses_user_reg() {
        let reg = "WINE REGISTRY Version 2\n;; All keys relative to \\\\User\\\\S-1-5-21\n\n\
[Software\\\\Other] 1690000000\n\"PlayerName\"=\"wrong\"\n\n\
[Software\\\\SAMP] 1694012345\n#time=1d9e0f0\n\
\"gta_sa_exe\"=\"C:\\\\Program Files (x86)\\\\Rockstar Games\\\\GTA San Andreas\\\\gta_sa.exe\"\n\
\"PlayerName\"=\"enchanter\"\n\"Other\"=dword:00000001\n\n[Software\\\\Wine] 1\n\"PlayerName\"=\"no\"\n";
        let r = parse_user_reg(reg);
        assert_eq!(r.player_name.as_deref(), Some("enchanter"));
        assert_eq!(
            r.gta_sa_exe.as_deref(),
            Some("C:\\Program Files (x86)\\Rockstar Games\\GTA San Andreas\\gta_sa.exe")
        );
        assert_eq!(r.game_dir_windows().as_deref(), Some("C:\\Program Files (x86)\\Rockstar Games\\GTA San Andreas"));
        assert_eq!(parse_user_reg(""), SampRegistry::default());
    }

    #[test]
    fn expand_sz_values() {
        let reg = "[Software\\\\SAMP] 1\n\"gta_sa_exe\"=str(2):\"D:\\\\gta\\\\gta_sa.exe\"\n";
        assert_eq!(parse_user_reg(reg).gta_sa_exe.as_deref(), Some("D:\\gta\\gta_sa.exe"));
    }

    #[test]
    fn userdata_candidates_scans_users() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("drive_c/users/davy/Documents/GTA San Andreas User Files/SAMP");
        std::fs::create_dir_all(&p).unwrap();
        std::fs::write(p.join("USERDATA.DAT"), b"SAMP").unwrap();
        let c = userdata_candidates(d.path());
        assert_eq!(c.len(), 1);
        assert!(c[0].ends_with("USERDATA.DAT"));
    }
}
