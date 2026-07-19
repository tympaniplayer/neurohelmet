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

//! Docs screenshot generator: renders curated scenes with the real dataset through the same
//! rasterizer as `--export`, writing the PNGs the guide (docs/guide) embeds. Ignored by default —
//! regenerate after a UI change with:
//!
//! ```sh
//! cargo test -p neurohelmet --release docs_screenshots -- --ignored --nocapture
//! ```
//!
//! Output dir: `$NEUROHELMET_SHOTS_DIR`, default `../../docs/guide/src/images` (the docs image dir
//! when run from the workspace via cargo). Scenes are deterministic: fixed units, fixed key
//! sequences, fixed 100×30 (Pi) / 120×34 (Modern) geometry, scale-2 glyphs.

use super::app::{App, EquipRow, Focus};
use super::icons::{set_icons, IconSet};
use super::profile::{set_profile, DisplayProfile};
use super::theme::{set_theme, Theme};
use super::view;
use crate::render;
use neurohelmet_core::data::bundle::Bundle;
use neurohelmet_core::domain::{Facing, GameMode, Location, Mech};
use neurohelmet_core::session::{self, SbfDoctrine, Session};
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Terminal;
use std::path::PathBuf;

fn real_bundle() -> Bundle {
    static DATA: &[u8] = include_bytes!("../../../../data/mechs.bin");
    Bundle::decode(DATA).expect("decode embedded bundle")
}

/// Find a unit by `"Chassis Model"` (case-insensitive; exact match preferred, else substring)
/// and return a clone of its spec.
fn pick(bundle: &Bundle, query: &str) -> Mech {
    let q = query.to_ascii_lowercase();
    let full = |m: &Mech| format!("{} {}", m.chassis, m.model).to_ascii_lowercase();
    let idx = bundle
        .mechs
        .iter()
        .position(|m| full(m) == q)
        .or_else(|| bundle.mechs.iter().position(|m| full(m).contains(&q)))
        .unwrap_or_else(|| panic!("no unit matching {query:?} in the bundle"));
    let m = bundle.get(idx).expect("indexed unit").clone();
    println!("  pick {query:?} -> {} {}", m.chassis, m.model);
    m
}

fn press(app: &mut App, code: KeyCode) {
    app.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
}

fn press_ctrl(app: &mut App, code: KeyCode) {
    app.handle_key(KeyEvent::new(code, KeyModifiers::CONTROL));
}

fn type_str(app: &mut App, s: &str) {
    for c in s.chars() {
        press(app, KeyCode::Char(c));
    }
}

fn outdir() -> PathBuf {
    let dir = std::env::var("NEUROHELMET_SHOTS_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("../../docs/guide/src/images"));
    std::fs::create_dir_all(&dir).expect("create shots dir");
    dir
}

/// Draw one frame at `w`×`h` and write it as a scale-2 PNG.
fn shot(app: &mut App, name: &str, w: u16, h: u16) {
    let backend = TestBackend::new(w, h);
    let mut terminal = Terminal::new(backend).expect("test backend");
    terminal.draw(|f| view::draw(f, app)).expect("draw");
    let img = render::rasterize(terminal.backend().buffer(), 2);
    let path = outdir().join(name);
    img.write_png(&path).expect("write png");
    println!("wrote {} ({}x{})", path.display(), img.w, img.h);
}

/// Same formula as `tests::isolate_data_dir` — keep every disk write out of the real data dir.
fn isolate_data_dir() {
    use std::sync::Once;
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let dir = std::env::temp_dir().join(format!("neurohelmet-test-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        std::env::set_var("NEUROHELMET_DIR", &dir);
    });
}

/// The Classic hero scene: a battle-worn Atlas leading a Davion medium lance.
fn classic_lance_app() -> App {
    let b = real_bundle();
    let mut s = Session::new();
    s.add_mech(pick(&b, "Phoenix Hawk PXH-1"));
    s.add_mech(pick(&b, "Wolverine WVR-6R"));
    s.add_mech(pick(&b, "Shadow Hawk SHD-2H"));
    s.add_mech(pick(&b, "Atlas AS7-D"));
    if let Some(tm) = s.active_mech_mut() {
        tm.damage(Location::CenterTorso, Facing::Front, 18);
        // Exactly enough to destroy the arm (34 armor + 17 structure) without cascading inward.
        tm.damage(Location::LeftArm, Facing::Front, 51);
        tm.damage(Location::RightTorso, Facing::Rear, 6);
        tm.adjust_heat(6);
        let ac20 = tm
            .spec
            .weapons
            .iter()
            .find(|w| w.name.contains("AC/20"))
            .map(|w| w.id);
        if let Some(id) = ac20 {
            tm.fire_weapon(id);
        }
    }
    let mut app = App::new(b, s, "solaris-duel".to_string());
    app.dirty = false;
    app
}

#[test]
#[ignore = "docs generator, not a test — writes PNGs into docs/guide/src/images"]
fn docs_screenshots() {
    isolate_data_dir();
    set_theme(Theme::truecolor());
    set_profile(DisplayProfile::Pi);
    set_icons(IconSet::Ascii);

    // ---- Classic tracker + its popups ----
    let mut app = classic_lance_app();
    shot(&mut app, "classic-overview.png", 100, 30);
    press(&mut app, KeyCode::Char('c'));
    shot(&mut app, "classic-crits.png", 100, 30);
    press(&mut app, KeyCode::Esc);
    press(&mut app, KeyCode::Char('t'));
    shot(&mut app, "classic-tohit.png", 100, 30);
    press(&mut app, KeyCode::Esc);
    // Dice reference over a cluster weapon (the Atlas's LRM 20).
    let lrm = app.session.active_mech().and_then(|tm| {
        tm.spec
            .weapons
            .iter()
            .find(|w| w.name.contains("LRM"))
            .map(|w| w.id)
    });
    if let Some(id) = lrm {
        app.focus = Focus::Equipment;
        if let Some(i) = app
            .equip_rows()
            .iter()
            .position(|r| matches!(r, EquipRow::Weapon(w) if *w == id))
        {
            app.equip_sel = i;
        }
        press(&mut app, KeyCode::Char('r'));
        shot(&mut app, "classic-dice.png", 100, 30);
        press(&mut app, KeyCode::Esc);
        app.focus = Focus::Doll;
    }
    press(&mut app, KeyCode::Char('?'));
    shot(&mut app, "classic-help.png", 100, 30);
    press(&mut app, KeyCode::Esc);

    // Theme picker over the Classic scene, then the theme gallery.
    press_ctrl(&mut app, KeyCode::Char('t'));
    shot(&mut app, "theme-picker.png", 100, 30);
    press(&mut app, KeyCode::Esc);
    for name in ["cockpit", "mocha", "davion", "jade-falcon"] {
        set_theme(Theme::from_name(name).expect("known theme"));
        shot(&mut app, &format!("theme-{name}.png"), 100, 30);
    }
    set_theme(Theme::truecolor());

    // Modern layout profile: the same lance with the force sidebar.
    set_profile(DisplayProfile::Modern);
    app.status = String::new(); // drop the theme picker's "Display: unchanged" note
    shot(&mut app, "modern-lance.png", 120, 34);
    set_profile(DisplayProfile::Pi);

    // ---- Classic: a combat vehicle (motive damage popup) ----
    let b = real_bundle();
    let mut s = Session::new();
    s.add_mech(pick(&b, "Manticore Heavy Tank"));
    let mut app = App::new(b, s, "armor-column".to_string());
    if let Some(tm) = app.session.active_mech_mut() {
        tm.damage(Location::Front, Facing::Front, 8);
    }
    press(&mut app, KeyCode::Char('m'));
    shot(&mut app, "classic-vehicle.png", 100, 30);
    press(&mut app, KeyCode::Esc);

    // ---- Classic: an aerospace fighter (arcs + SI) ----
    let b = real_bundle();
    let mut s = Session::new();
    s.add_mech(pick(&b, "Stuka STU-K5"));
    let mut app = App::new(b, s, "air-lance".to_string());
    if let Some(tm) = app.session.active_mech_mut() {
        tm.damage(Location::Nose, Facing::Front, 12);
        tm.adjust_heat(4);
    }
    shot(&mut app, "classic-aero.png", 100, 30);

    // ---- Alpha Strike cards ----
    let b = real_bundle();
    let mut s = Session::new_with_mode(GameMode::AlphaStrike);
    s.add_mech(pick(&b, "Locust LCT-1V"));
    s.add_mech(pick(&b, "Catapult CPLT-C1"));
    s.add_mech(pick(&b, "Atlas AS7-D"));
    s.add_mech(pick(&b, "Mad Cat (Timber Wolf) Prime"));
    let mut app = App::new(b, s, "trial-of-position".to_string());
    app.dirty = false;
    for _ in 0..5 {
        press(&mut app, KeyCode::Char(' ')); // chip the active card's armor
    }
    press(&mut app, KeyCode::Char('o')); // heat 1
    press(&mut app, KeyCode::Char('o')); // heat 2
    shot(&mut app, "as-cards.png", 100, 30);

    // ---- Override card ----
    let b = real_bundle();
    let mut s = Session::new_with_mode(GameMode::Override);
    s.add_mech(pick(&b, "Mad Cat (Timber Wolf) Prime"));
    let mut app = App::new(b, s, "steel-viper-raid".to_string());
    app.dirty = false;
    press(&mut app, KeyCode::Char(' ')); // mark damage on the focused region
    press(&mut app, KeyCode::Char(' '));
    press(&mut app, KeyCode::Char('o')); // bank heat
    shot(&mut app, "override-card.png", 100, 30);

    // ---- BattleForce: lances of AS elements + the group editor ----
    let b = real_bundle();
    let mut s = Session::new_with_mode(GameMode::BattleForce);
    for q in [
        "Atlas AS7-D",
        "Marauder MAD-3R",
        "Warhammer WHM-6R",
        "Archer ARC-2R",
        "Phoenix Hawk PXH-1",
        "Shadow Hawk SHD-2H",
        "Griffin GRF-1N",
        "Wolverine WVR-6R",
    ] {
        s.add_mech(pick(&b, q));
    }
    s.bf_group_doctrine(SbfDoctrine::InnerSphere);
    let mut app = App::new(b, s, "capellan-front".to_string());
    app.dirty = false;
    press(&mut app, KeyCode::Char(' '));
    press(&mut app, KeyCode::Char(' '));
    press(&mut app, KeyCode::Char('o'));
    shot(&mut app, "bf-screen.png", 100, 30);
    press(&mut app, KeyCode::Char('g'));
    shot(&mut app, "bf-groups.png", 100, 30);
    press(&mut app, KeyCode::Esc);

    // ---- Strategic BattleForce: a doctrine-grouped company ----
    let b = real_bundle();
    let mut s = Session::new_with_mode(GameMode::StrategicBattleForce);
    for q in [
        "Atlas AS7-D",
        "Marauder MAD-3R",
        "Warhammer WHM-6R",
        "Archer ARC-2R",
        "Rifleman RFL-3N",
        "Phoenix Hawk PXH-1",
        "Shadow Hawk SHD-2H",
        "Griffin GRF-1N",
        "Wolverine WVR-6R",
        "Wasp WSP-1A",
        "Stinger STG-3R",
        "Locust LCT-1V",
    ] {
        s.add_mech(pick(&b, q));
    }
    s.sbf_group_doctrine(SbfDoctrine::InnerSphere);
    // A sample blank record sheet from the same force, for the PDF-export docs page.
    session::save_named("tukayyid", &s).expect("save");
    crate::pdf::run("tukayyid", Some(outdir().join("sbf-record-sheet.pdf"))).expect("pdf");
    let mut app = App::new(b, s, "tukayyid".to_string());
    app.dirty = false;
    press(&mut app, KeyCode::Char(' '));
    press(&mut app, KeyCode::Char(' '));
    press(&mut app, KeyCode::Char(' '));
    shot(&mut app, "sbf-screen.png", 100, 30);

    // ---- Abstract Combat System: three panes ----
    let b = real_bundle();
    let mut s = Session::new_with_mode(GameMode::AbstractCombatSystem);
    s.acs.formations.clear();
    let names: Vec<&str> = vec![
        "Atlas AS7-D",
        "Marauder MAD-3R",
        "Warhammer WHM-6R",
        "Archer ARC-2R",
        "Rifleman RFL-3N",
        "Phoenix Hawk PXH-1",
        "Shadow Hawk SHD-2H",
        "Griffin GRF-1N",
        "Wolverine WVR-6R",
        "Wasp WSP-1A",
        "Stinger STG-3R",
        "Locust LCT-1V",
    ];
    for q in &names {
        s.add_mech(pick(&b, q));
    }
    let n = names.len();
    s.acs_new_formation("1st Davion Guards", 0..n);
    let mut app = App::new(b, s, "planetary-assault".to_string());
    app.dirty = false;
    shot(&mut app, "acs-screen.png", 100, 30);

    // ---- Sessions browser (isolated data dir; sessions across systems) ----
    let b = real_bundle();
    let mut classic = Session::new();
    classic.add_mech(pick(&b, "Atlas AS7-D"));
    classic.add_mech(pick(&b, "Marauder MAD-3R"));
    session::save_named("solaris-duel", &classic).expect("save");
    let mut asx = Session::new_with_mode(GameMode::AlphaStrike);
    asx.add_mech(pick(&b, "Mad Cat (Timber Wolf) Prime"));
    asx.add_mech(pick(&b, "Locust LCT-1V"));
    session::save_named("trial-of-position", &asx).expect("save");
    let mut ov = Session::new_with_mode(GameMode::Override);
    ov.add_mech(pick(&b, "Warhammer WHM-6R"));
    session::save_named("steel-viper-raid", &ov).expect("save");
    let mut sbf = Session::new_with_mode(GameMode::StrategicBattleForce);
    sbf.add_mech(pick(&b, "Wolverine WVR-6R"));
    session::save_named("tukayyid", &sbf).expect("save");
    let mut bf = Session::new_with_mode(GameMode::BattleForce);
    bf.add_mech(pick(&b, "Archer ARC-2R"));
    bf.add_mech(pick(&b, "Griffin GRF-1N"));
    session::save_named("capellan-front", &bf).expect("save");
    let mut acs = Session::new_with_mode(GameMode::AbstractCombatSystem);
    acs.add_mech(pick(&b, "Stinger STG-3R"));
    session::save_named("planetary-assault", &acs).expect("save");
    session::write_current("solaris-duel").expect("current");
    let mut app = App::new(b, classic, "solaris-duel".to_string());
    app.dirty = false;
    press(&mut app, KeyCode::Char('S'));
    shot(&mut app, "sessions-browser.png", 100, 30);

    // ---- The unit picker: search, preview, filters, force generator ----
    let b = real_bundle();
    let mut app = App::new(b, Session::new(), "new-game".to_string());
    type_str(&mut app, "atlas");
    shot(&mut app, "picker-search.png", 100, 30);
    for _ in 0..4 {
        press(&mut app, KeyCode::Down); // land on the classic AS7-D
    }
    press(&mut app, KeyCode::Tab); // stat preview for the selected row
    shot(&mut app, "picker-preview.png", 100, 30);
    press(&mut app, KeyCode::Esc);
    for _ in 0.."atlas".len() {
        press(&mut app, KeyCode::Backspace);
    }
    press_ctrl(&mut app, KeyCode::Char('f'));
    shot(&mut app, "picker-filters.png", 100, 30);
    press(&mut app, KeyCode::Esc);
    press_ctrl(&mut app, KeyCode::Char('g'));
    shot(&mut app, "picker-forcegen.png", 100, 30);
    press(&mut app, KeyCode::Esc);
}
