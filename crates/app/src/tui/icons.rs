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

//! §36 — icon set (Nerd Font glyphs vs plain text). A third display axis alongside
//! [`super::theme`] + [`super::profile`]: `Ascii` (the default — universal Unicode symbols) vs
//! `Nerd` (Private-Use-Area glyphs from a patched Nerd Font, e.g. MesloLGS NF).
//!
//! **Opt-in, never auto-detected.** A Nerd Font's presence can't be probed from a TUI, so `Nerd`
//! must be chosen explicitly (`NEUROHELMET_ICONS=nerd`, the saved config, or the Ctrl-T picker); without
//! a Nerd Font its glyphs render as tofu. Every icon is routed through a named accessor here that
//! returns the Nerd glyph *or* an `Ascii` fallback — the fallbacks match the pre-icon characters, so
//! `Ascii` mode is a no-op (snapshots unchanged). All glyphs are single-cell in a *mono* Nerd Font,
//! keeping the box-grid aligned.
//!
//! The Nerd codepoints below are the maintainer's best-known values; if any render as tofu in your
//! font, they're all in this one file to adjust. PUA glyphs are written as `\u{…}` with the
//! `nf-*` name in a comment.

use neurohelmet_core::domain::UnitType;
use std::cell::Cell;

/// Which glyph vocabulary the renderer uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IconSet {
    /// Universal Unicode symbols (the default; works in any monospace font).
    Ascii,
    /// Nerd Font Private-Use-Area glyphs (needs a patched Nerd Font).
    Nerd,
}

impl IconSet {
    pub fn from_name(name: &str) -> Option<IconSet> {
        match name.trim().to_ascii_lowercase().as_str() {
            "ascii" | "text" | "plain" | "off" => Some(IconSet::Ascii),
            "nerd" | "nerdfont" | "icons" | "on" => Some(IconSet::Nerd),
            _ => None,
        }
    }

    pub fn config_name(self) -> &'static str {
        match self {
            IconSet::Ascii => "ascii",
            IconSet::Nerd => "nerd",
        }
    }

    /// Short label for the Ctrl-T picker row.
    pub fn label(self) -> &'static str {
        match self {
            IconSet::Ascii => "Text",
            IconSet::Nerd => "Nerd Font",
        }
    }
}

thread_local! {
    static ACTIVE: Cell<IconSet> = const { Cell::new(IconSet::Ascii) };
}

/// The active icon set.
pub fn icons() -> IconSet {
    ACTIVE.with(|c| c.get())
}

/// Install the active icon set (startup + Ctrl-T toggle).
pub fn set_icons(set: IconSet) {
    ACTIVE.with(|c| c.set(set));
}

/// Pick the Nerd glyph or the Ascii fallback for the active set.
fn pick(nerd: &'static str, ascii: &'static str) -> &'static str {
    match icons() {
        IconSet::Nerd => nerd,
        IconSet::Ascii => ascii,
    }
}

// ---- Force-sidebar condition glyphs (fallbacks match the pre-icon ● ◐ ✖). ----

/// Unit healthy / fully operational.
pub fn cond_ok() -> &'static str {
    pick("\u{f06a9}", "\u{25cf}") // nf-md-robot · ●
}

/// Unit damaged / stressed (section lost, pilot hit, dangerous heat).
pub fn cond_damaged() -> &'static str {
    pick("\u{f071}", "\u{25d0}") // nf-fa-warning (exclamation-triangle) · ◐
}

/// Unit out of action.
pub fn cond_destroyed() -> &'static str {
    pick("\u{f068c}", "\u{2716}") // nf-md-skull · ✖
}

/// Per-[`UnitType`] picker glyph. `Ascii` returns `""` — these are *additive* (no glyph before
/// icons existed), so text mode leaves the picker rows exactly as they were.
pub fn unit_type(ut: UnitType) -> &'static str {
    match icons() {
        IconSet::Ascii => "",
        IconSet::Nerd => match ut {
            UnitType::Mech => "\u{f06a9}",        // nf-md-robot
            UnitType::Vehicle => "\u{f0d1}",      // nf-fa-truck
            UnitType::Infantry => "\u{f007}",     // nf-fa-user
            UnitType::BattleArmor => "\u{f132}",  // nf-fa-shield
            UnitType::Aerospace => "\u{f072}",    // nf-fa-plane
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_is_default_and_a_no_op() {
        assert_eq!(icons(), IconSet::Ascii);
        assert_eq!(cond_ok(), "\u{25cf}");
        assert_eq!(cond_damaged(), "\u{25d0}");
        assert_eq!(cond_destroyed(), "\u{2716}");
        assert_eq!(unit_type(UnitType::Mech), ""); // additive → nothing in text mode
    }

    #[test]
    fn nerd_swaps_the_glyphs() {
        set_icons(IconSet::Nerd);
        assert_ne!(cond_ok(), "\u{25cf}");
        assert!(!unit_type(UnitType::Vehicle).is_empty());
        set_icons(IconSet::Ascii); // restore for other tests on this thread
    }

    #[test]
    fn from_name_round_trips() {
        assert_eq!(IconSet::from_name("nerd"), Some(IconSet::Nerd));
        assert_eq!(IconSet::from_name("ASCII"), Some(IconSet::Ascii));
        assert_eq!(IconSet::from_name("off"), Some(IconSet::Ascii));
        assert_eq!(IconSet::from_name("?"), None);
    }
}
