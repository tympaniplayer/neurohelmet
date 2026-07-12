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

//! §36 Phase 2 — display profile (layout density). [`DisplayProfile`] selects how roomy the layout
//! is: `Pi` is the terse single-pane default tuned for a ~100×30 Raspberry Pi screen; `Modern` uses
//! the extra space a laptop terminal has (currently: a persistent force sidebar on the play screens).
//!
//! Like the §36 Phase 1 [`super::theme`], the profile is process-wide, set once at startup
//! ([`set_profile`]) from `NEUROHELMET_PROFILE`, and held in a thread-local defaulting to `Pi` — so
//! headless renders (export/selftest) and the snapshot tests stay on the original single-pane layout
//! with no change. **Selection is explicit (env/flag), not size-auto-detected** (Nate, 2026-06-30):
//! the default stays `Pi`; `Modern` is opt-in. The renderer still falls back to the single-pane
//! layout per-region when a forced `Modern` terminal is too narrow for the extra panes.

use std::cell::Cell;

/// Layout density. `Pi` = terse single-pane (the default); `Modern` = roomier multi-pane.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DisplayProfile {
    Pi,
    Modern,
}

impl DisplayProfile {
    /// Look up a profile by config name. `None` for an unknown name (the caller falls back to `Pi`).
    pub fn from_name(name: &str) -> Option<DisplayProfile> {
        match name.trim().to_ascii_lowercase().as_str() {
            "pi" | "default" | "compact" => Some(DisplayProfile::Pi),
            "modern" | "laptop" | "wide" => Some(DisplayProfile::Modern),
            _ => None,
        }
    }

    /// This profile's canonical config name, for saving to config.
    pub fn config_name(self) -> &'static str {
        match self {
            DisplayProfile::Pi => "pi",
            DisplayProfile::Modern => "modern",
        }
    }
}

thread_local! {
    /// The active profile. Thread-local for the same reasons as the theme: fresh `Pi` per test
    /// thread (snapshots untouched) and a future hot-reload is just another `set_profile`.
    static ACTIVE: Cell<DisplayProfile> = const { Cell::new(DisplayProfile::Pi) };
}

/// The active display profile.
pub fn profile() -> DisplayProfile {
    ACTIVE.with(|p| p.get())
}

/// Install the active display profile for this thread (called once at startup).
pub fn set_profile(p: DisplayProfile) {
    ACTIVE.with(|c| c.set(p));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pi_is_the_default() {
        assert_eq!(profile(), DisplayProfile::Pi);
    }

    #[test]
    fn from_name_maps_known_and_rejects_unknown() {
        assert_eq!(
            DisplayProfile::from_name("modern"),
            Some(DisplayProfile::Modern)
        );
        assert_eq!(
            DisplayProfile::from_name("Laptop"),
            Some(DisplayProfile::Modern)
        );
        assert_eq!(DisplayProfile::from_name("pi"), Some(DisplayProfile::Pi));
        assert_eq!(DisplayProfile::from_name("nonsense"), None);
    }

    #[test]
    fn set_profile_takes_effect_then_restores() {
        set_profile(DisplayProfile::Modern);
        assert_eq!(profile(), DisplayProfile::Modern);
        set_profile(DisplayProfile::Pi); // restore for other tests on this thread
    }
}
