// Saved server passwords are stored encrypted with a key that lives next to the data files
// (XChaCha20-Poly1305, key file mode 0600). This keeps passwords out of lists.toml so the file can
// be shared or backed up, it is not protection against someone with access to the home directory.

use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use std::fs;
use std::io;
use std::path::Path;

const PREFIX: &str = "enc1:";
const KEY_FILE: &str = "secret.key";

pub struct Keyring {
    cipher: XChaCha20Poly1305,
}

impl Keyring {
    // Loads the key file, creating it on first use.
    pub fn open(data_dir: &Path) -> io::Result<Self> {
        let path = data_dir.join(KEY_FILE);
        let key: [u8; 32] = match fs::read(&path) {
            Ok(bytes) if bytes.len() == 32 => bytes.try_into().unwrap(),
            Ok(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("{} is not a 32 byte key", path.display()),
                ));
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                let key: [u8; 32] = rand::random();
                fs::create_dir_all(data_dir)?;
                write_private(&path, &key)?;
                key
            }
            Err(e) => return Err(e),
        };
        Ok(Self { cipher: XChaCha20Poly1305::new((&key).into()) })
    }

    pub fn encrypt(&self, plain: &str) -> String {
        let nonce: [u8; 24] = rand::random();
        let sealed = self.cipher.encrypt(XNonce::from_slice(&nonce), plain.as_bytes()).expect("encryption cannot fail");
        format!("{PREFIX}{}{}", hex(&nonce), hex(&sealed))
    }

    // Values without the prefix are returned as they are, so files from before encryption still load.
    pub fn decrypt(&self, stored: &str) -> Option<String> {
        let Some(body) = stored.strip_prefix(PREFIX) else { return Some(stored.to_owned()) };
        let bytes = unhex(body)?;
        if bytes.len() < 24 {
            return None;
        }
        let (nonce, sealed) = bytes.split_at(24);
        let plain = self.cipher.decrypt(XNonce::from_slice(nonce), sealed).ok()?;
        String::from_utf8(plain).ok()
    }

    pub fn is_encrypted(stored: &str) -> bool {
        stored.starts_with(PREFIX)
    }
}

fn write_private(path: &Path, bytes: &[u8]) -> io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(path)?;
    f.write_all(bytes)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn roundtrip_and_key_file() {
        let d = tempfile::tempdir().unwrap();
        let k = Keyring::open(d.path()).unwrap();
        let mode = fs::metadata(d.path().join(KEY_FILE)).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let stored = k.encrypt("hunter2");
        assert!(Keyring::is_encrypted(&stored));
        assert!(!stored.contains("hunter2"));
        assert_ne!(k.encrypt("hunter2"), stored, "fresh nonce every time");
        assert_eq!(k.decrypt(&stored).as_deref(), Some("hunter2"));
        assert_eq!(k.decrypt("plain old password").as_deref(), Some("plain old password"));
        let again = Keyring::open(d.path()).unwrap();
        assert_eq!(again.decrypt(&stored).as_deref(), Some("hunter2"), "same key file, same result");
    }

    #[test]
    fn wrong_key_or_damaged_value_is_none() {
        let a = Keyring::open(tempfile::tempdir().unwrap().path()).unwrap();
        let b = Keyring::open(tempfile::tempdir().unwrap().path()).unwrap();
        let stored = a.encrypt("secret");
        assert_eq!(b.decrypt(&stored), None);
        assert_eq!(a.decrypt("enc1:zz"), None);
        assert_eq!(a.decrypt("enc1:00"), None);
        let mut damaged = stored.clone();
        let last = damaged.pop().unwrap();
        damaged.push(if last == '0' { '1' } else { '0' });
        assert_eq!(a.decrypt(&damaged), None);
    }

    #[test]
    fn bad_key_file_is_reported() {
        let d = tempfile::tempdir().unwrap();
        fs::write(d.path().join(KEY_FILE), b"short").unwrap();
        assert!(Keyring::open(d.path()).is_err());
    }
}
