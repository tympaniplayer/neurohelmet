// Neurohelmet — Copyright (C) 2026 Nate Palmer
//
// This file is part of Neurohelmet.
//
// Neurohelmet is free software: you can redistribute it and/or modify it under
// the terms of the GNU General Public License as published by the Free Software
// Foundation, either version 3 of the License, or (at your option) any later
// version.
//
// Neurohelmet is distributed in the hope that it will be useful, but WITHOUT ANY
// WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR
// A PARTICULAR PURPOSE. See the GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License along with
// Neurohelmet. If not, see <https://www.gnu.org/licenses/>.

//! §20 / §36 — persisted user config. A small `config.json` holding the picked theme + layout
//! profile so a Ctrl-T choice survives a restart. Lives under [`neurohelmet_dir`] (the same
//! `NEUROHELMET_DIR`-overridable root as sessions), so tests that `isolate_data_dir()` never touch the
//! real file, and a relocated data dir keeps its config with it. (This deviates from §20's original
//! `dirs::config_dir()` sketch in favor of that test-isolation + co-location; noted deliberately.)
//!
//! **Resolution precedence (both fields): env var > saved config > built-in default.** An explicit
//! `NEUROHELMET_THEME`/`NEUROHELMET_PROFILE` still wins for that launch; absent it, the saved choice
//! applies; absent that, the theme falls back to the terminal-derived [`Theme::auto`] and the profile
//! to `Pi`. Unknown/missing files load as an empty config (all `None`) — never an error.

use super::icons::{icons, IconSet};
use super::profile::{profile, DisplayProfile};
use super::theme::{theme, Theme};
use neurohelmet_core::session::neurohelmet_dir;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

/// The persisted config. Both fields are optional so a partial or older file still loads; a `None`
/// means "no saved preference — fall through to env/default".
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Saved theme config-name (see [`Theme::from_name`]); `None` = no preference.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theme: Option<String>,
    /// Saved layout profile config-name (`pi`/`modern`); `None` = no preference.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    /// Saved icon-set config-name (`ascii`/`nerd`); `None` = no preference.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icons: Option<String>,
    /// A user-managed git repo to publish game logs into, bypassing the `gh`-managed `neurohelmet-logs`
    /// clone. A leading `~/` is expanded to the home dir. `None` = use the default `gh` flow. See
    /// `docs/logging-setup.md`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log_repo: Option<String>,
    /// Whether `--publish` pushes automatically after committing. `None`/`true` = push (default);
    /// `false` = commit locally and stop so you can push it yourself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log_auto_push: Option<bool>,
}

/// Path to the config file: `<neurohelmet_dir>/config.json`.
fn config_file() -> PathBuf {
    neurohelmet_dir().join("config.json")
}

impl Config {
    /// Load the saved config, or an empty default if it's missing or unparseable (never errors —
    /// a corrupt config should not stop the app from starting).
    pub fn load() -> Config {
        std::fs::read(config_file())
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    /// Write the config atomically (tmp + rename), like the session store. Errors are returned so the
    /// caller can surface a status line, but a failure is non-fatal.
    pub fn save(&self) -> std::io::Result<()> {
        let path = config_file();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?;
        // Unique tmp name so concurrent writers (e.g. parallel tests sharing NEUROHELMET_DIR) don't
        // race on one tmp path and lose a rename to ENOENT.
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let tmp = path.with_extension(format!("json.{}.{n}.tmp", std::process::id()));
        std::fs::write(&tmp, &json)?;
        std::fs::rename(&tmp, &path)
    }

    /// The startup theme: `NEUROHELMET_THEME` env > saved `theme` > [`Theme::auto`].
    pub fn resolved_theme(&self) -> Theme {
        if let Ok(name) = std::env::var("NEUROHELMET_THEME") {
            if let Some(t) = Theme::from_name(&name) {
                return t;
            }
        }
        if let Some(name) = &self.theme {
            if let Some(t) = Theme::from_name(name) {
                return t;
            }
        }
        Theme::auto()
    }

    /// The startup profile: `NEUROHELMET_PROFILE` env > saved `profile` > `Pi`.
    pub fn resolved_profile(&self) -> DisplayProfile {
        if let Ok(name) = std::env::var("NEUROHELMET_PROFILE") {
            if let Some(p) = DisplayProfile::from_name(&name) {
                return p;
            }
        }
        self.profile
            .as_deref()
            .and_then(DisplayProfile::from_name)
            .unwrap_or(DisplayProfile::Pi)
    }

    /// The startup icon set: `NEUROHELMET_ICONS` env > saved `icons` > `Ascii`.
    pub fn resolved_icons(&self) -> IconSet {
        if let Ok(name) = std::env::var("NEUROHELMET_ICONS") {
            if let Some(i) = IconSet::from_name(&name) {
                return i;
            }
        }
        self.icons
            .as_deref()
            .and_then(IconSet::from_name)
            .unwrap_or(IconSet::Ascii)
    }

    /// The publish target: the saved `log_repo` with a leading `~/` expanded, or `None` to use the
    /// built-in `gh`-managed `neurohelmet-logs` flow. Unlike theme/profile, there is no env override —
    /// this is a set-once, hand-edited path.
    pub fn resolved_log_repo(&self) -> Option<PathBuf> {
        let raw = self.log_repo.as_deref().filter(|s| !s.is_empty())?;
        Some(expand_tilde(raw))
    }

    /// Whether `--publish` pushes automatically after committing (default `true`).
    pub fn resolved_auto_push(&self) -> bool {
        self.log_auto_push.unwrap_or(true)
    }
}

/// Expand a leading `~/` (or a bare `~`) to the user's home dir; any other path is returned as-is.
fn expand_tilde(path: &str) -> PathBuf {
    if path == "~" {
        return dirs::home_dir().unwrap_or_else(|| PathBuf::from(path));
    }
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }
    PathBuf::from(path)
}

/// Persist the currently-active theme + layout profile (as read from the live thread-locals). Called
/// when the Ctrl-T picker commits a choice, so it sticks next launch. Loads the existing config first
/// and updates only the display fields, so unrelated settings (e.g. `log_repo`) survive the write.
pub fn save_current() -> std::io::Result<()> {
    let mut cfg = Config::load();
    cfg.theme = Some(theme().config_name().to_string());
    cfg.profile = Some(profile().config_name().to_string());
    cfg.icons = Some(icons().config_name().to_string());
    cfg.save()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_config_resolves_when_no_env_override() {
        // These assertions only hold when the runner hasn't set the env overrides (the normal case);
        // guard so a dev with NEUROHELMET_THEME/PROFILE exported doesn't see spurious failures.
        let cfg = Config {
            theme: Some("mocha".into()),
            profile: Some("modern".into()),
            icons: Some("nerd".into()),
            ..Default::default()
        };
        if std::env::var("NEUROHELMET_THEME").is_err() {
            assert_eq!(cfg.resolved_theme(), Theme::catppuccin_mocha());
        }
        if std::env::var("NEUROHELMET_PROFILE").is_err() {
            assert_eq!(cfg.resolved_profile(), DisplayProfile::Modern);
        }
    }

    #[test]
    fn auto_push_defaults_true_and_honors_override() {
        assert!(
            Config::default().resolved_auto_push(),
            "unset = push by default"
        );
        assert!(Config {
            log_auto_push: Some(true),
            ..Default::default()
        }
        .resolved_auto_push());
        assert!(!Config {
            log_auto_push: Some(false),
            ..Default::default()
        }
        .resolved_auto_push());
    }

    #[test]
    fn log_repo_resolves_and_expands_tilde() {
        assert_eq!(
            Config::default().resolved_log_repo(),
            None,
            "unset = gh flow"
        );
        // An empty string is treated as unset, not as the current directory.
        assert_eq!(
            Config {
                log_repo: Some(String::new()),
                ..Default::default()
            }
            .resolved_log_repo(),
            None
        );
        // An absolute path passes through untouched.
        assert_eq!(
            Config {
                log_repo: Some("/srv/logs".into()),
                ..Default::default()
            }
            .resolved_log_repo(),
            Some(PathBuf::from("/srv/logs"))
        );
        // A leading `~/` expands to the home dir (only assert when we can resolve one).
        if let Some(home) = dirs::home_dir() {
            assert_eq!(
                Config {
                    log_repo: Some("~/neurohelmet-logs".into()),
                    ..Default::default()
                }
                .resolved_log_repo(),
                Some(home.join("neurohelmet-logs"))
            );
        }
    }

    #[test]
    fn empty_and_unknown_fall_through_to_defaults() {
        let bogus = Config {
            theme: Some("bogus".into()),
            profile: Some("x".into()),
            icons: Some("x".into()),
            ..Default::default()
        };
        for cfg in [Config::default(), bogus] {
            if std::env::var("NEUROHELMET_THEME").is_err() {
                assert_eq!(cfg.resolved_theme(), Theme::auto());
            }
            if std::env::var("NEUROHELMET_PROFILE").is_err() {
                assert_eq!(cfg.resolved_profile(), DisplayProfile::Pi);
            }
        }
    }
}
