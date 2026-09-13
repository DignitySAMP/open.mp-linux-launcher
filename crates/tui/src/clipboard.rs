use std::io::Write;
use std::process::{Command, Stdio};

fn try_tool(cmd: &str, args: &[&str], text: &str) -> bool {
    let Ok(mut child) =
        Command::new(cmd).args(args).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).spawn()
    else {
        return false;
    };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(text.as_bytes());
    }
    child.wait().map(|s| s.success()).unwrap_or(false)
}

pub fn base64(input: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

// Returns the name of the method that worked, shown in the status line.
pub fn copy(text: &str) -> &'static str {
    if std::env::var_os("WAYLAND_DISPLAY").is_some() && try_tool("wl-copy", &[], text) {
        return "wl-copy";
    }
    if std::env::var_os("DISPLAY").is_some() {
        if try_tool("xclip", &["-selection", "clipboard"], text) {
            return "xclip";
        }
        if try_tool("xsel", &["--clipboard", "--input"], text) {
            return "xsel";
        }
    }
    let seq = format!("\x1b]52;c;{}\x07", base64(text.as_bytes()));
    let mut out = std::io::stdout();
    let _ = out.write_all(seq.as_bytes());
    let _ = out.flush();
    "OSC 52"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_reference() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"1.2.3.4:7777"), "MS4yLjMuNDo3Nzc3");
    }
}
