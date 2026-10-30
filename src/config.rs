//! Config loading/saving for shotpipe (cross-platform; used by the Mac side).

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct DownscaleCfg {
    pub enabled: bool,
    pub max_width: u32,
}

impl Default for DownscaleCfg {
    fn default() -> Self {
        Self {
            enabled: true,
            max_width: 1600,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct GeneralCfg {
    /// Local folder that is watched and mirrored (usually where macOS saves screenshots).
    pub screenshot_dir: PathBuf,
    /// Destination dir on each server, relative to $HOME unless absolute.
    pub remote_dir: String,
    /// Local port the clipboard HTTP server listens on; also the remote port for `ssh -R`.
    pub clipboard_port: u16,
    /// Optional shared secret; if set, clients must send `X-Shotpipe-Token`.
    pub token: String,
    /// Watch the clipboard and auto-save any copied image into
    /// `screenshot_dir`, so clipboard-only workflows still populate
    /// `latest.png` on every server.
    pub mirror_clipboard: bool,
    /// Watch macOS temp dirs for terminal paste-images (e.g. iTerm2/Terminal
    /// write `clipboard-*.png` under /var/folders/.../T/ when you Cmd+V an
    /// image) and copy them into `screenshot_dir` unchanged, so the pasted
    /// path resolves on every server via `shotpipe resolve`.
    pub paste_capture: bool,
    pub downscale: DownscaleCfg,
}

impl Default for GeneralCfg {
    fn default() -> Self {
        Self {
            screenshot_dir: PathBuf::from("~/Screenshots"),
            remote_dir: "shots".into(),
            clipboard_port: 8777,
            token: String::new(),
            mirror_clipboard: true,
            paste_capture: true,
            downscale: DownscaleCfg::default(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct ServerCfg {
    /// Anything `ssh` accepts: alias, user@host, with jump hosts etc.
    pub host: String,
    /// Human label shown in the menu (defaults to host).
    pub name: Option<String>,
    pub enabled: bool,
    /// Mirror the screenshot folder to this server.
    pub sync: bool,
    /// Maintain an `ssh -R` clipboard tunnel to this server.
    pub clipboard: bool,
    /// Per-server override for the destination dir.
    pub remote_dir: Option<String>,
}

impl Default for ServerCfg {
    fn default() -> Self {
        Self {
            host: String::new(),
            name: None,
            enabled: true,
            sync: true,
            clipboard: true,
            remote_dir: None,
        }
    }
}

impl ServerCfg {
    pub fn label(&self) -> &str {
        self.name.as_deref().unwrap_or(&self.host)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub struct Config {
    pub general: GeneralCfg,
    pub servers: Vec<ServerCfg>,
}

impl Config {
    /// Path of the config file (~/.config/shotpipe/config.toml).
    pub fn path() -> PathBuf {
        if let Ok(dir) = std::env::var("SHOTPIPE_CONFIG_DIR") {
            return Path::new(&dir).join("config.toml");
        }
        home().join(".config").join("shotpipe").join("config.toml")
    }

    pub fn load_or_create() -> Result<(Self, PathBuf), String> {
        let path = Self::path();
        if !path.exists() {
            let cfg = Config::default_initial();
            cfg.save(&path)?;
            return Ok((cfg, path));
        }
        let text =
            fs::read_to_string(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
        let cfg: Config =
            toml::from_str(&text).map_err(|e| format!("parse {}: {e}", path.display()))?;
        Ok((cfg.normalized(), path))
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let text = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
        let cfg: Config =
            toml::from_str(&text).map_err(|e| format!("parse {}: {e}", path.display()))?;
        Ok(cfg.normalized())
    }

    /// First-run default: watch ~/Screenshots, one enabled `dev` server.
    fn default_initial() -> Self {
        Self {
            general: GeneralCfg::default(),
            servers: vec![ServerCfg {
                host: "dev".into(),
                ..ServerCfg::default()
            }],
        }
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("mkdir {}: {e}", parent.display()))?;
        }
        let text = toml::to_string_pretty(self).map_err(|e| format!("serialize config: {e}"))?;
        fs::write(path, text).map_err(|e| format!("write {}: {e}", path.display()))
    }

    /// Expand `~` in paths right after parsing.
    fn normalized(mut self) -> Self {
        self.general.screenshot_dir = expand_tilde(&self.general.screenshot_dir);
        self
    }

    pub fn effective_remote_dir(&self, server: &ServerCfg) -> String {
        server
            .remote_dir
            .clone()
            .unwrap_or_else(|| self.general.remote_dir.clone())
    }
}

pub fn home() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/"))
}

pub fn expand_tilde(p: &Path) -> PathBuf {
    let s = p.to_string_lossy();
    if s == "~" {
        return home();
    }
    if let Some(rest) = s.strip_prefix("~/") {
        return home().join(rest);
    }
    p.to_path_buf()
}

/// Shell-quote a string for a remote `sh -c` command.
pub fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_minimal() {
        let cfg: Config = toml::from_str(
            r#"
            [[servers]]
            host = "dev"
            "#,
        )
        .unwrap();
        assert_eq!(cfg.general.clipboard_port, 8777);
        assert!(cfg.servers[0].enabled);
        assert_eq!(cfg.effective_remote_dir(&cfg.servers[0]), "shots");
    }

    #[test]
    fn parse_full() {
        let cfg: Config = toml::from_str(
            r#"
            [general]
            screenshot_dir = "~/Pictures/Shots"
            clipboard_port = 9000
            token = "s3cret"
            downscale = { enabled = false, max_width = 800 }

            [[servers]]
            host = "user@box.example.com"
            name = "box"
            enabled = false
            remote_dir = "agent/shots"
            "#,
        )
        .unwrap();
        assert_eq!(cfg.general.clipboard_port, 9000);
        assert!(!cfg.general.downscale.enabled);
        assert!(!cfg.servers[0].enabled);
        assert_eq!(cfg.effective_remote_dir(&cfg.servers[0]), "agent/shots");
    }

    #[test]
    fn expand() {
        let h = home();
        assert_eq!(expand_tilde(Path::new("~/x")), h.join("x"));
        assert_eq!(expand_tilde(Path::new("/abs/x")), PathBuf::from("/abs/x"));
    }
}
