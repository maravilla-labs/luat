// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! Registry tokens in the user's config directory, never in project files.
//!
//! ```toml
//! # credentials.toml
//! [registries."https://registry.example.com"]
//! token = "..."
//! ```

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::error::{IoContext, PackageError, Result};
use super::normalize_url;

/// `$XDG_CONFIG_HOME/luat/credentials.toml`, falling back to
/// `~/.config/luat/credentials.toml` (and `%APPDATA%\luat\credentials.toml`
/// on Windows).
pub fn default_credentials_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            if cfg!(windows) {
                std::env::var_os("APPDATA").map(PathBuf::from)
            } else {
                std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config"))
            }
        })?;
    Some(base.join("luat").join("credentials.toml"))
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct File {
    #[serde(default)]
    registries: BTreeMap<String, Entry>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Entry {
    token: String,
}

/// The credentials file.
#[derive(Debug)]
pub struct Credentials {
    path: PathBuf,
    file: File,
}

impl Credentials {
    /// Reads the file at `path` (empty when it does not exist).
    pub fn load(path: &Path) -> Result<Self> {
        let file = match std::fs::read_to_string(path) {
            Ok(text) => toml::from_str(&text)
                .map_err(|e| PackageError::Manifest(format!("{}: {e}", path.display())))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => File::default(),
            Err(e) => return Err(e).ctx(|| format!("reading {}", path.display())),
        };
        Ok(Self { path: path.to_path_buf(), file })
    }

    /// The token stored for `registry`.
    pub fn token(&self, registry: &str) -> Option<&str> {
        self.file.registries.get(&normalize_url(registry)).map(|e| e.token.as_str())
    }

    /// Stores `token` for `registry` (call [`save`](Self::save) after).
    pub fn set_token(&mut self, registry: &str, token: &str) {
        self.file.registries.insert(normalize_url(registry), Entry { token: token.to_string() });
    }

    /// Writes the file, readable by the owner only (mode 0600 on unix).
    pub fn save(&self) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).ctx(|| format!("creating {}", parent.display()))?;
        }
        let text = toml::to_string(&self.file).expect("credentials serialize");
        write_private(&self.path, text.as_bytes()).ctx(|| format!("writing {}", self.path.display()))
    }
}

#[cfg(unix)]
fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    // The mode only applies on creation; tighten an existing file too.
    file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    file.write_all(bytes)
}

#[cfg(not(unix))]
fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    std::fs::write(path, bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stores_tokens_privately() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("luat/credentials.toml");
        let mut creds = Credentials::load(&path).unwrap();
        assert_eq!(creds.token("https://r.example"), None);
        creds.set_token("https://r.example/", "secret");
        creds.save().unwrap();
        let again = Credentials::load(&path).unwrap();
        assert_eq!(again.token("https://r.example"), Some("secret"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }
}
