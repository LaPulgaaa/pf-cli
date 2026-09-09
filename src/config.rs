//! Persisted credentials, so a token survives the shell that set it.
//!
//! Resolution order, highest first: `--token`, `--token-file`, the environment,
//! then the config file. The environment beating the file is what lets CI
//! override a developer's saved profile without editing anything on disk.

use std::collections::BTreeMap;
use std::io::{IsTerminal, Read};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::client::{DEFAULT_BASE_URL, Error, Result, TOKEN_ENVS, exit};

pub const PROFILE_ENV: &str = "PF_PROFILE";
pub const CONFIG_ENV: &str = "PF_CONFIG";
pub const BASE_URL_ENV: &str = "PF_BASE_URL";

#[derive(Debug, Default, Deserialize, Serialize)]
pub struct Config {
    /// Which profile to use when none is named on the command line.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_profile: Option<String>,

    /// Single-workspace shorthand: a token at the top level needs no profile.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,

    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub profiles: BTreeMap<String, Profile>,
}

#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct Profile {
    pub token: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
}

/// Where a token came from, for `auth status` and for error messages that need
/// to tell the user which of several sources actually won.
#[derive(Debug, Clone)]
pub struct Credentials {
    pub token: String,
    pub source: String,
    pub base_url: String,
}

impl Config {
    /// `$PF_CONFIG`, else `$XDG_CONFIG_HOME/pf/config.toml`, else
    /// `~/.config/pf/config.toml`.
    pub fn path(explicit: Option<&Path>) -> Option<PathBuf> {
        if let Some(path) = explicit {
            return Some(path.to_path_buf());
        }
        if let Some(path) = std::env::var_os(CONFIG_ENV).filter(|v| !v.is_empty()) {
            return Some(PathBuf::from(path));
        }
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .or_else(|| std::env::home_dir().map(|h| h.join(".config")))?;
        Some(base.join("pf").join("config.toml"))
    }

    /// A missing file is not an error: the environment alone is a valid setup.
    pub fn load(path: Option<&Path>) -> Result<Self> {
        let Some(path) = path else {
            return Ok(Self::default());
        };
        let raw = match std::fs::read_to_string(path) {
            Ok(raw) => raw,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => {
                return Err(Error::other(format!(
                    "Could not read {}: {e}",
                    path.display()
                )));
            }
        };

        warn_if_world_readable(path);

        toml::from_str(&raw).map_err(|e| {
            Error::other(format!("{} is not valid TOML: {e}", path.display()))
                .with_hint("Fix it by hand, or re-run `pf auth login` to rewrite it.")
        })
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| Error::other(format!("Could not create {}: {e}", parent.display())))?;
            restrict(parent, 0o700);
        }

        let body = toml::to_string_pretty(self)
            .map_err(|e| Error::other(format!("Could not serialise the config: {e}")))?;
        let document = format!(
            "# Passionfroot CLI configuration.\n\
             # Written by `pf auth login`. Tokens here are stored in plain text,\n\
             # so this file is kept readable only by you.\n\n{body}"
        );

        std::fs::write(path, document)
            .map_err(|e| Error::other(format!("Could not write {}: {e}", path.display())))?;
        restrict(path, 0o600);
        Ok(())
    }

    /// The profile a bare command would use.
    pub fn active_profile_name(&self, requested: Option<&str>) -> Option<String> {
        requested
            .map(str::to_owned)
            .or_else(|| std::env::var(PROFILE_ENV).ok().filter(|v| !v.is_empty()))
            .or_else(|| self.default_profile.clone())
    }

    pub fn profile_names(&self) -> Vec<&str> {
        self.profiles.keys().map(String::as_str).collect()
    }
}

/// What the CLI's global flags contribute to credential resolution.
pub struct TokenArgs<'a> {
    pub token: Option<&'a str>,
    pub token_file: Option<&'a Path>,
    pub profile: Option<&'a str>,
    pub base_url: Option<&'a str>,
}

pub fn resolve(args: &TokenArgs<'_>, config: &Config) -> Result<Credentials> {
    let requested_profile = config.active_profile_name(args.profile);

    // A named profile that does not exist is a typo, not a reason to silently
    // fall through to some other workspace's key.
    let profile = match &requested_profile {
        Some(name) => match config.profiles.get(name) {
            Some(profile) => Some((name.clone(), profile.clone())),
            None if args.profile.is_some() || std::env::var(PROFILE_ENV).is_ok() => {
                return Err(
                    Error::usage(format!("No profile named `{name}` in the config."))
                        .with_hint(profile_hint(config)),
                );
            }
            None => None,
        },
        None => None,
    };

    let (token, source) = if let Some(token) = args.token {
        (token.trim().to_string(), "--token".to_string())
    } else if let Some(path) = args.token_file {
        (
            read_token_file(path)?,
            format!("--token-file {}", path.display()),
        )
    } else if let Some((name, value)) = env_token() {
        (value, format!("env {name}"))
    } else if let Some((name, profile)) = &profile {
        (
            profile.token.trim().to_string(),
            format!("profile `{name}`"),
        )
    } else if let Some(token) = &config.token {
        (token.trim().to_string(), "config file".to_string())
    } else {
        return Err(no_token_error(config));
    };

    if token.is_empty() {
        return Err(Error::usage(format!("The token from {source} is empty.")));
    }

    let base_url = args
        .base_url
        .map(str::to_owned)
        .or_else(|| std::env::var(BASE_URL_ENV).ok().filter(|v| !v.is_empty()))
        .or_else(|| profile.as_ref().and_then(|(_, p)| p.base_url.clone()))
        .or_else(|| config.base_url.clone())
        .unwrap_or_else(|| DEFAULT_BASE_URL.to_string());

    Ok(Credentials {
        token,
        source,
        base_url,
    })
}

fn env_token() -> Option<(&'static str, String)> {
    TOKEN_ENVS.into_iter().find_map(|name| {
        let value = std::env::var(name).ok()?;
        let value = value.trim().to_string();
        (!value.is_empty()).then_some((name, value))
    })
}

/// Read a token from a file, or from stdin when the path is `-`.
pub fn read_token_file(path: &Path) -> Result<String> {
    let raw = if path.as_os_str() == "-" {
        let mut buf = String::new();
        std::io::stdin()
            .read_to_string(&mut buf)
            .map_err(|e| Error::usage(format!("Could not read the token from stdin: {e}")))?;
        buf
    } else {
        std::fs::read_to_string(path)
            .map_err(|e| Error::usage(format!("Could not read {}: {e}", path.display())))?
    };

    let token = raw.trim().to_string();
    if token.is_empty() {
        return Err(Error::usage(format!(
            "{} contains no token.",
            path.display()
        )));
    }
    Ok(token)
}

/// Prompt for a pasted key without echoing it to the terminal.
pub fn prompt_for_token() -> Result<String> {
    if !std::io::stdin().is_terminal() {
        return Err(Error::usage("No terminal to prompt on.")
            .with_hint("Pipe the key in with `pf auth login --token-file -` instead."));
    }
    // Say the input is hidden. Without this a paste looks like it did nothing,
    // and the natural response is to paste again into a prompt that already
    // holds the key.
    eprintln!("Paste your API key from Settings > API Keys.");
    eprintln!("The key is not shown as you type or paste it.");
    let token = rpassword::prompt_password("Key: ")
        .map_err(|e| Error::usage(format!("Could not read the key: {e}")))?;

    let token = token.trim().to_string();
    if token.is_empty() {
        return Err(Error::usage("No key entered."));
    }
    Ok(token)
}

fn no_token_error(config: &Config) -> Error {
    Error {
        code: "no_token",
        status: None,
        message: "No API token configured.".to_string(),
        hint: Some(format!(
            "Run `pf auth login` to save one, or set {}. {}",
            TOKEN_ENVS[0],
            profile_hint(config)
        )),
        exit: exit::AUTH,
    }
}

fn profile_hint(config: &Config) -> String {
    match config.profile_names().as_slice() {
        [] => "No profiles are configured yet.".to_string(),
        names => format!("Configured profiles: {}", names.join(", ")),
    }
}

/// A token in a file anyone can read is a token that has effectively leaked.
fn warn_if_world_readable(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = std::fs::metadata(path) {
            let mode = meta.permissions().mode() & 0o777;
            if mode & 0o077 != 0 {
                eprintln!(
                    "warning: {} is readable by other users (mode {mode:o}). \
                     Run `chmod 600 {}` -- it holds an API key.",
                    path.display(),
                    path.display()
                );
            }
        }
    }
    #[cfg(not(unix))]
    let _ = path;
}

fn restrict(path: &Path, mode: u32) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode));
    }
    #[cfg(not(unix))]
    let _ = (path, mode);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_a_profile_config() {
        let mut config = Config {
            default_profile: Some("work".into()),
            ..Default::default()
        };
        config.profiles.insert(
            "work".into(),
            Profile {
                token: "pf_live_x".into(),
                base_url: None,
            },
        );

        let text = toml::to_string_pretty(&config).unwrap();
        let parsed: Config = toml::from_str(&text).unwrap();
        assert_eq!(parsed.default_profile.as_deref(), Some("work"));
        assert_eq!(parsed.profiles["work"].token, "pf_live_x");
    }

    #[test]
    fn accepts_a_bare_top_level_token() {
        let parsed: Config = toml::from_str("token = \"pf_live_y\"\n").unwrap();
        assert_eq!(parsed.token.as_deref(), Some("pf_live_y"));
        assert!(parsed.profiles.is_empty());
    }
}
