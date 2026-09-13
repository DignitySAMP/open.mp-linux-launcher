use crate::model::ServerAddr;
use std::net::{Ipv4Addr, ToSocketAddrs};
use std::path::Path;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ValidationError {
    #[error("port must be 1-65535")]
    Port,
    #[error("hostname must be 1-253 characters of letters, digits, '.', '-' or '_'")]
    Hostname,
    #[error("could not resolve hostname to an IPv4 address")]
    Resolve,
    #[error("nickname must be 3-24 characters: letters, digits, '_', '[' or ']'")]
    Nickname,
    #[error("path must not contain '..' or '//'")]
    PathShape,
    #[error("path does not exist: {0}")]
    PathMissing(String),
    #[error("game executable not found: {0}")]
    ExeMissing(String),
}

pub fn validate_port(port: u32) -> Result<u16, ValidationError> {
    if (1..=65535).contains(&port) { Ok(port as u16) } else { Err(ValidationError::Port) }
}

pub fn validate_hostname(host: &str) -> Result<&str, ValidationError> {
    let host = host.trim();
    let ok = !host.is_empty()
        && host.len() <= 253
        && host.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'));
    if ok { Ok(host) } else { Err(ValidationError::Hostname) }
}

pub fn validate_nickname(name: &str) -> Result<String, ValidationError> {
    let name = name.trim();
    let ok = (3..=24).contains(&name.chars().count())
        && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '[' | ']'));
    if ok { Ok(name.to_owned()) } else { Err(ValidationError::Nickname) }
}

pub fn sanitize_password(pw: &str) -> String {
    pw.chars().filter(|c| !c.is_whitespace() && !c.is_control()).collect::<String>().trim().to_owned()
}

pub fn validate_dir(path: &Path) -> Result<(), ValidationError> {
    let s = path.to_string_lossy();
    if s.contains("..") || s.contains("//") {
        return Err(ValidationError::PathShape);
    }
    if !path.is_dir() {
        return Err(ValidationError::PathMissing(s.into_owned()));
    }
    Ok(())
}

pub fn validate_game(game_dir: &Path, exe_name: &str) -> Result<std::path::PathBuf, ValidationError> {
    validate_dir(game_dir)?;
    let exe = game_dir.join(exe_name);
    if !exe.is_file() {
        return Err(ValidationError::ExeMissing(exe.to_string_lossy().into_owned()));
    }
    Ok(exe)
}

// Does a blocking DNS lookup, call it from a blocking context.
pub fn resolve_host(input: &str) -> Result<(ServerAddr, String), ValidationError> {
    let input = input.trim();
    let (host, port) = match input.rsplit_once(':') {
        Some((h, p)) if !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()) => {
            (h, validate_port(p.parse::<u32>().map_err(|_| ValidationError::Port)?)?)
        }
        _ => (input, 7777),
    };
    let host = validate_hostname(host)?;
    if let Ok(ip) = host.parse::<Ipv4Addr>() {
        return Ok((ServerAddr::new(ip, port), format!("{ip}:{port}")));
    }
    let ip = (host, port)
        .to_socket_addrs()
        .map_err(|_| ValidationError::Resolve)?
        .find_map(|a| match a {
            std::net::SocketAddr::V4(v4) => Some(*v4.ip()),
            _ => None,
        })
        .ok_or(ValidationError::Resolve)?;
    Ok((ServerAddr::new(ip, port), format!("{host}:{port}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ports() {
        assert_eq!(validate_port(0), Err(ValidationError::Port));
        assert_eq!(validate_port(65536), Err(ValidationError::Port));
        assert_eq!(validate_port(7777), Ok(7777));
    }

    #[test]
    fn hostnames() {
        assert!(validate_hostname("server.example-1.org").is_ok());
        assert!(validate_hostname("").is_err());
        assert!(validate_hostname("bad host").is_err());
        assert!(validate_hostname(&"a".repeat(254)).is_err());
    }

    #[test]
    fn nicknames() {
        assert_eq!(validate_nickname("  CJ_[LS]  ").unwrap(), "CJ_[LS]");
        assert!(validate_nickname("ab").is_err());
        assert!(validate_nickname(&"a".repeat(25)).is_err());
        assert!(validate_nickname("big smoke").is_err());
        assert!(validate_nickname("ryder!").is_err());
    }

    #[test]
    fn passwords() {
        assert_eq!(sanitize_password(" pa ss\x01word\n"), "password");
    }

    #[test]
    fn paths() {
        let d = tempfile::tempdir().unwrap();
        assert!(validate_dir(d.path()).is_ok());
        assert_eq!(validate_dir(Path::new("/tmp/../etc")), Err(ValidationError::PathShape));
        assert!(matches!(validate_dir(&d.path().join("nope")), Err(ValidationError::PathMissing(_))));
        assert!(matches!(validate_game(d.path(), "gta_sa.exe"), Err(ValidationError::ExeMissing(_))));
        std::fs::write(d.path().join("gta_sa.exe"), b"MZ").unwrap();
        assert!(validate_game(d.path(), "gta_sa.exe").is_ok());
    }

    #[test]
    fn resolves_literal_ips_without_dns() {
        let (a, label) = resolve_host("127.0.0.1:7000").unwrap();
        assert_eq!(a.to_string(), "127.0.0.1:7000");
        assert_eq!(label, "127.0.0.1:7000");
        let (a, _) = resolve_host("127.0.0.1").unwrap();
        assert_eq!(a.port, 7777);
        assert_eq!(resolve_host("127.0.0.1:0"), Err(ValidationError::Port));
        assert_eq!(resolve_host("bad host:1"), Err(ValidationError::Hostname));
        let (a, label) = resolve_host("localhost:7777").unwrap();
        assert_eq!(a.ip, Ipv4Addr::LOCALHOST);
        assert_eq!(label, "localhost:7777");
    }
}
