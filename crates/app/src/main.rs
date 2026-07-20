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

//! neurohelmet: a keyboard-driven ratatui BattleTech record-sheet tracker.

mod export;
mod pdf;
mod publish;
mod render;
mod tui;

use neurohelmet_core::data::bundle::Bundle;
use neurohelmet_core::domain::{Facing, Location};
use neurohelmet_core::session::{self, Session};
use std::path::Path;

/// Load the baked dataset. Prefers `NEUROHELMET_DATA` (a path to a bundle file) for development;
/// otherwise uses the dataset embedded at build time so the Pi binary is self-contained.
fn load_bundle() -> color_eyre::Result<Bundle> {
    if let Ok(p) = std::env::var("NEUROHELMET_DATA") {
        return Ok(Bundle::load(Path::new(&p))?);
    }
    static DATA: &[u8] = include_bytes!("../../../data/mechs.bin");
    Ok(Bundle::decode(DATA)?)
}

/// Headless render of the tracker with the real dataset (no terminal needed).
fn selftest() -> color_eyre::Result<()> {
    let bundle = load_bundle()?;
    println!("loaded {} mechs", bundle.mechs.len());
    let idx = bundle
        .mechs
        .iter()
        .position(|m| m.chassis == "Atlas" && m.model == "AS7-D")
        .unwrap_or(0);
    let mut session = Session::new();
    if let Some(m) = bundle.get(idx).cloned() {
        session.add_mech(m);
    }
    if let Some(tm) = session.active_mech_mut() {
        tm.damage(Location::CenterTorso, Facing::Front, 20);
        tm.damage(Location::LeftArm, Facing::Front, 9999); // destroy a limb (cascades inward)
        tm.adjust_heat(8);
        // Fire the AC/20 three times to show weapon->ammo auto-linking + heat.
        if let Some(id) = tm
            .spec
            .weapons
            .iter()
            .find(|w| w.name == "AC/20")
            .map(|w| w.id)
        {
            for _ in 0..3 {
                tm.fire_weapon(id);
            }
        }
    }
    print!("{}", tui::render_once(bundle, session, 100, 30));
    Ok(())
}

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    // Before anything that needs a terminal — a bug report should be able to ask for this over a
    // chat window without the reporter having to find a TTY.
    if std::env::args().any(|a| a == "--version" || a == "-V") {
        println!("neurohelmet {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    if std::env::args().any(|a| a == "--selftest") {
        return selftest();
    }

    if std::env::args().any(|a| a == "--term-size") {
        match ratatui::crossterm::terminal::size() {
            Ok((cols, rows)) => println!("crossterm detects: {cols} cols x {rows} rows"),
            Err(e) => println!("crossterm size error: {e}"),
        }
        return Ok(());
    }

    // `--export <session> [outdir]`: render the game log to PPM frames + a transcript.
    let args: Vec<String> = std::env::args().collect();
    if let Some(i) = args.iter().position(|a| a == "--export") {
        let Some(name) = args.get(i + 1) else {
            eprintln!("usage: neurohelmet --export <session> [outdir]");
            std::process::exit(2);
        };
        let outdir = args.get(i + 2).map(std::path::PathBuf::from);
        export::run(name, outdir)?;
        return Ok(());
    }

    // `--publish <session> [--dry-run]`: render PNGs and push them to the GitHub logs gallery.
    if let Some(i) = args.iter().position(|a| a == "--publish") {
        let Some(name) = args.get(i + 1).filter(|a| !a.starts_with("--")) else {
            eprintln!("usage: neurohelmet --publish <session> [--dry-run]");
            std::process::exit(2);
        };
        let dry_run = args.iter().any(|a| a == "--dry-run");
        publish::run(name, dry_run)?;
        return Ok(());
    }

    // `--pdf <session> [outfile]`: render a BF/SBF/ACS session's blank record sheets to one
    // multi-page PDF.
    if let Some(i) = args.iter().position(|a| a == "--pdf") {
        let Some(name) = args.get(i + 1).filter(|a| !a.starts_with("--")) else {
            eprintln!("usage: neurohelmet --pdf <session> [outfile]");
            std::process::exit(2);
        };
        let outfile = args
            .get(i + 2)
            .filter(|a| !a.starts_with("--"))
            .map(std::path::PathBuf::from);
        pdf::run(name, outfile)?;
        return Ok(());
    }

    let bundle = load_bundle()?;
    session::migrate_legacy()?;
    let current = session::read_current().unwrap_or_else(|| "default".to_string());
    let session = session::load_named(&current)?.unwrap_or_default();

    let terminal = ratatui::init();
    let res = tui::run(terminal, bundle, session, current);
    ratatui::restore();
    res
}
