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

//! Offline game-log export: read a session's JSONL log and render each turn to images (one per
//! mech, plus a stacked montage) and a text transcript. Reuses the real tracker UI via
//! `tui::render_to_buffer`, so the images are exactly what you'd see on screen. The frame helpers
//! here are shared with `publish` (which writes PNGs instead of PPMs).

use crate::render::{rasterize, Image};
use crate::tui;
use neurohelmet_core::log::{self, LogEntry};
use neurohelmet_core::session::{sanitize_name, sessions_dir, Session};
use neurohelmet_core::Result;
use ratatui::buffer::Buffer;
use std::path::PathBuf;

/// Pixels per cell side (8px font × this). 2 keeps text legible.
pub(crate) const SCALE: usize = 2;
/// Black gap between mechs in a turn montage.
const GAP: usize = 8;

/// Re-render one logged turn off-screen: `(filename-stem, heading, buffer)` frames. Non-SBF/BF
/// entries get one frame per mech (each `TrackedMech` embeds its spec, so entries are
/// self-contained). SBF entries with formation state get one formation-sheet frame per
/// formation; Standard BF entries with any live `bf` state get one BF-sheet frame per Unit (the
/// page holding its first member, headers included) — or a single Unassigned frame when the
/// roster holds no Units; only old logs without the `bf` field fall back to per-element AS
/// cards.
pub(crate) fn render_turn(entry: &LogEntry) -> Vec<(String, String, Buffer)> {
    use neurohelmet_core::domain::GameMode;
    if entry.mode == GameMode::StrategicBattleForce && !entry.sbf.formations.is_empty() {
        return entry
            .sbf
            .formations
            .iter()
            .enumerate()
            .map(|(i, formation)| {
                let mut s = Session::new_with_mode(GameMode::StrategicBattleForce);
                s.mechs = entry.mechs.clone();
                s.sbf = entry.sbf.clone(); // replaces the seeded starter formation
                s.sbf.active_formation = i;
                s.sbf.active_unit = 0;
                let stem = format!("{:02}-{}", i + 1, sanitize_name(&formation.name));
                (stem, formation.name.clone(), tui::render_to_buffer(s))
            })
            .collect();
    }
    if entry.mode == GameMode::BattleForce && entry.bf != Default::default() {
        // Any live BF field ⇒ render the real BF screen (mode parity for the game log). A BF
        // session can legitimately hold zero Units — all-unassigned rosters are first-class
        // (spec §2.3) — so an entry with no units still gets one frame paging the implicit
        // Unassigned section; only genuinely pre-`bf`-field logs (bf == default) fall through
        // to the per-element AS cards below.
        if entry.bf.units.is_empty() {
            let mut s = Session::new_with_mode(GameMode::BattleForce);
            s.mechs = entry.mechs.clone();
            s.bf = entry.bf.clone(); // replaces the seeded starter Unit
            s.active = 0;
            return vec![(
                "01-Unassigned".into(),
                "Unassigned".into(),
                tui::render_to_buffer(s),
            )];
        }
        return entry
            .bf
            .units
            .iter()
            .enumerate()
            .map(|(i, unit)| {
                let mut s = Session::new_with_mode(GameMode::BattleForce);
                s.mechs = entry.mechs.clone();
                s.bf = entry.bf.clone(); // replaces the seeded starter Unit
                s.bf.active_unit = i;
                // The BF screen pages on the active element; land on this Unit's first member so
                // its header row + cards are the frame.
                s.active = unit.elements.first().copied().unwrap_or(0);
                let stem = format!("{:02}-{}", i + 1, sanitize_name(&unit.name));
                (stem, unit.name.clone(), tui::render_to_buffer(s))
            })
            .collect();
    }
    entry
        .mechs
        .iter()
        .enumerate()
        .map(|(i, tm)| {
            // Re-render in the entry's game mode so an Override/AS snapshot exports its own card.
            // Pre-grouping-field SBF/BF logs carry element state only → AS cards.
            let mode = match entry.mode {
                GameMode::StrategicBattleForce | GameMode::BattleForce => GameMode::AlphaStrike,
                m => m,
            };
            let mut s = Session::new_with_mode(mode);
            s.mechs = vec![tm.clone()];
            s.active = 0;
            let stem = format!("{:02}-{}", i + 1, sanitize_name(&tm.spec.display_name()));
            (stem, tm.spec.display_name(), tui::render_to_buffer(s))
        })
        .collect()
}

/// Stack per-mech frames vertically (black gaps between) into one montage image.
pub(crate) fn montage(frames: &[Image]) -> Image {
    let w = frames.iter().map(|f| f.w).max().unwrap_or(1);
    let h: usize = frames.iter().map(|f| f.h).sum::<usize>() + GAP * frames.len().saturating_sub(1);
    let mut out = Image::new(w.max(1), h.max(1));
    let mut y = 0;
    for f in frames {
        out.blit(f, 0, y);
        y += f.h + GAP;
    }
    out
}

/// A buffer rendered to text (one line per row, trailing blanks trimmed).
pub(crate) fn buffer_to_text(buf: &Buffer) -> String {
    let mut out = String::new();
    for y in 0..buf.area.height {
        let mut row = String::new();
        for x in 0..buf.area.width {
            row.push_str(buf[(x, y)].symbol());
        }
        out.push_str(row.trim_end());
        out.push('\n');
    }
    out
}

/// Export `name`'s log to `outdir` (default `<sessions_dir>/<name>-log/`). Returns the directory.
pub fn run(name: &str, outdir: Option<PathBuf>) -> Result<PathBuf> {
    let entries = log::read_log(name)?;
    let out = outdir.unwrap_or_else(|| sessions_dir().join(format!("{}-log", sanitize_name(name))));
    std::fs::create_dir_all(&out)?;
    if entries.is_empty() {
        eprintln!("No game log for session '{name}' (nothing to export).");
        return Ok(out);
    }

    let mut transcript = String::new();
    for e in &entries {
        let tdir = out.join(format!("turn-{:02}", e.turn));
        std::fs::create_dir_all(&tdir)?;

        let mut images = Vec::new();
        for (stem, heading, buf) in &render_turn(e) {
            let img = rasterize(buf, SCALE);
            img.write_ppm(&tdir.join(format!("{stem}.ppm")))?;
            transcript.push_str(&format!("== {} — {} ==\n", e.label, heading));
            transcript.push_str(&buffer_to_text(buf));
            transcript.push('\n');
            images.push(img);
        }
        montage(&images).write_ppm(&out.join(format!("turn-{:02}.ppm", e.turn)))?;
    }

    std::fs::write(out.join("transcript.txt"), transcript)?;
    println!(
        "Exported {} turn(s) for '{}' to {}",
        entries.len(),
        name,
        out.display()
    );
    Ok(out)
}
