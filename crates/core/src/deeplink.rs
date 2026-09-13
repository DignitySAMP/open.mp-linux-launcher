// omp:// and samp:// links: host, optional port, optional password as a path segment or query.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeepLink {
    pub scheme: String,
    pub host: String,
    pub port: u16,
    pub password: Option<String>,
}

impl DeepLink {
    pub fn host_port(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum DeepLinkError {
    #[error("not an omp:// or samp:// link")]
    Scheme,
    #[error("missing host")]
    Host,
    #[error("invalid port")]
    Port,
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16)
        {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(if b[i] == b'+' { b' ' } else { b[i] });
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub fn parse(link: &str) -> Result<DeepLink, DeepLinkError> {
    let link = link.trim();
    let (scheme, rest) = link.split_once("://").ok_or(DeepLinkError::Scheme)?;
    let scheme = scheme.to_ascii_lowercase();
    if scheme != "omp" && scheme != "samp" {
        return Err(DeepLinkError::Scheme);
    }
    let (rest, query) = rest.split_once('?').map_or((rest, ""), |(a, b)| (a, b));
    let rest = rest.split_once('#').map_or(rest, |(a, _)| a);
    let (authority, path) = rest.split_once('/').map_or((rest, ""), |(a, b)| (a, b));
    let (host, port) = match authority.rsplit_once(':') {
        Some((h, p)) if !p.is_empty() => (h, p.parse::<u16>().ok().filter(|p| *p > 0).ok_or(DeepLinkError::Port)?),
        _ => (authority, 7777),
    };
    if host.is_empty() {
        return Err(DeepLinkError::Host);
    }
    let mut password = None;
    for kv in query.split('&') {
        if let Some((k, v)) = kv.split_once('=')
            && matches!(k, "password" | "pw" | "pass")
            && !v.is_empty()
        {
            password = Some(percent_decode(v));
        }
    }
    if password.is_none() {
        let seg = path.trim_matches('/');
        if !seg.is_empty() {
            password = Some(percent_decode(seg));
        }
    }
    Ok(DeepLink { scheme, host: host.to_owned(), port, password })
}

pub fn looks_like_link(arg: &str) -> bool {
    let a = arg.trim().to_ascii_lowercase();
    a.starts_with("omp://") || a.starts_with("samp://")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_forms() {
        let l = parse("omp://1.2.3.4:7777").unwrap();
        assert_eq!(l, DeepLink { scheme: "omp".into(), host: "1.2.3.4".into(), port: 7777, password: None });
        let l = parse("SAMP://play.example.org").unwrap();
        assert_eq!(l.scheme, "samp");
        assert_eq!(l.port, 7777);
        assert_eq!(l.host_port(), "play.example.org:7777");
        let l = parse("omp://1.2.3.4:7000/").unwrap();
        assert_eq!(l.port, 7000);
        assert_eq!(l.password, None);
    }

    #[test]
    fn passwords() {
        assert_eq!(parse("omp://1.2.3.4:7777/?password=se%20cret").unwrap().password.as_deref(), Some("se cret"));
        assert_eq!(parse("omp://1.2.3.4:7777?pw=abc&x=1").unwrap().password.as_deref(), Some("abc"));
        assert_eq!(parse("samp://1.2.3.4:7777/abc").unwrap().password.as_deref(), Some("abc"));
        assert_eq!(parse("samp://1.2.3.4:7777/abc/").unwrap().password.as_deref(), Some("abc"));
    }

    #[test]
    fn errors() {
        assert_eq!(parse("http://x"), Err(DeepLinkError::Scheme));
        assert_eq!(parse("omp://"), Err(DeepLinkError::Host));
        assert_eq!(parse("omp://h:0"), Err(DeepLinkError::Port));
        assert_eq!(parse("omp://h:abc"), Err(DeepLinkError::Port));
        assert!(looks_like_link("OMP://x"));
        assert!(!looks_like_link("1.2.3.4:7777"));
    }
}
