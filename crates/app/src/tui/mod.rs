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

//! Terminal app loop: draw, poll input, autosave, repeat.

mod app;
pub(crate) mod config;
mod filters;
mod forcegen;
mod icons;
mod picker;
mod profile;
mod theme;
mod view;

use app::App;
use neurohelmet_core::data::bundle::Bundle;
use neurohelmet_core::session::{self, Session};
use ratatui::crossterm::event::{self, Event, KeyEventKind};
use ratatui::DefaultTerminal;
use std::time::Duration;

pub fn run(
    mut terminal: DefaultTerminal,
    bundle: Bundle,
    session: Session,
    current_name: String,
) -> color_eyre::Result<()> {
    let cfg = config::Config::load();
    theme::set_theme(cfg.resolved_theme());
    profile::set_profile(cfg.resolved_profile());
    icons::set_icons(cfg.resolved_icons());
    let mut app = App::new(bundle, session, current_name);
    loop {
        terminal.draw(|f| view::draw(f, &mut app))?;

        if event::poll(Duration::from_millis(250))? {
            if let Event::Key(k) = event::read()? {
                if k.kind == KeyEventKind::Press {
                    app.handle_key(k);
                }
            }
        }

        if app.dirty {
            session::save_named(&app.current_name, &app.session)?;
            session::write_current(&app.current_name)?;
            app.dirty = false;
        }
        if app.should_quit {
            break;
        }
    }
    Ok(())
}

/// The fixed frame size used for game-log renders (matches the snapshot test geometry).
pub const LOG_W: u16 = 100;
pub const LOG_H: u16 = 30;

/// Render a session to a ratatui cell buffer off-screen (no terminal). Used by the game-log export
/// to re-render a historical mech: pass a one-mech `Session` built from a logged `TrackedMech`. An
/// empty bundle is fine — `App::new`'s spec relink finds no match and keeps the embedded spec.
pub fn render_to_buffer(session: Session) -> ratatui::buffer::Buffer {
    let mut app = App::new(Bundle::new(vec![]), session, "export".to_string());
    let backend = ratatui::backend::TestBackend::new(LOG_W, LOG_H);
    let mut terminal = ratatui::Terminal::new(backend).expect("test backend");
    terminal.draw(|f| view::draw(f, &mut app)).expect("draw");
    terminal.backend().buffer().clone()
}

/// Render a single frame to an in-memory backend and return it as text (one line per row).
/// Used by `--selftest` for headless verification with the real dataset.
pub fn render_once(bundle: Bundle, session: Session, w: u16, h: u16) -> String {
    let mut app = App::new(bundle, session, "selftest".to_string());
    let backend = ratatui::backend::TestBackend::new(w, h);
    let mut terminal = ratatui::Terminal::new(backend).expect("test backend");
    terminal.draw(|f| view::draw(f, &mut app)).expect("draw");
    let buf = terminal.backend().buffer().clone();
    let mut out = String::new();
    for y in 0..h {
        for x in 0..w {
            out.push_str(buf[(x, y)].symbol());
        }
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::app::{App, EquipRow, Focus};
    use super::view;
    use neurohelmet_core::data::bundle::Bundle;
    use neurohelmet_core::domain::{
        AmmoBin, AsStats, CritSlot, Equipment, Facing, GameMode, HeatSinkType, Location,
        LocationArmor, Mech, MechConfig, MotiveType, UnitType, WeaponMount,
    };
    use neurohelmet_core::session::{MoraleStatus, Session};
    use ratatui::backend::TestBackend;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::Terminal;
    use std::collections::BTreeMap;

    fn sample_mech() -> Mech {
        let mut armor = BTreeMap::new();
        for loc in Location::ALL
            .into_iter()
            .filter(|l| !l.is_vehicle() && !l.is_infantry() && !l.is_aerospace())
        {
            armor.insert(
                loc,
                LocationArmor {
                    armor_max: 20,
                    rear_max: if loc.has_rear() { 6 } else { 0 },
                    internal_max: 10,
                },
            );
        }
        Mech {
            chassis: "Atlas".into(),
            model: "AS7-D".into(),
            tonnage: 100,
            tech_base: "Inner Sphere".into(),
            role: "Juggernaut".into(),
            weight_class: "Assault".into(),
            subtype: "BattleMek".into(),
            year: 2755,
            bv: 1897,
            cost: 9_626_000,
            armor_type: "Standard Armor".into(),
            structure_type: "Standard".into(),
            walk: 3,
            run: 5,
            jump: 0,
            heat_sinks: 20,
            heat_sink_type: HeatSinkType::Single,
            dissipation: 20,
            equipment: Vec::new(),
            config: MechConfig::Biped,
            unit_type: UnitType::Mech,
            motive: None,
            internal: 0,
            dpt: 0,
            transport: vec![],
            armor,
            weapons: vec![WeaponMount {
                id: 0,
                name: "Medium Laser".into(),
                location: Location::RightArm,
                rear: false,
                heat: 3,
                damage: "5".into(),
                range: "3/6/9".into(),
                crit_slots: 1,
                ammo_key: None,
                to_hit: 0,
                tc_eligible: false,
                count: 1,
            }],
            ammo: vec![AmmoBin {
                id: 0,
                name: "AC/20 Ammo".into(),
                location: Location::RightTorso,
                shots_per_ton: 5,
                tons: 2,
                ammo_key: Some("AC:20".into()),
                munition: String::new(),
                base_ammo: None,
            }],
            crit_slots: BTreeMap::from([(
                Location::CenterTorso,
                vec![
                    // First column (slots 0-5 -> shown as 1-6) and a second column
                    // (slots 6+ -> restart at 1) to exercise the paper-sheet numbering.
                    CritSlot { slot: 0, name: "Fusion Engine".into(), system: true, hittable: true, ..Default::default() },
                    CritSlot { slot: 1, name: "Gyro".into(), system: true, hittable: true, ..Default::default() },
                    CritSlot { slot: 2, name: "Life Support".into(), system: true, hittable: true, ..Default::default() },
                    CritSlot { slot: 3, name: "Sensors".into(), system: true, hittable: true, ..Default::default() },
                    CritSlot { slot: 6, name: "Medium Laser".into(), system: false, hittable: true, ..Default::default() },
                    CritSlot { slot: 7, name: "SRM 6".into(), system: false, hittable: true, ..Default::default() },
                ],
            )]),
            as_stats: AsStats {
                pv: 52,
                size: 4,
                tp: "BM".into(),
                movement: "6\"".into(),
                tmm: 1,
                armor: 10,
                structure: 8,
                dmg_s: "5".into(),
                dmg_m: "5".into(),
                dmg_l: "2".into(),
                dmg_e: "0".into(),
                overheat: 0,
                threshold: 0,
                specials: vec!["AC2/2/-".into(), "IF1".into(), "LRM1/1/1".into()],
                arcs: None,
                ..Default::default()
            },
            availability: BTreeMap::new(),
        }
    }

    /// A mech with a weapon linked to a small ammo bin (2 shots), for ammo/cascade scenarios.
    fn combat_mech() -> Mech {
        let mut m = sample_mech();
        m.weapons.push(WeaponMount {
            id: 1,
            name: "AC/20".into(),
            location: Location::RightTorso,
            rear: false,
            heat: 7,
            damage: "20".into(),
            range: "3/6/9".into(),
            crit_slots: 10,
            ammo_key: Some("AC:20".into()),
            to_hit: 0,
            tc_eligible: false,
            count: 1,
        });
        m.ammo = vec![AmmoBin {
            id: 0,
            name: "AC/20 Ammo".into(),
            location: Location::RightTorso,
            shots_per_ton: 2,
            tons: 1, // only 2 shots, so it empties fast
            ammo_key: Some("AC:20".into()),
            munition: String::new(),
            base_ammo: None,
        }];
        m
    }

    fn app_with_mech(m: Mech) -> App {
        let bundle = Bundle::new(vec![m.clone()]);
        let mut session = Session::new();
        session.add_mech(m);
        let mut app = App::new(bundle, session, "test".to_string());
        app.dirty = false;
        app
    }

    /// An app in an Override-mode session with one unit loaded (lands on the live Override card).
    fn app_with_override(m: Mech) -> App {
        let bundle = Bundle::new(vec![m.clone()]);
        let mut session = Session::new_with_mode(GameMode::Override);
        session.add_mech(m);
        let mut app = App::new(bundle, session, "test".to_string());
        app.dirty = false;
        app
    }

    /// A mech whose first weapon is an LRM-20 — a cluster weapon, for the dice-reference popup.
    fn cluster_mech() -> Mech {
        let mut m = sample_mech();
        m.weapons = vec![WeaponMount {
            id: 5,
            name: "LRM 20".into(),
            location: Location::LeftTorso,
            rear: false,
            heat: 6,
            damage: "1/msl".into(),
            range: "7/14/21".into(),
            crit_slots: 5,
            ammo_key: Some("LRM:20".into()),
            to_hit: 0,
            tc_eligible: false,
            count: 1,
        }];
        m.ammo = vec![AmmoBin {
            id: 0,
            name: "LRM 20 Ammo".into(),
            location: Location::LeftTorso,
            shots_per_ton: 6,
            tons: 1,
            ammo_key: Some("LRM:20".into()),
            munition: String::new(),
            base_ammo: None,
        }];
        m
    }

    fn app_with_one_mech() -> App {
        app_with_mech(sample_mech())
    }

    /// A combat vehicle: per-location armor (front/sides/rear/turret) + tracked motive.
    fn sample_vehicle() -> Mech {
        let mut m = sample_mech();
        m.chassis = "Manticore".into();
        m.model = "Heavy Tank".into();
        m.unit_type = UnitType::Vehicle;
        m.motive = Some(MotiveType::Tracked);
        m.internal = 30;
        m.heat_sinks = 0;
        m.crit_slots = BTreeMap::new();
        let mut armor = BTreeMap::new();
        for loc in [
            Location::Front,
            Location::LeftSide,
            Location::RightSide,
            Location::Rear,
            Location::Turret,
        ] {
            armor.insert(loc, LocationArmor { armor_max: 20, rear_max: 0, internal_max: 6 });
        }
        m.armor = armor;
        m.as_stats.tp = "CV".into();
        m.as_stats.movement = "8\"t".into();
        m
    }

    fn app_with_as_mech(m: Mech) -> App {
        let bundle = Bundle::new(vec![m.clone()]);
        let mut session = Session::new_with_mode(GameMode::AlphaStrike);
        session.add_mech(m);
        let mut app = App::new(bundle, session, "test".to_string());
        app.dirty = false;
        app
    }

    /// A Battle Armor squad: JSON-only bake shape — AS stats are the playable surface, no
    /// armor table or crit slots; `internal` is the squad size.
    fn sample_battle_armor() -> Mech {
        let mut m = sample_mech();
        m.chassis = "Achileus Light Battle Armor".into();
        m.model = "[David](Sqd4)".into();
        m.unit_type = UnitType::BattleArmor;
        m.tonnage = 4;
        m.walk = 1;
        m.run = 1;
        m.jump = 3;
        m.heat_sinks = 0;
        m.dissipation = 0;
        m.internal = 4;
        m.bv = 189;
        m.cost = 2_010_320;
        // Per-trooper suit tracks, as parsed from the sheet's T1..T4 pips (6 armor + the trooper).
        let mut armor = BTreeMap::new();
        for loc in &Location::TROOPERS[..4] {
            armor.insert(*loc, LocationArmor { armor_max: 6, rear_max: 0, internal_max: 1 });
        }
        m.armor = armor;
        m.crit_slots = BTreeMap::new();
        m.ammo = vec![];
        m.weapons = vec![WeaponMount {
            id: 0,
            name: "David Light Gauss Rifle".into(),
            location: Location::Body,
            rear: false,
            heat: 0,
            damage: "1".into(),
            range: "3/6/9".into(),
            crit_slots: 0,
            ammo_key: None,
            to_hit: 0,
            tc_eligible: false,
            count: 1,
        }];
        m.as_stats = AsStats {
            pv: 15,
            size: 1,
            tp: "BA".into(),
            movement: "6\"j".into(),
            tmm: 1,
            armor: 1,
            structure: 2,
            dmg_s: "1".into(),
            dmg_m: "0*".into(),
            dmg_l: "0".into(),
            dmg_e: "0".into(),
            overheat: 0,
            threshold: 0,
            specials: vec!["AM".into(), "CAR4".into(), "MEC".into(), "STL".into()],
            arcs: None,
            ..Default::default()
        };
        m
    }

    /// A 4-suit BA squad whose only weapon is an SRM-2 with a 2-shot ammo bin — for exercising
    /// per-suit ammo and firing from a chosen suit.
    fn ba_squad_with_ammo() -> Mech {
        let mut m = sample_battle_armor();
        m.chassis = "Elemental Battle Armor".into();
        m.model = "[SRM](Sqd4)".into();
        m.weapons = vec![WeaponMount {
            id: 0,
            name: "SRM 2".into(),
            location: Location::Trooper1,
            rear: false,
            heat: 0,
            damage: "2/msl".into(),
            range: "3/6/9".into(),
            crit_slots: 0,
            ammo_key: Some("SRM:2".into()),
            to_hit: 0,
            tc_eligible: false,
            count: 1,
        }];
        m.ammo = vec![AmmoBin {
            id: 0,
            name: "SRM 2 Ammo".into(),
            location: Location::Trooper1,
            shots_per_ton: 2,
            tons: 1,
            ammo_key: Some("SRM:2".into()),
            munition: String::new(),
            base_ammo: None,
        }];
        m
    }

    /// A conventional-infantry platoon: a single troop-strength pool, no armor.
    fn sample_platoon() -> Mech {
        let mut m = sample_battle_armor();
        m.chassis = "Clan Heavy Foot Infantry".into();
        m.model = "Ebon Keshik Point".into();
        m.unit_type = UnitType::Infantry;
        m.jump = 0;
        m.internal = 21;
        m.dpt = 13; // full-strength platoon damage
        m.bv = 68;
        m.cost = 1_369_441;
        // 21 troopers with auto-rifles; `damage` is per trooper (0.52 × 21 ≈ 11 for the group).
        m.weapons = vec![WeaponMount {
            id: 0,
            name: "Auto-Rifle".into(),
            location: Location::Platoon,
            rear: false,
            heat: 0,
            damage: "0.52".into(),
            range: "1".into(),
            crit_slots: 0,
            ammo_key: None,
            to_hit: 0,
            tc_eligible: false,
            count: 21,
        }];
        m.armor = BTreeMap::from([(
            Location::Platoon,
            LocationArmor { armor_max: 0, rear_max: 0, internal_max: 21 },
        )]);
        m.as_stats.tp = "CI".into();
        m.as_stats.movement = "2\"f".into();
        m
    }

    /// An aerospace fighter (Visigoth): four armor arcs + shared SI, thrust, an arc-mounted weapon.
    fn sample_aero_fighter() -> Mech {
        Mech {
            chassis: "Visigoth".into(),
            model: "Prime".into(),
            tonnage: 80,
            tech_base: "Clan".into(),
            role: "Interceptor".into(),
            weight_class: "Medium".into(),
            subtype: "Aerospace Fighter Omni".into(),
            year: 3060,
            unit_type: UnitType::Aerospace,
            walk: 5, // safe thrust
            run: 8,  // max thrust
            heat_sinks: 10,
            dissipation: 10,
            armor: BTreeMap::from([
                (Location::Nose, LocationArmor { armor_max: 12, rear_max: 0, internal_max: 0 }),
                (Location::LeftWing, LocationArmor { armor_max: 9, rear_max: 0, internal_max: 0 }),
                (Location::RightWing, LocationArmor { armor_max: 9, rear_max: 0, internal_max: 0 }),
                (Location::Aft, LocationArmor { armor_max: 7, rear_max: 0, internal_max: 0 }),
                (Location::AeroSI, LocationArmor { armor_max: 0, rear_max: 0, internal_max: 5 }),
            ]),
            weapons: vec![WeaponMount {
                id: 0,
                name: "ER Large Laser".into(),
                location: Location::Nose,
                rear: false,
                heat: 12,
                damage: "10".into(),
                range: "8/15/25".into(),
                crit_slots: 0,
                ammo_key: None,
                to_hit: 0,
                tc_eligible: true,
                count: 1,
            }],
            as_stats: AsStats {
                pv: 50,
                size: 2,
                tp: "AF".into(),
                movement: "7a".into(),
                tmm: 0,
                armor: 7,
                structure: 4,
                dmg_s: "6".into(),
                dmg_m: "6".into(),
                dmg_l: "5".into(),
                dmg_e: "0".into(),
                overheat: 0,
                threshold: 3,
                specials: vec!["BOMB2".into(), "FUEL20".into(), "VSTOL".into()],
                arcs: None,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    fn press(app: &mut App, code: KeyCode) {
        app.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
    }

    fn press_ctrl(app: &mut App, code: KeyCode) {
        app.handle_key(KeyEvent::new(code, KeyModifiers::CONTROL));
    }

    /// Redirect session/log persistence to a throwaway dir for the whole test process, so tests
    /// that exercise the on-disk create/load/log paths never touch the user's real data dir (which
    /// would litter their session list and clobber the "last active" pointer). Idempotent — the
    /// `Once` sets `NEUROHELMET_DIR` before any caller proceeds, so concurrent disk tests agree on it.
    fn isolate_data_dir() {
        use std::sync::Once;
        static ONCE: Once = Once::new();
        ONCE.call_once(|| {
            let dir = std::env::temp_dir().join(format!("neurohelmet-test-{}", std::process::id()));
            let _ = std::fs::create_dir_all(&dir);
            std::env::set_var("NEUROHELMET_DIR", &dir);
        });
    }

    /// Serialize the tests that persist to the *shared* `config.json` (the Ctrl-T picker saves the
    /// whole config from thread-locals, so two running at once clobber each other's fields). Each such
    /// test holds this lock across its save→load round-trip. Returns the guard; keep it alive.
    fn config_test_guard() -> std::sync::MutexGuard<'static, ()> {
        use std::sync::Mutex;
        static LOCK: Mutex<()> = Mutex::new(());
        LOCK.lock().unwrap_or_else(|e| e.into_inner()) // ignore poisoning from an unrelated panic
    }

    /// A Clan heavy 'Mech, distinct from the IS assault `sample_mech` (Atlas), for filter tests.
    fn clan_heavy_mech() -> Mech {
        let mut m = sample_mech();
        m.chassis = "Timber Wolf".into();
        m.model = "Prime".into();
        m.tech_base = "Clan".into();
        m.weight_class = "Heavy".into();
        m.tonnage = 75;
        m.year = 3055;
        m
    }

    /// Render a frame to a string, one line per terminal row (trailing space trimmed) so it
    /// reads like the real screen and snapshots cleanly.
    fn render(app: &mut App) -> String {
        render_dims(app, 100, 30)
    }

    /// Render a frame at an explicit size — for layout/profile tests that need a wide screen.
    fn render_dims(app: &mut App, w: u16, h: u16) -> String {
        let backend = TestBackend::new(w, h);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| view::draw(f, app)).unwrap();
        let buf = terminal.backend().buffer().clone();
        let mut out = String::new();
        for y in 0..h {
            let mut row = String::new();
            for x in 0..w {
                row.push_str(buf[(x, y)].symbol());
            }
            out.push_str(row.trim_end());
            out.push('\n');
        }
        out
    }

    #[test]
    fn e2e_force_gen_modal() {
        use neurohelmet_core::data::bundle::{EraInfo, FactionInfo};
        // Two IS 'Mechs available to the Draconis Combine (27) in the Clan Invasion (13).
        let mut a = sample_mech();
        a.bv = 1500;
        a.availability = BTreeMap::from([(13u16, BTreeMap::from([(27u16, 60u8)]))]);
        let mut b = clan_heavy_mech();
        b.bv = 1800;
        b.availability = BTreeMap::from([(13u16, BTreeMap::from([(27u16, 40u8)]))]);
        let mut bundle = Bundle::new(vec![a, b]);
        bundle.eras = vec![EraInfo { id: 13, name: "Clan Invasion".into(), from: 3050, to: 3061 }];
        bundle.factions =
            vec![FactionInfo { id: 27, name: "Draconis Combine".into(), group: "Inner Sphere".into() }];
        let mut app = App::new(bundle, Session::new(), "t".into());

        // ^G opens the generator on the config stage.
        press_ctrl(&mut app, KeyCode::Char('g'));
        let cfg = render(&mut app);
        assert!(cfg.contains("Generate force"), "modal title");
        assert!(cfg.contains("Faction") && cfg.contains("Size") && cfg.contains("Allow rare"));
        assert!(cfg.contains("Lance"), "size 4 shows the Lance formation name");

        // Enter rolls; the result stage offers Accept.
        press(&mut app, KeyCode::Enter);
        let preview = render(&mut app);
        assert!(preview.contains("accept"), "result stage shows the accept hint");

        // Enter accepts: the count floor (4) is appended and the modal closes.
        press(&mut app, KeyCode::Enter);
        assert!(app.modal.is_none(), "modal closes after accept");
        assert_eq!(app.session.mechs.len(), 4, "count floor appended to the roster");
    }

    /// Build an app whose facet values carry a few factions, for the combo-box tests.
    fn app_with_factions() -> App {
        use neurohelmet_core::data::bundle::FactionInfo;
        let mut bundle = Bundle::new(vec![sample_mech()]);
        bundle.factions = vec![
            FactionInfo { id: 1, name: "Draconis Combine".into(), group: "Inner Sphere".into() },
            FactionInfo { id: 2, name: "Clan Wolf".into(), group: "Clan".into() },
            FactionInfo { id: 3, name: "Wolf's Dragoons".into(), group: "Mercenary".into() },
            FactionInfo { id: 4, name: "Federated Suns".into(), group: "Inner Sphere".into() },
        ];
        App::new(bundle, Session::new(), "t".into())
    }

    fn faction_facet_idx() -> usize {
        super::filters::Facet::ALL.iter().position(|f| *f == super::filters::Facet::Faction).unwrap()
    }

    #[test]
    fn faction_pick_list_leads_with_any_then_filters_by_substring() {
        let app = app_with_factions();
        let all = app.faction_pick_list("");
        assert!(all[0].is_none(), "an empty query leads with the (any) clear entry");
        assert_eq!(all.len(), 5, "(any) + 4 factions");
        // Typing narrows to substring matches, case-insensitive, with no (any) row.
        let wolf = app.faction_pick_list("WOLF");
        assert_eq!(wolf.len(), 2);
        assert!(wolf.iter().all(Option::is_some));
        assert!(wolf.iter().flatten().all(|(_, n)| n.contains("Wolf")));
    }

    #[test]
    fn faction_combo_box_filters_and_sets() {
        use super::app::Modal;
        let mut app = app_with_factions();
        app.modal = Some(Modal::Filters { sel: faction_facet_idx() });
        // Enter on the Faction row opens the combo box (not "close modal").
        press(&mut app, KeyCode::Enter);
        assert!(matches!(app.modal, Some(Modal::FactionPick { .. })), "Enter opens the picker");

        for c in "wolf".chars() {
            press(&mut app, KeyCode::Char(c));
        }
        let screen = render(&mut app);
        assert!(screen.contains("Clan Wolf") && screen.contains("Wolf's Dragoons"), "matches shown");
        assert!(!screen.contains("Federated Suns"), "non-matches filtered out");

        // Enter commits the top match and returns to the Filters modal.
        press(&mut app, KeyCode::Enter);
        assert!(matches!(app.modal, Some(Modal::Filters { .. })), "returns to Filters");
        assert_eq!(app.filters.faction.as_ref().map(|(_, n)| n.as_str()), Some("Clan Wolf"));
    }

    #[test]
    fn faction_combo_box_esc_keeps_current_and_any_clears() {
        use super::app::Modal;
        let mut app = app_with_factions();
        // Pre-set a faction, then open the picker and Esc — the lens is unchanged.
        app.filters.faction = Some((4, "Federated Suns".into()));
        app.modal = Some(Modal::FactionPick { query: "dra".into(), sel: 0 });
        press(&mut app, KeyCode::Esc);
        assert!(matches!(app.modal, Some(Modal::Filters { .. })));
        assert_eq!(app.filters.faction.as_ref().map(|(id, _)| *id), Some(4), "Esc keeps the faction");

        // Reopen, pick the "(any)" entry (row 0 on an empty query) — clears the lens.
        app.modal = Some(Modal::FactionPick { query: String::new(), sel: 0 });
        press(&mut app, KeyCode::Enter);
        assert!(app.filters.faction.is_none(), "(any) clears the faction lens");
    }

    #[test]
    fn tracker_renders_key_elements() {
        let mut app = app_with_one_mech();
        let screen = render(&mut app);
        assert!(screen.contains("Atlas"), "roster tab");
        assert!(screen.contains("HEAT"), "heat panel");
        assert!(screen.contains("WEAPONS"), "equipment panel");
        assert!(screen.contains("CT"), "center torso box");
        assert!(screen.contains("Medium Laser"), "weapon row");
        // Atlas: Walk 3 / Run 5 / Jump 0 — the MOVE panel must surface these.
        assert!(screen.contains("MOVE"), "move panel");
        assert!(screen.contains("Walk 3") && screen.contains("Run 5"), "movement numbers");
    }

    /// Dev-only: render each theme's tracker to montage PNGs so the palettes can be eyeballed
    /// (e.g. on mobile). Ignored by default; run with a target dir:
    ///   NEUROHELMET_PREVIEW_DIR=/tmp/x cargo test -p neurohelmet generate_theme_previews -- --ignored
    #[test]
    #[ignore]
    fn generate_theme_previews() {
        use super::profile::{set_profile, DisplayProfile};
        use super::theme::{set_theme, THEMES};
        let out = std::path::PathBuf::from(
            std::env::var("NEUROHELMET_PREVIEW_DIR").unwrap_or_else(|_| "/tmp".into()),
        );
        const SCALE: usize = 2;
        const GAP: usize = 10;
        set_profile(DisplayProfile::Modern); // shows the force sidebar (self-labels the unit)

        // One rasterized tracker frame for a theme, with the unit named after the theme + some
        // heat/damage so the good/warning/danger status colors all appear.
        let frame = |label: &str, theme: super::theme::Theme| {
            set_theme(theme);
            let mut m = sample_mech();
            m.chassis = label.to_string();
            m.model = String::new();
            let mut app = app_with_mech(m);
            for _ in 0..3 {
                press(&mut app, KeyCode::Char(' ')); // damage the cursor location (armor → red)
            }
            for _ in 0..14 {
                press(&mut app, KeyCode::Char('o')); // heat into the yellow band
            }
            let backend = TestBackend::new(100, 30);
            let mut term = Terminal::new(backend).unwrap();
            term.draw(|f| view::draw(f, &mut app)).unwrap();
            crate::render::rasterize(term.backend().buffer(), SCALE)
        };

        let groups = [
            ("theme-preview-general", &THEMES[0..5]),
            ("theme-preview-houses", &THEMES[5..10]),
            ("theme-preview-clans", &THEMES[10..15]),
        ];
        for (file, group) in groups {
            let frames: Vec<_> = group.iter().map(|(_, label, t)| frame(label, *t)).collect();
            let w = frames[0].w;
            let fh = frames[0].h;
            let mut montage = crate::render::Image::new(w, frames.len() * fh + (frames.len() - 1) * GAP);
            for (i, fr) in frames.iter().enumerate() {
                montage.blit(fr, 0, i * (fh + GAP));
            }
            montage.write_png(&out.join(format!("{file}.png"))).unwrap();
        }
        set_theme(super::theme::Theme::pi());
        set_profile(DisplayProfile::Pi);
    }

    /// Dev-only: render representative app screens to PNGs for the docs site
    /// (`docs/guide/src/images/`), using the same rasterize path the game-log export uses. Ignored by
    /// default; regenerate with:
    ///   cargo test -p neurohelmet --bin neurohelmet generate_doc_screenshots -- --ignored
    #[test]
    #[ignore]
    fn generate_doc_screenshots() {
        use super::profile::{set_profile, DisplayProfile};
        use super::theme::{set_theme, Theme};
        const SCALE: usize = 2;
        let out =
            std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/guide/src/images"));
        std::fs::create_dir_all(&out).unwrap();
        // A self-contained theme so the PNG has a real background (terminal-reset themes wouldn't).
        set_theme(Theme::catppuccin_mocha());

        let shot = |app: &mut App, w: u16, h: u16| {
            let backend = TestBackend::new(w, h);
            let mut term = Terminal::new(backend).unwrap();
            term.draw(|f| view::draw(f, app)).unwrap();
            crate::render::rasterize(term.backend().buffer(), SCALE)
        };

        // 1) Classic tracker, Pi (compact) profile — one 'Mech with a little damage + heat so the
        //    good/warning/danger status colors all show.
        set_profile(DisplayProfile::Pi);
        let mut m = sample_mech();
        m.chassis = "Atlas".into();
        m.model = "AS7-D".into();
        let mut app = app_with_mech(m);
        for _ in 0..4 {
            press(&mut app, KeyCode::Char(' ')); // damage the cursor location
        }
        for _ in 0..12 {
            press(&mut app, KeyCode::Char('o')); // heat into the warning band
        }
        shot(&mut app, 100, 30).write_png(&out.join("classic-tracker.png")).unwrap();

        // 2) Modern (roomy) profile — a lance in the force sidebar.
        set_profile(DisplayProfile::Modern);
        let roster = [
            ("Atlas", "AS7-D", 100u16),
            ("Warhammer", "WHM-6R", 70),
            ("Phoenix Hawk", "PXH-1", 45),
            ("Locust", "LCT-1V", 20),
        ];
        let mut session = Session::new();
        let mut mechs = Vec::new();
        for (chassis, model, tons) in roster {
            let mut m = sample_mech();
            m.chassis = chassis.into();
            m.model = model.into();
            m.tonnage = tons;
            session.add_mech(m.clone());
            mechs.push(m);
        }
        let bundle = Bundle::new(mechs);
        let mut app = App::new(bundle, session, "Demo Lance".into());
        app.dirty = false;
        for _ in 0..3 {
            press(&mut app, KeyCode::Char(' '));
        }
        for _ in 0..10 {
            press(&mut app, KeyCode::Char('o'));
        }
        shot(&mut app, 120, 34).write_png(&out.join("modern-force.png")).unwrap();

        set_theme(Theme::pi());
        set_profile(DisplayProfile::Pi);
    }

    /// Dev-only: plant a few real, openable demo sessions in the REAL data dir (no isolation), so
    /// the sessions browser has good material to screenshot from a real terminal. Distinct "Demo …"
    /// names — it never touches your own sessions or the last-active pointer. Ignored by default:
    ///   cargo test -p neurohelmet --bin neurohelmet seed_demo_sessions -- --ignored --nocapture
    #[test]
    #[ignore]
    fn seed_demo_sessions() {
        use neurohelmet_core::domain::GameMode;
        use neurohelmet_core::session::{save_named, sessions_dir};
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/mechs.bin");
        let bundle = Bundle::load(std::path::Path::new(path)).expect("load bundle");
        let find = |chassis: &str| {
            bundle
                .mechs
                .iter()
                .find(|m| m.chassis.eq_ignore_ascii_case(chassis))
                .unwrap_or_else(|| panic!("no '{chassis}' in bundle"))
                .clone()
        };
        std::fs::create_dir_all(sessions_dir()).unwrap();

        // 1) Classic lance, battle-damaged on the active unit (Classic tracker + Modern force view).
        let mut session = Session::new();
        let mut specs = Vec::new();
        for c in ["Atlas", "Marauder", "Warhammer", "Phoenix Hawk"] {
            let m = find(c);
            session.add_mech(m.clone());
            specs.push(m);
        }
        let mut app = App::new(Bundle::new(specs), session, "Demo Lance".into());
        for _ in 0..7 {
            press(&mut app, KeyCode::Char(' ')); // damage the active unit into structure
        }
        for _ in 0..14 {
            press(&mut app, KeyCode::Char('o')); // heat into the warning band
        }
        press(&mut app, KeyCode::Char('p')); // one pilot hit
        save_named("Demo Lance", &app.session).unwrap();

        // 2) Alpha Strike force (a 2×2 card grid), fresh.
        let mut as_session = Session::new_with_mode(GameMode::AlphaStrike);
        for c in ["Battlemaster", "Rifleman", "Wolverine", "Locust"] {
            as_session.add_mech(find(c));
        }
        save_named("Demo Alpha Strike", &as_session).unwrap();

        // 3) Override card, fresh.
        let mut ov_session = Session::new_with_mode(GameMode::Override);
        ov_session.add_mech(find("Atlas"));
        save_named("Demo Override", &ov_session).unwrap();

        println!(
            "Seeded 3 demo sessions (Demo Lance / Demo Alpha Strike / Demo Override) in {}",
            sessions_dir().display()
        );
    }

    #[test]
    fn ctrl_t_theme_picker_previews_and_keeps() {
        use super::app::Modal;
        use super::theme::{set_theme, theme, Theme, THEMES};
        let _guard = config_test_guard(); // shares config.json with the other save tests
        isolate_data_dir(); // Enter persists the choice to config.json — keep it off the real dir
        set_theme(Theme::pi());
        let mut app = app_with_one_mech();
        press_ctrl(&mut app, KeyCode::Char('t'));
        assert!(matches!(app.modal, Some(Modal::ThemePicker { sel: 0, .. })), "opens on pi");
        press(&mut app, KeyCode::Down);
        assert_eq!(theme(), THEMES[1].2, "Down live-previews the next theme");
        press(&mut app, KeyCode::Enter);
        assert!(app.modal.is_none(), "Enter closes the picker");
        assert_eq!(theme(), THEMES[1].2, "Enter keeps the previewed theme");
        set_theme(Theme::pi()); // restore for other tests on this thread
    }

    #[test]
    fn ctrl_t_enter_persists_choice_to_config() {
        use super::config::Config;
        use super::profile::{set_profile, DisplayProfile};
        use super::theme::{set_theme, Theme};
        let _guard = config_test_guard(); // shares config.json with the other save tests
        isolate_data_dir();
        set_theme(Theme::pi());
        set_profile(DisplayProfile::Pi);
        let mut app = app_with_one_mech();
        press_ctrl(&mut app, KeyCode::Char('t'));
        press(&mut app, KeyCode::Down); // preview + select the 2nd theme (truecolor)
        press(&mut app, KeyCode::Enter); // commit → save_current writes config.json
        assert!(app.status.contains("saved"), "status confirms the save: {}", app.status);
        // The saved file names the picked theme; loading it back resolves to that theme.
        let cfg = Config::load();
        assert_eq!(cfg.theme.as_deref(), Some(super::theme::THEMES[1].0), "saved the picked theme");
        set_theme(Theme::pi()); // restore for other tests on this thread
    }

    #[test]
    fn ctrl_t_icon_row_toggles_persists_and_esc_restores() {
        use super::config::Config;
        use super::icons::{icons, set_icons, IconSet};
        use super::profile::{set_profile, DisplayProfile};
        use super::theme::{set_theme, Theme};
        let _guard = config_test_guard(); // shares config.json with the other save tests
        isolate_data_dir();
        set_theme(Theme::pi());
        set_profile(DisplayProfile::Pi);
        set_icons(IconSet::Ascii);
        let mut app = app_with_one_mech();
        press_ctrl(&mut app, KeyCode::Char('t'));
        // Up from the seeded theme row 0 wraps to the last row — the icon-set row — without
        // previewing (and thus committing) any theme along the way.
        press(&mut app, KeyCode::Up);
        press(&mut app, KeyCode::Right); // toggle the icon set live
        assert_eq!(icons(), IconSet::Nerd, "←→ toggles the icon set live");
        press(&mut app, KeyCode::Enter); // commit → persists to config.json
        assert!(app.status.contains("saved"), "status confirms the save: {}", app.status);
        assert_eq!(Config::load().icons.as_deref(), Some("nerd"), "saved the picked icon set");

        // Esc restores the icon set in effect when the picker opened.
        press_ctrl(&mut app, KeyCode::Char('t'));
        press(&mut app, KeyCode::Up);
        press(&mut app, KeyCode::Right); // back to Ascii live
        assert_eq!(icons(), IconSet::Ascii);
        press(&mut app, KeyCode::Esc);
        assert_eq!(icons(), IconSet::Nerd, "Esc restores the icon set from when the picker opened");
        set_icons(IconSet::Ascii); // restore for other tests on this thread
    }

    #[test]
    fn save_current_preserves_unrelated_config_fields() {
        use super::config::{save_current, Config};
        use super::icons::{set_icons, IconSet};
        use super::profile::{set_profile, DisplayProfile};
        use super::theme::{set_theme, Theme};
        let _guard = config_test_guard(); // shares config.json with the other save tests
        isolate_data_dir();
        // Seed a config carrying a log setting the Ctrl-T picker knows nothing about.
        Config { log_repo: Some("/srv/logs".into()), ..Default::default() }.save().unwrap();
        // A picker commit saves the live display choices...
        set_theme(Theme::pi());
        set_profile(DisplayProfile::Pi);
        set_icons(IconSet::Ascii);
        save_current().unwrap();
        // ...without dropping the hand-edited log_repo (the pre-fix bug clobbered it).
        let cfg = Config::load();
        assert_eq!(cfg.log_repo.as_deref(), Some("/srv/logs"), "save_current kept log_repo");
        assert!(cfg.theme.is_some(), "and still recorded the picked theme");
    }

    #[test]
    fn theme_picker_esc_restores_theme_and_layout() {
        use super::profile::{profile, set_profile, DisplayProfile};
        use super::theme::{set_theme, theme, Theme, THEMES};
        // Open from pi so the picker seeds on row 0 (deterministic navigation).
        set_theme(Theme::pi());
        set_profile(DisplayProfile::Pi);
        let mut app = app_with_one_mech();
        press_ctrl(&mut app, KeyCode::Char('t'));
        // Down past every theme row lands on the layout row; the last theme passed is still previewed.
        for _ in 0..THEMES.len() {
            press(&mut app, KeyCode::Down);
        }
        assert_eq!(theme(), THEMES[THEMES.len() - 1].2, "moving through theme rows previews them");
        press(&mut app, KeyCode::Right); // toggle the layout row live
        assert_eq!(profile(), DisplayProfile::Modern, "←→ toggles layout live");
        press(&mut app, KeyCode::Esc);
        assert!(app.modal.is_none());
        assert_eq!(theme(), Theme::pi(), "Esc restores the original theme");
        assert_eq!(profile(), DisplayProfile::Pi, "Esc restores the original layout");
        set_theme(Theme::pi()); // restore for other tests on this thread
    }

    #[test]
    fn modern_profile_shows_force_sidebar() {
        use super::profile::{set_profile, DisplayProfile};
        set_profile(DisplayProfile::Modern);
        let mut app = app_with_one_mech();
        let screen = render(&mut app); // render() is 100 wide → sidebar (26) + main (≥70) fits
        set_profile(DisplayProfile::Pi); // restore before asserts so a failure can't leak the profile
        assert!(screen.contains("Force"), "sidebar title present\n{screen}");
        assert!(
            screen.contains('●') || screen.contains('◐') || screen.contains('✖'),
            "a condition glyph is shown\n{screen}"
        );
        assert!(screen.contains("BV"), "force total in the sidebar footer\n{screen}");
        // The unit is still drawn in the (narrower) main area alongside the sidebar.
        assert!(screen.contains("HEAT") && screen.contains("Medium Laser"), "main view intact");
        // The redundant top tabs (rendered as "1:Atlas") are dropped when the sidebar is shown.
        assert!(!screen.contains("1:Atlas"), "top roster tabs collapsed under the sidebar\n{screen}");
    }

    #[test]
    fn modern_alpha_strike_grid_grows_with_space() {
        use super::profile::{set_profile, DisplayProfile};
        let names = ["AlphaOne", "AlphaTwo", "AlphaThree", "AlphaFour", "AlphaFive", "AlphaSix"];
        let mechs: Vec<Mech> = names
            .iter()
            .map(|n| {
                let mut m = sample_mech();
                m.chassis = (*n).into();
                m.model = String::new();
                m
            })
            .collect();
        let bundle = Bundle::new(mechs.clone());
        let mut session = Session::new_with_mode(GameMode::AlphaStrike);
        for m in mechs {
            session.add_mech(m);
        }
        let mut app = App::new(bundle, session, "test".to_string());
        app.session.active = 0; // add_mech selects the last unit; view page 1 deterministically

        // A big Modern screen fits all six cards on one page (3×3 grid).
        set_profile(DisplayProfile::Modern);
        let big = render_dims(&mut app, 160, 50);
        // The terse Pi 2×2 shows only the first page of four + a page indicator.
        set_profile(DisplayProfile::Pi);
        let small = render_dims(&mut app, 100, 30);

        // Big Modern: all six cards fit on one page (3×3), so there's no paging indicator.
        for n in names {
            assert!(big.contains(n), "{n} should appear on the big Modern grid\n{big}");
        }
        assert!(!big.contains("page 1/"), "all six fit one page on the big grid\n{big}");
        // Terse Pi 2×2 fits only four per page, so the roster pages — shown by the indicator.
        assert!(small.contains("page 1/2"), "Pi 2×2 pages the roster (page 1/2)\n{small}");
    }

    #[test]
    fn pi_profile_has_no_sidebar() {
        let mut app = app_with_one_mech();
        let screen = render(&mut app); // default Pi profile
        assert!(!screen.contains(" Force "), "no force sidebar in the Pi profile\n{screen}");
        assert!(screen.contains("1:Atlas"), "Pi profile keeps the top roster tabs\n{screen}");
    }

    #[test]
    fn picker_renders_and_filters() {
        let bundle = Bundle::new(vec![sample_mech()]);
        let mut app = App::new(bundle, Session::new(), "test".to_string());
        // No mechs -> starts on the picker.
        let screen = render(&mut app);
        assert!(screen.contains("Pick a"), "picker title");
        assert!(screen.contains("Atlas"), "listed unit");
        // Type a query that matches nothing.
        press(&mut app, KeyCode::Char('z'));
        assert!(app.picker.filtered.is_empty());
    }

    #[test]
    fn facet_filter_narrows_and_composes_with_query() {
        // Atlas (IS/Assault), Timber Wolf (Clan/Heavy), Manticore (vehicle).
        let bundle = Bundle::new(vec![sample_mech(), clan_heavy_mech(), sample_vehicle()]);
        let mut app = App::new(bundle, Session::new(), "test".to_string());
        assert_eq!(app.picker.filtered.len(), 3, "no filter = all units");

        // Filter Tech = Clan → only the Timber Wolf.
        app.filters.tech = Some("Clan".into());
        app.picker.refilter(&app.names, &app.bundle, &app.filters);
        assert_eq!(app.picker.filtered.len(), 1);
        assert_eq!(app.bundle.get(app.picker.filtered[0]).unwrap().chassis, "Timber Wolf");

        // Add a Type=Vehicle filter on top → nothing matches (Clan AND Vehicle).
        app.filters.unit_type = Some(crate::tui::filters::TypeFilter::Unit(UnitType::Vehicle));
        app.picker.refilter(&app.names, &app.bundle, &app.filters);
        assert!(app.picker.filtered.is_empty());

        // Clear, then a fuzzy query composes with a Tech=Clan filter.
        app.filters.clear();
        app.filters.tech = Some("Clan".into());
        app.picker.query = "atlas".into(); // matches the IS Atlas, but it's filtered out
        app.picker.refilter(&app.names, &app.bundle, &app.filters);
        assert!(app.picker.filtered.is_empty(), "query + filter both apply");
    }

    #[test]
    fn year_range_facet_types_bounds() {
        let bundle = Bundle::new(vec![sample_mech(), clan_heavy_mech(), sample_vehicle()]);
        let mut app = App::new(bundle, Session::new(), "test".to_string());
        press_ctrl(&mut app, KeyCode::Char('f')); // open filters
        for _ in 0..5 {
            press(&mut app, KeyCode::Down); // Type -> ... -> Year ≥ (index 5)
        }
        for d in "3000".chars() {
            press(&mut app, KeyCode::Char(d));
        }
        assert_eq!(app.filters.year_min, Some(3000));
        // Only the Clan Timber Wolf (3055) is ≥ 3000; the Atlas/Manticore (2755) drop out.
        assert_eq!(app.picker.filtered.len(), 1);
        assert_eq!(app.bundle.get(app.picker.filtered[0]).unwrap().chassis, "Timber Wolf");

        // Move to Year ≤ and cap at 3050 → the Timber Wolf (3055) now drops too.
        press(&mut app, KeyCode::Down);
        for d in "3050".chars() {
            press(&mut app, KeyCode::Char(d));
        }
        assert_eq!(app.filters.year_max, Some(3050));
        assert!(app.picker.filtered.is_empty());

        // Backspace deletes a digit (3050 -> 305).
        press(&mut app, KeyCode::Backspace);
        assert_eq!(app.filters.year_max, Some(305));
        let screen = render(&mut app);
        assert!(screen.contains("Year ≤"), "upper-bound facet row");
        insta::assert_snapshot!(screen);
    }

    #[test]
    fn e2e_picker_filter_modal() {
        let bundle = Bundle::new(vec![sample_mech(), clan_heavy_mech(), sample_vehicle()]);
        let mut app = App::new(bundle, Session::new(), "test".to_string());
        press_ctrl(&mut app, KeyCode::Char('f')); // open the filter editor
        press(&mut app, KeyCode::Down); // Type -> Tech
        press(&mut app, KeyCode::Right); // cycle Tech off "(any)"
        let screen = render(&mut app);
        assert!(screen.contains("Filters"), "filter modal");
        assert!(screen.contains("Tech"), "tech facet row");
        insta::assert_snapshot!(screen);
    }

    #[test]
    fn space_damages_cursor_location() {
        let mut app = app_with_one_mech();
        // Cursor starts on CenterTorso, Front facing.
        let before = app
            .session
            .active_mech()
            .unwrap()
            .armor_remaining(Location::CenterTorso, Facing::Front);
        press(&mut app, KeyCode::Char(' '));
        let after = app
            .session
            .active_mech()
            .unwrap()
            .armor_remaining(Location::CenterTorso, Facing::Front);
        assert_eq!(after, before - 1);
        assert!(app.dirty);
    }

    #[test]
    fn equipment_focus_fires_weapon_adds_heat() {
        let mut app = app_with_one_mech();
        press(&mut app, KeyCode::Tab); // focus -> Equipment
        assert!(matches!(app.focus, Focus::Equipment));
        press(&mut app, KeyCode::Char(' ')); // fire Medium Laser (heat 3)
        assert_eq!(app.session.active_mech().unwrap().heat, 3);
    }

    #[test]
    fn undo_reverts_last_action() {
        let mut app = app_with_one_mech();
        let full = app
            .session
            .active_mech()
            .unwrap()
            .armor_remaining(Location::CenterTorso, Facing::Front);
        press(&mut app, KeyCode::Char(' ')); // damage CT
        press(&mut app, KeyCode::Char(' ')); // damage CT again
        assert_eq!(
            app.session
                .active_mech()
                .unwrap()
                .armor_remaining(Location::CenterTorso, Facing::Front),
            full - 2
        );
        press(&mut app, KeyCode::Char('z')); // undo one
        assert_eq!(
            app.session
                .active_mech()
                .unwrap()
                .armor_remaining(Location::CenterTorso, Facing::Front),
            full - 1
        );
        press(&mut app, KeyCode::Char('z')); // undo the other
        assert_eq!(
            app.session
                .active_mech()
                .unwrap()
                .armor_remaining(Location::CenterTorso, Facing::Front),
            full
        );
        press(&mut app, KeyCode::Char('z')); // nothing left
        assert_eq!(app.status, "Nothing to undo");
    }

    #[test]
    fn repair_restores_internal_before_armor() {
        let mut app = app_with_one_mech();
        // Chew through CT front armor (20) and into internal structure (down to 7/10).
        app.session
            .active_mech_mut()
            .unwrap()
            .damage(Location::CenterTorso, Facing::Front, 23);
        fn ct(app: &App) -> (u16, u16) {
            let tm = app.session.active_mech().unwrap();
            (
                tm.armor_remaining(Location::CenterTorso, Facing::Front),
                tm.internal_remaining(Location::CenterTorso),
            )
        }
        assert_eq!(ct(&app), (0, 7), "armor gone, 3 internal hits");

        // 'u' should repair internal structure first, leaving armor at 0.
        press(&mut app, KeyCode::Char('u'));
        assert_eq!(ct(&app), (0, 8), "internal repaired, armor untouched");
        press(&mut app, KeyCode::Char('u'));
        press(&mut app, KeyCode::Char('u'));
        assert_eq!(ct(&app), (0, 10), "internal fully restored");

        // Only now does armor start coming back.
        press(&mut app, KeyCode::Char('u'));
        assert_eq!(ct(&app), (1, 10), "armor repaired once internal is full");
    }

    // ----- E2E snapshot tests -----
    //
    // These drive real key sequences through `handle_key` and snapshot the rendered frame.
    // The committed `.snap` files (under src/snapshots/) are readable pictures of each screen
    // and act as regression guards. To update after an intentional UI change:
    //   INSTA_UPDATE=always cargo test -p neurohelmet
    // then review the diff in the `.snap` files before committing.

    #[test]
    fn e2e_tracker_initial() {
        let mut app = app_with_one_mech();
        insta::assert_snapshot!(render(&mut app));
    }

    #[test]
    fn e2e_tracker_after_combat() {
        let mut app = app_with_one_mech();
        for _ in 0..5 {
            press(&mut app, KeyCode::Char(' ')); // 5 damage to CT (cursor default)
        }
        for _ in 0..12 {
            press(&mut app, KeyCode::Char('o')); // heat to 12
        }
        press(&mut app, KeyCode::Tab); // focus equipment
        press(&mut app, KeyCode::Char(' ')); // fire Medium Laser (+3 heat)
        insta::assert_snapshot!(render(&mut app));
    }

    #[test]
    fn e2e_dice_reference_cluster_and_hitloc() {
        // LRM-20 selected → `r` opens the Cluster tab with the rack-20 column; Tab flips to the
        // 'Mech hit-location table.
        let mut app = app_with_mech(cluster_mech());
        press(&mut app, KeyCode::Tab); // focus the WEAPONS panel (LRM is row 0)
        press(&mut app, KeyCode::Char('r')); // dice reference → Cluster tab
        insta::assert_snapshot!("dice_cluster", render(&mut app));
        press(&mut app, KeyCode::Tab); // → Full Table
        press(&mut app, KeyCode::Tab); // → Hit Location tab
        insta::assert_snapshot!("dice_hit_location", render(&mut app));
    }

    #[test]
    fn e2e_dice_full_cluster_table() {
        // `r` then Tab reaches the full Cluster Hits Table; the LRM-20's rack row is highlighted.
        let mut app = app_with_mech(cluster_mech());
        press(&mut app, KeyCode::Tab); // focus WEAPONS (LRM-20)
        press(&mut app, KeyCode::Char('r')); // Cluster tab
        press(&mut app, KeyCode::Tab); // → Full Table
        let screen = render(&mut app);
        assert!(screen.contains("Full Table"));
        assert!(screen.contains("40"), "table runs past 30 to 40");
        insta::assert_snapshot!(screen);
    }

    #[test]
    fn e2e_dice_reference_single_weapon() {
        // A Medium Laser is single-hit: `r` opens on the Hit Location tab; its Cluster page reads
        // "single hit — no cluster roll".
        let mut app = app_with_one_mech();
        press(&mut app, KeyCode::Char('r')); // opens on Hit Location (selected weapon is single-hit)
        press(&mut app, KeyCode::Tab); // flip to the Cluster page to show the single-hit note
        insta::assert_snapshot!("dice_single_weapon", render(&mut app));
    }

    #[test]
    fn e2e_picker() {
        let bundle = Bundle::new(vec![sample_mech()]);
        let mut app = App::new(bundle, Session::new(), "test".to_string());
        insta::assert_snapshot!(render(&mut app));
    }

    #[test]
    fn e2e_alpha_strike_card() {
        use super::app::Screen;
        let mut app = app_with_as_mech(sample_mech());
        assert!(matches!(app.screen, Screen::AlphaStrike), "AS session shows the card");
        let screen = render(&mut app);
        assert!(screen.contains("Atlas AS7-D"), "card titled with the unit");
        assert!(screen.contains("PV 52"));
        assert!(screen.contains("Armor"));
        assert!(screen.contains("AC2/2/-"), "specials shown");
        insta::assert_snapshot!(screen);
    }

    #[test]
    fn e2e_aerospace_as_card() {
        use super::app::Screen;
        let mut app = app_with_as_mech(sample_aero_fighter());
        assert!(matches!(app.screen, Screen::AlphaStrike));
        let screen = render(&mut app);
        assert!(screen.contains("Visigoth"), "fighter name on the card");
        assert!(screen.contains("AF"), "aerospace TP code");
        assert!(screen.contains("VSTOL"), "specials shown");
        assert!(screen.contains("TH 3"), "aerospace armor Threshold shown");
        assert!(!screen.contains("MP"), "aerospace has no MP crit");
        insta::assert_snapshot!(screen);
    }

    #[test]
    fn e2e_aerospace_classic_doll() {
        let mut app = app_with_mech(sample_aero_fighter()); // Classic session
        assert_eq!(app.cursor, Location::Nose, "cursor snaps to a real arc");
        // Strip the nose (12) + 3 more → spills into the shared SI.
        for _ in 0..15 {
            press(&mut app, KeyCode::Char(' '));
        }
        let tm = app.session.active_mech().unwrap();
        assert_eq!(tm.internal_remaining(Location::AeroSI), 2, "armor overflow hit SI");
        // Set velocity + altitude via the `v` editor: v, down→altitude, but first bump velocity.
        press(&mut app, KeyCode::Char('v'));
        for _ in 0..7 {
            press(&mut app, KeyCode::Right); // velocity 7
        }
        press(&mut app, KeyCode::Down); // -> altitude
        for _ in 0..5 {
            press(&mut app, KeyCode::Right); // altitude 5
        }
        press(&mut app, KeyCode::Esc);
        assert_eq!((app.session.active_mech().unwrap().velocity, app.session.active_mech().unwrap().altitude), (7, 5));
        // Heat to 21: aero scale shows control + to-hit + shutdown + pilot — never an MP penalty.
        for _ in 0..21 {
            press(&mut app, KeyCode::Char('o'));
        }
        let screen = render(&mut app);
        assert!(screen.contains("NOS") && screen.contains("SI"), "arc + SI doll");
        assert!(screen.contains("Thrust safe 5"), "heat does not cut thrust");
        assert!(screen.contains("Vel 7") && screen.contains("Alt 5"), "velocity + altitude");
        assert!(screen.contains("control") && screen.contains("pilot"), "aero heat scale");
        assert!(!screen.contains("MP"), "no 'Mech MP penalty for aero heat");
        assert!(screen.contains("ER Large Laser"), "arc weapon kept");
        insta::assert_snapshot!(screen);
    }

    #[test]
    fn aerospace_adds_in_classic_and_alpha_strike() {
        // Classic now supports aero fighters (arcs + SI doll), so the add is accepted.
        let bundle = Bundle::new(vec![sample_aero_fighter()]);
        let mut app = App::new(bundle, Session::new(), "test".to_string());
        press(&mut app, KeyCode::Enter); // open the pre-add skill/cost modal
        press(&mut app, KeyCode::Enter); // commit the add
        assert_eq!(app.session.mechs.len(), 1, "aerospace added in a Classic session");

        // And in Alpha Strike.
        let bundle = Bundle::new(vec![sample_aero_fighter()]);
        let mut app =
            App::new(bundle, Session::new_with_mode(GameMode::AlphaStrike), "test".to_string());
        press(&mut app, KeyCode::Enter); // open the pre-add skill/cost modal
        press(&mut app, KeyCode::Enter); // commit the add
        assert_eq!(app.session.mechs.len(), 1, "aerospace added in Alpha Strike");
    }

    #[test]
    fn e2e_as_after_damage() {
        use neurohelmet_core::session::AsCritKind;
        let mut app = app_with_as_mech(sample_mech());
        for _ in 0..12 {
            press(&mut app, KeyCode::Char(' ')); // 10 armor + 2 structure
        }
        press(&mut app, KeyCode::Char('o')); // heat 1
        press(&mut app, KeyCode::Char('o')); // heat 2
        press(&mut app, KeyCode::Char('c')); // crit popup (Engine selected)
        press(&mut app, KeyCode::Char(' ')); // engine +1
        press(&mut app, KeyCode::Esc);
        let tm = app.session.active_mech().unwrap();
        assert_eq!(tm.as_armor_remaining(), 0);
        assert_eq!(tm.as_struct_remaining(), 6);
        assert_eq!(tm.as_heat, 2);
        assert_eq!(tm.as_crit(AsCritKind::Engine), 1);
        insta::assert_snapshot!(render(&mut app));
    }

    #[test]
    fn e2e_emplacement_as_card() {
        // The hand-entered Heavy Emplacement (data/extra_units.json, AS-only) renders on the AS
        // card straight from the real baked bundle.
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/mechs.bin");
        let bundle = Bundle::load(std::path::Path::new(path)).expect("load bundle");
        let m = bundle
            .mechs
            .iter()
            .find(|m| m.display_name() == "Heavy Emplacement (AC/20)")
            .expect("Heavy Emplacement baked")
            .clone();
        assert!(m.is_as_only());
        let mut app = app_with_as_mech(m);
        let screen = render(&mut app);
        assert!(screen.contains("Heavy Emplacement"));
        assert!(screen.contains("IMMOBILE"));
        insta::assert_snapshot!(screen);
    }

    #[test]
    fn large_craft_baked_with_multi_arc_card() {
        // Phase 1 of the large-craft initiative: DropShips + Small Craft bake from the real bundle,
        // typed Aerospace, carrying the multi-arc AS/BF card (front/left/right/rear ×
        // STD/CAP/SCAP/MSL) over a single Arm/Str/Th pool.
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/mechs.bin");
        let bundle = Bundle::load(std::path::Path::new(path)).expect("load bundle");
        // Phase 1 fields DropShips + Small Craft (AS type codes DS / DA / SC). Key on those codes:
        // large *support vehicles* (ground and fixed-wing) also legitimately carry firing arcs per
        // the rules (`usesArcs` = large aerospace OR large SV), so "has arcs" alone is broader than
        // the large-craft set.
        let large_craft: Vec<_> = bundle
            .mechs
            .iter()
            .filter(|m| matches!(m.as_stats.tp.as_str(), "DS" | "DA" | "SC"))
            .collect();
        assert!(
            large_craft.len() >= 200,
            "expected the ~248 baked DropShips/Small Craft, found {}",
            large_craft.len()
        );
        for m in &large_craft {
            assert!(m.is_aerospace(), "{} (DropShip/Small Craft) should be aerospace", m.display_name());
            assert!(
                m.as_stats.arcs.is_some(),
                "{} (DropShip/Small Craft) should carry the multi-arc card",
                m.display_name()
            );
        }
        // At least one DropShip carries real (non-zero) front-arc STD short-range damage.
        assert!(
            large_craft.iter().any(|m| {
                m.as_stats
                    .arcs
                    .as_ref()
                    .is_some_and(|a| !a.front.std.s.is_empty() && a.front.std.s != "0")
            }),
            "expected some DropShip to have front-arc STD damage"
        );
    }

    #[test]
    fn capital_ships_baked_phase2() {
        use super::app::bf_element_of;
        use neurohelmet_core::engine::battleforce::{bf_crit_col, bf_is_aero, BfCritCol};
        // Phase 2: JumpShips / WarShips / Space Stations bake from the real bundle, typed Aerospace,
        // carrying the multi-arc card and — where the source SUAs have them — a DT rating and doors.
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/mechs.bin");
        let bundle = Bundle::load(std::path::Path::new(path)).expect("load bundle");
        let capital: Vec<_> = bundle
            .mechs
            .iter()
            .filter(|m| matches!(m.as_stats.tp.as_str(), "JS" | "WS" | "SS"))
            .collect();
        assert!(
            capital.len() >= 180,
            "expected the ~185 baked JumpShips/WarShips/Space Stations, found {}",
            capital.len()
        );
        for m in &capital {
            assert!(m.is_aerospace(), "{} should be aerospace-typed", m.display_name());
            assert!(m.as_stats.arcs.is_some(), "{} should carry the multi-arc card", m.display_name());
            let el = bf_element_of(&neurohelmet_core::session::TrackedMech::new((*m).clone()));
            assert!(bf_is_aero(&el), "{} fights at aerospace ranges", m.display_name());
            assert_eq!(
                bf_crit_col(&el),
                Some(BfCritCol::JumpShip),
                "{} rolls the JumpShip crit column",
                m.display_name()
            );
        }
        // WarShips carry the CAP capital class in at least one arc, and the DT/door SUAs bake into
        // the numeric fields the Dock/Door crits decrement.
        let aegis = capital
            .iter()
            .find(|m| m.chassis.contains("Aegis"))
            .expect("Aegis WarShip baked");
        let arcs = aegis.as_stats.arcs.as_ref().unwrap();
        assert!(
            [&arcs.front, &arcs.left, &arcs.right, &arcs.rear]
                .iter()
                .any(|a| !a.cap.s.is_empty() && a.cap.s != "0"),
            "the Aegis carries capital (CAP) weapons"
        );
        assert!(aegis.as_stats.dt_rating > 0, "the Aegis has a baked DT rating");
        assert!(
            capital.iter().any(|m| m.as_stats.door_count > 0),
            "some capital ship bakes a transport-bay door count"
        );
    }

    #[test]
    fn e2e_as_heat_shutdown() {
        // The Alpha Strike heat scale's 4th box is S (Shutdown): heat 4 shows a shutdown banner.
        let mut app = app_with_as_mech(sample_mech());
        for _ in 0..4 {
            press(&mut app, KeyCode::Char('o')); // heat up to 4 (the S box)
        }
        let screen = render(&mut app);
        assert!(screen.contains("SHUTDOWN"), "heat 4 shows the shutdown banner");
        assert!(screen.contains("[S]"), "heat dial marks the S box");
        assert!(app.session.active_mech().unwrap().as_shutdown());
        insta::assert_snapshot!(screen);
    }

    #[test]
    fn e2e_as_ground_scale_toggle() {
        // `1` toggles 1:1 ground scale: the card MV switches to hexes (6" -> 3) and the footer
        // shows the hex range brackets.
        let mut app = app_with_as_mech(sample_mech()); // sample_mech AS movement = 6"
        let inches = render(&mut app);
        assert!(inches.contains("MV 6\""), "standard scale shows inches");

        press(&mut app, KeyCode::Char('1'));
        assert!(app.session.as_ground_scale);
        let hexes = render(&mut app);
        assert!(hexes.contains("MV 3"), "ground scale shows hexes (6\" -> 3)");
        assert!(!hexes.contains("MV 6\""), "no inches in ground scale");
        assert!(hexes.contains("1:1 hex scale"), "footer marks the scale");
        assert!(hexes.contains("S 0-3"), "footer shows hex range brackets");
        insta::assert_snapshot!(hexes);

        // Toggling back restores inches.
        press(&mut app, KeyCode::Char('1'));
        assert!(!app.session.as_ground_scale);
        assert!(render(&mut app).contains("MV 6\""));
    }

    #[test]
    fn e2e_alpha_strike_grid() {
        // Four units in an AS session render as a 2x2 grid of cards.
        let mut app = app_with_as_mech(sample_mech());
        for _ in 0..3 {
            app.session.add_mech(sample_mech());
        }
        app.session.active = 2; // third card highlighted
        let screen = render(&mut app);
        // Four cards, each with one Skill/PV line (the header PV also reads "PV 52", so count the
        // per-card Skill line instead to tally cards).
        assert_eq!(screen.matches("Skill ").count(), 4, "four cards rendered");
        insta::assert_snapshot!(screen);
    }

    #[test]
    fn a_key_creates_alpha_strike_session() {
        isolate_data_dir(); // execute_input persists to disk; keep it out of the real data dir
        let mut app = app_with_one_mech();
        press(&mut app, KeyCode::Char('S')); // open sessions browser
        press(&mut app, KeyCode::Char('A')); // new Alpha Strike session
        for c in "asgame".chars() {
            press(&mut app, KeyCode::Char(c));
        }
        press(&mut app, KeyCode::Enter);
        assert_eq!(app.session.mode, GameMode::AlphaStrike);
        assert!(app.session.mechs.is_empty(), "fresh session");
    }

    #[test]
    fn e2e_mech_preview() {
        let bundle = Bundle::new(vec![sample_mech()]);
        let mut app = App::new(bundle, Session::new(), "test".to_string());
        press(&mut app, KeyCode::Tab); // open the preview popup
        let screen = render(&mut app);
        assert!(screen.contains("Standard Armor"), "armor type shown");
        assert!(screen.contains("intro 2755"), "intro year shown");
        assert!(screen.contains("Medium Laser"), "weapon shown");
        insta::assert_snapshot!(screen);
    }

    #[test]
    fn e2e_override_card_mech() {
        use super::app::Screen;
        // An Override session lands straight on the live card. A 'Mech with a Medium Laser + AC/20
        // exercises the weapons (TIC) table, the 2d6 hit-location armor diagram, the 0–5 heat
        // ladder, and the pilot condition monitor.
        let mut app = app_with_override(combat_mech());
        assert!(matches!(app.screen, Screen::Override), "Override session shows the live card");
        let screen = render(&mut app);
        assert!(screen.contains("BattleMech"), "unit type");
        assert!(screen.contains("AC/20"), "weapon row");
        // The armor diagram is a doll: each box shows its 2d6 hit-location number (the head box is
        // unfocused at start, so it shows the number rather than the focus facing indicator).
        assert!(screen.contains("HD 12"), "head doll box + hit-location");
        assert!(screen.contains("CT"), "merged torso doll box");
        assert!(screen.contains("Automatic Shutdown"), "heat ladder");
        assert!(screen.contains("Pilot"), "condition monitor");
        insta::assert_snapshot!(screen);
    }

    #[test]
    fn e2e_override_tracking() {
        // Live tracking: damage the cursored armor region, fire a TIC (heat rises), take a pilot
        // hit. Each is the Classic-style bookkeeping the player drives.
        let mut app = app_with_override(combat_mech());
        // The doll cursor starts on the centre torso; move up to the head, then Space marks one
        // armor pip off its front.
        press(&mut app, KeyCode::Up); // Torso -> Head
        let before = app.session.active_mech().unwrap().ov_armor_remaining(Location::Head);
        press(&mut app, KeyCode::Char(' '));
        let after = app.session.active_mech().unwrap().ov_armor_remaining(Location::Head);
        assert_eq!(after, before - 1, "Space damages the cursored region");
        // Tab to the weapons panel, fire the first TIC: heat climbs off 0.
        press(&mut app, KeyCode::Tab);
        press(&mut app, KeyCode::Char(' '));
        assert!(app.session.active_mech().unwrap().ov_heat > 0, "firing a TIC banks heat");
        assert!(app.session.active_mech().unwrap().ov_fired.contains(&0), "TIC marked fired");
        // Pilot hit marks the first condition-monitor box.
        press(&mut app, KeyCode::Char('p'));
        assert_eq!(app.session.active_mech().unwrap().pilot_hits, 1, "pilot hit recorded");
        // End-turn dissipates the banked heat back down by sinks.
        press(&mut app, KeyCode::Char('e'));
        assert_eq!(app.session.active_mech().unwrap().ov_heat, 0, "end-turn dissipates heat");
        assert!(app.session.active_mech().unwrap().ov_fired.is_empty(), "fired marks cleared");
        insta::assert_snapshot!(render(&mut app));
    }

    #[test]
    fn e2e_override_help_modal() {
        // `?` in Override mode shows the Override keymap (armor / weapons panels), not the Classic
        // sheet's keys (no dice-reference, prone, GATOR, or motive entries here).
        let mut app = app_with_override(combat_mech());
        press(&mut app, KeyCode::Char('?'));
        let screen = render(&mut app);
        assert!(screen.contains("fire TIC"), "Override-specific help");
        assert!(screen.contains("Armor panel"), "armor panel section");
        assert!(!screen.contains("GATOR"), "no Classic GATOR entry");
        assert!(!screen.contains("dice reference"), "no Classic dice-reference entry");
        // DFA attribution is shown with permission — keep it visible in the Override help.
        assert!(screen.contains("Death From Above Wargaming"), "DFA attribution present");
        assert!(screen.contains("dfawargaming.com"), "DFA link present");
        insta::assert_snapshot!(screen);
    }

    #[test]
    fn e2e_override_crit_popup() {
        // `c` opens the per-region Override crit table for the cursored region. The doll cursor
        // starts on the centre torso, so this is the 'Mech torso table.
        let mut app = app_with_override(combat_mech());
        press(&mut app, KeyCode::Char('c'));
        let screen = render(&mut app);
        assert!(screen.contains("Override criticals"), "crit popup open");
        assert!(screen.contains("Gyro"), "torso crit table row");
        assert!(screen.contains("Engine"), "torso crit table row");
        insta::assert_snapshot!(screen);
    }

    #[test]
    fn e2e_override_crit_effects() {
        use neurohelmet_core::domain::Location;
        // Mark a leg actuator crit; the derived move/TMM penalty is applied and summarised on the
        // card. From the centre-torso cursor, Down reaches the Left Leg (leg crit row 1 = Actuator).
        let mut app = app_with_override(combat_mech());
        press(&mut app, KeyCode::Down); // Torso -> Left Leg
        press(&mut app, KeyCode::Char('c'));
        press(&mut app, KeyCode::Down); // select the Actuator row
        press(&mut app, KeyCode::Char(' ')); // record one actuator hit
        press(&mut app, KeyCode::Esc);
        let fx = app.session.active_mech().unwrap().ov_crit_effects();
        assert_eq!((fx.move_penalty, fx.tmm_penalty), (2, 1), "actuator crit applies");
        let screen = render(&mut app);
        assert!(screen.contains("Crits"), "crit-effects summary line");
        assert!(screen.contains("move"), "move penalty summarised");
        // The region carries a crit marker.
        assert!(
            app.session.active_mech().unwrap().ov_crit_count(Location::LeftLeg, 1) == 1,
            "leg actuator hit recorded"
        );
        insta::assert_snapshot!(screen);
    }

    #[test]
    fn e2e_override_to_hit() {
        // `t` opens the Override shot editor; setting a target TMM raises the per-TIC To-Hit row.
        let mut app = app_with_override(combat_mech());
        let base = app.session.active_mech().unwrap().ov_to_hit(0);
        press(&mut app, KeyCode::Char('t'));
        let screen = render(&mut app);
        assert!(screen.contains("To-hit shot"), "shot editor open");
        assert!(screen.contains("Target TMM"), "target TMM row");
        // Row 0 = target TMM (attacker movement is set via `v`, not here); bump it twice.
        press(&mut app, KeyCode::Char(' '));
        press(&mut app, KeyCode::Char(' '));
        assert_eq!(app.session.active_mech().unwrap().ov_shot.target_tmm, 2, "TMM set");
        assert_eq!(
            app.session.active_mech().unwrap().ov_to_hit(0),
            base + 2,
            "target TMM raises the to-hit"
        );
        insta::assert_snapshot!(render(&mut app));
        // Close the editor; the panel carries the selected weapon's range/to-hit detail + a Shot
        // summary.
        press(&mut app, KeyCode::Esc);
        let card = render(&mut app);
        assert!(card.contains("Rng"), "selected-weapon range/to-hit detail on the panel");
        assert!(card.contains("Shot"), "shot-context summary on the panel");
    }

    #[test]
    fn e2e_override_psr() {
        // Taking 10+ pips in a phase owes a PSR; stripping the torso also trips the morale prompt.
        // The cursor starts on the centre torso, so Space marks torso pips.
        let mut app = app_with_override(combat_mech());
        for _ in 0..10 {
            press(&mut app, KeyCode::Char(' '));
        }
        assert!(app.session.active_mech().unwrap().ov_psr_due(), "massive damage owes a PSR");
        let screen = render(&mut app);
        assert!(screen.contains("PSR"), "PSR prompt shown");
        assert!(screen.contains("massive"), "massive-damage situation listed");
        insta::assert_snapshot!(screen);
        // Shutting down forces an automatic-failure PSR.
        press(&mut app, KeyCode::Char('x'));
        assert_eq!(
            app.session.active_mech().unwrap().ov_psr_auto_fail(),
            Some("shutdown"),
            "shutdown auto-fails the PSR"
        );
        assert!(render(&mut app).contains("auto-fail"), "auto-fail prompt shown");
    }

    #[test]
    fn e2e_override_movement() {
        use neurohelmet_core::engine::MoveMode;
        // `v` sets this turn's movement, which feeds the to-hit attacker modifier. A fresh unit is
        // stationary (standstill, −1 to hit); declaring a ground move drops that bonus.
        let mut app = app_with_override(combat_mech());
        let standstill = app.session.active_mech().unwrap().ov_to_hit(0);
        press(&mut app, KeyCode::Char('v'));
        assert!(render(&mut app).contains("Movement"), "movement editor open");
        press(&mut app, KeyCode::Char(' ')); // row 0: cycle Stationary -> Walked
        press(&mut app, KeyCode::Esc);
        assert_eq!(app.session.active_mech().unwrap().move_mode, MoveMode::Walked, "move mode set");
        assert_eq!(
            app.session.active_mech().unwrap().ov_to_hit(0),
            standstill + 1,
            "leaving standstill drops the −1 attacker bonus"
        );
        let card = render(&mut app);
        assert!(card.contains("Moved walked"), "movement shown on the card");
        insta::assert_snapshot!(card);
    }

    #[test]
    fn e2e_override_physicals() {
        // §34.8: 'Mechs show Punch ⌈Mass/30⌉ / Kick ⌈Mass/15⌉. Atlas is 100t → 4 / 7.
        let mut app = app_with_override(combat_mech());
        assert!(render(&mut app).contains("Punch/Kick 4 / 7"), "physical-attack damage on the card");
        // Vehicles have no physical row.
        let mut veh = app_with_override(sample_vehicle());
        assert!(!render(&mut veh).contains("Punch/Kick"), "no physicals for vehicles");
    }

    #[test]
    fn e2e_override_log_export() {
        // §34.9: an Override snapshot exports its Override card (not the Classic doll). The log
        // entry carries the mode, and render_turn honours it.
        use neurohelmet_core::log::{self, LogEntry};
        use neurohelmet_core::session::TrackedMech;
        isolate_data_dir(); // append_log writes under the data dir
        let name = "neurohelmet-unit-override-export-test";
        let _ = std::fs::remove_file(log::log_file(name));
        let entry = LogEntry {
            turn: 1,
            label: "Turn 1".into(),
            ts: None,
            mode: GameMode::Override,
            mechs: vec![TrackedMech::new(combat_mech())],
            sbf: Default::default(),
            bf: Default::default(),
        };
        log::append_log(name, &entry).unwrap();
        let out = tempfile::tempdir().unwrap();
        let dir = crate::export::run(name, Some(out.path().to_path_buf())).unwrap();
        let transcript = std::fs::read_to_string(dir.join("transcript.txt")).unwrap();
        // The Override card's signatures (a doll hit-location box + the heat ladder), which the
        // Classic record sheet does not render.
        assert!(transcript.contains("HD 12"), "Override doll box in the export");
        assert!(transcript.contains("Automatic Shutdown"), "Override heat ladder in the export");
        let _ = std::fs::remove_file(log::log_file(name));
    }

    #[test]
    fn e2e_override_ammo_explosion() {
        // combat_mech carries an AC/20 ammo bin in the right torso → the merged torso (the default
        // cursor) has live ammo. Marking the torso ammo crit detonates it.
        let mut app = app_with_override(combat_mech());
        press(&mut app, KeyCode::Char('c')); // crit popup on the centre torso
        let popup = render(&mut app);
        assert!(popup.contains("Ammo here: live"), "live-ammo status in the popup");
        press(&mut app, KeyCode::Char(' ')); // mark row 0 = "Ammo (or weapon)"
        assert!(app.session.active_mech().unwrap().ov_ammo_exploded(), "ammo detonates");
        assert_eq!(
            app.session.active_mech().unwrap().ov_destroyed_reason(),
            Some("ammo"),
            "ammo explosion wrecks the unit"
        );
        let boom = render(&mut app);
        assert!(boom.contains("EXPLOSION"), "explosion called out in the popup");
        insta::assert_snapshot!(boom);
        // Marking the bin spent (a) turns the crit into a dud — no explosion.
        press(&mut app, KeyCode::Char('a'));
        assert!(!app.session.active_mech().unwrap().ov_ammo_exploded(), "spent bin → dud");
        assert_eq!(app.session.active_mech().unwrap().ov_destroyed_reason(), None);
    }

    #[test]
    fn e2e_override_card_vehicle() {
        // Vehicles drop the Ht column and the heat ladder; their armor diagram uses vehicle
        // hit-location numbers (Front 6,7,8 / Turret 5,9) and a Crew condition monitor.
        let mut app = app_with_override(sample_vehicle());
        let screen = render(&mut app);
        assert!(screen.contains("Combat Vehicle"), "vehicle type");
        assert!(!screen.contains("Automatic Shutdown"), "no heat ladder for vehicles");
        assert!(screen.contains("Crew"), "crew condition monitor");
        insta::assert_snapshot!(screen);
    }

    #[test]
    fn e2e_override_card_aero() {
        // Aerospace keep heat but label movement Thrust and carry a damage threshold (DThr).
        let mut app = app_with_override(sample_aero_fighter());
        let screen = render(&mut app);
        assert!(screen.contains("Aerospace Fighter"), "aero type");
        assert!(screen.contains("DThr"), "damage threshold");
        assert!(screen.contains("Thrust"), "thrust label");
        insta::assert_snapshot!(screen);
    }

    #[test]
    fn preview_toggles_with_tab() {
        use super::app::Screen;
        let bundle = Bundle::new(vec![sample_mech()]);
        let mut app = App::new(bundle, Session::new(), "test".to_string());
        assert!(!app.show_preview);
        press(&mut app, KeyCode::Tab);
        assert!(app.show_preview);
        press(&mut app, KeyCode::Esc); // Esc closes the preview first
        assert!(!app.show_preview);
        assert!(matches!(app.screen, Screen::Picker), "still on picker");
    }

    #[test]
    fn e2e_sessions_browser() {
        use super::app::Screen;
        use neurohelmet_core::session::SessionMeta;
        let mut app = app_with_one_mech();
        app.current_name = "default".into();
        app.sessions = vec![
            SessionMeta {
                name: "default".into(),
                mech_count: 1,
                summary: "Atlas".into(),
                mode: GameMode::Classic, force_total: 1897, limit: None,
            },
            SessionMeta {
                name: "Friday Game".into(),
                mech_count: 4,
                summary: "Atlas, Locust, Shadow Hawk +1".into(),
                mode: GameMode::AlphaStrike, force_total: 174, limit: Some(200),
            },
            SessionMeta { name: "scratch".into(), mech_count: 0, summary: "empty".into(), mode: GameMode::Classic, force_total: 0, limit: None },
        ];
        app.sessions_sel = 1;
        app.screen = Screen::Sessions;
        insta::assert_snapshot!(render(&mut app));
    }

    #[test]
    fn e2e_confirm_modal() {
        let mut app = app_with_one_mech();
        press(&mut app, KeyCode::Char('D')); // delete-mech confirm
        insta::assert_snapshot!(render(&mut app));
    }

    #[test]
    fn e2e_input_modal() {
        use super::app::{Modal, PendingAction, Screen};
        use neurohelmet_core::session::SessionMeta;
        let mut app = app_with_one_mech();
        app.current_name = "default".into();
        app.sessions = vec![SessionMeta {
            name: "default".into(),
            mech_count: 1,
            summary: "Atlas".into(),
            mode: GameMode::Classic, force_total: 1897, limit: None,
        }];
        app.screen = Screen::Sessions;
        app.modal = Some(Modal::Input {
            prompt: "New session name:".into(),
            buffer: "Saturday Game".into(),
            action: PendingAction::NewSession(neurohelmet_core::domain::GameMode::Classic),
        });
        insta::assert_snapshot!(render(&mut app));
    }

    #[test]
    fn e2e_cascade_destroyed() {
        // Overkill the left arm: damage cascades LA -> LT -> CT, destroying all three.
        let mut app = app_with_mech(combat_mech());
        app.session
            .active_mech_mut()
            .unwrap()
            .damage(Location::LeftArm, Facing::Front, 200);
        insta::assert_snapshot!(render(&mut app));
    }

    /// A mech with engine crit slots in the CT and an AC/20 (with its own crit slot) in the RT,
    /// for exercising engine-heat / engine-death / disabled-weapon rendering.
    fn consequence_mech() -> Mech {
        let mut m = sample_mech();
        m.weapons = vec![WeaponMount {
            id: 0,
            name: "AC/20".into(),
            location: Location::RightTorso,
            rear: false,
            heat: 7,
            damage: "20".into(),
            range: "3/6/9".into(),
            crit_slots: 10,
            ammo_key: None,
            to_hit: 0,
            tc_eligible: false,
            count: 1,
        }];
        m.ammo = vec![];
        m.crit_slots = BTreeMap::from([
            (
                Location::CenterTorso,
                vec![
                    CritSlot { slot: 0, name: "Fusion Engine".into(), system: true, hittable: true, ..Default::default() },
                    CritSlot { slot: 1, name: "Fusion Engine".into(), system: true, hittable: true, ..Default::default() },
                    CritSlot { slot: 2, name: "Fusion Engine".into(), system: true, hittable: true, ..Default::default() },
                ],
            ),
            (
                Location::RightTorso,
                vec![CritSlot { slot: 0, name: "AC/20".into(), system: false, hittable: true, ..Default::default() }],
            ),
        ]);
        m
    }

    #[test]
    fn e2e_engine_heat_and_disabled_weapon() {
        let mut app = app_with_mech(consequence_mech());
        // Two engine crits -> +10 heat/turn shown in the HEAT panel.
        press(&mut app, KeyCode::Char('c'));
        press(&mut app, KeyCode::Char(' ')); // CT slot 0
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Char(' ')); // CT slot 1
        press(&mut app, KeyCode::Esc);
        // Disable the AC/20 via its RT crit slot -> renders red, still fireable.
        press(&mut app, KeyCode::Right); // cursor CT -> RT
        press(&mut app, KeyCode::Char('c'));
        press(&mut app, KeyCode::Char(' ')); // RT slot 0 (AC/20)
        press(&mut app, KeyCode::Esc);
        assert_eq!(app.session.active_mech().unwrap().engine_heat(), 10);
        assert!(app.session.active_mech().unwrap().is_weapon_disabled(&consequence_mech().weapons[0]));
        insta::assert_snapshot!(render(&mut app));
    }

    #[test]
    fn e2e_engine_destroyed_banner() {
        let mut app = app_with_mech(consequence_mech());
        press(&mut app, KeyCode::Char('c'));
        press(&mut app, KeyCode::Char(' ')); // slot 0
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Char(' ')); // slot 1
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Char(' ')); // slot 2 -> 3 engine hits
        press(&mut app, KeyCode::Esc);
        assert_eq!(
            app.session.active_mech().unwrap().destroyed_reason(),
            Some("engine destroyed")
        );
        insta::assert_snapshot!(render(&mut app));
    }

    #[test]
    fn e2e_heat_sink_crit_reduces_dissipation() {
        let mut m = consequence_mech();
        // A 2-slot double heat sink in the left torso (both slots share its uid).
        m.crit_slots.insert(
            Location::LeftTorso,
            vec![
                CritSlot {
                    slot: 0,
                    name: "Double Heat Sink".into(),
                    hittable: true,
                    uid: "DoubleHeatSink@LT#0".into(),
                    hs: 2,
                    ..Default::default()
                },
                CritSlot {
                    slot: 1,
                    name: "Double Heat Sink".into(),
                    hittable: true,
                    uid: "DoubleHeatSink@LT#0".into(),
                    hs: 2,
                    ..Default::default()
                },
            ],
        );
        let mut app = app_with_mech(m);
        // Mark both slots of the one sink: dissipation drops by 2, not 4.
        press(&mut app, KeyCode::Left); // cursor CT -> LT
        press(&mut app, KeyCode::Char('c'));
        press(&mut app, KeyCode::Char(' ')); // LT slot 0
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Char(' ')); // LT slot 1 (same sink)
        press(&mut app, KeyCode::Esc);
        let tm = app.session.active_mech().unwrap();
        assert_eq!(tm.sink_dissipation_lost(), 2);
        assert_eq!(tm.dissipation(), tm.spec.dissipation - 2);
        // HEAT panel shows the effective dissipation + the loss tag.
        let screen = render(&mut app);
        assert!(screen.contains("−2 crit"), "sink loss tag in the HEAT panel");
        insta::assert_snapshot!(screen);
    }

    /// A four-legged 'Mech: torsos + four quad legs (FLL/FRL/RLL/RRL), no arms.
    fn quad_mech() -> Mech {
        let mut m = sample_mech();
        m.config = MechConfig::Quad;
        m.chassis = "Goliath".into();
        // sample_mech already armors every Location (incl. the quad legs) via Location::ALL.
        m.crit_slots = BTreeMap::from([(
            Location::FrontLeftLeg,
            vec![
                CritSlot { slot: 0, name: "Hip".into(), system: true, hittable: true, ..Default::default() },
                CritSlot { slot: 1, name: "Upper Leg Actuator".into(), system: true, hittable: true, ..Default::default() },
            ],
        )]);
        m
    }

    #[test]
    fn e2e_quad_doll() {
        let mut app = app_with_mech(quad_mech());
        let screen = render(&mut app);
        // Four quad legs render; no arms.
        assert!(screen.contains("FLL"), "front left leg");
        assert!(screen.contains("FRL"), "front right leg");
        assert!(screen.contains("RLL"), "rear left leg");
        assert!(screen.contains("RRL"), "rear right leg");
        assert!(!screen.contains("LA"), "no left arm on a quad");
        insta::assert_snapshot!(screen);
    }

    #[test]
    fn quad_cursor_skips_arms() {
        // Cursor starts on CT; moving around must only land on quad locations.
        let mut app = app_with_mech(quad_mech());
        let valid = neurohelmet_core::domain::MechConfig::Quad.locations();
        for code in [
            KeyCode::Left, KeyCode::Left, KeyCode::Down, KeyCode::Down, KeyCode::Right,
            KeyCode::Right, KeyCode::Up, KeyCode::Up,
        ] {
            press(&mut app, code);
            assert!(valid.contains(&app.cursor), "cursor on {:?} not valid for a quad", app.cursor);
        }
    }

    #[test]
    fn e2e_crit_popup() {
        // Cursor starts on CenterTorso, which sample_mech gives 4 crit slots.
        let mut app = app_with_one_mech();
        press(&mut app, KeyCode::Char('c')); // open crit popup for CT
        press(&mut app, KeyCode::Char(' ')); // mark slot 0 (Fusion Engine)
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Char(' ')); // mark slot 2 (Life Support)
        insta::assert_snapshot!(render(&mut app));
    }

    #[test]
    fn e2e_crit_persists_and_toggles_off() {
        let mut app = app_with_one_mech();
        press(&mut app, KeyCode::Char('c'));
        press(&mut app, KeyCode::Char(' ')); // mark slot 0
        assert!(app.session.active_mech().unwrap().is_crit_hit(Location::CenterTorso, 0));
        press(&mut app, KeyCode::Char(' ')); // unmark slot 0
        assert!(!app.session.active_mech().unwrap().is_crit_hit(Location::CenterTorso, 0));
        press(&mut app, KeyCode::Esc); // close
        assert!(app.modal.is_none());
    }

    #[test]
    fn crit_popup_a_sets_active_ammo_bin() {
        use super::app::Modal;
        // Two compatible AC/20 bins (LT + RT) feeding an AC/20; firing defaults to the
        // first (LT, id 0). Give the RT bin a crit slot so the popup can target it.
        let mut m = sample_mech();
        m.weapons.push(WeaponMount {
            id: 1,
            name: "AC/20".into(),
            location: Location::RightTorso,
            rear: false,
            heat: 7,
            damage: "20".into(),
            range: "3/6/9".into(),
            crit_slots: 1,
            ammo_key: Some("AC:20".into()),
            to_hit: 0,
            tc_eligible: false,
            count: 1,
        });
        m.ammo = vec![
            AmmoBin {
                id: 0,
                name: "AC/20 Ammo".into(),
                location: Location::LeftTorso,
                shots_per_ton: 5,
                tons: 1,
                ammo_key: Some("AC:20".into()),
                munition: String::new(),
                base_ammo: None,
            },
            AmmoBin {
                id: 1,
                name: "AC/20 Ammo".into(),
                location: Location::RightTorso,
                shots_per_ton: 5,
                tons: 1,
                ammo_key: Some("AC:20".into()),
                munition: String::new(),
                base_ammo: None,
            },
        ];
        m.crit_slots.insert(
            Location::RightTorso,
            vec![CritSlot { slot: 0, name: "AC/20 Ammo".into(), system: false, hittable: true, ..Default::default() }],
        );
        let mut app = app_with_mech(m);

        // Default: the weapon draws from the first compatible bin (LT, id 0).
        assert_eq!(app.session.active_mech().unwrap().weapon_bin(1), Some(0));

        // Open the RT crit popup on its ammo slot and press `a`.
        app.modal = Some(Modal::Crit { loc: Location::RightTorso, sel: 0 });
        press(&mut app, KeyCode::Char('a'));

        let tm = app.session.active_mech().unwrap();
        assert!(tm.is_active_bin(1), "RT bin is now active");
        assert_eq!(tm.weapon_bin(1), Some(1), "weapon now draws from the RT bin");
        assert!(app.status.contains("Active bin"), "status confirms the choice");
    }

    /// A mech with one LRM bin whose `base_ammo` group offers four munitions, in a bundle whose
    /// catalog carries them — the setup needed to exercise the `t` munition picker.
    fn munition_app() -> App {
        let mut m = sample_mech();
        m.ammo = vec![AmmoBin {
            id: 0,
            name: "LRM 20 Ammo".into(),
            location: Location::LeftTorso,
            shots_per_ton: 6,
            tons: 2,
            ammo_key: Some("LRM:20".into()),
            munition: String::new(),
            base_ammo: Some("LRM20".into()),
        }];
        m.crit_slots.insert(
            Location::LeftTorso,
            vec![CritSlot { slot: 0, name: "LRM 20 Ammo".into(), system: false, hittable: true, ..Default::default() }],
        );
        let mut bundle = Bundle::new(vec![m.clone()]);
        bundle.munitions.insert(
            "LRM20".into(),
            vec![
                "Standard".into(),
                "Fragmentation".into(),
                "Semi-Guided".into(),
                "Inferno".into(),
            ],
        );
        let mut session = Session::new();
        session.add_mech(m);
        let mut app = App::new(bundle, session, "test".to_string());
        app.dirty = false;
        app
    }

    #[test]
    fn crit_popup_t_loads_munition() {
        use super::app::Modal;
        let mut app = munition_app();
        // Default munition is Standard.
        assert_eq!(app.session.active_mech().unwrap().bin_munition(0), "Standard");

        // Open crit on LT, press `t` -> picker opens at the loaded munition (index 0).
        app.modal = Some(Modal::Crit { loc: Location::LeftTorso, sel: 0 });
        press(&mut app, KeyCode::Char('t'));
        assert!(matches!(app.modal, Some(Modal::Munition { bin: 0, sel: 0, .. })));

        // Scroll to Semi-Guided (index 2) and load it; closing returns to the crit popup.
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Enter);
        assert_eq!(app.session.active_mech().unwrap().bin_munition(0), "Semi-Guided");
        assert!(matches!(app.modal, Some(Modal::Crit { .. })));
    }

    #[test]
    fn t_on_non_ammo_slot_is_noop() {
        use super::app::Modal;
        // sample_mech's CT slot 0 is the Fusion Engine, not ammo.
        let mut app = app_with_one_mech();
        app.modal = Some(Modal::Crit { loc: Location::CenterTorso, sel: 0 });
        press(&mut app, KeyCode::Char('t'));
        assert!(matches!(app.modal, Some(Modal::Crit { .. })), "stays on crit popup");
        assert_eq!(app.status, "No munition options");
    }

    #[test]
    fn e2e_munition_picker() {
        use super::app::Modal;
        let mut app = munition_app();
        app.modal = Some(Modal::Munition { loc: Location::LeftTorso, crit_sel: 0, bin: 0, sel: 1 });
        insta::assert_snapshot!(render(&mut app));
    }

    /// Atlas + a physical weapon (Hatchet) and a spread of gear, to exercise the equipment rows
    /// and preview section.
    fn equipped_mech() -> Mech {
        let mut m = sample_mech();
        m.weapons.push(WeaponMount {
            id: 1,
            name: "Hatchet".into(),
            location: Location::RightArm,
            rear: false,
            heat: 0,
            damage: "20".into(),
            range: String::new(),
            crit_slots: 0,
            ammo_key: None,
            to_hit: 0,
            tc_eligible: false,
            count: 1,
        });
        m.equipment = vec![
            Equipment { name: "Jump Jet".into(), location: Location::LeftLeg },
            Equipment { name: "Jump Jet".into(), location: Location::RightLeg },
            Equipment { name: "ECM Suite (Guardian)".into(), location: Location::LeftTorso },
            Equipment { name: "CASE".into(), location: Location::RightTorso },
        ];
        m
    }

    #[test]
    fn equipment_rows_have_no_fire_action() {
        let mut app = app_with_mech(equipped_mech());
        // Gear shows as rows after weapons + ammo.
        let rows = app.equip_rows();
        let equip_count = rows.iter().filter(|r| matches!(r, EquipRow::Equip(_))).count();
        assert_eq!(equip_count, 4, "four gear rows");

        // Select the last row (a gear row) and "fire" it -> a harmless no-op with a status.
        app.focus = Focus::Equipment;
        app.equip_sel = rows.len() - 1;
        press(&mut app, KeyCode::Char(' '));
        assert!(app.status.contains("no action"), "got: {}", app.status);
    }

    #[test]
    fn e2e_equipment_panel() {
        let mut app = app_with_mech(equipped_mech());
        press(&mut app, KeyCode::Tab); // focus the equipment panel
        let screen = render(&mut app);
        assert!(screen.contains("EQUIP"), "panel title");
        assert!(screen.contains("Hatchet"), "physical weapon in the weapon list");
        assert!(screen.contains("Jump Jet"), "gear listed");
        assert!(screen.contains("sinks 20× Single"), "heat sinks in HEAT panel");
        insta::assert_snapshot!(screen);
    }

    #[test]
    fn e2e_gator_to_hit_target() {
        use super::app::Modal;
        let mut app = app_with_one_mech();
        press(&mut app, KeyCode::Tab); // focus the equipment panel (selects the Medium Laser)

        // `t` opens the GATOR target editor.
        press(&mut app, KeyCode::Char('t'));
        assert!(matches!(app.modal, Some(Modal::Gator { sel: 0 })));
        // Distance 0 -> 5: a 3/6/9 weapon brackets that as Medium (+2).
        for _ in 0..5 {
            press(&mut app, KeyCode::Right);
        }
        // Row 1: target moved 5 hexes (Target Movement Modifier +2).
        press(&mut app, KeyCode::Down);
        for _ in 0..5 {
            press(&mut app, KeyCode::Right);
        }
        let modal = render(&mut app);
        assert!(modal.contains("Distance"), "modal shows the distance row");
        assert!(modal.contains("TMM +2"), "derived target movement modifier shown");

        press(&mut app, KeyCode::Esc); // close; the target stays set
        let tm = app.session.active_mech().unwrap();
        assert_eq!(tm.ct_target.unwrap().distance, 5);

        // Gunnery 4 + stationary 0 + target TMM +2 + medium range +2 = 8.
        let screen = render(&mut app);
        assert!(screen.contains("8+"), "per-weapon GATOR target number on the weapon row");
        insta::assert_snapshot!(screen);
    }

    #[test]
    fn e2e_gator_out_of_range() {
        let mut app = app_with_one_mech();
        // Set a target far beyond the Medium Laser's 3/6/9 (extreme = 18 hexes). Stepping up from
        // "no target" creates it at 1 hex; a second step pushes it out past extreme range.
        if let Some(tm) = app.session.active_mech_mut() {
            tm.ct_adjust_distance(1); // None -> 1 hex
            tm.ct_adjust_distance(39); // -> 40 hexes
        }
        press(&mut app, KeyCode::Tab);
        let screen = render(&mut app);
        assert!(screen.contains(" X"), "out-of-range weapon shows X");
    }

    #[test]
    fn e2e_equipment_preview() {
        let bundle = Bundle::new(vec![equipped_mech()]);
        let mut app = App::new(bundle, Session::new(), "test".to_string());
        press(&mut app, KeyCode::Tab); // open the preview popup
        let screen = render(&mut app);
        assert!(screen.contains("Heat sinks"), "heat sink line");
        assert!(screen.contains("Equipment"), "equipment section");
        assert!(screen.contains("Jump Jet"), "grouped gear");
        insta::assert_snapshot!(screen);
    }

    #[test]
    fn skills_editor_edits_and_shows() {
        use super::app::Modal;
        let mut app = app_with_one_mech();
        // PILOT panel shows the default 4/5.
        let screen = render(&mut app);
        assert!(screen.contains("Gunnery 4+") && screen.contains("Piloting 5+"), "skills shown");

        // `g` opens the editor; `+` improves a skill (lower number), nav with ↑↓.
        press(&mut app, KeyCode::Char('g'));
        assert!(matches!(app.modal, Some(Modal::Skills { sel: 0 })));
        press(&mut app, KeyCode::Right); // gunnery 4 -> 3
        press(&mut app, KeyCode::Down); // select piloting
        press(&mut app, KeyCode::Left); // piloting 5 -> 6
        press(&mut app, KeyCode::Esc);

        let tm = app.session.active_mech().unwrap();
        assert_eq!((tm.gunnery, tm.piloting), (3, 6));
        let screen = render(&mut app);
        assert!(screen.contains("Gunnery 3+") && screen.contains("Piloting 6+"));
    }

    #[test]
    fn e2e_skills_modal() {
        let mut app = app_with_one_mech();
        press(&mut app, KeyCode::Char('g'));
        insta::assert_snapshot!(render(&mut app));
    }

    #[test]
    fn e2e_add_unit_modal_shows_skill_cost() {
        use neurohelmet_core::domain::GameMode;
        // Picker Enter opens the pre-add skill/cost preview: the unit's skill-adjusted cost and
        // the running force total against the session budget.
        let mut app = app_with_one_mech();
        app.session.limit = Some(4000);
        press(&mut app, KeyCode::Char('a')); // tracker -> picker
        press(&mut app, KeyCode::Enter); // open the AddUnit modal (idx 0 = the Atlas)
        let modal = render(&mut app);
        assert!(modal.contains("Cost"), "shows the cost line");
        // Default 4/5 leaves the baked BV unchanged; force = existing 1897 + this 1897 vs 4000.
        assert!(modal.contains("BV 1897"), "default-skill cost is the base BV");
        assert!(modal.contains("3794/4000"), "running force total vs the budget");
        insta::assert_snapshot!(modal);

        // Improve gunnery to 0 (table 0/5 = 1.75 -> round(1897*1.75) = 3320); now it busts 4000.
        for _ in 0..4 {
            press(&mut app, KeyCode::Right);
        }
        let elite = render(&mut app);
        assert!(elite.contains("BV 3320"), "elite gunnery raises the adjusted BV");
        assert!(elite.contains("OVER"), "5217/4000 flags the busted budget");

        // Commit the add at the chosen skills.
        press(&mut app, KeyCode::Enter);
        let tm = app.session.active_mech().unwrap();
        assert_eq!(tm.gunnery, 0);
        assert_eq!(tm.point_cost(GameMode::Classic), 3320);
        assert!(app.session.over_limit(), "force now exceeds the limit");
    }

    #[test]
    fn e2e_add_unit_modal_alpha_strike_skill_pv() {
        use neurohelmet_core::domain::GameMode;
        // In an Alpha Strike session the pre-add modal shows a single Skill row (not Gunnery/
        // Piloting) and the skill-adjusted PV against the budget.
        let mut app = app_with_as_mech(sample_mech());
        app.session.limit = Some(200);
        press(&mut app, KeyCode::Char('a')); // AS screen -> picker
        press(&mut app, KeyCode::Enter); // open the AddUnit modal (idx 0 = the Atlas)
        let modal = render(&mut app);
        assert!(modal.contains("Skill"), "single AS Skill row");
        assert!(!modal.contains("Piloting"), "no Piloting row in Alpha Strike");
        assert!(modal.contains("PV 52"), "default Skill 4 = base PV");
        assert!(modal.contains("104/200"), "running force total (52 + 52) vs the budget");
        insta::assert_snapshot!(modal);

        // Worsen the Skill to 5: PV drops (52 - (5-4) * (1 + (52-5)/10) = 47).
        press(&mut app, KeyCode::Left);
        let worse = render(&mut app);
        assert!(worse.contains("PV 47"), "Skill 5 lowers the adjusted PV");

        // Commit at the chosen Skill.
        press(&mut app, KeyCode::Enter);
        let tm = app.session.active_mech().unwrap();
        assert_eq!(tm.gunnery, 5);
        assert_eq!(tm.point_cost(GameMode::AlphaStrike), 47);
    }

    #[test]
    fn movement_editor_edits_and_shows() {
        use super::app::Modal;
        use neurohelmet_core::engine::MoveMode;
        let mut app = app_with_one_mech();
        // Unset turn: the MOVE panel shows the dim stationary hint.
        let screen = render(&mut app);
        assert!(screen.contains("stationary"), "unset movement hint");

        // `v` opens the editor; `+` cycles the mode, ↓ selects hexes. The sample mech has no
        // jump MP, so the cycle skips jumped; hexes cap at the mode's MP (run 5).
        press(&mut app, KeyCode::Char('v'));
        assert!(matches!(app.modal, Some(Modal::Move { sel: 0 })));
        press(&mut app, KeyCode::Right); // stationary -> walked
        press(&mut app, KeyCode::Right); // walked -> ran
        press(&mut app, KeyCode::Right); // ran -> (skips jumped: no jump MP) -> stationary
        assert_eq!(app.session.active_mech().unwrap().move_mode, MoveMode::Stationary);
        press(&mut app, KeyCode::Left); // back to ran
        press(&mut app, KeyCode::Down); // select hexes
        for _ in 0..7 {
            press(&mut app, KeyCode::Right);
        }
        press(&mut app, KeyCode::Esc);

        let tm = app.session.active_mech().unwrap();
        assert_eq!(tm.move_mode, MoveMode::Ran);
        assert_eq!(tm.hexes_moved, 5, "capped at run MP");
        let screen = render(&mut app);
        assert!(screen.contains("ran 5"), "mode + hexes on the MOVE panel");
        assert!(screen.contains("atk +2") && screen.contains("TMM +2"), "derived modifiers");

        // End turn clears it back to stationary.
        press(&mut app, KeyCode::Char('e'));
        assert_eq!(app.session.active_mech().unwrap().move_mode, MoveMode::Stationary);
    }

    #[test]
    fn e2e_movement_modal() {
        let mut app = app_with_one_mech();
        press(&mut app, KeyCode::Char('v'));
        press(&mut app, KeyCode::Right); // walked
        press(&mut app, KeyCode::Down);
        for _ in 0..4 {
            press(&mut app, KeyCode::Right); // 4 hexes
        }
        insta::assert_snapshot!(render(&mut app));
    }

    #[test]
    fn prone_toggle_and_psr_prompt() {
        let mut app = app_with_one_mech();
        // `d` toggles prone; the MOVE panel shows the banner.
        press(&mut app, KeyCode::Char('d'));
        assert!(app.session.active_mech().unwrap().prone);
        assert!(render(&mut app).contains("PRONE"), "prone banner");
        press(&mut app, KeyCode::Char('d'));
        assert!(!app.session.active_mech().unwrap().prone);

        // 20+ damage this turn flags a PSR; end-turn (`e`) clears it.
        app.session
            .active_mech_mut()
            .unwrap()
            .damage(Location::CenterTorso, Facing::Front, 20);
        assert!(render(&mut app).contains("PSR"), "PSR prompt after 20 damage");
        press(&mut app, KeyCode::Char('e'));
        assert!(render(&mut app).contains("standing"), "PSR cleared on end-turn");
    }

    #[test]
    fn e2e_move_leg_destroyed() {
        let mut app = app_with_one_mech();
        // Exactly armor (20) + internal (10) destroys the leg without cascading into the torso.
        app.session
            .active_mech_mut()
            .unwrap()
            .damage(Location::LeftLeg, Facing::Front, 30);
        let screen = render(&mut app);
        assert!(screen.contains("Walk 1"), "hobble at 1 MP");
        assert!(screen.contains("leg gone"));
        insta::assert_snapshot!(screen);
    }

    #[test]
    fn fired_weapon_marked_until_unfire_or_end_turn() {
        let mut app = app_with_one_mech(); // weapon 0 = Medium Laser, heat 3
        press(&mut app, KeyCode::Tab); // focus the weapons panel

        // Fire -> marked, and a ✓ shows in the panel.
        press(&mut app, KeyCode::Char(' '));
        assert!(app.session.active_mech().unwrap().is_fired(0));
        assert!(render(&mut app).contains('✓'), "fired marker");

        // Re-firing is a no-op: no extra heat, just a reminder.
        let heat = app.session.active_mech().unwrap().heat;
        press(&mut app, KeyCode::Char(' '));
        assert_eq!(app.session.active_mech().unwrap().heat, heat, "no double heat");
        assert!(app.status.contains("Already fired"));

        // `u` un-fires (clears the mark); end-turn would too.
        press(&mut app, KeyCode::Char('u'));
        assert!(!app.session.active_mech().unwrap().is_fired(0));
        press(&mut app, KeyCode::Char(' ')); // fire again
        press(&mut app, KeyCode::Char('e')); // end turn
        assert!(!app.session.active_mech().unwrap().is_fired(0));
    }

    #[test]
    fn ultra_ac_fires_twice_then_caps() {
        let mut m = sample_mech();
        m.weapons = vec![WeaponMount {
            id: 0,
            name: "Ultra AC/5".into(),
            location: Location::RightTorso,
            rear: false,
            heat: 1,
            damage: "5".into(),
            range: "6/12/18".into(),
            crit_slots: 1,
            ammo_key: Some("AC_ULTRA:5".into()),
            to_hit: 0,
            tc_eligible: false,
            count: 1,
        }];
        let mut app = app_with_mech(m);
        press(&mut app, KeyCode::Tab); // focus weapons

        press(&mut app, KeyCode::Char(' ')); // shot 1
        press(&mut app, KeyCode::Char(' ')); // shot 2
        assert_eq!(app.session.active_mech().unwrap().shots_fired(0), 2);
        assert_eq!(app.session.active_mech().unwrap().heat, 2); // 1 heat x 2 shots
        assert!(render(&mut app).contains("✓2/2"), "shows shots/max");

        // Third press is capped.
        press(&mut app, KeyCode::Char(' '));
        assert_eq!(app.session.active_mech().unwrap().shots_fired(0), 2);
        assert!(app.status.contains("Max shots"));
    }

    #[test]
    fn e2e_weapon_fired_marker() {
        let mut app = app_with_one_mech();
        press(&mut app, KeyCode::Tab);
        press(&mut app, KeyCode::Char(' ')); // fire the Medium Laser
        insta::assert_snapshot!(render(&mut app));
    }

    #[test]
    fn e2e_weapon_to_hit() {
        let mut m = sample_mech();
        m.weapons = vec![WeaponMount {
            id: 0,
            name: "Medium Pulse Laser".into(),
            location: Location::RightArm,
            rear: false,
            heat: 4,
            damage: "6".into(),
            range: "2/4/6".into(),
            crit_slots: 1,
            ammo_key: None,
            to_hit: -2,
            tc_eligible: true,
            count: 1,
        }];
        m.equipment = vec![Equipment { name: "Targeting Computer".into(), location: Location::Head }];
        let mut app = app_with_mech(m);
        press(&mut app, KeyCode::Tab); // focus the weapons panel
        let screen = render(&mut app);
        assert!(screen.contains("to-hit -3"), "detail shows the total (pulse -2 + TC -1)");
        assert!(screen.contains("(TC)"), "TC contribution flagged");
        insta::assert_snapshot!(screen);
    }

    #[test]
    fn log_snapshot_increments_turn_and_writes() {
        use neurohelmet_core::log;
        let mut app = app_with_one_mech();
        app.current_name = "neurohelmet-unit-log-test".into();
        let _ = std::fs::remove_file(log::log_file(&app.current_name));

        press(&mut app, KeyCode::Char('L'));
        assert_eq!(app.session.turn, 1);
        assert!(app.status.contains("Logged Turn 1"), "got: {}", app.status);
        assert!(app.dirty, "bumped turn must persist");

        let entries = log::read_log(&app.current_name).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].turn, 1);
        assert_eq!(entries[0].mechs.len(), 1);

        // Roster shows the snapshot count.
        assert!(render(&mut app).contains("log 1"), "roster indicator");

        let _ = std::fs::remove_file(log::log_file(&app.current_name));
    }

    #[test]
    fn export_renders_log_to_ppm_frames() {
        use neurohelmet_core::log::{self, LogEntry};
        use neurohelmet_core::session::TrackedMech;

        isolate_data_dir(); // append_log writes under the data dir
        let name = "neurohelmet-unit-export-test";
        let _ = std::fs::remove_file(log::log_file(name));
        let entry = LogEntry {
            turn: 1,
            label: "Turn 1".into(),
            ts: None,
            mode: GameMode::Classic,
            mechs: vec![TrackedMech::new(sample_mech()), TrackedMech::new(combat_mech())],
            sbf: Default::default(),
            bf: Default::default(),
        };
        log::append_log(name, &entry).unwrap();

        let out = tempfile::tempdir().unwrap();
        let dir = crate::export::run(name, Some(out.path().to_path_buf())).unwrap();

        // Montage + per-mech PPMs + transcript exist; PPMs are valid P6.
        let montage = std::fs::read(dir.join("turn-01.ppm")).unwrap();
        assert!(montage.starts_with(b"P6\n"), "montage is a PPM");
        let mut per_mech: Vec<_> = std::fs::read_dir(dir.join("turn-01"))
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "ppm"))
            .collect();
        per_mech.sort();
        assert_eq!(per_mech.len(), 2, "one PPM per mech");
        for p in &per_mech {
            assert!(std::fs::read(p).unwrap().starts_with(b"P6\n"));
        }
        let transcript = std::fs::read_to_string(dir.join("transcript.txt")).unwrap();
        assert!(transcript.contains("Atlas"), "transcript names the mech");

        let _ = std::fs::remove_file(log::log_file(name));
    }

    #[test]
    fn e2e_vehicle_as_card() {
        let mut app = app_with_as_mech(sample_vehicle());
        let screen = render(&mut app);
        assert!(screen.contains("Manticore"), "vehicle name on the AS card");
        insta::assert_snapshot!(screen);
    }

    #[test]
    fn e2e_vehicle_classic_doll() {
        // A vehicle in a Classic session renders its front/sides/rear/turret doll + CRITS/CREW/MOVE.
        let mut app = app_with_mech(sample_vehicle());
        let screen = render(&mut app);
        assert!(screen.contains("FR") && screen.contains("TU"), "vehicle locations");
        assert!(screen.contains("CRITS") && screen.contains("CREW"), "vehicle panels");
        assert!(screen.contains("Cruise"), "vehicle movement");
        insta::assert_snapshot!(screen);
    }

    #[test]
    fn vehicle_damage_and_crits() {
        use super::app::Modal;
        let mut app = app_with_mech(sample_vehicle());
        // Damage the front past its armor into the internal pool.
        app.session
            .active_mech_mut()
            .unwrap()
            .damage(Location::Front, Facing::Front, 23); // 20 armor + 3 internal
        let tm = app.session.active_mech().unwrap();
        assert_eq!(tm.armor_remaining(Location::Front, Facing::Front), 0);
        assert_eq!(tm.internal_remaining(Location::Front), 3); // 6 - 3

        // Motive + crew + a vehicle crit via keys/popup.
        press(&mut app, KeyCode::Char('m')); // open Motive System Damage popup
        press(&mut app, KeyCode::Down); // -> Moderate (−1 MP)
        press(&mut app, KeyCode::Char(' ')); // apply it
        press(&mut app, KeyCode::Esc); // close the popup
        press(&mut app, KeyCode::Char('p')); // crew hit
        assert_eq!(
            app.session.active_mech().unwrap().motive_damage,
            vec![neurohelmet_core::session::MotiveLevel::Moderate]
        );
        assert_eq!(app.session.active_mech().unwrap().crew_hits, 1);
        press(&mut app, KeyCode::Char('c')); // vehicle crit popup
        assert!(matches!(app.modal, Some(Modal::VehicleCrit { .. })));
        press(&mut app, KeyCode::Char(' ')); // toggle the first crit (Engine)
        assert!(app.session.active_mech().unwrap().is_vehicle_crit("Engine"));
    }

    #[test]
    fn e2e_vehicle_crit_popup() {
        let mut app = app_with_mech(sample_vehicle());
        press(&mut app, KeyCode::Char('c')); // vehicle crit popup
        press(&mut app, KeyCode::Down); // -> Weapon
        press(&mut app, KeyCode::Char(' ')); // mark it
        insta::assert_snapshot!(render(&mut app));
    }

    #[test]
    fn e2e_aero_graded_engine_crit() {
        // Engine is index 1 in AEROSPACE_CRITS (Avionics, Engine, ...). Two hits → −4 Safe Thrust.
        let mut app = app_with_mech(sample_aero_fighter());
        press(&mut app, KeyCode::Char('c')); // aerospace crit popup
        press(&mut app, KeyCode::Down); // -> Engine
        press(&mut app, KeyCode::Char(' ')); // 1 hit
        press(&mut app, KeyCode::Char(' ')); // 2 hits
        let tm = app.session.active_mech().unwrap();
        assert_eq!(tm.aero_engine_hits(), 2);
        assert_eq!((tm.movement().walk, tm.movement().run), (1, 2)); // 5 − 4 Safe; ⌈1.5×1⌉ Max
        assert_eq!(tm.engine_heat(), 4);
        // Snapshot the popup open — the row shows "Engine  ×2  −4 thrust, +4 heat".
        insta::assert_snapshot!(render(&mut app));
    }

    #[test]
    fn e2e_aero_weapon_crit_popup() {
        use super::app::Modal;
        use neurohelmet_core::session::AEROSPACE_CRITS;
        let mut app = app_with_mech(sample_aero_fighter());
        press(&mut app, KeyCode::Char('c')); // aerospace crit popup
        assert!(matches!(app.modal, Some(Modal::VehicleCrit { .. })));
        // 5 system crits (Avionics..Sensors), then the lone Nose weapon row at index 5.
        for _ in 0..AEROSPACE_CRITS.len() {
            press(&mut app, KeyCode::Down);
        }
        press(&mut app, KeyCode::Char(' ')); // destroy the weapon
        let tm = app.session.active_mech().unwrap();
        assert!(tm.is_weapon_disabled(&tm.spec.weapons[0]));
        // No system crit got marked.
        assert!(tm.vehicle_crits.is_empty());
        insta::assert_snapshot!(render(&mut app));
    }

    #[test]
    fn e2e_as_crit_popup() {
        let mut app = app_with_as_mech(sample_mech());
        press(&mut app, KeyCode::Char('c')); // Alpha Strike crit popup (opens on Engine)
        press(&mut app, KeyCode::Char(' ')); // engine +1
        insta::assert_snapshot!(render(&mut app)); // captured while the popup is open
    }

    #[test]
    fn e2e_as_to_hit_shot() {
        // §33 Phase 2: attacker jump + target TMM fold into the card's To-Hit row.
        let mut app = app_with_as_mech(sample_mech()); // Atlas, Skill 4
        press(&mut app, KeyCode::Char('t')); // open the to-hit shot editor
        press(&mut app, KeyCode::Char(' ')); // row 0: attacker jumped on (+2)
        press(&mut app, KeyCode::Down); // -> target TMM
        for _ in 0..3 {
            press(&mut app, KeyCode::Right); // None -> TMM 2
        }
        press(&mut app, KeyCode::Down); // -> target jumped
        press(&mut app, KeyCode::Char(' ')); // target jumped on (+1)
        let modal = render(&mut app);
        assert!(modal.contains("To-hit target"), "shot modal titled");
        // Skill 4 + range 0 + atk jump 2 + (TMM 2 + jumped 1) = 9 at short range.
        assert!(modal.contains("S 9+"), "preview folds the shot context");
        press(&mut app, KeyCode::Esc); // close
        let screen = render(&mut app);
        assert!(screen.contains("To-Hit    S 9+"), "card to-hit reflects the shot");
        assert!(screen.contains("atk jump"), "shot summary line");
        assert!(screen.contains("tgt TMM2 jumped"));
        insta::assert_snapshot!(screen);
    }

    #[test]
    fn as_log_snapshot_bumps_turn() {
        // The game log (§13) is mode-agnostic; `L` is now wired into the AS card too.
        use neurohelmet_core::log;
        let mut app = app_with_as_mech(sample_mech());
        app.current_name = "neurohelmet-as-log-test".into();
        let _ = std::fs::remove_file(log::log_file(&app.current_name));
        press(&mut app, KeyCode::Char('L'));
        assert_eq!(app.session.turn, 1, "AS log snapshot bumps the turn");
        let _ = std::fs::remove_file(log::log_file(&app.current_name));
    }

    #[test]
    fn q_confirms_before_quitting() {
        let mut app = app_with_one_mech();
        press(&mut app, KeyCode::Char('q'));
        assert!(!app.should_quit, "q opens a confirm dialog, doesn't quit");
        assert!(app.modal.is_some(), "confirm modal shown");
        press(&mut app, KeyCode::Char('n'));
        assert!(!app.should_quit, "declining keeps the app open");
        assert!(app.modal.is_none(), "modal dismissed");
        press(&mut app, KeyCode::Char('q'));
        press(&mut app, KeyCode::Char('y'));
        assert!(app.should_quit, "confirming quits");

        // Ctrl+C still bypasses the prompt entirely.
        let mut app2 = app_with_one_mech();
        press_ctrl(&mut app2, KeyCode::Char('c'));
        assert!(app2.should_quit, "Ctrl+C quits immediately");
    }

    #[test]
    fn ba_trooper_damage_and_wipe() {
        let mut app = app_with_mech(sample_battle_armor());
        // Cursor snaps to the first trooper; Space chews through suit armor then the trooper.
        assert_eq!(app.cursor, Location::Trooper1);
        for _ in 0..7 {
            press(&mut app, KeyCode::Char(' ')); // 6 armor + 1 trooper
        }
        let tm = app.session.active_mech().unwrap();
        assert!(tm.is_destroyed(Location::Trooper1), "trooper 1 down");
        assert_eq!(tm.troopers_remaining(), 3);
        assert!(tm.destroyed_reason().is_none(), "squad still fighting");

        // Wipe the rest: the squad is destroyed.
        for loc in &Location::TROOPERS[1..4] {
            let tm = app.session.active_mech_mut().unwrap();
            tm.damage(*loc, Facing::Front, 7);
        }
        let tm = app.session.active_mech().unwrap();
        assert_eq!(tm.troopers_remaining(), 0);
        assert_eq!(tm.destroyed_reason(), Some("squad wiped out"));
        assert!(tm.movement().immobile);
    }

    #[test]
    fn ci_strength_track() {
        let mut app = app_with_mech(sample_platoon());
        assert_eq!(app.cursor, Location::Platoon);
        // No armor: damage goes straight to strength.
        press(&mut app, KeyCode::Char(' '));
        press(&mut app, KeyCode::Char(' '));
        let tm = app.session.active_mech().unwrap();
        assert_eq!(tm.troopers_remaining(), 19);
        // Wipe it out.
        app.session.active_mech_mut().unwrap().damage(Location::Platoon, Facing::Front, 19);
        let tm = app.session.active_mech().unwrap();
        assert_eq!(tm.destroyed_reason(), Some("platoon wiped out"));
    }

    #[test]
    fn cursor_moves_between_troopers() {
        // Regression: the doll cursor must move over the unit's REAL locations (troopers),
        // not the mech-config set — and damage must land on the cursored trooper.
        let mut app = app_with_mech(sample_battle_armor());
        assert_eq!(app.cursor, Location::Trooper1);
        press(&mut app, KeyCode::Right);
        assert_eq!(app.cursor, Location::Trooper2);
        press(&mut app, KeyCode::Char(' ')); // 1 damage to T2's armor
        let tm = app.session.active_mech().unwrap();
        assert_eq!(tm.armor_remaining(Location::Trooper2, Facing::Front), 5);
        assert_eq!(tm.armor_remaining(Location::Trooper1, Facing::Front), 6, "T1 untouched");
        press(&mut app, KeyCode::Down);
        assert_eq!(app.cursor, Location::Trooper4, "row 2 of the squad grid");
        press(&mut app, KeyCode::Up); // column-aligned: back to T1
        assert_eq!(app.cursor, Location::Trooper1);
        press(&mut app, KeyCode::Right);
        press(&mut app, KeyCode::Right);
        assert_eq!(app.cursor, Location::Trooper3);
    }

    #[test]
    fn adding_infantry_to_a_session_snaps_the_cursor() {
        // Regression: adding a unit via the picker made it active but left the doll cursor on
        // the previous unit's location (e.g. CT), so infantry could not be damaged at all —
        // the Platoon even shares CT's grid cell, so arrows couldn't escape.
        let bundle = Bundle::new(vec![sample_mech(), sample_platoon()]);
        let mut session = Session::new();
        session.add_mech(sample_mech());
        let mut app = App::new(bundle, session, "test".to_string());
        assert_eq!(app.cursor, Location::CenterTorso);

        // Add the platoon via the picker (it becomes the active unit).
        press(&mut app, KeyCode::Char('a'));
        for c in "Foot".chars() {
            press(&mut app, KeyCode::Char(c));
        }
        press(&mut app, KeyCode::Enter); // open the pre-add skill/cost modal
        press(&mut app, KeyCode::Enter); // commit the add
        assert_eq!(app.session.active_mech().unwrap().spec.chassis, "Clan Heavy Foot Infantry");
        assert_eq!(app.cursor, Location::Platoon, "cursor snapped onto the platoon");

        // And damage lands.
        press(&mut app, KeyCode::Char(' '));
        assert_eq!(app.session.active_mech().unwrap().troopers_remaining(), 20);
    }

    #[test]
    fn e2e_ba_classic_doll() {
        let mut app = app_with_mech(sample_battle_armor());
        // Knock out trooper 2 so the doll shows a dead suit.
        app.session.active_mech_mut().unwrap().damage(Location::Trooper2, Facing::Front, 7);
        let screen = render(&mut app);
        assert!(screen.contains("T1") && screen.contains("T4"), "trooper boxes");
        assert!(screen.contains("SQUAD"), "squad panel");
        assert!(screen.contains("Anti-Mech"), "infantry skills panel");
        insta::assert_snapshot!(screen);
    }

    #[test]
    fn ba_fires_from_the_cursor_suit() {
        let mut app = app_with_mech(ba_squad_with_ammo());
        assert_eq!(app.cursor, Location::Trooper1);
        press(&mut app, KeyCode::Tab); // focus weapons
        press(&mut app, KeyCode::Char(' ')); // fire SRM from suit 1
        let tm = app.session.active_mech().unwrap();
        assert_eq!(tm.suit_ammo[&0], vec![1, 2, 2, 2], "only suit 1 spent");

        // Move the doll cursor to another suit and fire from it (a squad fires once per suit).
        press(&mut app, KeyCode::Tab); // back to doll
        press(&mut app, KeyCode::Right);
        let moved_to = app.cursor;
        assert_ne!(moved_to, Location::Trooper1, "cursor moved to another suit");
        press(&mut app, KeyCode::Tab); // weapons
        press(&mut app, KeyCode::Char(' ')); // fire from the new suit
        let tm = app.session.active_mech().unwrap();
        let suit = tm.suit_index_of(moved_to).unwrap();
        assert_eq!(tm.suit_ammo[&0][suit], 1, "the cursor suit spent a shot");
        assert_eq!(tm.suit_ammo[&0][0], 1, "suit 1 unchanged by the second shot");
    }

    #[test]
    fn e2e_ba_per_suit_ammo() {
        let mut app = app_with_mech(ba_squad_with_ammo());
        // Fire suit 1 once, kill suit 3 — leaving a spent suit, a dead suit, and full suits.
        press(&mut app, KeyCode::Tab);
        press(&mut app, KeyCode::Char(' ')); // suit 1: 1 shot
        press(&mut app, KeyCode::Tab);
        app.session.active_mech_mut().unwrap().damage(Location::Trooper3, Facing::Front, 7);
        let screen = render(&mut app);
        assert!(screen.contains("SRM 2"), "per-suit ammo row");
        insta::assert_snapshot!(screen);
    }

    #[test]
    fn e2e_ci_classic_platoon() {
        let mut app = app_with_mech(sample_platoon());
        // Take some losses so the strength bar shows wear.
        app.session.active_mech_mut().unwrap().damage(Location::Platoon, Facing::Front, 6);
        let screen = render(&mut app);
        assert!(screen.contains("PLT"), "platoon box");
        assert!(screen.contains("Strength"), "strength panel");
        insta::assert_snapshot!(screen);
    }

    #[test]
    fn e2e_infantry_weapon_range_table() {
        use super::app::Focus;
        let mut app = app_with_mech(sample_platoon());
        // Focus the EQUIP panel so the selected weapon's detail line (range span + the per-hex
        // to-hit modifiers from the Conventional Infantry Range Modifier Table) renders.
        app.focus = Focus::Equipment;
        let screen = render(&mut app);
        // Class-1 weapon (Auto-Rifle) reaches 0-3 with per-hex to-hit -2/0/+2/+4.
        assert!(screen.contains("0-3"), "hex span shown");
        assert!(screen.contains("-2/0/+2/+4"), "per-hex range modifier table");
        insta::assert_snapshot!(screen);
    }

    #[test]
    fn e2e_battle_armor_as_card() {
        let mut app = app_with_as_mech(sample_battle_armor());
        let screen = render(&mut app);
        assert!(screen.contains("Achileus"), "BA name on the AS card");
        assert!(screen.contains("BA"), "AS type code");
        insta::assert_snapshot!(screen);
    }

    #[test]
    fn e2e_infantry_preview() {
        let bundle = Bundle::new(vec![sample_battle_armor()]);
        let mut app = App::new(bundle, Session::new(), "test".to_string());
        press(&mut app, KeyCode::Tab); // preview popup
        let screen = render(&mut app);
        assert!(screen.contains("squad 4"), "squad size instead of tonnage");
        assert!(screen.contains("Battle Armor"), "type row");
        assert!(screen.contains("Troopers"), "trooper count row");
        insta::assert_snapshot!(screen);
    }

    #[test]
    fn e2e_vehicle_preview() {
        let bundle = Bundle::new(vec![sample_vehicle()]);
        let mut app = App::new(bundle, Session::new(), "test".to_string());
        press(&mut app, KeyCode::Tab); // preview popup
        let screen = render(&mut app);
        assert!(screen.contains("Cruise") && screen.contains("Combat vehicle"));
        insta::assert_snapshot!(screen);
    }

    #[test]
    fn e2e_prone_banner() {
        let mut app = app_with_one_mech();
        press(&mut app, KeyCode::Char('d')); // prone
        insta::assert_snapshot!(render(&mut app));
    }

    #[test]
    fn e2e_active_bin_marker() {
        use super::app::Modal;
        // Two AC/20 bins (LT + RT); mark the RT bin active from the crit popup -> "◀ active".
        let mut m = sample_mech();
        m.weapons.push(WeaponMount {
            id: 1,
            name: "AC/20".into(),
            location: Location::RightTorso,
            rear: false,
            heat: 7,
            damage: "20".into(),
            range: "3/6/9".into(),
            crit_slots: 10,
            ammo_key: Some("AC:20".into()),
            to_hit: 0,
            tc_eligible: false,
            count: 1,
        });
        let bin = |id, loc| AmmoBin {
            id,
            name: "AC/20 Ammo".into(),
            location: loc,
            shots_per_ton: 5,
            tons: 1,
            ammo_key: Some("AC:20".into()),
            munition: String::new(),
            base_ammo: None,
        };
        m.ammo = vec![bin(0, Location::LeftTorso), bin(1, Location::RightTorso)];
        m.crit_slots.insert(
            Location::RightTorso,
            vec![CritSlot { slot: 0, name: "AC/20 Ammo".into(), system: false, hittable: true, ..Default::default() }],
        );
        let mut app = app_with_mech(m);
        app.modal = Some(Modal::Crit { loc: Location::RightTorso, sel: 0 });
        press(&mut app, KeyCode::Char('a')); // mark the RT bin active
        let screen = render(&mut app);
        assert!(screen.contains("active"), "active-bin marker");
        insta::assert_snapshot!(screen);
    }

    #[test]
    fn e2e_log_indicator() {
        use neurohelmet_core::log;
        let mut app = app_with_one_mech();
        app.current_name = "neurohelmet-unit-snap-log".into();
        let _ = std::fs::remove_file(log::log_file(&app.current_name));
        press(&mut app, KeyCode::Char('L')); // log snapshot -> roster "· log 1"
        let screen = render(&mut app);
        let _ = std::fs::remove_file(log::log_file(&app.current_name));
        insta::assert_snapshot!(screen);
    }

    #[test]
    fn e2e_motive_damage_popup() {
        use super::app::Modal;
        use neurohelmet_core::session::MotiveLevel;
        let mut app = app_with_mech(sample_vehicle());
        press(&mut app, KeyCode::Char('m')); // open Motive System Damage popup
        assert!(matches!(app.modal, Some(Modal::Motive { .. })));
        press(&mut app, KeyCode::Down); // -> Moderate
        press(&mut app, KeyCode::Char(' ')); // apply −1 MP
        press(&mut app, KeyCode::Down); // -> Heavy
        press(&mut app, KeyCode::Char(' ')); // apply half MP
        let tm = app.session.active_mech().unwrap();
        assert_eq!(
            tm.motive_damage,
            vec![MotiveLevel::Moderate, MotiveLevel::Heavy]
        );
        insta::assert_snapshot!(render(&mut app));
    }

    #[test]
    fn e2e_vehicle_immobilized() {
        let mut app = app_with_mech(sample_vehicle());
        {
            let tm = app.session.active_mech_mut().unwrap();
            tm.add_motive(neurohelmet_core::session::MotiveLevel::Immobilized);
            tm.hit_crew();
        }
        let screen = render(&mut app);
        assert!(screen.contains("IMMOBILIZED"));
        insta::assert_snapshot!(screen);
    }

    #[test]
    fn e2e_help_modal() {
        let mut app = app_with_one_mech();
        press(&mut app, KeyCode::Char('?'));
        insta::assert_snapshot!(render(&mut app));
    }

    /// A weapon-boat with far more rows than the panel can show, to exercise scrolling.
    fn many_weapons_mech() -> Mech {
        let mut m = sample_mech();
        m.chassis = "Longbow".into();
        m.weapons = (0..30)
            .map(|i| WeaponMount {
                id: i,
                name: format!("LRM {}", 5 + i),
                location: Location::LeftTorso,
                rear: false,
                heat: 5,
                damage: "1/msl".into(),
                range: "7/14/21".into(),
                crit_slots: 5,
                ammo_key: None,
                to_hit: 0,
                tc_eligible: false,
                count: 1,
            })
            .collect();
        m.ammo = vec![];
        m
    }

    #[test]
    fn e2e_weapons_scrollbar() {
        let mut app = app_with_mech(many_weapons_mech());
        press(&mut app, KeyCode::Tab); // focus the weapons panel
        // Scroll to the bottom so the thumb is near the end.
        for _ in 0..29 {
            press(&mut app, KeyCode::Down);
        }
        let screen = render(&mut app);
        assert!(screen.contains('█'), "scrollbar thumb present");
        insta::assert_snapshot!(screen);
    }

    #[test]
    fn e2e_pilot_panel() {
        let mut app = app_with_one_mech();
        press(&mut app, KeyCode::Char('p'));
        press(&mut app, KeyCode::Char('p'));
        press(&mut app, KeyCode::Char('p')); // 3 hits -> conscious 7+
        let screen = render(&mut app);
        assert!(screen.contains("PILOT"), "pilot panel present");
        assert!(screen.contains("conscious 7+"), "consciousness number for 3 hits");
        insta::assert_snapshot!(screen);
    }

    #[test]
    fn e2e_pilot_unconscious() {
        let mut app = app_with_one_mech();
        press(&mut app, KeyCode::Char('p')); // 1 hit
        press(&mut app, KeyCode::Char('X')); // knock out
        let screen = render(&mut app);
        assert!(screen.contains("UNCONSCIOUS"), "KO state shown");
        assert!(screen.contains("wake 3+"), "wake number for 1 hit");
        insta::assert_snapshot!(screen);
    }

    #[test]
    fn pilot_consciousness_key_toggles() {
        let mut app = app_with_one_mech();
        let ko = |app: &App| app.session.active_mech().unwrap().pilot_unconscious;
        assert!(!ko(&app));
        press(&mut app, KeyCode::Char('X'));
        assert!(ko(&app));
        press(&mut app, KeyCode::Char('X'));
        assert!(!ko(&app));
    }

    #[test]
    fn o_i_adjust_heat() {
        let mut app = app_with_one_mech();
        press(&mut app, KeyCode::Char('o')); // +1
        press(&mut app, KeyCode::Char('o')); // +1
        assert_eq!(app.session.active_mech().unwrap().heat, 2);
        press(&mut app, KeyCode::Char('i')); // -1
        assert_eq!(app.session.active_mech().unwrap().heat, 1);
    }

    #[test]
    fn comma_period_switch_mechs() {
        let mut app = app_with_one_mech();
        app.session.add_mech(sample_mech()); // 2 mechs, newly-added is active (index 1)
        assert_eq!(app.session.active, 1);
        press(&mut app, KeyCode::Char(',')); // previous
        assert_eq!(app.session.active, 0);
        press(&mut app, KeyCode::Char('.')); // next
        assert_eq!(app.session.active, 1);
    }

    #[test]
    fn pilot_keys_track_and_kill() {
        let mut app = app_with_one_mech();
        let hits = |app: &App| app.session.active_mech().unwrap().pilot_hits;
        press(&mut app, KeyCode::Char('p'));
        press(&mut app, KeyCode::Char('p'));
        assert_eq!(hits(&app), 2);
        press(&mut app, KeyCode::Char('P')); // heal
        assert_eq!(hits(&app), 1);
        for _ in 0..6 {
            press(&mut app, KeyCode::Char('p')); // clamps at 6
        }
        assert_eq!(hits(&app), 6);
        assert_eq!(
            app.session.active_mech().unwrap().destroyed_reason(),
            Some("pilot dead")
        );
    }

    #[test]
    fn e2e_weapon_range_detail() {
        let mut app = app_with_one_mech();
        press(&mut app, KeyCode::Tab); // focus the weapons panel
        let screen = render(&mut app);
        // The selected weapon's range shows in the pinned detail line.
        assert!(screen.contains("range"), "range detail line present");
        assert!(screen.contains("3/6/9"), "Medium Laser range");
        insta::assert_snapshot!(screen);
    }

    #[test]
    fn e2e_out_of_ammo() {
        let mut app = app_with_mech(combat_mech());
        press(&mut app, KeyCode::Tab); // focus equipment
        press(&mut app, KeyCode::Down); // select the AC/20 (row 1)
        press(&mut app, KeyCode::Char(' ')); // fire (2 -> 1)
        press(&mut app, KeyCode::Char(' ')); // fire (1 -> 0)
        press(&mut app, KeyCode::Char(' ')); // fire -> OUT OF AMMO
        insta::assert_snapshot!(render(&mut app));
    }

    #[test]
    fn e2e_shutdown() {
        let mut app = app_with_mech(combat_mech());
        app.session.active_mech_mut().unwrap().adjust_heat(31); // forces shutdown
        insta::assert_snapshot!(render(&mut app));
    }

    #[test]
    fn e2e_rear_armor_damage() {
        let mut app = app_with_one_mech(); // cursor starts on CenterTorso (has rear)
        press(&mut app, KeyCode::Char('f')); // toggle to rear facing
        press(&mut app, KeyCode::Char(' '));
        press(&mut app, KeyCode::Char(' ')); // 2 hits to CT rear
        insta::assert_snapshot!(render(&mut app));
    }

    #[test]
    fn e2e_full_roster() {
        let mut app = app_with_one_mech();
        app.session.add_mech(sample_mech());
        app.session.add_mech(combat_mech());
        app.session.switch(-1); // make the middle one active
        insta::assert_snapshot!(render(&mut app));
    }

    #[test]
    fn roster_tabs_letter_duplicate_chassis() {
        // Same chassis more than once -> trailing A/B/… in roster order; a unique chassis stays bare.
        let mut app = app_with_one_mech(); // Atlas
        app.session.add_mech(sample_mech()); // Atlas (duplicate)
        app.session.add_mech(sample_vehicle()); // Manticore (unique)
        app.session.active = 0;
        let roster = render(&mut app).lines().next().unwrap().to_string();
        assert!(roster.contains("1:Atlas A"), "first duplicate -> A: {roster:?}");
        assert!(roster.contains("2:Atlas B"), "second duplicate -> B: {roster:?}");
        assert!(roster.contains("3:Manticore"), "unique chassis present: {roster:?}");
        assert!(!roster.contains("Manticore A"), "no letter for a lone chassis: {roster:?}");
    }

    #[test]
    fn roster_tabs_window_when_overflowing() {
        // Fill the roster past what fits on one 100-col line; the tab strip should window around
        // the active tab and show `‹n` / `n›` markers for the hidden tabs on each side.
        let mut app = app_with_one_mech();
        for _ in 1..neurohelmet_core::session::MAX_MECHS {
            app.session.add_mech(sample_mech());
        }
        app.session.active = neurohelmet_core::session::MAX_MECHS / 2; // a middle tab is active
        let screen = render(&mut app);
        let roster = screen.lines().next().unwrap();
        assert!(roster.contains('‹'), "hidden tabs to the left are marked: {roster:?}");
        assert!(roster.contains('›'), "hidden tabs to the right are marked: {roster:?}");
        assert!(
            roster.contains(&format!("{}:", neurohelmet_core::session::MAX_MECHS / 2 + 1)),
            "the active tab stays visible: {roster:?}"
        );
        insta::assert_snapshot!(screen);
    }

    #[test]
    fn e2e_rename_modal() {
        use super::app::{Modal, PendingAction, Screen};
        use neurohelmet_core::session::SessionMeta;
        let mut app = app_with_one_mech();
        app.current_name = "default".into();
        app.sessions = vec![SessionMeta {
            name: "Friday Game".into(),
            mech_count: 4,
            summary: "Atlas, Locust +2".into(),
            mode: GameMode::Classic, force_total: 0, limit: None,
        }];
        app.screen = Screen::Sessions;
        app.modal = Some(Modal::Input {
            prompt: "Rename 'Friday Game' to:".into(),
            buffer: "Friday Night".into(),
            action: PendingAction::RenameSession("Friday Game".into()),
        });
        insta::assert_snapshot!(render(&mut app));
    }

    #[test]
    fn e2e_delete_session_confirm() {
        use super::app::{Modal, PendingAction, Screen};
        use neurohelmet_core::session::SessionMeta;
        let mut app = app_with_one_mech();
        app.current_name = "default".into();
        app.sessions = vec![SessionMeta {
            name: "old game".into(),
            mech_count: 2,
            summary: "Warhammer, Marauder".into(),
            mode: GameMode::Classic, force_total: 0, limit: None,
        }];
        app.screen = Screen::Sessions;
        app.modal = Some(Modal::Confirm {
            prompt: "Delete session 'old game'? (y/n)".into(),
            action: PendingAction::DeleteSession("old game".into()),
        });
        insta::assert_snapshot!(render(&mut app));
    }

    // ----- Key-handler coverage: paths the e2e suite previously left dark. (Heat up/down `o`/`i`,
    // `e` end-turn, and `q` confirm-quit are covered by o_i_adjust_heat / heat_keys_and_end_turn /
    // q_confirms_before_quitting above.) These assert in-memory state only — handle_key never
    // persists; saving happens in the run() loop on `dirty`. -----

    #[test]
    fn log_snapshot_on_empty_roster_is_a_no_op() {
        use super::app::Screen;
        // An empty Classic session, parked on the tracker.
        let app_full = app_with_one_mech();
        let mut app = App::new(app_full.bundle.clone(), Session::new(), "test".to_string());
        app.screen = Screen::Tracker;
        let turn_before = app.session.turn;
        press(&mut app, KeyCode::Char('L'));
        assert_eq!(app.session.turn, turn_before, "no turn bump with nothing to log");
        assert!(app.status.contains("Nothing to log"), "status: {}", app.status);
    }

    #[test]
    fn sessions_screen_new_keys_open_input_for_each_mode() {
        use super::app::{Modal, PendingAction, Screen};
        let new_mode = |app: &App| match &app.modal {
            Some(Modal::Input { action: PendingAction::NewSession(mode), buffer, .. }) => {
                assert!(buffer.is_empty(), "new-session name starts empty");
                *mode
            }
            _ => panic!("expected a NewSession input modal"),
        };
        let mut app = app_with_one_mech();
        app.screen = Screen::Sessions;

        press(&mut app, KeyCode::Char('n'));
        assert_eq!(new_mode(&app), GameMode::Classic);
        press(&mut app, KeyCode::Esc); // dismiss before the next

        app.screen = Screen::Sessions;
        press(&mut app, KeyCode::Char('A'));
        assert_eq!(new_mode(&app), GameMode::AlphaStrike);
        press(&mut app, KeyCode::Esc);

        app.screen = Screen::Sessions;
        press(&mut app, KeyCode::Char('O'));
        assert_eq!(new_mode(&app), GameMode::Override);
    }

    #[test]
    fn override_heat_down_and_quit_guard() {
        use super::app::{Modal, PendingAction};
        let mut app = app_with_override(sample_mech());
        // Override has its own heat ladder (`ov_heat`); `i` is its (untested) decrement.
        press(&mut app, KeyCode::Char('o'));
        press(&mut app, KeyCode::Char('o'));
        let after_up = app.session.active_mech().unwrap().ov_heat;
        assert!(after_up > 0, "o raises Override heat");
        press(&mut app, KeyCode::Char('i'));
        assert!(
            app.session.active_mech().unwrap().ov_heat < after_up,
            "i lowers Override heat"
        );
        // Quit is guarded here too.
        press(&mut app, KeyCode::Char('q'));
        assert!(!app.should_quit);
        assert!(matches!(app.modal, Some(Modal::Confirm { action: PendingAction::Quit, .. })));
    }

    #[test]
    fn delete_mech_requires_confirm() {
        let mut app = app_with_one_mech();
        app.session.add_mech(sample_mech()); // 2 mechs, active = 1
        assert_eq!(app.session.mechs.len(), 2);

        press(&mut app, KeyCode::Char('D')); // opens confirm modal
        assert!(app.modal.is_some());
        assert_eq!(app.session.mechs.len(), 2, "not removed until confirmed");

        press(&mut app, KeyCode::Char('n')); // cancel
        assert!(app.modal.is_none());
        assert_eq!(app.session.mechs.len(), 2);

        press(&mut app, KeyCode::Char('D'));
        press(&mut app, KeyCode::Char('y')); // confirm
        assert!(app.modal.is_none());
        assert_eq!(app.session.mechs.len(), 1);
    }

    #[test]
    fn deleting_last_mech_returns_to_picker() {
        use super::app::Screen;
        let mut app = app_with_one_mech();
        press(&mut app, KeyCode::Char('D'));
        press(&mut app, KeyCode::Char('y'));
        assert!(app.session.mechs.is_empty());
        assert!(matches!(app.screen, Screen::Picker));
    }

    #[test]
    fn sessions_screen_renders() {
        let mut app = app_with_one_mech();
        press(&mut app, KeyCode::Char('S')); // open sessions browser
        let screen = render(&mut app);
        assert!(screen.contains("Sessions"));
        assert!(screen.contains("current"));
    }

    #[test]
    fn heat_keys_and_end_turn() {
        let mut app = app_with_one_mech();
        for _ in 0..25 {
            press(&mut app, KeyCode::Char('o'));
        }
        assert_eq!(app.session.active_mech().unwrap().heat, 25);
        let fx = render(&mut app);
        assert!(fx.contains("MP") || fx.contains("hit"), "heat effects shown");
        press(&mut app, KeyCode::Char('e')); // dissipate 20
        assert_eq!(app.session.active_mech().unwrap().heat, 5);
    }

    // ---- Strategic BattleForce screen (spec Phase 5) ----

    /// An app in an SBF-mode session: `n` Atlas elements in the pool, grouped under Inner
    /// Sphere doctrine (Lances of 4 → Companies) — the same result as `g` → `a` → Enter.
    fn app_with_sbf(n: usize) -> App {
        let m = sample_mech();
        let bundle = Bundle::new(vec![m.clone()]);
        let mut session = Session::new_with_mode(GameMode::StrategicBattleForce);
        for _ in 0..n {
            session.add_mech(m.clone());
        }
        if n > 0 {
            session.sbf_group_doctrine(neurohelmet_core::session::SbfDoctrine::InnerSphere);
        }
        let mut app = App::new(bundle, session, "test".to_string());
        app.dirty = false;
        app
    }

    #[test]
    fn e2e_sbf_initial() {
        use super::app::Screen;
        // 8 elements auto-group into one formation of two units (6 + 2); the three panes render.
        let mut app = app_with_sbf(8);
        assert!(matches!(app.screen, Screen::Sbf), "SBF mode lands on the SBF screen");
        assert_eq!(app.session.sbf.formations.len(), 1);
        assert_eq!(app.session.sbf.formations[0].units.len(), 2);
        let screen = render(&mut app);
        assert!(screen.contains("FORMATIONS"), "formation pane title");
        assert!(screen.contains("Round 0"), "round counter shown");
        assert!(screen.contains("To-Hit"), "live to-hit line in the detail pane");
        insta::assert_snapshot!(screen);
    }

    #[test]
    fn e2e_sbf_tracking() {
        // Damage, spillover message, crit counters, morale rung, round/turn — the live verbs.
        let mut app = app_with_sbf(8);
        let derived = app.session.sbf_unit(&app.session.sbf.formations[0].units[0]);
        // Space marks one point of damage on the active unit.
        press(&mut app, KeyCode::Char(' '));
        let u = &app.session.sbf.formations[0].units[0];
        assert_eq!(u.armor_remaining(&derived), derived.armor - 1, "Space damages the unit");
        // u repairs it.
        press(&mut app, KeyCode::Char('u'));
        assert_eq!(
            app.session.sbf.formations[0].units[0].armor_remaining(&derived),
            derived.armor,
            "u repairs"
        );
        // Crit popup: mark one damage crit and one targeting crit.
        press(&mut app, KeyCode::Char('c'));
        press(&mut app, KeyCode::Char(' ')); // +1 damage crit (row 0)
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Char(' ')); // +1 targeting crit (row 1)
        press(&mut app, KeyCode::Esc);
        let u = &app.session.sbf.formations[0].units[0];
        assert_eq!((u.damage_crits, u.targeting_crits), (1, 1), "crit popup marks counters");
        // Morale is a manual rung: m worsens one step.
        press(&mut app, KeyCode::Char('m'));
        assert_eq!(app.session.sbf.formations[0].morale, MoraleStatus::Shaken);
        // e marks the formation done; n begins a new round and re-arms it.
        press(&mut app, KeyCode::Char('e'));
        assert!(app.session.sbf.formations[0].is_done);
        press(&mut app, KeyCode::Char('n'));
        assert_eq!(app.session.sbf.round, 1);
        assert!(!app.session.sbf.formations[0].is_done);
        assert_eq!(app.session.sbf.formations[0].morale, MoraleStatus::Shaken, "morale persists");
        insta::assert_snapshot!(render(&mut app));
    }

    #[test]
    fn e2e_sbf_spillover_status() {
        // Damaging past a unit's armor reports the spillover (§4.2 — never discarded).
        let mut app = app_with_sbf(8);
        let derived = app.session.sbf_unit(&app.session.sbf.formations[0].units[0]);
        for _ in 0..derived.armor {
            press(&mut app, KeyCode::Char(' '));
        }
        let u = &app.session.sbf.formations[0].units[0];
        assert!(u.is_destroyed(&derived));
        press(&mut app, KeyCode::Char(' ')); // one more point — pure overflow
        assert!(app.status.contains("spill"), "overflow points the player at spillover");
        // The unit list shows the destroyed flag.
        assert!(render(&mut app).contains("DESTROYED"));
    }

    #[test]
    fn e2e_sbf_to_hit_modal() {
        // The t editor drives the printed p.172 table and previews the number live.
        let mut app = app_with_sbf(4);
        press(&mut app, KeyCode::Char('t'));
        // Row 0: range Medium (+1) → Long (+2): skill 4 + 2 = 6.
        press(&mut app, KeyCode::Char(' '));
        // Row 6: target TMM +2 → 8.
        for _ in 0..6 {
            press(&mut app, KeyCode::Down);
        }
        press(&mut app, KeyCode::Char(' '));
        press(&mut app, KeyCode::Char(' '));
        let screen = render(&mut app);
        assert!(screen.contains("To-Hit   8+"), "4 skill + 2 long + 2 TMM:\n{screen}");
        insta::assert_snapshot!(screen);
        press(&mut app, KeyCode::Esc);
        // The detail pane keeps showing the entered shot.
        assert!(render(&mut app).contains("8+"));

        // Extreme is a legal attack under the printed ladder (+3), never Impossible.
        press(&mut app, KeyCode::Char('t'));
        press(&mut app, KeyCode::Char(' ')); // Long → Extreme
        assert!(render(&mut app).contains("To-Hit   9+"), "extreme is +3, still a number");
        press(&mut app, KeyCode::Esc);
    }

    // ---- Strategic Aerospace (SAS layer, IO:BF pp.177–181) ----

    /// An aerospace fighter (AS type `AF`, Thrust 10, BOMB2) for the SAS-layer tests.
    fn sample_aero_mech() -> Mech {
        let mut m = sample_mech();
        m.chassis = "Visigoth".into();
        m.model = "C".into();
        m.as_stats = AsStats {
            pv: 30,
            size: 2,
            tp: "AF".into(),
            movement: "10a".into(),
            armor: 6,
            structure: 2,
            dmg_s: "3".into(),
            dmg_m: "3".into(),
            dmg_l: "2".into(),
            dmg_e: "1".into(),
            specials: vec!["BOMB2".into()],
            ..Default::default()
        };
        m
    }

    /// An app in an SBF session over `n` aerospace fighters, doctrine-grouped into Flights of 2
    /// under a Squadron (the aero arm of `g` → `a` → Enter).
    fn app_with_sbf_aero(n: usize) -> App {
        let m = sample_aero_mech();
        let bundle = Bundle::new(vec![m.clone()]);
        let mut session = Session::new_with_mode(GameMode::StrategicBattleForce);
        for _ in 0..n {
            session.add_mech(m.clone());
        }
        session.sbf_group_doctrine(neurohelmet_core::session::SbfDoctrine::InnerSphere);
        let mut app = App::new(bundle, session, "test".to_string());
        app.dirty = false;
        app
    }

    #[test]
    fn e2e_sbf_aero_shot_modal() {
        // The t editor's SAS rows (the p.179 table): an air-to-air kind gates the +2 airborne
        // row off for an aero attacker and suppresses the target-movement legs.
        let mut app = app_with_sbf_aero(4);
        assert_eq!(app.session.sbf.formations[0].name, "Squadron 1");
        press(&mut app, KeyCode::Char('t'));
        for _ in 0..6 {
            press(&mut app, KeyCode::Down); // row 6: target TMM
        }
        press(&mut app, KeyCode::Char(' '));
        press(&mut app, KeyCode::Char(' '));
        assert!(render(&mut app).contains("To-Hit   7+"), "ground shot: 4 skill + 1 M + 2 TMM");
        for _ in 0..4 {
            press(&mut app, KeyCode::Down); // row 10: aero attack kind
        }
        press(&mut app, KeyCode::Char(' ')); // off → air-to-air
        let screen = render(&mut app);
        assert!(
            screen.contains("To-Hit   5+"),
            "air-to-air: no +2 vs an aero attacker (p.179 fn), TMM suppressed:\n{screen}"
        );
        assert!(screen.contains("no TMM vs airborne"), "suppression note renders");
        assert!(screen.contains("+2 atmosphere"), "engagement-modifier reference line");
        insta::assert_snapshot!(screen);
        // Cycle on to strafing: the +4 attack row, and the target legs come back (Open Q 25).
        for _ in 0..4 {
            press(&mut app, KeyCode::Char(' '));
        }
        let screen = render(&mut app);
        assert!(screen.contains("To-Hit   11+"), "strafing: 4 + 1 M + 4 + 2 TMM:\n{screen}");
        assert!(screen.contains("strafe ⌈S/4⌉"), "p.180 damage reference:\n{screen}");
        press(&mut app, KeyCode::Esc);
        // The detail pane summary names the kind — "vs TMM" would misread under suppression.
        assert!(render(&mut app).contains("(Medium strafing)"));
    }

    #[test]
    fn e2e_sbf_ground_to_air_gate() {
        // A GROUND formation firing ground-to-air takes the +2 airborne target row (the p.179
        // fn gate reads the attacker's type) and drops its hand-entered target-movement legs.
        let mut app = app_with_sbf(4);
        app.sbf_shot.target_tmm = 3;
        app.sbf_shot.aero_kind = super::app::SbfAeroUiKind::GroundToAir;
        let ctx = app.sbf_to_hit_ctx().expect("active unit");
        assert!(!ctx.aero.expect("aero leg").attacker_airborne_aero);
        // Skill 4 + Medium 1 + airborne +2; the TMM 3 is suppressed (p.181).
        assert_eq!(neurohelmet_core::engine::sbf::sbf_to_hit(&ctx), 7);
    }

    #[test]
    fn e2e_sbf_aero_crash_badge() {
        // Thrust loss (p.178/p.181): an aero unit at 0 movement has crashed — an End Phase
        // removal, not the ground immobile treatment — and destruction outranks the badge.
        let mut app = app_with_sbf_aero(4);
        let screen = render(&mut app);
        // Both TMM sites (formation stat line, unit detail header) hide the converter's −4 on
        // aero (Open Q 1, MOOT); the hand-entered target-TMM summary is a different thing.
        assert!(screen.contains("SZ2 MV10a"), "formation line without TMM:\n{screen}");
        assert!(!screen.contains("TMM-4"), "no formation TMM on aero:\n{screen}");
        assert!(!screen.contains("TMM -4"), "no unit TMM on aero:\n{screen}");
        assert!(screen.contains("bombs: −1 Thrust each (min 1)"), "BOMB card note:\n{screen}");
        // Mark MP crits down to 0 Thrust (AF Thrust 10 → Flight movement 10).
        press(&mut app, KeyCode::Char('c'));
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Down); // MP crits row
        for _ in 0..10 {
            press(&mut app, KeyCode::Char(' '));
        }
        assert!(
            render(&mut app).contains("0 = crashes, End Phase"),
            "aero wording on the crit popup's MP row"
        );
        press(&mut app, KeyCode::Esc);
        let derived = app.session.sbf_unit(&app.session.sbf.formations[0].units[0]);
        assert_eq!(app.session.sbf.formations[0].units[0].current_movement(&derived), 0);
        let screen = render(&mut app);
        // (The 100-col test grid clips the badge's tail in the middle pane, like any long name.)
        assert!(screen.contains("CRASHES (End"), "crash badge:\n{screen}");
        insta::assert_snapshot!(screen);
        // Destruction takes precedence over the crash badge.
        for _ in 0..derived.armor {
            press(&mut app, KeyCode::Char(' '));
        }
        let screen = render(&mut app);
        assert!(screen.contains("DESTROYED"));
        assert!(!screen.contains("CRASHES"), "destroyed replaces the crash badge:\n{screen}");
    }

    #[test]
    fn e2e_sbf_commander_and_leader() {
        let mut app = app_with_sbf(8); // Company: Lance 1 + Lance 2
        press(&mut app, KeyCode::Char('C'));
        assert!(app.session.sbf.formations[0].units[0].is_commander);
        press(&mut app, KeyCode::Char('j')); // select Lance 2
        press(&mut app, KeyCode::Char('l'));
        assert!(app.session.sbf.formations[0].units[1].is_leader);
        let screen = render(&mut app);
        assert!(screen.contains("COM"), "commander tag renders");
        assert!(screen.contains("LEAD"), "leader tag renders");
        assert!(screen.contains("+2 Tactics"), "Step-5b defender hint renders");
        insta::assert_snapshot!(screen);
        // Toggling off clears the mark.
        press(&mut app, KeyCode::Char('l'));
        assert!(!app.session.sbf.formations[0].units[1].is_leader);
    }

    #[test]
    fn e2e_sbf_editor_skill_and_remove() {
        let mut app = app_with_sbf(8);
        press(&mut app, KeyCode::Char('g')); // editor opens on element 7 (last added)
        press(&mut app, KeyCode::Char('s'));
        assert_eq!(app.session.mechs[7].gunnery, 5, "s worsens the element's Skill");
        press(&mut app, KeyCode::Char('S'));
        assert_eq!(app.session.mechs[7].gunnery, 4, "S improves it back");
        press(&mut app, KeyCode::Char('x'));
        assert_eq!(app.session.mechs.len(), 7, "x removes the element from the force");
        assert_eq!(
            app.session.sbf.formations[0].units[1].elements,
            vec![4, 5, 6],
            "indices remap"
        );
        press(&mut app, KeyCode::Esc);
        assert!(app.modal.is_none());
    }

    #[test]
    fn e2e_sbf_bfc_and_drone_derive_from_composition() {
        // BFC aggregates at unit level (half the elements); DRO derives from the elements
        // directly (the converter never aggregates it — Open Q 6). Both add +1 to the number.
        let mut m = sample_mech();
        m.as_stats.specials = vec!["BFC".into(), "DRO".into()];
        let bundle = Bundle::new(vec![m.clone()]);
        let mut session = Session::new_with_mode(GameMode::StrategicBattleForce);
        for _ in 0..4 {
            session.add_mech(m.clone());
        }
        session.sbf_group_doctrine(neurohelmet_core::session::SbfDoctrine::InnerSphere);
        let mut app = App::new(bundle, session, "test".to_string());
        let ctx = app.sbf_to_hit_ctx().expect("active unit");
        assert!(ctx.bfc, "unit-level BFC reaches the to-hit ctx");
        assert!(ctx.drone, "all-DRO composition marks the unit as a drone");
        // Base skill: 4 avg + 1 (Step 1G BFC/DRO conversion) = 5; +1 Medium +1 BFC +1 DRO = 8.
        assert!(render(&mut app).contains("8+"), "conversion skill and attack rows both apply");
    }

    #[test]
    fn e2e_sbf_doctrine_confirms_itemized_losses() {
        use super::app::Modal;
        // Pristine grouping → no confirmation (the group-first flow stays frictionless):
        // covered by e2e_sbf_doctrine_auto_group. Hand-entered state → an itemized bill.
        let mut app = app_with_sbf(8);
        press(&mut app, KeyCode::Char(' ')); // 1 armor hit
        press(&mut app, KeyCode::Char('C')); // COM
        press(&mut app, KeyCode::Char('m')); // morale rung
        press(&mut app, KeyCode::Char('R')); // rename the unit
        for _ in 0.."Lance 1".len() {
            press(&mut app, KeyCode::Backspace);
        }
        for c in "Command Lance".chars() {
            press(&mut app, KeyCode::Char(c));
        }
        press(&mut app, KeyCode::Enter);

        press(&mut app, KeyCode::Char('g'));
        press(&mut app, KeyCode::Char('a'));
        press(&mut app, KeyCode::Enter); // Inner Sphere — but there is state at stake
        let Some(Modal::Confirm { .. }) = app.modal else {
            panic!("expected an itemized confirmation, got {:?}", render(&mut app));
        };
        let screen = render(&mut app);
        for needle in
            ["1 custom name(s)", "1 armor hit(s)", "1 morale rung(s)", "the COM mark", "z undoes"]
        {
            assert!(screen.contains(needle), "prompt itemizes {needle:?}:\n{screen}");
        }
        // n cancels: nothing changed.
        press(&mut app, KeyCode::Char('n'));
        assert_eq!(app.session.sbf.formations[0].units[0].name, "Command Lance");
        assert!(app.session.sbf.formations[0].units[0].is_commander);

        // y applies: rebuilt with doctrine names, one undo step restores everything.
        press(&mut app, KeyCode::Char('g'));
        press(&mut app, KeyCode::Char('a'));
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Char('y'));
        assert_eq!(app.session.sbf.formations[0].units[0].name, "Lance 1");
        assert!(!app.session.sbf.formations[0].units[0].is_commander);
        press(&mut app, KeyCode::Char('z'));
        assert_eq!(app.session.sbf.formations[0].units[0].name, "Command Lance");
        assert!(app.session.sbf.formations[0].units[0].is_commander, "one z restores the lot");
    }

    #[test]
    fn e2e_sbf_rename_unit() {
        let mut app = app_with_sbf(4); // unit "Lance 1"
        press(&mut app, KeyCode::Char('R'));
        for _ in 0.."Lance 1".len() {
            press(&mut app, KeyCode::Backspace);
        }
        for c in "Command Lance".chars() {
            press(&mut app, KeyCode::Char(c));
        }
        press(&mut app, KeyCode::Enter);
        assert_eq!(app.session.sbf.formations[0].units[0].name, "Command Lance");
        assert!(render(&mut app).contains("Command Lance"));
    }

    #[test]
    fn e2e_sbf_help_modal() {
        let mut app = app_with_sbf(4);
        press(&mut app, KeyCode::Char('?'));
        let screen = render(&mut app);
        assert!(screen.contains("Strategic BattleForce"));
        assert!(screen.contains("morale rung"));
        insta::assert_snapshot!(screen);
    }

    #[test]
    fn e2e_add_unit_modal_sbf() {
        use super::app::Screen;
        // The add-unit modal in SBF mode shows the AS-style single Skill row and PV cost.
        let mut app = app_with_sbf(1);
        press(&mut app, KeyCode::Char('a')); // picker
        assert!(matches!(app.screen, Screen::Picker));
        press(&mut app, KeyCode::Enter); // open AddUnit for the highlighted unit
        let screen = render(&mut app);
        assert!(screen.contains("PV"), "SBF costs in PV");
        insta::assert_snapshot!(screen);
    }

    #[test]
    fn e2e_sbf_delete_formation() {
        use super::app::Screen;
        // D removes the active formation and its pool elements; last one returns to the picker.
        let mut app = app_with_sbf(4);
        press(&mut app, KeyCode::Char('D'));
        press(&mut app, KeyCode::Char('y'));
        assert!(app.session.sbf.formations.is_empty());
        assert!(app.session.mechs.is_empty(), "elements removed with the formation");
        assert!(matches!(app.screen, Screen::Picker));
    }

    #[test]
    fn e2e_sbf_group_editor() {
        use neurohelmet_core::session::SbfAssign;
        // Manual grouping is the primary flow: g opens the editor on the sidebar-highlighted
        // element; f moves it into a brand-new formation; Esc prunes and closes.
        let mut app = app_with_sbf(8); // Company 1: Lance 4 + Lance 4
        assert_eq!(app.session.active, 7, "last added element is highlighted");
        press(&mut app, KeyCode::Char('g'));
        let screen = render(&mut app);
        assert!(screen.contains("Group force"));
        assert!(screen.contains("new formation"));
        insta::assert_snapshot!(screen);

        press(&mut app, KeyCode::Char('f')); // element 7 → new formation
        assert_eq!(app.session.sbf_element_assignment(7), Some((1, 0)));
        // ←→ cycles the grouping stops: → wraps it back into the first Lance. The vacated
        // formation STAYS (first-class empty workspace — the play-test regret scenario).
        press(&mut app, KeyCode::Right);
        assert_eq!(app.session.sbf_element_assignment(7), Some((0, 0)));
        assert_eq!(app.session.sbf.formations.len(), 2, "vacated formation remains a target");
        // …so a DIFFERENT decision can still reach it: ← from (0,0) wraps backwards onto the
        // empty formation's virtual "new unit" stop.
        press(&mut app, KeyCode::Left);
        assert_eq!(app.session.sbf_element_assignment(7), Some((1, 0)), "re-enters the empty formation");
        press(&mut app, KeyCode::Right); // back out again for the split test
        assert_eq!(app.session.sbf_element_assignment(7), Some((0, 0)));
        // n splits it into a new unit of its formation.
        press(&mut app, KeyCode::Char('n'));
        assert_eq!(app.session.sbf_element_assignment(7), Some((0, 2)));
        press(&mut app, KeyCode::Esc);
        assert!(app.modal.is_none());
        assert_eq!(app.session.sbf.formations.len(), 2, "empty formation survives close");
        assert!(app.session.sbf.formations[1].units.is_empty(), "its empty unit was pruned");
        assert_eq!(app.session.sbf.formations[0].units.len(), 3);
        // The empty formation renders as a workspace, never as destroyed/eliminated.
        let screen = render(&mut app);
        assert!(screen.contains("(no units"), "placeholder rendering:\n{screen}");
        assert!(!screen.contains("eliminated"));

        // Undo unwinds the manual moves one step at a time.
        press(&mut app, KeyCode::Char('z'));
        let _ = app.session.sbf_element_assignment(7); // whatever the previous step was — no panic
        // (fine-grained undo per assignment is asserted via the session comparisons above)

        // Manual assignment API guard: out-of-range target is a no-op.
        app.session.sbf_assign_element(7, SbfAssign::Unit(9, 9));
        assert_eq!(app.session.sbf_element_assignment(7), None);
    }

    #[test]
    fn e2e_sbf_doctrine_auto_group() {
        // Auto-group is the opt-in: g → a → pick Clan → Enter rebuilds as Stars of 5 (a Binary).
        let mut app = app_with_sbf(8);
        press(&mut app, KeyCode::Char('g'));
        press(&mut app, KeyCode::Char('a'));
        let screen = render(&mut app);
        assert!(screen.contains("Inner Sphere"));
        assert!(screen.contains("Stars of 5"));
        insta::assert_snapshot!(screen);
        press(&mut app, KeyCode::Down); // Clan
        press(&mut app, KeyCode::Enter);
        assert!(app.modal.is_none());
        assert_eq!(app.session.sbf.formations[0].name, "Binary 1");
        let sizes: Vec<usize> =
            app.session.sbf.formations[0].units.iter().map(|u| u.elements.len()).collect();
        assert_eq!(sizes, vec![5, 3]);
        assert_eq!(app.session.sbf.formations[0].units[0].name, "Star 1");
        // One undo restores the pre-doctrine grouping.
        press(&mut app, KeyCode::Char('z'));
        assert_eq!(app.session.sbf.formations[0].name, "Company 1");
    }

    #[test]
    fn e2e_sbf_rename_formation() {
        let mut app = app_with_sbf(4); // "Lance 1"
        press(&mut app, KeyCode::Char('r'));
        for _ in 0.."Lance 1".len() {
            press(&mut app, KeyCode::Backspace);
        }
        for c in "Alpha Talon".chars() {
            press(&mut app, KeyCode::Char(c));
        }
        press(&mut app, KeyCode::Enter);
        assert_eq!(app.session.sbf.formations[0].name, "Alpha Talon");
        assert!(render(&mut app).contains("Alpha Talon"));
    }

    #[test]
    fn e2e_sbf_log_export() {
        // An SBF snapshot exports its formation sheet (one frame per formation), not AS cards.
        use neurohelmet_core::log;
        isolate_data_dir(); // append_log writes under the data dir
        let name = "neurohelmet-unit-sbf-export-test";
        let _ = std::fs::remove_file(log::log_file(name));

        // Play a little first so the log captures live state: damage + COM + morale.
        let mut app = app_with_sbf(8); // Company 1: Lance 4+4
        app.current_name = name.to_string();
        press(&mut app, KeyCode::Char(' ')); // 1 damage
        press(&mut app, KeyCode::Char('C')); // COM
        press(&mut app, KeyCode::Char('m')); // Shaken
        press(&mut app, KeyCode::Char('L')); // snapshot
        assert!(app.status.contains("Logged"), "snapshot recorded: {}", app.status);

        let out = tempfile::tempdir().unwrap();
        let dir = crate::export::run(name, Some(out.path().to_path_buf())).unwrap();
        let transcript = std::fs::read_to_string(dir.join("transcript.txt")).unwrap();
        // The formation sheet's signatures — panes, doctrine names, live marks — none of which
        // the AS-card fallback renders.
        assert!(transcript.contains("== Turn 1 — Company 1 =="), "formation heading:\n{transcript}");
        assert!(transcript.contains("FORMATIONS"), "formation pane exported");
        assert!(transcript.contains("Lance 1"), "unit rows exported");
        assert!(transcript.contains("COM"), "COM mark exported");
        assert!(transcript.contains("Shaken"), "morale rung exported");
        assert!(transcript.contains("24/25"), "armor mark exported");
        // One frame per formation, named after it.
        assert!(dir.join("turn-01").join("01-Company 1.ppm").exists());
        let _ = std::fs::remove_file(log::log_file(name));
    }

    #[test]
    fn b_key_creates_sbf_session() {
        isolate_data_dir(); // execute_input persists to disk; keep it out of the real data dir
        let mut app = app_with_one_mech();
        press(&mut app, KeyCode::Char('S')); // open sessions browser
        press(&mut app, KeyCode::Char('B')); // new Strategic BattleForce session
        for c in "sbfgame".chars() {
            press(&mut app, KeyCode::Char(c));
        }
        press(&mut app, KeyCode::Enter);
        assert_eq!(app.session.mode, GameMode::StrategicBattleForce);
        assert!(app.session.mechs.is_empty(), "fresh session");
        // The sheet starts with one empty formation, like a blank Formation Record Sheet.
        assert_eq!(app.session.sbf.formations.len(), 1);
        assert_eq!(app.session.sbf.formations[0].name, "Formation 1");
    }

    #[test]
    fn e2e_sbf_fresh_sheet_renders_empty_formation() {
        // One element in the pool, nothing grouped: the seeded formation shows as a workspace
        // with placeholders — not the bare "No formations" screen, and never as destroyed.
        let m = sample_mech();
        let bundle = Bundle::new(vec![m.clone()]);
        let mut session = Session::new_with_mode(GameMode::StrategicBattleForce);
        session.add_mech(m);
        let mut app = App::new(bundle, session, "test".to_string());
        app.dirty = false;
        let screen = render(&mut app);
        assert!(screen.contains("Formation 1"));
        assert!(screen.contains("(no units"), "formation placeholder:\n{screen}");
        assert!(screen.contains("+1 ungrouped"), "pool hint");
        assert!(!screen.contains("DESTROYED"));
        insta::assert_snapshot!(screen);
        // The empty formation is reachable as a grouping stop: g → → assigns into it.
        press(&mut app, KeyCode::Char('g'));
        press(&mut app, KeyCode::Right);
        assert_eq!(app.session.sbf_element_assignment(0), Some((0, 0)));
    }

    // ---- Abstract Combat System screen (acs spec Phase 4) ----

    /// An app in an ACS session with `n` sample elements grouped into one Formation via `g`.
    fn app_with_acs(n: usize) -> App {
        let m = sample_mech();
        let bundle = Bundle::new(vec![m.clone()]);
        let mut session = Session::new_with_mode(GameMode::AbstractCombatSystem);
        session.acs.formations.clear();
        for _ in 0..n {
            session.add_mech(m.clone());
        }
        if n > 0 {
            session.acs_new_formation("Regiment", 0..n);
        }
        let mut app = App::new(bundle, session, "test".to_string());
        app.dirty = false;
        app
    }

    #[test]
    fn e2e_acs_initial_renders_three_panes() {
        use super::app::Screen;
        let mut app = app_with_acs(6);
        assert!(matches!(app.screen, Screen::Acs), "ACS mode lands on the ACS screen");
        assert_eq!(app.session.acs.formations.len(), 1);
        let screen = render(&mut app);
        assert!(screen.contains("FORMATIONS"), "formation pane title");
        assert!(screen.contains("Round 0"), "round counter");
        assert!(screen.contains("To-Hit"), "detail-pane to-hit readout");
        assert!(screen.contains("Morale"), "detail-pane morale readout");
        assert!(screen.contains("Combat Teams"), "derivation fold");
        insta::assert_snapshot!(screen);
    }

    #[test]
    fn e2e_acs_damage_and_thresholds() {
        let mut app = app_with_acs(6);
        let derived = app.session.acs_combat_unit(&app.session.acs.formations[0].units[0]);
        assert!(derived.armor > 0);
        // Space opens a damage input; type the announced amount past the first threshold.
        let dmg = derived.armor - derived.damage_thresholds[0] + 1;
        press(&mut app, KeyCode::Char(' '));
        for c in dmg.to_string().chars() {
            press(&mut app, KeyCode::Char(c));
        }
        press(&mut app, KeyCode::Enter);
        let st = &app.session.acs.formations[0].units[0];
        assert_eq!(st.armor_remaining(&derived), derived.armor - dmg);
        assert!(app.status.contains("threshold"), "threshold-crossing prompts a morale check");
        // u repairs one point.
        press(&mut app, KeyCode::Char('u'));
        assert_eq!(
            app.session.acs.formations[0].units[0].armor_remaining(&derived),
            derived.armor - dmg + 1
        );
    }

    #[test]
    fn e2e_acs_morale_fatigue_round() {
        use neurohelmet_core::engine::acs::AcsMorale;
        let mut app = app_with_acs(6);
        // m cycles the Combat Unit's morale rung; M cycles the Formation's.
        press(&mut app, KeyCode::Char('m'));
        assert_eq!(app.session.acs.formations[0].units[0].morale, AcsMorale::Shaken);
        press(&mut app, KeyCode::Char('M'));
        assert_eq!(app.session.acs.formations[0].morale, AcsMorale::Shaken);
        // f accrues fatigue (skill-4 Regular → 0.5 FP); F rests it back.
        press(&mut app, KeyCode::Char('f'));
        assert_eq!(app.session.acs.formations[0].units[0].fatigue_points(), 0.5);
        press(&mut app, KeyCode::Char('F'));
        assert_eq!(app.session.acs.formations[0].units[0].fatigue_points(), 0.0);
        // e marks done; n begins a new round and re-arms it; morale persists.
        press(&mut app, KeyCode::Char('e'));
        assert!(app.session.acs.formations[0].is_done);
        press(&mut app, KeyCode::Char('n'));
        assert_eq!(app.session.acs.round, 1);
        assert!(!app.session.acs.formations[0].is_done);
        assert_eq!(app.session.acs.formations[0].units[0].morale, AcsMorale::Shaken);
    }

    #[test]
    fn e2e_acs_readout_cycles_range_and_tmm() {
        let mut app = app_with_acs(6);
        let base = app.acs_to_hit_ctx().map(|c| neurohelmet_core::engine::acs::acs_to_hit(&c)).unwrap();
        // ] steps range up (Medium→Long, +2 more to-hit); + steps target TMM up.
        press(&mut app, KeyCode::Char(']'));
        press(&mut app, KeyCode::Char('+'));
        let after = app.acs_to_hit_ctx().map(|c| neurohelmet_core::engine::acs::acs_to_hit(&c)).unwrap();
        assert_eq!(after, base + 2 + 1, "Long (+2 over Medium) and TMM +1");
    }

    #[test]
    fn e2e_acs_commander_leader_unique() {
        let mut app = app_with_acs(6);
        press(&mut app, KeyCode::Char('C'));
        assert!(app.session.acs.formations[0].units[0].is_commander);
        press(&mut app, KeyCode::Char('l'));
        assert!(app.session.acs.formations[0].units[0].is_leader);
        let screen = render(&mut app);
        assert!(screen.contains("COM"));
    }

    #[test]
    fn e2e_acs_group_editor() {
        // A fresh ACS session (one seeded empty Formation) with an ungrouped pool: g opens the
        // editor; a auto-groups; F splits an element into its own Formation.
        let m = sample_mech();
        let bundle = Bundle::new(vec![m.clone()]);
        let mut session = Session::new_with_mode(GameMode::AbstractCombatSystem);
        for _ in 0..3 {
            session.add_mech(m.clone());
        }
        let mut app = App::new(bundle, session, "test".to_string());
        app.dirty = false;
        // g opens the grouping editor.
        press(&mut app, KeyCode::Char('g'));
        assert!(matches!(app.modal, Some(super::app::Modal::AcsGroup { .. })));
        let screen = render(&mut app);
        assert!(screen.contains("unassigned"), "pool shows as unassigned:\n{screen}");
        // a auto-groups the whole pool into one Formation.
        press(&mut app, KeyCode::Char('a'));
        assert_eq!(app.session.acs_element_assignment(0), Some((0, 0, 0, 0)));
        // Move the cursor to element 2 and split it into its own new Formation.
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Char('F'));
        let (fi, _, _, _) = app.session.acs_element_assignment(2).unwrap();
        assert!(fi > 0, "element 2 moved to a new Formation");
        // Esc closes and prunes; the pool is fully grouped.
        press(&mut app, KeyCode::Esc);
        assert!(app.modal.is_none());
        assert!((0..3).all(|e| app.session.acs_element_assignment(e).is_some()));
    }

    #[test]
    fn c_key_creates_acs_session() {
        isolate_data_dir();
        let mut app = app_with_one_mech();
        press(&mut app, KeyCode::Char('S')); // sessions browser
        press(&mut app, KeyCode::Char('C')); // new Abstract Combat System session
        for c in "acsgame".chars() {
            press(&mut app, KeyCode::Char(c));
        }
        press(&mut app, KeyCode::Enter);
        assert_eq!(app.session.mode, GameMode::AbstractCombatSystem);
        assert!(app.session.mechs.is_empty(), "fresh session");
        assert_eq!(app.session.acs.formations.len(), 1);
        assert_eq!(app.session.acs.formations[0].name, "Formation 1");
    }

    // ---- Standard BattleForce screen (standard-bf spec Phase 3) ----

    /// An app in a Standard BF session: `n` Atlas elements grouped under Inner Sphere doctrine
    /// (Lances of 4) — the same result as `g` → `a` → Enter. The sample Atlas fields
    /// MV 6″ → 3 hexes, TMM 1, 5/5/2 damage (ground E derives as 1), Skill 4, and carries IF1.
    fn app_with_bf(n: usize) -> App {
        app_with_bf_mech(sample_mech(), n)
    }

    fn app_with_bf_mech(m: Mech, n: usize) -> App {
        let bundle = Bundle::new(vec![m.clone()]);
        let mut session = Session::new_with_mode(GameMode::BattleForce);
        for _ in 0..n {
            session.add_mech(m.clone());
        }
        if n > 0 {
            session.bf_group_doctrine(neurohelmet_core::session::SbfDoctrine::InnerSphere);
        }
        let mut app = App::new(bundle, session, "test".to_string());
        app.dirty = false;
        app
    }

    #[test]
    fn e2e_bf_initial() {
        use super::app::Screen;
        // 4 elements auto-group into one Lance; the header row + card grid render at hex scale.
        let mut app = app_with_bf(4);
        assert!(matches!(app.screen, Screen::BattleForce), "BF mode lands on the BF screen");
        assert_eq!(app.session.bf.units.len(), 1);
        assert_eq!(app.session.bf.units[0].elements.len(), 4);
        let screen = render(&mut app);
        assert!(screen.contains("Lance 1"), "unit header renders");
        assert!(screen.contains("MV 3"), "hex-native unit MV (6\" = 3 hexes)");
        assert!(screen.contains("E 1"), "ground Extreme derives as L−1");
        assert!(screen.contains("S 0-1  M 2-4  L 5-8"), "hex range brackets, not AS inches");
        assert!(screen.contains("To-Hit"), "per-bracket to-hit row");
        insta::assert_snapshot!(screen);
    }

    fn sample_dropship() -> Mech {
        use neurohelmet_core::domain::{ArcCard, ArcDamage, FiringArc, UnitType};
        let ad = |s: &str, m: &str, l: &str, e: &str| ArcDamage {
            s: s.into(),
            m: m.into(),
            l: l.into(),
            e: e.into(),
        };
        Mech {
            chassis: "Union".into(),
            model: "(2708)".into(),
            unit_type: UnitType::Aerospace,
            as_stats: AsStats {
                tp: "DS".into(),
                size: 2,
                movement: "3p".into(),
                armor: 10,
                structure: 5,
                threshold: 1,
                pv: 200,
                arcs: Some(ArcCard {
                    front: FiringArc {
                        std: ad("4", "3", "2", "0*"),
                        msl: ad("1", "1", "1", "1"),
                        specials: vec!["PNT1".into()],
                        ..Default::default()
                    },
                    left: FiringArc { std: ad("2", "2", "1", "0"), ..Default::default() },
                    right: FiringArc { std: ad("2", "2", "1", "0"), ..Default::default() },
                    rear: FiringArc { std: ad("1", "0", "0", "0"), ..Default::default() },
                }),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn e2e_bf_large_craft_arc_card() {
        // A DropShip renders its multi-arc card (per-arc STD/MSL damage, preserving 0*) in place of
        // the single S/M/L/E line, plus a per-arc to-hit note.
        let mut app = app_with_bf_mech(sample_dropship(), 1);
        let screen = render(&mut app);
        assert!(screen.contains("Union"), "DropShip name");
        assert!(screen.contains("Nose"), "nose arc label");
        assert!(screen.contains("STD 4/3/2/0*"), "front STD damage preserves 0*");
        assert!(screen.contains("MSL 1/1/1/1"), "front MSL line");
        assert!(screen.contains("PNT1"), "arc-level special");
        assert!(screen.contains("TH 1"), "aerospace threshold row");
        assert!(screen.contains("Crits"), "crits row still visible after the compact layout");
        assert!(!screen.contains("Heat"), "large craft omit the heat row");
        insta::assert_snapshot!(screen);
    }

    #[test]
    fn e2e_bf_large_craft_shot_builder() {
        use neurohelmet_core::engine::battleforce::BfRange;
        use neurohelmet_core::engine::large_craft::WeaponClass;
        let mut app = app_with_bf_mech(sample_dropship(), 1);
        press(&mut app, KeyCode::Char('t')); // open the BF shot modal
        app.bf_shot.range = BfRange::Short;
        let screen = render(&mut app);
        assert!(screen.contains("Firing arc"), "arc picker row");
        assert!(screen.contains("Weapon class"), "weapon-class picker row");
        // Default arc = Nose, class = STD, range = Short → standard BF to-hit + front STD short (4).
        assert!(screen.contains("Nose STD @ S:"), "per-arc STD preview");
        assert!(screen.contains("damage 4"), "front STD short-range damage");
        assert!(screen.contains("TN"), "STD arc weapons resolve a standard to-hit");

        // Phase 2: a capital class (MSL) now RESOLVES its to-hit through the standard BF table —
        // no more "resolve at table" deferral. MSL takes no capital-vs-small modifier.
        app.bf_shot.weapon_class = WeaponClass::Msl;
        let screen = render(&mut app);
        assert!(screen.contains("Nose MSL @ S:"), "per-arc MSL preview");
        assert!(screen.contains("TN"), "capital classes resolve a real to-hit in Phase 2");
        assert!(
            !screen.contains("resolve to-hit at table"),
            "capital to-hit is no longer deferred"
        );
    }

    #[test]
    fn e2e_bf_large_craft_crit_column() {
        // A DropShip crit rolls the DropShip column; roll 2 (row 0) = KF Boom, whose transport-side
        // effect is described and flagged as resolved at the table.
        let mut app = app_with_bf_mech(sample_dropship(), 1);
        press(&mut app, KeyCode::Char('c')); // open the crit modal
        press(&mut app, KeyCode::Enter); // row 0 = roll 2 = KF Boom on the DropShip column
        assert!(app.status.contains("KF Boom"), "KF Boom effect described: {}", app.status);
        assert!(app.status.contains("resolve at table"), "table-side flag: {}", app.status);
        // The KF Boom flag persists (stateful in Phase 2), not just a status string.
        assert!(app.session.mechs[0].bf.kf_boom, "KF Boom flag set");
    }

    /// An Aegis-class WarShip: a capital front arc (CAP + MSL + STD), the big Arm/Str/Th pool,
    /// the bare-number `Warship` movement mode, and baked DT rating + bay-door counts.
    fn sample_warship() -> Mech {
        use neurohelmet_core::domain::{ArcCard, ArcDamage, FiringArc, UnitType};
        let ad = |s: &str, m: &str, l: &str, e: &str| ArcDamage {
            s: s.into(),
            m: m.into(),
            l: l.into(),
            e: e.into(),
        };
        Mech {
            chassis: "Aegis".into(),
            model: "Heavy Cruiser".into(),
            unit_type: UnitType::Aerospace,
            as_stats: AsStats {
                tp: "WS".into(),
                size: 2,
                movement: "2".into(), // the Warship move mode (bare thrust)
                armor: 193,
                structure: 75,
                threshold: 16,
                pv: 5000,
                dt_rating: 4,
                door_count: 6,
                arcs: Some(ArcCard {
                    front: FiringArc {
                        std: ad("10", "10", "8", "6"),
                        cap: ad("170", "170", "0", "0"),
                        msl: ad("2", "2", "2", "2"),
                        ..Default::default()
                    },
                    left: FiringArc { cap: ad("85", "85", "0", "0"), ..Default::default() },
                    right: FiringArc { cap: ad("85", "85", "0", "0"), ..Default::default() },
                    rear: FiringArc { std: ad("4", "4", "2", "0"), ..Default::default() },
                }),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn e2e_bf_warship_rolls_jumpship_column() {
        // A WarShip rolls the JumpShip column (p.87 footnote **), NOT the DropShip column.
        let mut app = app_with_bf_mech(sample_warship(), 1);
        press(&mut app, KeyCode::Char('c'));
        let screen = render(&mut app);
        assert!(screen.contains("JumpShip column"), "WarShip uses the JumpShip crit column");

        // Roll 10 (row 8) = K-F Drive: −2 integrity per hit, stateful.
        for _ in 0..8 {
            press(&mut app, KeyCode::Down);
        }
        press(&mut app, KeyCode::Enter);
        assert!(app.status.contains("K-F Drive hit 1"), "K-F Drive status: {}", app.status);
        assert_eq!(app.session.mechs[0].bf.kf_drive, 1, "K-F Drive hit tracked");

        // Roll 3 (row 1) = Dock: −1 DT rating; the baked DT 4 → 3 remaining.
        for _ in 0..7 {
            press(&mut app, KeyCode::Up);
        }
        press(&mut app, KeyCode::Enter);
        assert!(app.status.contains("Dock hit 1"), "Dock status: {}", app.status);
        assert!(app.status.contains("3 capacity"), "DT decrement 4→3: {}", app.status);
        assert_eq!(app.session.mechs[0].bf.dock_hits, 1);
    }

    #[test]
    fn e2e_bf_kf_drive_no_effect_on_space_station() {
        // A Space Station rolls the JumpShip column but K-F Drive hits have no effect on it (p.85).
        let mut m = sample_warship();
        m.as_stats.tp = "SS".into();
        let mut app = app_with_bf_mech(m, 1);
        press(&mut app, KeyCode::Char('c'));
        for _ in 0..8 {
            press(&mut app, KeyCode::Down); // row 8 = roll 10 = K-F Drive
        }
        press(&mut app, KeyCode::Enter);
        assert!(
            app.status.contains("no effect on a Space Station"),
            "K-F Drive is inert on a station: {}",
            app.status
        );
        assert_eq!(app.session.mechs[0].bf.kf_drive, 0, "no K-F integrity spent on a station");
    }

    #[test]
    fn e2e_bf_crew_hit_ladders() {
        // JumpShip-column craft (WS/JS/SS) take a 3-stage crew-hit ladder: +2 / +4 / eliminate.
        let mut app = app_with_bf_mech(sample_warship(), 1);
        press(&mut app, KeyCode::Char('c'));
        for _ in 0..10 {
            press(&mut app, KeyCode::Down); // row 10 = roll 12 = Crew Hit
        }
        press(&mut app, KeyCode::Enter);
        assert!(app.status.contains("Crew hit 1: +2"), "1st crew hit +2: {}", app.status);
        press(&mut app, KeyCode::Enter);
        assert!(app.status.contains("Crew hit 2: +4"), "2nd crew hit +4: {}", app.status);
        assert!(app.session.mechs[0].bf.killed.is_none(), "still alive after 2 crew hits");
        press(&mut app, KeyCode::Enter);
        assert!(app.status.contains("crew eliminated"), "3rd crew hit kills: {}", app.status);
        assert!(app.session.mechs[0].bf.killed.is_some(), "3rd crew hit destroys the WarShip");

        // DropShip-column craft take a 2-stage ladder: +2 / eliminate.
        let mut ds = app_with_bf_mech(sample_dropship(), 1);
        press(&mut ds, KeyCode::Char('c'));
        for _ in 0..10 {
            press(&mut ds, KeyCode::Down); // row 10 = roll 12 = Crew Hit (DropShip column)
        }
        press(&mut ds, KeyCode::Enter);
        assert!(ds.status.contains("Crew hit 1: +2"), "1st crew hit +2: {}", ds.status);
        assert!(ds.session.mechs[0].bf.killed.is_none(), "DropShip alive after 1 crew hit");
        press(&mut ds, KeyCode::Enter);
        assert!(ds.status.contains("crew eliminated"), "2nd crew hit kills a DropShip: {}", ds.status);
    }

    #[test]
    fn e2e_bf_large_craft_weapon_crit_is_table_side() {
        // A large-craft Weapon crit halves ONE random firing arc (×0.5, p.85) — resolved at the
        // table, NOT the standard-scale −1/damage counter (which the arc-damage path ignores).
        let mut app = app_with_bf_mech(sample_warship(), 1);
        press(&mut app, KeyCode::Char('c'));
        for _ in 0..4 {
            press(&mut app, KeyCode::Down); // row 4 = roll 6 = Weapon on the JumpShip column
        }
        press(&mut app, KeyCode::Enter);
        assert!(
            app.status.contains("halve one randomly-determined firing arc"),
            "table-side weapon effect: {}",
            app.status
        );
        assert!(app.status.contains("resolve at table"), "flagged table-side: {}", app.status);
        assert_eq!(
            app.session.mechs[0].bf.weapon, 0,
            "large craft do NOT increment the standard −1/damage weapon counter"
        );
    }

    #[test]
    fn e2e_bf_capital_to_hit_vs_small_target() {
        use neurohelmet_core::engine::battleforce::{BfAeroAngle, BfRange, BfTargetKind};
        use neurohelmet_core::engine::large_craft::WeaponClass;
        // A capital (CAP) attack takes +5 to-hit vs a small airborne aerospace target (p.83
        // footnote 28); a standard attack takes no such modifier and neither does a shot at a
        // large-craft / ground target.
        let mut app = app_with_bf_mech(sample_warship(), 1);
        press(&mut app, KeyCode::Char('t')); // open the BF shot modal
        app.bf_shot.range = BfRange::Short;
        app.bf_shot.target_kind = BfTargetKind::AirborneAero(BfAeroAngle::Nose);
        app.bf_shot.weapon_class = WeaponClass::Cap;
        let screen = render(&mut app);
        assert!(screen.contains("Nose CAP @ S:"), "per-arc CAP preview with a resolved TN");
        assert!(screen.contains("CAP vs small target: +5"), "capital-vs-small +5 note: {screen}");

        // Vs a large-craft target the modifier is waived.
        app.bf_shot.target_kind = BfTargetKind::AirborneDropship;
        let screen = render(&mut app);
        assert!(
            !screen.contains("vs small target"),
            "capital modifier waived vs a large-craft target"
        );

        // A standard-class shot never takes the capital-vs-small modifier.
        app.bf_shot.target_kind = BfTargetKind::AirborneAero(BfAeroAngle::Nose);
        app.bf_shot.weapon_class = WeaponClass::Std;
        let screen = render(&mut app);
        assert!(!screen.contains("vs small target"), "STD takes no capital modifier");
    }

    #[test]
    fn e2e_bf_tracking() {
        // Damage, heat and a crit all react on the card AND in the unit-header live MV.
        let mut app = app_with_bf(4);
        press(&mut app, KeyCode::Char(' ')); // 1 damage
        assert!(app.status.contains("Armor 9"), "damage status: {}", app.status);
        assert_eq!(app.session.mechs[3].as_armor_hits, 1, "Space damages the active element");
        press(&mut app, KeyCode::Char('u')); // repair
        assert_eq!(app.session.mechs[3].as_armor_hits, 0);
        press(&mut app, KeyCode::Char(' ')); // re-damage for the snapshot
        press(&mut app, KeyCode::Char('o')); // heat 1
        let screen = render(&mut app);
        assert!(screen.contains("MV 3→2 (heat 1)"), "live MV degradation:\n{screen}");
        assert!(screen.contains("TMM 1 (live 0)"), "live TMM from the bracket table");
        assert!(screen.contains("MV 2"), "unit MV = slowest survivor");
        // Crit modal: roll 4 on the 'Mech column = Fire Control (+2 to-hit).
        press(&mut app, KeyCode::Char('c'));
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Esc);
        assert_eq!(app.session.mechs[3].bf.fire_control, 1, "FC crit marked");
        let screen = render(&mut app);
        assert!(screen.contains("FC1"), "BF crit vocabulary on the card:\n{screen}");
        insta::assert_snapshot!(screen);
        // One undo unwinds the crit; heat stays (separate step).
        press(&mut app, KeyCode::Char('z'));
        assert_eq!(app.session.mechs[3].bf.fire_control, 0, "one z per crit");
    }

    #[test]
    fn e2e_bf_crit_modal() {
        // Ammo (roll 2) with CASE: 1 damage, element lives. Without CASE: destroyed outright.
        let mut cased = sample_mech();
        cased.as_stats.specials.push("CASE".into());
        let mut app = app_with_bf_mech(cased, 1);
        press(&mut app, KeyCode::Char('c'));
        let screen = render(&mut app);
        assert!(screen.contains("'Mech column"), "column reference:\n{screen}");
        assert!(screen.contains("Head Blown Off"), "table rows render");
        insta::assert_snapshot!(screen);
        press(&mut app, KeyCode::Enter); // sel 0 = roll 2 = Ammo
        assert!(app.status.contains("CASE"), "CASE outcome auto-selected: {}", app.status);
        assert_eq!(app.session.mechs[0].bf.killed, None);
        assert_eq!(app.session.mechs[0].as_armor_hits, 1, "CASE ammo = 1 damage");
        press(&mut app, KeyCode::Esc);

        let mut app = app_with_bf(1); // no CASE
        press(&mut app, KeyCode::Char('c'));
        press(&mut app, KeyCode::Enter);
        assert!(app.session.mechs[0].bf.killed.is_some(), "unprotected ammo destroys");
        assert!(app.session.mechs[0].bf_destroyed());
        press(&mut app, KeyCode::Esc);
        assert!(render(&mut app).contains("DESTROYED"));
        press(&mut app, KeyCode::Char('z'));
        assert!(!app.session.mechs[0].bf_destroyed(), "one z undoes the crit");

        // MP crit (roll 7): −half CURRENT MP, round normally, min 1 — multiplicative (spec §1.2).
        press(&mut app, KeyCode::Char('c'));
        for _ in 0..5 {
            press(&mut app, KeyCode::Down);
        }
        press(&mut app, KeyCode::Enter); // MV 3 → loses jround(1.5) = 2
        assert_eq!(app.session.mechs[0].bf.mp_lost, 2);
        assert_eq!(app.session.bf_current_mp(0), 1);
        press(&mut app, KeyCode::Enter); // MV 1 → floors at 1 lost → 0 = immobile
        assert_eq!(app.session.bf_current_mp(0), 0);
        press(&mut app, KeyCode::Esc);

        // CASEP ammo (p.151): the modal hands off to a 1D6 confirm — y detonates, n ignores.
        let mut casep = sample_mech();
        casep.as_stats.specials.push("CASEP".into());
        let mut app = app_with_bf_mech(casep, 1);
        press(&mut app, KeyCode::Char('c'));
        press(&mut app, KeyCode::Enter); // Ammo → CASEP prompt replaces the crit modal
        assert!(render(&mut app).contains("CASEP"), "1D6 prompt shown");
        press(&mut app, KeyCode::Char('n')); // 3+: ignored
        assert_eq!(app.session.mechs[0].bf.killed, None);
        press(&mut app, KeyCode::Char('c'));
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Char('y')); // 1-2: detonation
        assert!(app.session.mechs[0].bf_destroyed());
        press(&mut app, KeyCode::Char('z'));
        assert!(!app.session.mechs[0].bf_destroyed(), "the detonation is one undo step");
    }

    #[test]
    fn e2e_bf_aero_card() {
        // Aerospace elements: TH row instead of TMM, air-to-air range labels, thrust unconverted,
        // the aero crit column, and the doctrine pairing them off into an Air Lance of 2.
        let mut app = app_with_bf_mech(sample_aero_fighter(), 2);
        assert_eq!(app.session.bf.units[0].name, "Air Lance 1");
        assert_eq!(app.session.bf.units[0].elements.len(), 2);
        let screen = render(&mut app);
        assert!(screen.contains("TH 3 (crit if hit > 3)"), "aero threshold row:\n{screen}");
        assert!(screen.contains("S 0-32  M 33-64"), "air-to-air brackets");
        assert!(screen.contains("MV 7a"), "thrust passes through unconverted");
        press(&mut app, KeyCode::Char('c'));
        assert!(render(&mut app).contains("Aerospace column"), "aero crit column");
        // Engine ×2 on aero: TP 0 + shutdown, NOT destruction (spec §1.4).
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Down); // roll 4 = Engine on the aero column
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Esc);
        assert!(!app.session.mechs[1].bf_destroyed(), "aero survives 2 engine hits");
        assert_eq!(app.session.bf_current_mp(1), 0, "TP 0");
        assert!(render(&mut app).contains("TP 0 — SHUTDOWN (engine)"));
        // TP 0 derives live from the hit count (§1.4 as built): heat changes in either
        // direction can't resurrect thrust, and nothing engine-related lands in mp_lost.
        press(&mut app, KeyCode::Char('o')); // heat 1
        press(&mut app, KeyCode::Char('i')); // cool back to 0
        assert_eq!(app.session.bf_current_mp(1), 0, "TP 0 survives heat repair");
        assert_eq!(app.session.mechs[1].bf.mp_lost, 0, "mp_lost stays MP-crits-only");
    }

    #[test]
    fn e2e_bf_vehicle_crit_modal() {
        // The Vehicle crit column end to end — the modal path is the only in-app way to set
        // Crew Stunned and the motive spent-flags, so it gets its own walk: motive rows render,
        // Crew Stunned applies + clears on `n`, and the once-per-game flags stack (§1.2).
        use neurohelmet_core::engine::battleforce::BfMotive;
        let mut app = app_with_bf_mech(sample_vehicle(), 1);
        press(&mut app, KeyCode::Char('c'));
        let screen = render(&mut app);
        assert!(screen.contains("Vehicle column"), "column reference:\n{screen}");
        assert!(screen.contains("Motive damage (p.44)"), "motive section renders");
        assert!(screen.contains("−1 MV") && screen.contains("Immobilized"), "motive rows");
        insta::assert_snapshot!(screen);
        // Roll 3 (sel 1) = Crew Stunned on the Vehicle column.
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Enter);
        assert!(app.session.mechs[0].bf.crew_stunned, "modal sets the turn flag");
        assert!(app.status.contains("no attacks"), "status: {}", app.status);
        // Down to the −1 MV row (sel 11) and apply.
        for _ in 0..10 {
            press(&mut app, KeyCode::Down);
        }
        press(&mut app, KeyCode::Enter);
        assert_eq!(
            app.session.mechs[0].bf.motive,
            BfMotive { minus_one: true, half: false, immobile: false }
        );
        press(&mut app, KeyCode::Enter); // the same effect again: once per game (p.43)
        assert!(app.status.contains("once per game"), "status: {}", app.status);
        press(&mut app, KeyCode::Down); // ½ MV row (sel 12)
        press(&mut app, KeyCode::Enter);
        let m = app.session.mechs[0].bf.motive;
        assert!(m.minus_one && m.half, "different effects stack");
        assert_eq!(app.session.bf_current_mp(0), 1, "MV 4: (4 − 1) / 2, round down");
        let screen = render(&mut app);
        assert!(screen.contains("MOT−1 MOT½"), "stacked flags in the live line:\n{screen}");
        assert!(screen.contains("✓ spent"), "marked rungs show spent");
        press(&mut app, KeyCode::Esc);
        let screen = render(&mut app);
        assert!(screen.contains("STUN"), "crew-stun mark on the card:\n{screen}");
        assert!(screen.contains("MOT−1"), "motive mark on the card");
        assert!(screen.contains("→1 (motive)"), "live MV degradation:\n{screen}");
        // `n` clears the turn flag through the same app the modal set it in; the motive
        // spent-flags are permanent (p.42) and survive.
        press(&mut app, KeyCode::Char('n'));
        assert!(!app.session.mechs[0].bf.crew_stunned, "crew-stunned is turn-scoped");
        assert!(app.session.mechs[0].bf.motive.minus_one, "spent flags persist");
    }

    #[test]
    fn e2e_bf_vehicle_engine_crit_is_live_derived() {
        // Vehicle Engine crit (roll 12 = sel 10): MV and damage halve LIVE from the hit count —
        // the card and the shot-modal preview agree, and nothing lands in mp_lost (§1.4).
        let mut app = app_with_bf_mech(sample_vehicle(), 1);
        press(&mut app, KeyCode::Char('c'));
        for _ in 0..10 {
            press(&mut app, KeyCode::Down);
        }
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Esc);
        assert_eq!(app.session.mechs[0].bf.engine, 1);
        assert_eq!(app.session.mechs[0].bf.mp_lost, 0, "no mp_lost snapshot");
        assert_eq!(app.session.bf_current_mp(0), 2, "MV 4 → 2 live");
        let screen = render(&mut app);
        assert!(screen.contains("→2 (engine)"), "live MV names the engine crit:\n{screen}");
        assert!(screen.contains("S 2"), "card damage halves (5 → 2):\n{screen}");
        // The t-modal preview routes through the same engine leg (default bracket Medium: the
        // card's 5 → 2) — the announced number can never disagree with the card.
        press(&mut app, KeyCode::Char('t'));
        assert!(render(&mut app).contains("damage 2"), "modal preview matches the card");
    }

    #[test]
    fn e2e_bf_bd_crit_modal_is_weapons_only() {
        // BD gun emplacements (spec §Data-fidelity 8): the Vehicle column WITHOUT the motive
        // rows; non-Weapon results render dimmed as "+1 damage instead" (p.42) and applying
        // one deals 1 damage rather than the effect.
        let mut bd = sample_mech();
        bd.as_stats.tp = "BD".into();
        let mut app = app_with_bf_mech(bd, 1);
        press(&mut app, KeyCode::Char('c'));
        let screen = render(&mut app);
        assert!(screen.contains("Vehicle column"), "BD rolls the Vehicle column:\n{screen}");
        assert!(!screen.contains("Motive damage"), "no motive rows for an emplacement");
        assert!(
            screen.contains("Ammo Hit  (+1 damage instead, p.42)"),
            "substitute effect named:\n{screen}"
        );
        assert_eq!(app.bf_crit_row_count(), 11);
        press(&mut app, KeyCode::Enter); // roll 2 = Ammo: doesn't apply to a BD
        assert_eq!(app.session.mechs[0].bf.killed, None, "no ammo explosion");
        assert_eq!(app.session.mechs[0].as_armor_hits, 1, "+1 damage instead");
        assert!(app.status.contains("+1 damage instead"), "status: {}", app.status);
        // Weapon rows still apply normally (sel 7 = roll 9 = Weapon).
        for _ in 0..7 {
            press(&mut app, KeyCode::Down);
        }
        press(&mut app, KeyCode::Enter);
        assert_eq!(app.session.mechs[0].bf.weapon, 1, "Weapon crits stay real effects");
    }

    #[test]
    fn e2e_bf_shot_modal_dfa_airborne_warning() {
        // DFA may not target airborne aerospace (p.45): the modal warns and the composed shot
        // falls back to Standard, so the card and preview never price the illegal combination.
        use neurohelmet_core::engine::battleforce::{
            BfAeroAngle, BfAttackKind, BfPhysical, BfTargetKind,
        };
        let mut jumper = sample_mech();
        jumper.as_stats.movement = "6\"/6\"j".into();
        let mut app = app_with_bf_mech(jumper, 1);
        press(&mut app, KeyCode::Char('t'));
        app.bf_shot.kind = BfAttackKind::Physical(BfPhysical::Dfa);
        app.bf_shot.target_kind = BfTargetKind::AirborneAero(BfAeroAngle::Side);
        let screen = render(&mut app);
        assert!(
            screen.contains("DFA may not target airborne aerospace"),
            "warning line:\n{screen}"
        );
        assert_eq!(app.bf_shot_for(0).kind, BfAttackKind::Standard, "sanitized to Standard");
        // The same declaration against a ground target is legal (jump-capable attacker).
        app.bf_shot.target_kind = BfTargetKind::None;
        assert_eq!(app.bf_shot_for(0).kind, BfAttackKind::Physical(BfPhysical::Dfa));
        assert!(!render(&mut app).contains("DFA may not target"));
    }

    #[test]
    fn e2e_bf_shot_modal_a2g_previews() {
        // Altitude bombing drops ONE bomb per hex (p.47): the preview prices the flat per-hex
        // 2 with the hex count; dive bombing keeps the all-bombs-in-one-hex aggregate. The
        // strafing/striking rear +1 toggle folds in — before the halving for strafing (§1.5).
        use neurohelmet_core::engine::battleforce::{BfA2G, BfAttackKind};
        let mut app = app_with_bf_mech(sample_aero_fighter(), 1); // BOMB2, S 6
        press(&mut app, KeyCode::Char('t'));
        app.bf_shot.kind = BfAttackKind::AirToGround(BfA2G::AltitudeBombing);
        let screen = render(&mut app);
        assert!(
            screen.contains("2 to every element in the hex — one hex per bomb, 2 hex(es)"),
            "per-hex altitude preview:\n{screen}"
        );
        app.bf_shot.kind = BfAttackKind::AirToGround(BfA2G::DiveBombing);
        assert!(render(&mut app).contains("4 to every element in the hex (2 bomb(s) × 2)"));
        app.bf_shot.kind = BfAttackKind::AirToGround(BfA2G::Strafing);
        assert!(render(&mut app).contains("strafing damage 3"), "6/2 = 3");
        for _ in 0..21 {
            press(&mut app, KeyCode::Down); // last row = strikes rear
        }
        press(&mut app, KeyCode::Char(' '));
        assert!(app.bf_shot.strike_rear, "toggle sticks for strafing");
        assert!(render(&mut app).contains("strafing damage 4"), "(6+1)/2 = 3.5 → 4");
        app.bf_shot.kind = BfAttackKind::AirToGround(BfA2G::Striking);
        assert!(render(&mut app).contains("striking damage 7"), "S 6 + rear 1");
    }

    #[test]
    fn e2e_bf_shot_modal() {
        // The t editor drives the p.39 table: skill 4 + Medium (+2, the default bracket)
        // + target TMM 2 = 8.
        let mut app = app_with_bf(4);
        press(&mut app, KeyCode::Char('t'));
        for _ in 0..10 {
            press(&mut app, KeyCode::Down); // row 10 = target TMM
        }
        press(&mut app, KeyCode::Char(' '));
        press(&mut app, KeyCode::Char(' '));
        let screen = render(&mut app);
        assert!(screen.contains("To-Hit   8+"), "4 skill + 2 medium + 2 TMM:\n{screen}");
        insta::assert_snapshot!(screen);
        press(&mut app, KeyCode::Esc);
        // The card folds the persisted context in (`To-Hit*`): M bracket = 8+.
        let screen = render(&mut app);
        assert!(screen.contains("To-Hit*"), "shot-context tag:\n{screen}");
        assert!(screen.contains("M 8+"), "context reaches every card bracket");
        // No floor: standstill −1 at Short with no other mods = 3+ (OQ 3/4).
        press(&mut app, KeyCode::Char('t'));
        press(&mut app, KeyCode::Left); // attacker move → stood still
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Left); // range Medium → Short
        for _ in 0..9 {
            press(&mut app, KeyCode::Down);
        }
        press(&mut app, KeyCode::Left);
        press(&mut app, KeyCode::Left); // TMM back to 0
        assert!(render(&mut app).contains("To-Hit   3+"), "attacker standstill −1, floorless");
        press(&mut app, KeyCode::Esc);
    }

    #[test]
    fn e2e_bf_new_round() {
        // `n` bumps the round and clears the one turn-scoped flag (vehicle Crew Stunned).
        let mut app = app_with_bf(4);
        app.session.mechs[0].bf.crew_stunned = true;
        press(&mut app, KeyCode::Char('n'));
        assert_eq!(app.session.bf.round, 1);
        assert!(!app.session.mechs[0].bf.crew_stunned, "crew-stunned is turn-scoped");
        assert!(app.status.contains("Round 1"), "status: {}", app.status);
    }

    #[test]
    fn e2e_bf_group_editor() {
        // Manual grouping is the primary flow: g opens on the active element; ←→ move it
        // between Units, n splits, u unassigns; Esc prunes emptied Units.
        let mut app = app_with_bf(5); // Lance 1 ×4 + Lance 2 ×1
        assert_eq!(app.session.bf_element_assignment(4), Some(1));
        press(&mut app, KeyCode::Char('g'));
        let screen = render(&mut app);
        assert!(screen.contains("Group force"));
        assert!(screen.contains("split to new unit"));
        insta::assert_snapshot!(screen);
        press(&mut app, KeyCode::Right); // Lance 2 → wraps onto Lance 1
        assert_eq!(app.session.bf_element_assignment(4), Some(0));
        assert_eq!(app.session.bf.units.len(), 2, "emptied Lance 2 stays mid-edit");
        press(&mut app, KeyCode::Char('n')); // split to a fresh Unit
        assert_eq!(app.session.bf_element_assignment(4), Some(2));
        press(&mut app, KeyCode::Char('u')); // unassign
        assert_eq!(app.session.bf_element_assignment(4), None);
        press(&mut app, KeyCode::Esc);
        assert!(app.modal.is_none());
        assert_eq!(app.session.bf.units.len(), 1, "emptied units pruned on close");
        // The ungrouped element renders under the implicit Unassigned section.
        assert!(render(&mut app).contains("Unassigned"));
        // Membership edits restamp the static Unit Size (all Size-4 Atlases → 4).
        assert_eq!(app.session.bf.units[0].size, 4);
    }

    #[test]
    fn e2e_bf_doctrine_auto_group() {
        // Pristine grouping: g → a → Clan applies immediately (Stars of 5).
        let mut app = app_with_bf(8); // Lance 4 + Lance 4
        press(&mut app, KeyCode::Char('g'));
        press(&mut app, KeyCode::Char('a'));
        let screen = render(&mut app);
        assert!(screen.contains("Stars of 5"));
        assert!(screen.contains("element damage/heat/crits stay"), "loss scope named");
        insta::assert_snapshot!(screen);
        press(&mut app, KeyCode::Down); // Clan
        press(&mut app, KeyCode::Enter);
        assert!(app.modal.is_none(), "pristine grouping applies without a confirm");
        assert_eq!(app.session.bf.units[0].name, "Star 1");
        let sizes: Vec<usize> =
            app.session.bf.units.iter().map(|u| u.elements.len()).collect();
        assert_eq!(sizes, vec![5, 3]);
        press(&mut app, KeyCode::Char('z'));
        assert_eq!(app.session.bf.units[0].name, "Lance 1", "one undo restores");

        // Hand-entered state (a morale rung) → the itemized destructive-regroup confirm.
        // `m` acts on the unit holding the ACTIVE element (index 7 → Lance 2 = units[1]).
        press(&mut app, KeyCode::Char('m')); // Broken
        assert_eq!(app.session.bf.units[1].morale, neurohelmet_core::session::BfMorale::Broken);
        press(&mut app, KeyCode::Char('g'));
        press(&mut app, KeyCode::Char('a'));
        press(&mut app, KeyCode::Enter); // Inner Sphere — but a rung is at stake
        let Some(super::app::Modal::Confirm { .. }) = app.modal else {
            panic!("expected an itemized confirmation:\n{}", render(&mut app));
        };
        let screen = render(&mut app);
        assert!(screen.contains("1 morale rung(s)"), "prompt itemizes:\n{screen}");
        assert!(screen.contains("z undoes"));
        press(&mut app, KeyCode::Char('n')); // cancel: nothing changed
        assert_eq!(app.session.bf.units[1].morale, neurohelmet_core::session::BfMorale::Broken);
        press(&mut app, KeyCode::Char('g'));
        press(&mut app, KeyCode::Char('a'));
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Char('y')); // apply: rungs reset with the rebuild
        assert!(
            app.session
                .bf
                .units
                .iter()
                .all(|u| u.morale == neurohelmet_core::session::BfMorale::Normal),
            "the rebuild discards the rung (it was on the bill)"
        );
    }

    #[test]
    fn e2e_bf_unit_header_live_mv() {
        // The Unit MV header recomputes from the members' live MP (p.52) each frame.
        let mut app = app_with_bf(4);
        assert!(render(&mut app).contains("MV 3"), "full-health unit MV");
        press(&mut app, KeyCode::Char('o'));
        press(&mut app, KeyCode::Char('o')); // heat 2 on one member → its MP 1 pins the unit
        assert!(render(&mut app).contains("MV 1"), "slowest survivor pins the Unit");
        press(&mut app, KeyCode::Char('o'));
        press(&mut app, KeyCode::Char('o')); // heat 4 = shutdown
        let screen = render(&mut app);
        assert!(screen.contains("CANNOT MOVE (shutdown)"), "p.49 shutdown pin:\n{screen}");
    }

    #[test]
    fn e2e_bf_morale() {
        // Manual per-Unit rung: Normal → Broken → Routed → Normal (m cycles).
        use neurohelmet_core::session::BfMorale;
        let mut app = app_with_bf(4);
        press(&mut app, KeyCode::Char('m'));
        assert_eq!(app.session.bf.units[0].morale, BfMorale::Broken);
        assert!(render(&mut app).contains("Broken"));
        press(&mut app, KeyCode::Char('m'));
        assert_eq!(app.session.bf.units[0].morale, BfMorale::Routed);
        press(&mut app, KeyCode::Char('m'));
        assert_eq!(app.session.bf.units[0].morale, BfMorale::Normal);
    }

    #[test]
    fn e2e_bf_rename_unit() {
        let mut app = app_with_bf(4); // "Lance 1"
        press(&mut app, KeyCode::Char('r'));
        for _ in 0.."Lance 1".len() {
            press(&mut app, KeyCode::Backspace);
        }
        for c in "Fire Lance".chars() {
            press(&mut app, KeyCode::Char(c));
        }
        press(&mut app, KeyCode::Enter);
        assert_eq!(app.session.bf.units[0].name, "Fire Lance");
        assert!(render(&mut app).contains("Fire Lance"));
    }

    #[test]
    fn e2e_bf_help_modal() {
        let mut app = app_with_bf(4);
        press(&mut app, KeyCode::Char('?'));
        let screen = render(&mut app);
        assert!(screen.contains("Standard BattleForce"));
        assert!(screen.contains("grouping editor"));
        assert!(screen.contains("morale rung"));
        insta::assert_snapshot!(screen);
    }

    #[test]
    fn e2e_add_unit_modal_bf() {
        use super::app::Screen;
        // The add-unit modal in BF mode shows the AS-style single Skill row and PV cost (p.50).
        let mut app = app_with_bf(1);
        press(&mut app, KeyCode::Char('a')); // picker
        assert!(matches!(app.screen, Screen::Picker));
        press(&mut app, KeyCode::Enter); // open AddUnit for the highlighted unit
        let screen = render(&mut app);
        assert!(screen.contains("PV"), "BF costs in PV");
        assert!(screen.contains("Skill"), "single Skill row");
        insta::assert_snapshot!(screen);
    }

    #[test]
    fn f_key_creates_bf_session() {
        isolate_data_dir(); // execute_input persists to disk; keep it out of the real data dir
        let mut app = app_with_one_mech();
        press(&mut app, KeyCode::Char('S')); // open sessions browser
        press(&mut app, KeyCode::Char('F')); // new Standard BattleForce session
        for c in "bfgame".chars() {
            press(&mut app, KeyCode::Char(c));
        }
        press(&mut app, KeyCode::Enter);
        assert_eq!(app.session.mode, GameMode::BattleForce);
        assert!(app.session.mechs.is_empty(), "fresh session");
        // The sheet starts with one empty Unit — the blank ground record sheet's first wrapper.
        assert_eq!(app.session.bf.units.len(), 1);
        assert_eq!(app.session.bf.units[0].name, "Unit 1");
    }

    #[test]
    fn e2e_bf_log_export() {
        // A BF snapshot exports its Unit sheet (one frame per Unit, headers + cards), not the
        // AS-card fallback; pre-`bf`-field logs still take that fallback (log.rs tests cover it).
        use neurohelmet_core::log;
        isolate_data_dir(); // append_log writes under the data dir
        let name = "neurohelmet-unit-bf-export-test";
        let _ = std::fs::remove_file(log::log_file(name));

        let mut app = app_with_bf(5); // Lance 1 ×4 + Lance 2 ×1
        app.current_name = name.to_string();
        press(&mut app, KeyCode::Char(' ')); // 1 damage (active = element 4 in Lance 2)
        press(&mut app, KeyCode::Char('o')); // heat 1
        press(&mut app, KeyCode::Char('m')); // Lance 2 → Broken
        press(&mut app, KeyCode::Char('L')); // snapshot
        assert!(app.status.contains("Logged"), "snapshot recorded: {}", app.status);

        let out = tempfile::tempdir().unwrap();
        let dir = crate::export::run(name, Some(out.path().to_path_buf())).unwrap();
        let transcript = std::fs::read_to_string(dir.join("transcript.txt")).unwrap();
        assert!(transcript.contains("== Turn 1 — Lance 1 =="), "unit heading:\n{transcript}");
        assert!(transcript.contains("== Turn 1 — Lance 2 =="), "one frame per Unit");
        assert!(transcript.contains("Broken"), "morale rung exported");
        assert!(transcript.contains("9/10"), "armor mark exported");
        assert!(transcript.contains("MV 3→2 (heat 1)"), "live MV exported");
        assert!(transcript.contains("S 0-1  M 2-4  L 5-8"), "BF sheet, not the AS fallback");
        assert!(dir.join("turn-01").join("01-Lance 1.ppm").exists());
        assert!(dir.join("turn-01").join("02-Lance 2.ppm").exists());

        // Zero-Unit BF sessions are first-class (all-unassigned rosters, spec §2.3): the
        // export still renders the BF screen — one frame paging the implicit Unassigned
        // section — never the AS-card fallback (reserved for pre-`bf`-field logs).
        app.session.bf.units.clear();
        press(&mut app, KeyCode::Char('n')); // round 1: any live bf field ⇒ bf != default
        press(&mut app, KeyCode::Char('L'));
        let out = tempfile::tempdir().unwrap();
        let dir = crate::export::run(name, Some(out.path().to_path_buf())).unwrap();
        let transcript = std::fs::read_to_string(dir.join("transcript.txt")).unwrap();
        assert!(
            transcript.contains("== Turn 2 — Unassigned =="),
            "zero-unit frame:\n{transcript}"
        );
        assert!(dir.join("turn-02").join("01-Unassigned.ppm").exists());
        let turn2 = transcript.split("== Turn 2").nth(1).unwrap();
        assert!(turn2.contains("S 0-1  M 2-4  L 5-8"), "BF sheet, not AS cards:\n{turn2}");
        let _ = std::fs::remove_file(log::log_file(name));
    }
}
