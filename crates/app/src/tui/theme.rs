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

//! §36 display-profile palette. A [`Theme`] is the semantic foreground palette the renderer
//! resolves every color through (`view.rs` reads `theme().accent`, `.danger`, … instead of bare
//! `Color::*`). The active theme is process-wide, set once at startup ([`set_theme`]); a thread-local
//! holding [`Theme::pi`] is the default, so headless renders (export, selftest) and the snapshot
//! tests stay on the original Pi-framebuffer ANSI palette with no visual change.
//!
//! **Scope (Phase 1):** themes recolor the *semantic foreground* slots only — the accent, status,
//! and rarity colors. Primary body text and the background still use the terminal's own defaults
//! (which on a Catppuccin terminal already match `Mocha`). Full background/base-fg painting and a
//! graded multi-stop heat ramp are deferred (see ROADMAP §36); the band logic here matches today's
//! three-band heat/resource coloring exactly.

use neurohelmet_core::domain::Rarity;
use ratatui::style::Color;
use std::cell::Cell;

/// The semantic foreground palette. `Copy` (all fields are `Color`, itself `Copy`) so the
/// thread-local accessor can hand back a value cheaply without borrows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Theme {
    /// Base text color for the whole frame. `Color::Reset` = use the terminal's own foreground
    /// (the `pi`/`truecolor` themes do this); a real color paints body text to fully take over.
    pub fg: Color,
    /// Base background for the whole frame. `Color::Reset` = use the terminal's own background;
    /// a real color paints the root so the theme owns the screen on any terminal.
    pub bg: Color,
    /// Dim/secondary text: hints, labels, separators, empty states (was the lone `DIM` const).
    pub dim: Color,
    /// Primary accent: selection background, focused borders, section headers, active markers.
    pub accent: Color,
    /// Secondary accent: the picker's role column (was `Color::Blue`).
    pub accent_alt: Color,
    /// Text drawn *on* an accent/bright background (selected rows, rarity-tinted chips).
    pub on_accent: Color,
    /// Strong/bright callout text (was `Color::White`).
    pub fg_strong: Color,
    /// Healthy / ok / fired / bonus / under-budget (was `Color::Green`).
    pub good: Color,
    /// Caution: mid heat, degraded systems, non-standard munition, PSR due (was `Color::Yellow`).
    pub warning: Color,
    /// Danger: destroyed, high heat, penalties, KIA (was `Color::Red`).
    pub danger: Color,
    // §35 rarity tiers. `Unknown` uses `dim`; the rest are distinct so the availability lens reads
    // at a glance.
    pub rarity_not_avail: Color,
    pub rarity_very_rare: Color,
    pub rarity_rare: Color,
    pub rarity_uncommon: Color,
    pub rarity_common: Color,
    pub rarity_very_common: Color,
}

impl Theme {
    /// §35 rarity → tint.
    pub fn rarity(&self, r: Rarity) -> Color {
        match r {
            Rarity::Unknown => self.dim,
            Rarity::NotAvailable => self.rarity_not_avail,
            Rarity::VeryRare => self.rarity_very_rare,
            Rarity::Rare => self.rarity_rare,
            Rarity::Uncommon => self.rarity_uncommon,
            Rarity::Common => self.rarity_common,
            Rarity::VeryCommon => self.rarity_very_common,
        }
    }

    /// Resource-fraction color (ammo rem/max): >50% good, >25% caution, else danger; empty = dim.
    pub fn frac(&self, rem: u16, max: u16) -> Color {
        if max == 0 || rem == 0 {
            return self.dim;
        }
        let f = rem as f32 / max as f32;
        if f > 0.5 {
            self.good
        } else if f > 0.25 {
            self.warning
        } else {
            self.danger
        }
    }

    /// 'Mech heat band (0–30 scale): 0–9 good, 10–19 caution, 20+ danger.
    pub fn heat_mech(&self, heat: i32) -> Color {
        if heat >= 20 {
            self.danger
        } else if heat >= 10 {
            self.warning
        } else {
            self.good
        }
    }

    /// Override heat band (0–5 ladder): 0–1 good, 2–3 caution, 4–5 danger.
    pub fn heat_ov(&self, heat: u8) -> Color {
        match heat {
            0..=1 => self.good,
            2..=3 => self.warning,
            _ => self.danger,
        }
    }

    /// The original Pi-framebuffer palette: the 16-color ANSI set used before §36. This is the
    /// default everywhere, so anything that doesn't opt into a modern theme renders identically.
    pub const fn pi() -> Theme {
        Theme {
            // Respect the terminal's own fg/bg — the Pi-framebuffer default.
            fg: Color::Reset,
            bg: Color::Reset,
            // `Gray` (ANSI 7), deliberately not `DarkGray` (ANSI 8) which is near-invisible on a
            // Linux framebuffer / small Pi LCD.
            dim: Color::Gray,
            accent: Color::Cyan,
            accent_alt: Color::Blue,
            on_accent: Color::Black,
            fg_strong: Color::White,
            good: Color::Green,
            warning: Color::Yellow,
            danger: Color::Red,
            rarity_not_avail: Color::Gray,
            rarity_very_rare: Color::Red,
            rarity_rare: Color::LightRed,
            rarity_uncommon: Color::Yellow,
            rarity_common: Color::Green,
            rarity_very_common: Color::LightGreen,
        }
    }

    /// "Cockpit MFD" — diegetic 'Mech console styled like a modern fighter-jet multi-function display:
    /// green-phosphor primary on near-black, with amber CAUTION and red WARNING alert tiers. (Amber is
    /// the caution color here, not the chrome — which keeps it clearly distinct from House Davion's gold.)
    pub const fn cockpit() -> Theme {
        Theme {
            fg: Color::Rgb(0x9C, 0xD6, 0xB0),
            bg: Color::Rgb(0x03, 0x08, 0x05),
            dim: Color::Rgb(0x48, 0x6A, 0x54),
            accent: Color::Rgb(0x35, 0xF5, 0x8C),
            accent_alt: Color::Rgb(0x24, 0xB8, 0x6C),
            on_accent: Color::Rgb(0x02, 0x12, 0x0A),
            fg_strong: Color::Rgb(0xDC, 0xFF, 0xE6),
            good: Color::Rgb(0x7C, 0xE0, 0x5C),
            warning: Color::Rgb(0xFF, 0xB0, 0x00),
            danger: Color::Rgb(0xFF, 0x3B, 0x30),
            rarity_not_avail: Color::Rgb(0x35, 0x50, 0x3E),
            rarity_very_rare: Color::Rgb(0xFF, 0x3B, 0x30),
            rarity_rare: Color::Rgb(0xFF, 0x8C, 0x1A),
            rarity_uncommon: Color::Rgb(0xFF, 0xC2, 0x33),
            rarity_common: Color::Rgb(0x7C, 0xE0, 0x5C),
            rarity_very_common: Color::Rgb(0x9C, 0xFF, 0x7A),
        }
    }

    /// "Tokyo Night" — soft slate, muted-but-distinct accents; easy on the eyes for long sessions.
    pub const fn tokyo_night() -> Theme {
        Theme {
            fg: Color::Rgb(0xC0, 0xCA, 0xF5),
            bg: Color::Rgb(0x1A, 0x1B, 0x26),
            dim: Color::Rgb(0x56, 0x5F, 0x89),
            accent: Color::Rgb(0x7D, 0xCF, 0xFF),
            accent_alt: Color::Rgb(0x7A, 0xA2, 0xF7),
            on_accent: Color::Rgb(0x1A, 0x1B, 0x26),
            fg_strong: Color::Rgb(0xC0, 0xCA, 0xF5),
            good: Color::Rgb(0x9E, 0xCE, 0x6A),
            warning: Color::Rgb(0xE0, 0xAF, 0x68),
            danger: Color::Rgb(0xF7, 0x76, 0x8E),
            rarity_not_avail: Color::Rgb(0x41, 0x48, 0x68),
            rarity_very_rare: Color::Rgb(0xF7, 0x76, 0x8E),
            rarity_rare: Color::Rgb(0xFF, 0x9E, 0x64),
            rarity_uncommon: Color::Rgb(0xE0, 0xAF, 0x68),
            rarity_common: Color::Rgb(0x9E, 0xCE, 0x6A),
            rarity_very_common: Color::Rgb(0x73, 0xDA, 0xCA),
        }
    }

    /// "Richer truecolor" — the original look, finer shades: the same semantics as `pi` but tuned
    /// 24-bit hues instead of the 16-color ANSI approximations.
    pub const fn truecolor() -> Theme {
        Theme {
            // The auto-default for truecolor terminals: respect their own fg/bg, recolor accents.
            fg: Color::Reset,
            bg: Color::Reset,
            dim: Color::Rgb(0x8A, 0x8F, 0x98),
            accent: Color::Rgb(0x2A, 0xA9, 0xC4),
            accent_alt: Color::Rgb(0x4C, 0x8B, 0xF5),
            on_accent: Color::Rgb(0x0B, 0x0B, 0x0B),
            fg_strong: Color::Rgb(0xFF, 0xFF, 0xFF),
            good: Color::Rgb(0x2F, 0xBF, 0x5B),
            warning: Color::Rgb(0xE6, 0xB4, 0x22),
            danger: Color::Rgb(0xE5, 0x48, 0x4D),
            rarity_not_avail: Color::Rgb(0x4A, 0x4A, 0x4A),
            rarity_very_rare: Color::Rgb(0xE5, 0x48, 0x4D),
            rarity_rare: Color::Rgb(0xF2, 0x79, 0x2B),
            rarity_uncommon: Color::Rgb(0xE6, 0xB4, 0x22),
            rarity_common: Color::Rgb(0x2F, 0xBF, 0x5B),
            rarity_very_common: Color::Rgb(0x5C, 0xE0, 0x8A),
        }
    }

    /// "Catppuccin Mocha" — Nate's Alacritty palette (the upstream Mocha flavor), so the app blends
    /// into the terminal it runs in.
    pub const fn catppuccin_mocha() -> Theme {
        Theme {
            fg: Color::Rgb(0xCD, 0xD6, 0xF4),                 // Text
            bg: Color::Rgb(0x1E, 0x1E, 0x2E),                 // Base
            dim: Color::Rgb(0x93, 0x99, 0xB2),                // Overlay2
            accent: Color::Rgb(0x89, 0xDC, 0xEB),             // Sky
            accent_alt: Color::Rgb(0x89, 0xB4, 0xFA),         // Blue
            on_accent: Color::Rgb(0x1E, 0x1E, 0x2E),          // Base
            fg_strong: Color::Rgb(0xCD, 0xD6, 0xF4),          // Text
            good: Color::Rgb(0xA6, 0xE3, 0xA1),               // Green
            warning: Color::Rgb(0xF9, 0xE2, 0xAF),            // Yellow
            danger: Color::Rgb(0xF3, 0x8B, 0xA8),             // Red
            rarity_not_avail: Color::Rgb(0x58, 0x5B, 0x70),   // Surface2
            rarity_very_rare: Color::Rgb(0xF3, 0x8B, 0xA8),   // Red
            rarity_rare: Color::Rgb(0xFA, 0xB3, 0x87),        // Peach
            rarity_uncommon: Color::Rgb(0xF9, 0xE2, 0xAF),    // Yellow
            rarity_common: Color::Rgb(0xA6, 0xE3, 0xA1),      // Green
            rarity_very_common: Color::Rgb(0x94, 0xE2, 0xD5), // Teal
        }
    }

    /// Build a faction (House/Clan) livery theme from its core colors, filling in a **shared**
    /// readable status ramp (good/warning/danger) and §35 rarity ramp so the heat bands and
    /// availability tiers stay legible regardless of house colors (a purple Marik card still shows
    /// green = healthy, red = destroyed). `on_accent` = the dark background, since every faction
    /// accent here is a bright livery color used as a selection fill.
    const fn faction(bg: Color, fg: Color, dim: Color, accent: Color, accent_alt: Color) -> Theme {
        Theme {
            fg,
            bg,
            dim,
            accent,
            accent_alt,
            on_accent: bg,
            fg_strong: fg,
            good: Color::Rgb(0x5C, 0xC8, 0x6A),
            warning: Color::Rgb(0xE8, 0xB0, 0x3A),
            danger: Color::Rgb(0xE5, 0x4B, 0x4B),
            rarity_not_avail: dim,
            rarity_very_rare: Color::Rgb(0xE5, 0x4B, 0x4B),
            rarity_rare: Color::Rgb(0xE8, 0x7B, 0x2E),
            rarity_uncommon: Color::Rgb(0xE8, 0xB0, 0x3A),
            rarity_common: Color::Rgb(0x5C, 0xC8, 0x6A),
            rarity_very_common: Color::Rgb(0x4F, 0xC9, 0xA8),
        }
    }

    // ---- The five Great Houses (Inner Sphere Successor States) ----

    /// House Davion — Federated Suns: gold sunburst, crimson accent, on near-black.
    pub const fn davion() -> Theme {
        Theme::faction(
            Color::Rgb(0x0E, 0x0B, 0x08),
            Color::Rgb(0xF0, 0xE6, 0xD2),
            Color::Rgb(0x7A, 0x6E, 0x58),
            Color::Rgb(0xE8, 0xB9, 0x23),
            Color::Rgb(0xC0, 0x39, 0x2B),
        )
    }

    /// House Steiner — Lyran Commonwealth: the azure mailed fist, steel-blue accent.
    pub const fn steiner() -> Theme {
        Theme::faction(
            Color::Rgb(0x0A, 0x10, 0x18),
            Color::Rgb(0xDC, 0xE8, 0xF5),
            Color::Rgb(0x5A, 0x6E, 0x85),
            Color::Rgb(0x3D, 0x8B, 0xE0),
            Color::Rgb(0xA9, 0xC4, 0xE0),
        )
    }

    /// House Marik — Free Worlds League: royal purple eagle with gold.
    pub const fn marik() -> Theme {
        Theme::faction(
            Color::Rgb(0x12, 0x0A, 0x1A),
            Color::Rgb(0xEC, 0xE0, 0xF5),
            Color::Rgb(0x6E, 0x5A, 0x85),
            Color::Rgb(0x9B, 0x59, 0xD0),
            Color::Rgb(0xE8, 0xB9, 0x23),
        )
    }

    /// House Liao — Capellan Confederation: the green talon, gold accent.
    pub const fn liao() -> Theme {
        Theme::faction(
            Color::Rgb(0x06, 0x12, 0x0A),
            Color::Rgb(0xDE, 0xF0, 0xE4),
            Color::Rgb(0x5A, 0x7A, 0x66),
            Color::Rgb(0x2E, 0xA8, 0x60),
            Color::Rgb(0xE8, 0xB9, 0x23),
        )
    }

    /// House Kurita — Draconis Combine: the crimson dragon on black, white accent.
    pub const fn kurita() -> Theme {
        Theme::faction(
            Color::Rgb(0x0C, 0x08, 0x08),
            Color::Rgb(0xF0, 0xDC, 0xDC),
            Color::Rgb(0x85, 0x60, 0x58),
            Color::Rgb(0xD8, 0x28, 0x3C),
            Color::Rgb(0xE8, 0xE8, 0xE8),
        )
    }

    // ---- Five Clans ----

    /// Clan Wolf — deep red on gunmetal, steel-grey accent.
    pub const fn clan_wolf() -> Theme {
        Theme::faction(
            Color::Rgb(0x13, 0x10, 0x10),
            Color::Rgb(0xE8, 0xE0, 0xDC),
            Color::Rgb(0x6E, 0x60, 0x58),
            Color::Rgb(0xB0, 0x26, 0x3A),
            Color::Rgb(0x8A, 0x90, 0x98),
        )
    }

    /// Clan Jade Falcon — jade green and gold.
    pub const fn clan_jade_falcon() -> Theme {
        Theme::faction(
            Color::Rgb(0x06, 0x12, 0x0C),
            Color::Rgb(0xDE, 0xF0, 0xE6),
            Color::Rgb(0x5A, 0x7A, 0x6A),
            Color::Rgb(0x12, 0xB8, 0x84),
            Color::Rgb(0xE8, 0xC2, 0x3A),
        )
    }

    /// Clan Smoke Jaguar — amber on charcoal, smoke-grey accent.
    pub const fn clan_smoke_jaguar() -> Theme {
        Theme::faction(
            Color::Rgb(0x10, 0x0E, 0x0C),
            Color::Rgb(0xEC, 0xE4, 0xD8),
            Color::Rgb(0x6E, 0x64, 0x58),
            Color::Rgb(0xD8, 0x90, 0x2B),
            Color::Rgb(0x8A, 0x80, 0x78),
        )
    }

    /// Clan Ghost Bear — icy blue and white on deep navy.
    pub const fn clan_ghost_bear() -> Theme {
        Theme::faction(
            Color::Rgb(0x0A, 0x14, 0x1E),
            Color::Rgb(0xE4, 0xEE, 0xF5),
            Color::Rgb(0x5A, 0x70, 0x85),
            Color::Rgb(0x5A, 0xB0, 0xE0),
            Color::Rgb(0xE8, 0xEE, 0xF2),
        )
    }

    /// Clan Sea Fox (formerly Diamond Shark) — teal and aqua.
    pub const fn clan_sea_fox() -> Theme {
        Theme::faction(
            Color::Rgb(0x06, 0x14, 0x14),
            Color::Rgb(0xDC, 0xF0, 0xEE),
            Color::Rgb(0x5A, 0x7A, 0x78),
            Color::Rgb(0x16, 0xA8, 0xA0),
            Color::Rgb(0x4F, 0xC9, 0xC0),
        )
    }

    /// Index of `self` in [`THEMES`] (by value), or 0 (`pi`) if it isn't one of the presets — used
    /// to seed the in-app theme picker on the current theme.
    pub fn preset_index(&self) -> usize {
        THEMES.iter().position(|(_, _, t)| t == self).unwrap_or(0)
    }

    /// Look up a theme by its config name. `None` for an unknown name (the caller falls back).
    pub fn from_name(name: &str) -> Option<Theme> {
        match name.trim().to_ascii_lowercase().as_str() {
            "pi" | "default" => Some(Theme::pi()),
            "cockpit" | "mfd" => Some(Theme::cockpit()),
            "tokyo" | "tokyo-night" | "tokyonight" => Some(Theme::tokyo_night()),
            "truecolor" | "rich" => Some(Theme::truecolor()),
            "mocha" | "catppuccin" | "catppuccin-mocha" => Some(Theme::catppuccin_mocha()),
            "davion" | "fedsuns" => Some(Theme::davion()),
            "steiner" | "lyran" => Some(Theme::steiner()),
            "marik" | "fwl" => Some(Theme::marik()),
            "liao" | "capellan" => Some(Theme::liao()),
            "kurita" | "combine" | "draconis" => Some(Theme::kurita()),
            "wolf" | "clan-wolf" => Some(Theme::clan_wolf()),
            "jade-falcon" | "jadefalcon" | "falcon" => Some(Theme::clan_jade_falcon()),
            "smoke-jaguar" | "smokejaguar" | "jaguar" => Some(Theme::clan_smoke_jaguar()),
            "ghost-bear" | "ghostbear" | "bear" => Some(Theme::clan_ghost_bear()),
            "sea-fox" | "seafox" | "diamond-shark" => Some(Theme::clan_sea_fox()),
            _ => None,
        }
    }

    /// This theme's canonical config name (the first alias for its preset), for saving to config.
    pub fn config_name(&self) -> &'static str {
        THEMES.get(self.preset_index()).map_or("pi", |(n, _, _)| n)
    }

    /// The terminal-derived default (no explicit choice): a truecolor terminal
    /// (`COLORTERM` = `truecolor`/`24bit`) gets `truecolor` (the faithful upgrade of the Pi palette);
    /// everything else gets the Pi ANSI palette.
    pub fn auto() -> Theme {
        match std::env::var("COLORTERM").as_deref() {
            Ok("truecolor") | Ok("24bit") => Theme::truecolor(),
            _ => Theme::pi(),
        }
    }
}

/// The selectable presets, in picker order: `(config name, display label, theme)`. Drives the
/// in-app Ctrl-T picker; the names match [`Theme::from_name`] / `NEUROHELMET_THEME`.
pub const THEMES: [(&str, &str, Theme); 15] = [
    ("pi", "Pi (16-color)", Theme::pi()),
    ("truecolor", "Truecolor", Theme::truecolor()),
    ("mocha", "Catppuccin Mocha", Theme::catppuccin_mocha()),
    ("tokyo", "Tokyo Night", Theme::tokyo_night()),
    ("cockpit", "Cockpit MFD", Theme::cockpit()),
    ("davion", "House Davion", Theme::davion()),
    ("steiner", "House Steiner", Theme::steiner()),
    ("marik", "House Marik", Theme::marik()),
    ("liao", "House Liao", Theme::liao()),
    ("kurita", "House Kurita", Theme::kurita()),
    ("wolf", "Clan Wolf", Theme::clan_wolf()),
    ("jade-falcon", "Clan Jade Falcon", Theme::clan_jade_falcon()),
    (
        "smoke-jaguar",
        "Clan Smoke Jaguar",
        Theme::clan_smoke_jaguar(),
    ),
    ("ghost-bear", "Clan Ghost Bear", Theme::clan_ghost_bear()),
    ("sea-fox", "Clan Sea Fox", Theme::clan_sea_fox()),
];

thread_local! {
    /// The active theme. Thread-local (the TUI is single-threaded) so each `cargo test` thread
    /// starts fresh on `Theme::pi` — no cross-test contamination — and a future hot-reload (§20)
    /// can just `set_theme` again.
    static ACTIVE: Cell<Theme> = const { Cell::new(Theme::pi()) };
}

/// The active theme. Cheap (returns a `Copy` value); call it at each color site.
pub fn theme() -> Theme {
    ACTIVE.with(|t| t.get())
}

/// Install the active theme for this thread (called once at startup; safe to call again to reload).
pub fn set_theme(t: Theme) {
    ACTIVE.with(|c| c.set(t));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pi_is_the_default() {
        assert_eq!(theme(), Theme::pi());
    }

    #[test]
    fn from_name_covers_every_theme() {
        assert_eq!(Theme::from_name("pi"), Some(Theme::pi()));
        assert_eq!(Theme::from_name("Cockpit"), Some(Theme::cockpit()));
        assert_eq!(Theme::from_name("tokyo-night"), Some(Theme::tokyo_night()));
        assert_eq!(Theme::from_name("truecolor"), Some(Theme::truecolor()));
        assert_eq!(Theme::from_name("mocha"), Some(Theme::catppuccin_mocha()));
        assert_eq!(Theme::from_name("nonsense"), None);
    }

    #[test]
    fn set_theme_takes_effect() {
        set_theme(Theme::catppuccin_mocha());
        assert_eq!(theme(), Theme::catppuccin_mocha());
        // restore so other tests on this thread see the default (thread-locals persist per thread).
        set_theme(Theme::pi());
    }

    #[test]
    fn terminal_respecting_themes_use_reset() {
        // pi + truecolor leave the terminal's own fg/bg → `draw` skips the root paint, so headless
        // and Pi renders stay byte-identical.
        for t in [Theme::pi(), Theme::truecolor()] {
            assert_eq!(t.fg, Color::Reset);
            assert_eq!(t.bg, Color::Reset);
        }
    }

    #[test]
    fn self_contained_themes_paint_a_background() {
        // cockpit/tokyo/mocha carry a real fg+bg so they own the whole screen on any terminal.
        for t in [
            Theme::cockpit(),
            Theme::tokyo_night(),
            Theme::catppuccin_mocha(),
        ] {
            assert_ne!(t.fg, Color::Reset);
            assert_ne!(t.bg, Color::Reset);
        }
    }

    #[test]
    fn pi_rarity_matches_legacy_ansi() {
        let t = Theme::pi();
        assert_eq!(t.rarity(Rarity::VeryRare), Color::Red);
        assert_eq!(t.rarity(Rarity::VeryCommon), Color::LightGreen);
        assert_eq!(t.rarity(Rarity::Unknown), Color::Gray);
    }
}
