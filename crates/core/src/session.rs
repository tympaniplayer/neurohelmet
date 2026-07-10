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

//! Mutable per-game session state layered over the immutable [`Mech`] spec, plus JSON
//! persistence with atomic writes so an in-progress game survives a crash or restart.

use crate::data::bundle::Bundle;
use crate::domain::{
    AmmoBin, Equipment, Facing, GameMode, Location, Mech, MechConfig, UnitType, WeaponMount,
};
use crate::engine::damage::{self, DamageOutcome, LocState};
use crate::engine::dice::{cluster_profile, ClusterProfile};
use crate::engine::heat::{aero_heat_effects, heat_effects, AeroHeatEffects, HeatEffects};
use crate::engine::movement::{attacker_movement_modifier, target_movement_modifier, MoveMode};
use crate::engine::alpha_strike::inches_to_hexes;
use crate::engine::as_element::{self, AsElement, DamageVector, SbfElementType};
use crate::engine::battleforce::{self, BfCritCol, BfMotive, BfRange};
use crate::engine::override_conv::{self, ArmorRegion, OverrideCard, OvCritKind};
use crate::engine::pilot;
use crate::engine::acs::{self, AcsCombatTeam, AcsCombatUnit, AcsFormation, AcsMorale};
use crate::engine::sbf::{self, SbfFormation, SbfUnit};
use crate::engine::skill;
use crate::error::Result;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// Roster cap for Classic / Override — a reinforced company-ish limit. Alpha Strike is **uncapped**
/// (it's played at company/battalion/galaxy scale, one card per unit); see [`Session::mech_cap`].
pub const MAX_MECHS: usize = 12;

/// Session file format version, for forward migration.
pub const SESSION_VERSION: u32 = 1;

/// Worst (highest) pilot skill value; skills run 0–8, lower is better.
pub const SKILL_MAX: u8 = 8;

/// Default crew skills (the "Regular" pilot): Gunnery 4 / Piloting 5. Also the Alpha Strike
/// neutral Skill of 4. Scenario-set, not in the unit data.
pub const DEFAULT_GUNNERY: u8 = 4;
pub const DEFAULT_PILOTING: u8 = 5;

/// Vehicle crew hits that knock the crew out of action (tracker convenience — vehicle crews vary;
/// each hit also adds +to-hit while alive).
pub const CREW_MAX: u8 = 5;

/// Alpha Strike heat level that forces a shutdown — the heat scale's 4th box is **S** (Shutdown),
/// so heat 4 = shut down. Also the top of the [`TrackedMech::as_adjust_heat`] dial.
pub const AS_HEAT_SHUTDOWN: u8 = 4;

/// Top of the Override 0–5 heat ladder (the printed card's heat scale). Heat 5 = **Automatic
/// Shutdown**; the dial clamps here.
pub const OV_HEAT_MAX: u8 = 5;

/// Override heat level that forces an automatic shutdown (the top box on the 0–5 ladder).
pub const OV_HEAT_SHUTDOWN: u8 = 5;

/// Most times one Override crit-table row can be marked (stacking results: actuator/motive/engine).
/// A soft cap — a unit is long since out of action before this matters.
pub const OV_CRIT_MAX: u8 = 6;

/// A result on the Total Warfare Ground Combat Vehicle Motive System Damage Table (TW p.193),
/// transcribed from MegaMek `Tank.addMovementDamage`. Each result reduces Cruise MP differently and
/// adds a one-per-severity Steering (driving-roll) penalty; results stack in the order rolled (see
/// [`TrackedMech::motive_damage`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MotiveLevel {
    /// +1 Steering, no MP loss.
    Minor,
    /// +2 Steering, −1 Cruise MP.
    Moderate,
    /// +3 Steering, halves remaining Cruise MP (round up).
    Heavy,
    /// Immobilized — 0 MP.
    Immobilized,
}

impl MotiveLevel {
    /// The four table results in severity order.
    pub const ALL: [MotiveLevel; 4] = [
        MotiveLevel::Minor,
        MotiveLevel::Moderate,
        MotiveLevel::Heavy,
        MotiveLevel::Immobilized,
    ];

    pub fn label(self) -> &'static str {
        match self {
            MotiveLevel::Minor => "Minor",
            MotiveLevel::Moderate => "Moderate",
            MotiveLevel::Heavy => "Heavy",
            MotiveLevel::Immobilized => "Immobilized",
        }
    }

    /// This severity's Steering penalty (applied once however many results of this severity land).
    pub fn steering(self) -> i32 {
        match self {
            MotiveLevel::Minor => 1,
            MotiveLevel::Moderate => 2,
            MotiveLevel::Heavy => 3,
            MotiveLevel::Immobilized => 0,
        }
    }
}

/// Combat-vehicle critical-hit results a player can mark by hand (from the Total Warfare vehicle
/// crit table). `Fuel Tank` and `Ammo` are auto-taken as catastrophic (see
/// `vehicle_destroyed_reason`); motive + crew are tracked separately. `Engine` is a single toggle
/// here, so the "two engine hits = destroyed" rule isn't auto-applied — mark the wreck by hand.
/// Indices here are what `TrackedMech::vehicle_crits` stores.
pub const VEHICLE_CRITS: [&str; 8] = [
    "Engine", "Weapon", "Sensors", "Stabilizer", "Turret", "Fuel Tank", "Ammo", "Cargo",
];

/// Aerospace-fighter critical-damage results, from the sheet's CRITICAL DAMAGE track. Unlike a
/// combat vehicle's one-shot crit list these are **cumulative** (1–3 hits each), and the tracker
/// applies their Total Warfare effects (see [`TrackedMech::aero_engine_hits`] and friends):
/// Engine −2 Thrust / +2 heat per hit (destroyed at 3), Sensors +to-hit (+5 destroyed),
/// FCS +2 to-hit per hit (weapons offline past 2), Avionics +control-roll. Landing Gear has no
/// on-map combat effect — it stays a plain mark. Hit counts live in [`TrackedMech::aero_crit_hits`].
pub const AEROSPACE_CRITS: [&str; 5] = ["Avionics", "Engine", "FCS", "Landing Gear", "Sensors"];

/// Maximum hits an aerospace critical system can take before it's destroyed/maxed.
pub const AERO_CRIT_MAX: u8 = 3;

/// A selectable row in the `c` crit popup. Either a system crit-result (an index into
/// [`TrackedMech::unit_crits`]) or — for aerospace fighters — a specific weapon mount that a rolled
/// weapon crit can destroy. See [`TrackedMech::crit_rows`].
///
/// System rows carry a `hits`/`max` pair: combat vehicles are one-shot (`max == 1`, a plain
/// on/off mark), aerospace systems accumulate (`max == `[`AERO_CRIT_MAX`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CritRow {
    System {
        idx: usize,
        label: &'static str,
        hits: u8,
        max: u8,
    },
    Weapon {
        id: u32,
        label: String,
        destroyed: bool,
    },
}

impl CritRow {
    /// The text shown for this row.
    pub fn label(&self) -> &str {
        match self {
            CritRow::System { label, .. } => label,
            CritRow::Weapon { label, .. } => label,
        }
    }

    /// Whether this row has any hit marked.
    pub fn marked(&self) -> bool {
        match self {
            CritRow::System { hits, .. } => *hits > 0,
            CritRow::Weapon { destroyed, .. } => *destroyed,
        }
    }
}

/// Number of Battle Armor suits a spec has (trooper armor tracks present on the record sheet).
fn ba_suit_count(spec: &Mech) -> usize {
    Location::TROOPERS.iter().filter(|l| spec.armor.contains_key(l)).count()
}

fn default_gunnery() -> u8 {
    DEFAULT_GUNNERY
}
fn default_piloting() -> u8 {
    DEFAULT_PILOTING
}

/// The aggregated mechanical effects of a unit's marked Override crits (see
/// [`TrackedMech::ov_crit_effects`]). The deterministic effects (`move_penalty`/`tmm_penalty` from
/// actuator/motive crits, `engine_hits` driving end-turn heat) are applied; the remaining flags/
/// counts are surfaced in the card's crit-effects summary for the player to resolve.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OvCritEffects {
    /// Total Move reduction (−2 per actuator/motive crit).
    pub move_penalty: i32,
    /// Total TMM reduction (−1 per actuator/motive crit).
    pub tmm_penalty: i32,
    /// Engine crit hits (each banks +1 heat at end-turn).
    pub engine_hits: u8,
    /// Gyro crit hits (1 = +2 PSR; 2 = fall, can't stand).
    pub gyro_hits: u8,
    /// Weapon crit hits (attacker's choice which weapon — informational here).
    pub weapon_hits: u8,
    /// Crew-hit / cockpit crit results (a pilot/crew wound — apply with `p`).
    pub crew_pilot_hits: u8,
    pub stunned: bool,
    pub avionics: bool,
    pub ammo_marked: bool,
    pub fuel_marked: bool,
}

impl OvCritEffects {
    /// Whether any crit effect is in play (drives whether the card shows the summary line).
    pub fn any(&self) -> bool {
        self.move_penalty != 0
            || self.engine_hits > 0
            || self.gyro_hits > 0
            || self.weapon_hits > 0
            || self.crew_pilot_hits > 0
            || self.stunned
            || self.avionics
            || self.ammo_marked
            || self.fuel_marked
    }
}

/// One mech being tracked during play: its immutable spec plus all live damage/heat/ammo.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrackedMech {
    pub spec: Mech,
    pub locations: BTreeMap<Location, LocState>,
    pub heat: i32,
    /// Bin id -> shots remaining. For Battle Armor this is unused; see [`Self::suit_ammo`].
    pub ammo: BTreeMap<u32, u16>,
    /// Battle Armor per-suit ammo: bin id -> shots remaining for each suit, indexed by suit
    /// position in the squad. Each suit carries its own copy of the squad loadout, so a 4-suit
    /// squad with a 2-shot SRM bin starts `[2, 2, 2, 2]` and you fire from one suit at a time.
    /// A suit that dies can no longer fire, so its shots are lost. Empty for non-BA / old sessions.
    #[serde(default)]
    pub suit_ammo: BTreeMap<u32, Vec<u16>>,
    /// Battle Armor: which suit is currently selected to fire from (index into the squad's suits;
    /// mirrors the doll cursor). Ignored for other unit types. Defaulted for old sessions.
    #[serde(default)]
    pub active_suit: usize,
    pub shutdown: bool,
    /// Destroyed critical slots, by location (the set of slot indices marked off).
    /// Defaulted so sessions saved before crit support still deserialize.
    #[serde(default)]
    pub crit_hits: BTreeMap<Location, BTreeSet<u8>>,
    /// MechWarrior damage: number of pilot hits taken (0..=6). Defaulted for old sessions.
    #[serde(default)]
    pub pilot_hits: u8,
    /// Whether the pilot is currently knocked out (failed a consciousness roll). Tracked
    /// manually; `false` (conscious) by default.
    #[serde(default)]
    pub pilot_unconscious: bool,
    /// Pilot Gunnery skill (0–8, lower is better). Scenario-set, not in the unit data; defaults
    /// to the regular 4. (Custom default since 0 is a valid — legendary — skill.)
    #[serde(default = "default_gunnery")]
    pub gunnery: u8,
    /// Pilot Piloting skill (0–8, lower is better). Defaults to the regular 5.
    #[serde(default = "default_piloting")]
    pub piloting: u8,
    /// Whether the 'Mech is prone (knocked down / lying down). Set by hand after a fall; a
    /// prone 'Mech must spend MP + pass a PSR to stand. Defaulted for old sessions.
    #[serde(default)]
    pub prone: bool,
    /// Damage taken so far this turn (reset on end-turn). 20+ triggers a Piloting Skill Roll.
    #[serde(default)]
    pub damage_this_turn: u16,
    /// How the unit moved this turn (reset on end-turn). Feeds the attacker to-hit modifier,
    /// the unit's own TMM, and movement-derived PSR prompts.
    #[serde(default)]
    pub move_mode: MoveMode,
    /// Hexes moved this turn (reset on end-turn). Feeds the TMM table.
    #[serde(default)]
    pub hexes_moved: u8,
    /// Aerospace: current velocity in hexes on the (low-altitude) ground map. Persists across
    /// turns (a fighter keeps its velocity, changing it by ±thrust). Also drives the aero TMM.
    #[serde(default)]
    pub velocity: u8,
    /// Aerospace: current altitude level (0 = on the ground/NOE, up to 10 on the low-altitude
    /// map). Persists across turns; the player climbs/dives by hand.
    #[serde(default)]
    pub altitude: u8,
    /// Shots fired this turn per weapon ([`WeaponMount::id`] -> shot count). Most weapons cap at 1;
    /// Ultra ACs fire 2 and Rotary ACs up to 6 ([`WeaponMount::max_shots`]). Cleared on end-turn.
    #[serde(default)]
    pub fired: BTreeMap<u32, u8>,
    /// Battle Armor: which suits have fired each weapon this turn (weapon id -> set of suit
    /// indices). A BA squad fires one suit at a time, so each suit may fire each weapon once a
    /// turn — tracked per suit rather than as the squad-wide [`Self::fired`] count. Cleared on
    /// end-turn. Empty for non-BA / old sessions.
    #[serde(default)]
    pub suit_fired: BTreeMap<u32, BTreeSet<usize>>,
    // ----- Combat-vehicle live state (used only when the spec is a vehicle) -----
    /// Motive System Damage Table results, in the order rolled. Each [`MotiveLevel`] reduces Cruise
    /// MP and adds a Steering penalty (see [`Self::motive_cruise`] / [`Self::motive_steering`]).
    /// Defaulted (empty) for old sessions.
    #[serde(default)]
    pub motive_damage: Vec<MotiveLevel>,
    /// Crew hits (each adds +to-hit; the crew is lost at [`CREW_MAX`]). Defaulted for old sessions.
    #[serde(default)]
    pub crew_hits: u8,
    /// Marked vehicle critical results — indices into [`VEHICLE_CRITS`]. Set by hand from the crit
    /// popup (vehicle crits are a rolled table, not slots). Defaulted for old sessions.
    #[serde(default)]
    pub vehicle_crits: BTreeSet<u8>,
    /// Aerospace critical-damage hit counts — index into [`AEROSPACE_CRITS`] -> hits (0..=
    /// [`AERO_CRIT_MAX`]). Aero systems accumulate hits (each escalating its effect), unlike a
    /// vehicle's one-shot [`Self::vehicle_crits`]. Set from the crit popup. Defaulted for old
    /// sessions.
    #[serde(default)]
    pub aero_crit_hits: BTreeMap<u8, u8>,
    /// Weapons knocked out by an explicit weapon critical, keyed by [`WeaponMount::id`]. Aerospace
    /// fighters have no crit-slot grid, so a rolled weapon crit is recorded here rather than as a
    /// destroyed slot (see [`Self::is_weapon_disabled`]). Defaulted for old sessions.
    #[serde(default)]
    pub weapon_crits: BTreeSet<u32>,
    /// Per-ammo-type chosen bin: `ammo_key` -> the [`AmmoBin::id`] a weapon of that type
    /// should draw from. Set by hand from the crit popup; when unset (or the bin is empty),
    /// firing falls back to the first compatible non-empty bin. Defaulted for old sessions.
    #[serde(default)]
    pub active_bin: BTreeMap<String, u32>,
    /// Per-bin chosen munition: [`AmmoBin::id`] -> munition display name. Set by hand from the
    /// crit popup (`t`); when unset, the bin's baked [`AmmoBin::munition`] applies. Defaulted
    /// for old sessions.
    #[serde(default)]
    pub munition_choice: BTreeMap<u32, String>,
    // ----- Alpha Strike live state (used only in AlphaStrike sessions) -----
    #[serde(default)]
    pub as_armor_hits: u8,
    #[serde(default)]
    pub as_struct_hits: u8,
    #[serde(default)]
    pub as_heat: u8,
    #[serde(default)]
    pub as_crits: AsCrits,
    /// §33 Phase 2: did this unit jump this turn? (+2 to its AS to-hit; the AS attacker move mod.)
    #[serde(default)]
    pub as_attacker_jumped: bool,
    /// §33 Phase 2: the current hand-entered AS target (for the on-card to-hit). `None` = no target,
    /// so the To-Hit row shows only the attacker-side ("self") number.
    #[serde(default)]
    pub as_target: Option<AsTarget>,
    /// Standard BattleForce live crit state (used only in BattleForce sessions, which otherwise
    /// reuse the AS armor/structure/heat fields above — spec §2.2). Defaulted for old sessions.
    #[serde(default)]
    pub bf: BfLive,
    /// §24: the current hand-entered Classic GATOR target. `None` = no target, so the equipment
    /// panel shows the equipment-derived to-hit *modifier* rather than a per-weapon target number.
    /// Cleared on end-turn alongside the attacker's own movement (the target moves anew each turn).
    #[serde(default)]
    pub ct_target: Option<CtTarget>,
    // ----- Override live state (used only in Override sessions) -----
    /// Armor pips lost per Override diagram region (keyed by the region's representative
    /// [`Location`]; the three 'Mech torsos collapse to `CenterTorso`). Defaulted for old sessions.
    #[serde(default)]
    pub ov_armor_hits: BTreeMap<Location, u16>,
    /// Structure pips lost per Override region.
    #[serde(default)]
    pub ov_struct_hits: BTreeMap<Location, u16>,
    /// Rear-armor pips lost (the merged 'Mech torso region only).
    #[serde(default)]
    pub ov_rear_hits: BTreeMap<Location, u16>,
    /// Current Override heat on the 0–5 ladder (persists across turns; dissipated by sinks on
    /// end-turn).
    #[serde(default)]
    pub ov_heat: u8,
    /// Which TICs (by index into the packed weapons table) fired this turn. Cleared on end-turn.
    #[serde(default)]
    pub ov_fired: BTreeSet<usize>,
    /// Marked Override critical-hit results per region: the region's representative [`Location`] ->
    /// row indices into that region's crit table (see `engine::override_conv` crit tables). Set by
    /// hand from the crit popup. Defaulted for old sessions.
    #[serde(default)]
    pub ov_crits: BTreeMap<Location, Vec<u8>>,
    /// Override to-hit shot context (attacker movement + hand-entered target), folded into the live
    /// per-TIC To-Hit row. Always present (default = ground / no target); see [`OvShot`].
    #[serde(default)]
    pub ov_shot: OvShot,
    /// Override regions whose ammo the player has marked **spent** (Override doesn't count shots, so
    /// this is the manual toggle): a spent region's ammo crit no longer detonates (it "becomes a
    /// weapon result"). Keyed by the region's representative [`Location`].
    #[serde(default)]
    pub ov_ammo_spent: BTreeSet<Location>,
}

/// Maximum hand-entered Override target TMM (matches the AS cap; one spare over the derived stat).
pub const OV_TARGET_TMM_MAX: u8 = 6;

/// The Override to-hit shot context (QRG attack modifiers). Like [`AsTarget`] the opponent isn't a
/// tracked unit — the player sets attacker movement and the target's movement/state by hand. Always
/// present on a [`TrackedMech`] (default = attacker on the ground, no target), so the per-TIC
/// To-Hit row always has a base number; [`TrackedMech::ov_shot_active`] reports whether it's been
/// changed from the default.
///
/// The *attacker's* movement modifier comes from the unit's own [`TrackedMech::move_mode`] (set with
/// the `v` movement editor), not from this struct — there's a single source of truth for "did I
/// move".
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OvShot {
    /// Target movement modifier (TMM) to add (0..=[`OV_TARGET_TMM_MAX`]).
    pub target_tmm: u8,
    /// Target jumped (+1 over its TMM).
    pub target_jumped: bool,
    /// Target immobile / shut down / unconscious (flat −2, overrides movement).
    pub target_immobile: bool,
    /// Secondary-target arc (+1).
    pub secondary: bool,
    /// Rear-arc shot (+1).
    pub rear: bool,
}

/// Maximum hand-entered AS target TMM (AS:CE caps the derived stat at 5 for 35"+; one spare).
pub const AS_TARGET_TMM_MAX: u8 = 6;

/// A hand-entered Alpha Strike target for the on-card to-hit (§33 Phase 2). The opponent is *not*
/// tracked as a unit — the player types the target's movement modifier (TMM) and a couple of flags.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AsTarget {
    pub tmm: u8,
    pub jumped: bool,
    pub immobile: bool,
}

/// Caps for the hand-entered Classic GATOR target (§24). Distance covers the longest weapon's
/// extreme bracket (LRM long 21 → extreme 42, plus headroom); hexes-moved caps where the TMM table
/// tops out (25+ → TMM 6).
pub const CT_TARGET_DISTANCE_MAX: u16 = 60;
pub const CT_TARGET_HEXES_MAX: u8 = 30;

/// A hand-entered Classic to-hit target (§24, the GATOR analog of [`AsTarget`]). The opponent isn't
/// tracked as a unit — the player enters the range to it (hexes) and how it moved this turn; the
/// per-weapon range bracket and the target movement modifier are derived from that.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CtTarget {
    /// Distance to the target in hexes — drives each weapon's range bracket.
    pub distance: u16,
    /// Hexes the target moved this turn — drives its Target Movement Modifier.
    pub hexes_moved: u8,
    /// Target jumped (+1 TMM).
    pub jumped: bool,
    /// Target immobile (flat −4, overrides movement).
    pub immobile: bool,
}

/// Alpha Strike critical-hit counters (marked by hand).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AsCrits {
    pub engine: u8,
    pub fire_control: u8,
    pub mp: u8,
    pub weapon: u8,
    /// Combat-vehicle Motive hits (0–5, marked in order): 1–2 = −2″/−1 hex MV, 3–4 = ½ MV, 5 = 0 MV.
    #[serde(default)]
    pub motive: u8,
}

/// The Alpha Strike critical-hit types (the set in play varies by unit type — see
/// [`TrackedMech::as_crit_kinds`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AsCritKind {
    Engine,
    FireControl,
    Mp,
    Weapon,
    /// Combat-vehicle motive system (MV loss); not used by 'Mechs or aerospace.
    Motive,
}

impl AsCritKind {
    /// The 'Mech / ground crit set (also the default for vehicles & infantry).
    pub const ALL: [AsCritKind; 4] = [
        AsCritKind::Engine,
        AsCritKind::FireControl,
        AsCritKind::Mp,
        AsCritKind::Weapon,
    ];

    /// Aerospace-fighter crit set — Engine / Fire Control / Weapon. Unlike 'Mechs, AS aerospace has
    /// **no MP crit** (movement loss comes from Engine crits halving thrust). Per AS:CE.
    pub const AEROSPACE: [AsCritKind; 3] =
        [AsCritKind::Engine, AsCritKind::FireControl, AsCritKind::Weapon];

    /// Combat-vehicle crit set — Engine / Fire Control / Weapon / Motive. Vehicles take **Motive**
    /// hits (MV loss) rather than the 'Mech MP crit. Per AS:CE.
    pub const VEHICLE: [AsCritKind; 4] =
        [AsCritKind::Engine, AsCritKind::FireControl, AsCritKind::Weapon, AsCritKind::Motive];

    /// Gun emplacements / Battlefield Support (AS type `BD`): immobile, so only Weapons crits apply.
    pub const WEAPON_ONLY: [AsCritKind; 1] = [AsCritKind::Weapon];

    /// Maximum number of hits tracked (engine death at 2; fire control caps at +8 = 4 hits; the
    /// vehicle Motive track is 5 boxes: −2″/−1 hex ×2, ½ MV ×2, immobile ×1).
    pub fn cap(self) -> u8 {
        match self {
            AsCritKind::Engine => 2,
            AsCritKind::FireControl => 4,
            AsCritKind::Motive => 5,
            AsCritKind::Mp | AsCritKind::Weapon => 8,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            AsCritKind::Engine => "Engine",
            AsCritKind::FireControl => "Fire Control",
            AsCritKind::Mp => "MP",
            AsCritKind::Weapon => "Weapon",
            AsCritKind::Motive => "Motive",
        }
    }
}

/// Standard BattleForce per-element live crit state (spec §2.2). The armor/structure/heat live
/// state is the shared AS block ([`TrackedMech::as_armor_hits`] & co — same 0–4/S scale, p.26);
/// this struct holds only what the BF crit table (p.42) adds. [`AsCrits`] is deliberately NOT
/// reused — BF's vocabulary and arithmetic differ (multiplicative MP loss, once-per-game motive
/// rungs, type-columned results). "The effects of Critical Hits are permanent" (p.42).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BfLive {
    /// 'Mech/Vehicle/Aerospace Engine hits (effects per column, spec §1.4; two hits destroy a
    /// 'Mech or Vehicle — [`battleforce::BF_ENGINE_HITS_DESTROY`], via [`TrackedMech::bf_destroyed`]).
    #[serde(default)]
    pub engine: u8,
    /// Fire Control hits: +2 to-hit each, cumulative, never on physicals (p.43).
    #[serde(default)]
    pub fire_control: u8,
    /// Accumulated MP-crit loss (applied-at-crit-time, spec §1.2): each MP crit removes half of
    /// CURRENT MP — computed when the crit lands ([`Session::bf_apply_mp_crit`]) and accumulated
    /// here, never `count × k`. MP crits ONLY (as built 2026-07-05): vehicle/aerospace
    /// Engine-crit MV/TP effects derive live from [`BfLive::engine`] in
    /// [`Session::bf_current_mp`], never as a snapshot here.
    #[serde(default)]
    pub mp_lost: u32,
    /// Weapon hits: −1 to every damage value each, floored 0 (p.43).
    #[serde(default)]
    pub weapon: u8,
    /// Vehicles: Crew Stunned — no attacks next turn; a turn flag, cleared by `n` new round
    /// ([`Session::bf_begin_round`]).
    #[serde(default)]
    pub crew_stunned: bool,
    /// Vehicles: motive-damage effects (p.44) — independent once-per-game spent-flags
    /// ([`TrackedMech::bf_mark_motive`]; as built 2026-07-05 the effects stack, they are not a
    /// monotone rung).
    #[serde(default)]
    pub motive: BfMotive,
    /// ARM: the first crit chance of the scenario is ignored outright, marked spent here (p.143).
    #[serde(default)]
    pub arm_spent: bool,
    /// An outright-kill crit result marked on the sheet (`None` = still standing).
    #[serde(default)]
    pub killed: Option<BfKill>,
    // ---- large-craft crit ladders (IO:BF p.85; all #[serde(default)], mostly 0 for non-large
    // craft). The transport/jump consequences are table-side (spec §10); these track the numbers.
    /// Crew Hit stages (large craft): DropShip/Small Craft +2 then eliminate (2 stages); JumpShip /
    /// WarShip / Space Station +2 / +4 / eliminate (3 stages). Each stage adds +2 to all shots.
    #[serde(default)]
    pub crew_hit: u8,
    /// K-F Drive hits (JumpShip column): −2 drive integrity each; no jump at 0. No effect on
    /// Space Stations.
    #[serde(default)]
    pub kf_drive: u8,
    /// "Dock" hits (JumpShip column): −1 DropShip-Transport (DT) rating each.
    #[serde(default)]
    pub dock_hits: u8,
    /// "Door" hits (DropShip column): one transport-bay door lost each.
    #[serde(default)]
    pub door_hits: u8,
    /// KF Boom destroyed (DropShip column, roll 2): may dock but not jump.
    #[serde(default)]
    pub kf_boom: bool,
    /// Docking Collar destroyed (DropShip column, roll 3): may not dock with a station/JS/WS.
    #[serde(default)]
    pub docking_collar: bool,
    /// Thruster hit (large craft): +1 Thrust per facing change; hit once per element.
    #[serde(default)]
    pub thruster: bool,
}

/// The BF crit results that destroy an element outright when marked (spec §2.2): Ammo (no CASE
/// protection), Head Blown Off, Crew Killed, Fuel, Proto Destroyed — plus `Engine2` for the
/// aerospace-column bookkeeping case where the modal marks the kill explicitly rather than
/// counting hits.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BfKill {
    Ammo,
    HeadBlownOff,
    CrewKilled,
    Fuel,
    ProtoDestroyed,
    Engine2,
}

/// Current effective movement after damage and heat (see [`TrackedMech::movement`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Movement {
    pub walk: u8,
    pub run: u8,
    pub jump: u8,
    /// The 'Mech can't move at all (both legs gone, shut down, or pilot out).
    pub immobile: bool,
    /// A short reason the numbers are reduced/zero (`"leg destroyed"`, `"immobile"`, …).
    pub note: Option<&'static str>,
}

/// Outcome of firing a weapon.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FireResult {
    pub heat: u8,
    /// A round was deducted from a compatible ammo bin.
    pub ammo_spent: bool,
    /// The weapon uses ammo but every compatible bin is empty.
    pub out_of_ammo: bool,
}

impl TrackedMech {
    /// Initialize a fresh, undamaged tracked mech from a spec.
    pub fn new(spec: Mech) -> Self {
        let locations = Location::ALL
            .iter()
            .map(|&loc| (loc, LocState::default()))
            .collect();
        let ammo = spec.ammo.iter().map(|b| (b.id, b.shots_max())).collect();
        // Battle Armor: each suit gets its own copy of every ammo bin (fired one suit at a time).
        let suit_ammo = if spec.unit_type == UnitType::BattleArmor {
            let suits = ba_suit_count(&spec);
            spec.ammo.iter().map(|b| (b.id, vec![b.shots_max(); suits])).collect()
        } else {
            BTreeMap::new()
        };
        TrackedMech {
            spec,
            locations,
            heat: 0,
            ammo,
            suit_ammo,
            active_suit: 0,
            shutdown: false,
            crit_hits: BTreeMap::new(),
            pilot_hits: 0,
            pilot_unconscious: false,
            gunnery: default_gunnery(),
            piloting: default_piloting(),
            prone: false,
            damage_this_turn: 0,
            move_mode: MoveMode::default(),
            hexes_moved: 0,
            velocity: 0,
            altitude: 0,
            fired: BTreeMap::new(),
            suit_fired: BTreeMap::new(),
            motive_damage: Vec::new(),
            crew_hits: 0,
            vehicle_crits: BTreeSet::new(),
            aero_crit_hits: BTreeMap::new(),
            weapon_crits: BTreeSet::new(),
            active_bin: BTreeMap::new(),
            munition_choice: BTreeMap::new(),
            as_armor_hits: 0,
            as_struct_hits: 0,
            as_heat: 0,
            as_crits: AsCrits::default(),
            as_attacker_jumped: false,
            as_target: None,
            bf: BfLive::default(),
            ct_target: None,
            ov_armor_hits: BTreeMap::new(),
            ov_struct_hits: BTreeMap::new(),
            ov_rear_hits: BTreeMap::new(),
            ov_heat: 0,
            ov_fired: BTreeSet::new(),
            ov_crits: BTreeMap::new(),
            ov_shot: OvShot::default(),
            ov_ammo_spent: BTreeSet::new(),
        }
    }

    /// Record a pilot hit (clamped at [`pilot::PILOT_MAX`]).
    pub fn hit_pilot(&mut self) {
        self.pilot_hits = (self.pilot_hits + 1).min(pilot::PILOT_MAX);
    }

    /// Heal a pilot hit.
    pub fn heal_pilot(&mut self) {
        self.pilot_hits = self.pilot_hits.saturating_sub(1);
    }

    /// The 2d6 target to stay conscious at the current hit count (`None` = no roll needed).
    pub fn consciousness_avoid(&self) -> Option<u8> {
        pilot::consciousness_avoid(self.pilot_hits)
    }

    /// Whether the pilot has been killed.
    pub fn pilot_dead(&self) -> bool {
        self.pilot_hits >= pilot::PILOT_MAX
    }

    /// Toggle the pilot's knocked-out state (no effect once the pilot is dead).
    pub fn toggle_unconscious(&mut self) {
        if !self.pilot_dead() {
            self.pilot_unconscious = !self.pilot_unconscious;
        }
    }

    // ----- Alpha Strike -----

    pub fn as_armor_remaining(&self) -> u8 {
        self.spec.as_stats.armor.saturating_sub(self.as_armor_hits)
    }

    pub fn as_struct_remaining(&self) -> u8 {
        self.spec.as_stats.structure.saturating_sub(self.as_struct_hits)
    }

    /// Apply one point of Alpha Strike damage: armor first, then structure.
    pub fn as_damage(&mut self) {
        if self.as_armor_remaining() > 0 {
            self.as_armor_hits += 1;
        } else if self.as_struct_remaining() > 0 {
            self.as_struct_hits += 1;
        }
    }

    /// Undo one point of damage (structure first, then armor — reverse of [`Self::as_damage`]).
    pub fn as_repair(&mut self) {
        if self.as_struct_hits > 0 {
            self.as_struct_hits -= 1;
        } else if self.as_armor_hits > 0 {
            self.as_armor_hits -= 1;
        }
    }

    /// Adjust AS heat, clamped to the `0..=S` dial (the 4th box, [`AS_HEAT_SHUTDOWN`], is Shutdown).
    pub fn as_adjust_heat(&mut self, delta: i32) {
        self.as_heat = (self.as_heat as i32 + delta).clamp(0, AS_HEAT_SHUTDOWN as i32) as u8;
    }

    /// The Alpha Strike crit types this unit can take, by unit type: aerospace omits MP, combat
    /// vehicles swap MP for a Motive track, everything else uses the full 'Mech set (all per AS:CE).
    /// Drives the crit popup and the card's Crits row.
    pub fn as_crit_kinds(&self) -> &'static [AsCritKind] {
        if self.spec.as_stats.tp == "BD" {
            // Gun emplacements / Battlefield Support: immobile, weapons-only (checked before the
            // vehicle case, since these are baked as the Vehicle unit type).
            &AsCritKind::WEAPON_ONLY
        } else if self.spec.is_aerospace() {
            &AsCritKind::AEROSPACE
        } else if self.spec.is_vehicle() {
            &AsCritKind::VEHICLE
        } else {
            &AsCritKind::ALL
        }
    }

    pub fn as_crit(&self, kind: AsCritKind) -> u8 {
        match kind {
            AsCritKind::Engine => self.as_crits.engine,
            AsCritKind::FireControl => self.as_crits.fire_control,
            AsCritKind::Mp => self.as_crits.mp,
            AsCritKind::Weapon => self.as_crits.weapon,
            AsCritKind::Motive => self.as_crits.motive,
        }
    }

    pub fn as_crit_inc(&mut self, kind: AsCritKind) {
        let cap = kind.cap();
        let f = self.as_crit_mut(kind);
        *f = (*f + 1).min(cap);
    }

    pub fn as_crit_dec(&mut self, kind: AsCritKind) {
        let f = self.as_crit_mut(kind);
        *f = f.saturating_sub(1);
    }

    fn as_crit_mut(&mut self, kind: AsCritKind) -> &mut u8 {
        match kind {
            AsCritKind::Engine => &mut self.as_crits.engine,
            AsCritKind::FireControl => &mut self.as_crits.fire_control,
            AsCritKind::Mp => &mut self.as_crits.mp,
            AsCritKind::Weapon => &mut self.as_crits.weapon,
            AsCritKind::Motive => &mut self.as_crits.motive,
        }
    }

    /// §33 Phase 2: a shot context is active when the unit jumped or has a target set; only then does
    /// the To-Hit row fold in the opponent-dependent modifiers (otherwise it shows the "self" number).
    pub fn as_shot_active(&self) -> bool {
        self.as_attacker_jumped || self.as_target.is_some()
    }

    /// Toggle the attacker-jumped flag (the AS +2 attacker-movement to-hit modifier).
    pub fn as_toggle_attacker_jumped(&mut self) {
        self.as_attacker_jumped = !self.as_attacker_jumped;
    }

    /// Adjust the hand-entered target TMM. Stepping below 0 clears the target entirely; stepping up
    /// from "no target" creates one at TMM 0. Clamped to [`AS_TARGET_TMM_MAX`].
    pub fn as_adjust_target_tmm(&mut self, delta: i32) {
        match &mut self.as_target {
            None => {
                if delta > 0 {
                    self.as_target = Some(AsTarget::default());
                }
            }
            Some(t) => {
                let next = t.tmm as i32 + delta;
                if next < 0 {
                    self.as_target = None;
                } else {
                    t.tmm = next.min(AS_TARGET_TMM_MAX as i32) as u8;
                }
            }
        }
    }

    /// Toggle the target's jumped flag (+1 TMM); no-op when no target is set.
    pub fn as_toggle_target_jumped(&mut self) {
        if let Some(t) = &mut self.as_target {
            t.jumped = !t.jumped;
        }
    }

    /// Toggle the target's immobile flag (flat −4, overrides TMM); no-op when no target is set.
    pub fn as_toggle_target_immobile(&mut self) {
        if let Some(t) = &mut self.as_target {
            t.immobile = !t.immobile;
        }
    }

    /// §24: is a Classic GATOR target set? (When true, the equipment panel shows per-weapon target
    /// numbers instead of bare equipment modifiers.)
    pub fn ct_shot_active(&self) -> bool {
        self.ct_target.is_some()
    }

    /// Adjust the GATOR target distance (hexes). Stepping below 1 clears the target entirely;
    /// stepping up from "no target" creates one at 1 hex. Clamped to [`CT_TARGET_DISTANCE_MAX`].
    pub fn ct_adjust_distance(&mut self, delta: i32) {
        match &mut self.ct_target {
            None => {
                if delta > 0 {
                    self.ct_target = Some(CtTarget { distance: 1, ..CtTarget::default() });
                }
            }
            Some(t) => {
                let next = t.distance as i32 + delta;
                if next < 1 {
                    self.ct_target = None;
                } else {
                    t.distance = next.min(CT_TARGET_DISTANCE_MAX as i32) as u16;
                }
            }
        }
    }

    /// Adjust the target's hexes-moved this turn (its TMM); clamped to [`CT_TARGET_HEXES_MAX`].
    /// No-op when no target is set.
    pub fn ct_adjust_hexes(&mut self, delta: i32) {
        if let Some(t) = &mut self.ct_target {
            t.hexes_moved = (t.hexes_moved as i32 + delta).clamp(0, CT_TARGET_HEXES_MAX as i32) as u8;
        }
    }

    /// Toggle the target's jumped flag (+1 TMM); no-op when no target is set.
    pub fn ct_toggle_target_jumped(&mut self) {
        if let Some(t) = &mut self.ct_target {
            t.jumped = !t.jumped;
        }
    }

    /// Toggle the target's immobile flag (flat −4, overrides movement); no-op when no target is set.
    pub fn ct_toggle_target_immobile(&mut self) {
        if let Some(t) = &mut self.ct_target {
            t.immobile = !t.immobile;
        }
    }

    /// Destroyed in AS when structure is gone, or — for 'Mechs only — after 2 engine crits (engine
    /// destroyed). Vehicles/aerospace at 2 engine crits just lose MV/thrust, not the unit.
    pub fn as_destroyed(&self) -> bool {
        let struct_gone = self.spec.as_stats.structure > 0 && self.as_struct_remaining() == 0;
        let engine_killed = self.as_crits.engine >= 2 && self.spec.unit_type == UnitType::Mech;
        struct_gone || engine_killed
    }

    /// Shut down in Alpha Strike: the heat scale's 4th box is **S** (Shutdown), so a unit at heat
    /// 4 is shut down (MegaMek AS heat scale: `1 2 3 S`). Recoverable — cool back down with `i`.
    pub fn as_shutdown(&self) -> bool {
        self.as_heat >= AS_HEAT_SHUTDOWN
    }

    // ----- Standard BattleForce -----

    /// The typed AS element this mech fields in BF — identical derivation to
    /// [`Session::sbf_element`]; the BF element IS the AS card (spec §"Data fidelity" 1).
    fn bf_element(&self) -> AsElement {
        as_element::as_element(&self.spec.as_stats, &self.spec.display_name(), self.gunnery)
    }

    /// Destroyed in Standard BF (spec §2.2): structure gone, an outright-kill crit marked
    /// ([`BfLive::killed`]), or 2 Engine hits on a destroy-at-2 column — 'Mech and Vehicle only
    /// ([`battleforce::BF_ENGINE_HITS_DESTROY`]); the aerospace 2nd Engine hit is TP 0 +
    /// shutdown, not destruction (§1.4), and infantry/BA roll no crits at all (p.42).
    pub fn bf_destroyed(&self) -> bool {
        let struct_gone = self.spec.as_stats.structure > 0 && self.as_struct_remaining() == 0;
        let engine_killed = self.bf.engine >= battleforce::BF_ENGINE_HITS_DESTROY
            && matches!(
                battleforce::bf_crit_col(&self.bf_element()),
                Some(BfCritCol::Mech | BfCritCol::Vehicle)
            );
        struct_gone || self.bf.killed.is_some() || engine_killed
    }

    /// Mark motive-damage effects (p.44): "a vehicle may only suffer each effect once per game"
    /// (p.43) — that limits repeats of the SAME effect, not combinations, so the flags are
    /// independent and stack (−1 MV and ½ MV can both be spent). Flags only ever set; marking an
    /// already-marked effect is a no-op.
    pub fn bf_mark_motive(&mut self, effect: BfMotive) {
        self.bf.motive.minus_one |= effect.minus_one;
        self.bf.motive.half |= effect.half;
        self.bf.motive.immobile |= effect.immobile;
    }

    // ----- Override -----

    /// The unit's full Override card (unit data, packed TICs, armor regions). Recomputed from the
    /// spec on demand — the conversion is deterministic, so live state stays in the hit maps.
    pub fn ov_card(&self) -> OverrideCard {
        override_conv::override_card(&self.spec)
    }

    /// The Override armor diagram regions (cheaper than [`Self::ov_card`]: no TIC packing).
    pub fn ov_regions(&self) -> Vec<ArmorRegion> {
        override_conv::override_armor(&self.spec)
    }

    /// The representative [`Location`] of each Override region — the navigable doll cells.
    pub fn ov_region_locs(&self) -> Vec<Location> {
        self.ov_regions().into_iter().map(|r| r.loc).collect()
    }

    fn ov_region(&self, loc: Location) -> Option<ArmorRegion> {
        self.ov_regions().into_iter().find(|r| r.loc == loc)
    }

    /// Remaining armor pips in a region (`0` if the region has no armor or is unknown).
    pub fn ov_armor_remaining(&self, loc: Location) -> u16 {
        let max = self.ov_region(loc).map_or(0, |r| r.armor);
        max.saturating_sub(self.ov_armor_hits.get(&loc).copied().unwrap_or(0))
    }

    /// Remaining structure pips in a region.
    pub fn ov_struct_remaining(&self, loc: Location) -> u16 {
        let max = self.ov_region(loc).map_or(0, |r| r.structure);
        max.saturating_sub(self.ov_struct_hits.get(&loc).copied().unwrap_or(0))
    }

    /// Remaining rear-armor pips (the merged 'Mech torso region only; `0` elsewhere).
    pub fn ov_rear_remaining(&self, loc: Location) -> u16 {
        let max = self.ov_region(loc).and_then(|r| r.rear).unwrap_or(0);
        max.saturating_sub(self.ov_rear_hits.get(&loc).copied().unwrap_or(0))
    }

    /// Apply one pip of Override damage to a region: front armor → structure (or, when `rear`, the
    /// torso's rear armor → structure). Damage does not transfer between regions (Override's
    /// limited-transfer model), so excess on a depleted region is simply ignored.
    pub fn ov_damage(&mut self, loc: Location, rear: bool) {
        // Each marked pip is a point of damage this phase (feeds the massive-damage PSR trigger).
        if rear {
            if self.ov_rear_remaining(loc) > 0 {
                *self.ov_rear_hits.entry(loc).or_default() += 1;
                self.damage_this_turn = self.damage_this_turn.saturating_add(1);
                return;
            }
        } else if self.ov_armor_remaining(loc) > 0 {
            *self.ov_armor_hits.entry(loc).or_default() += 1;
            self.damage_this_turn = self.damage_this_turn.saturating_add(1);
            return;
        }
        if self.ov_struct_remaining(loc) > 0 {
            *self.ov_struct_hits.entry(loc).or_default() += 1;
            self.damage_this_turn = self.damage_this_turn.saturating_add(1);
        }
    }

    /// Repair one pip in a region: structure first, then the relevant armor layer (mirror of
    /// [`Self::ov_damage`]).
    pub fn ov_repair(&mut self, loc: Location, rear: bool) {
        if let Some(n) = self.ov_struct_hits.get_mut(&loc).filter(|n| **n > 0) {
            *n -= 1;
            return;
        }
        let layer = if rear { &mut self.ov_rear_hits } else { &mut self.ov_armor_hits };
        if let Some(n) = layer.get_mut(&loc).filter(|n| **n > 0) {
            *n -= 1;
        }
    }

    /// Adjust Override heat, clamped to the 0–5 ladder.
    pub fn ov_adjust_heat(&mut self, delta: i32) {
        self.ov_heat = (self.ov_heat as i32 + delta).clamp(0, OV_HEAT_MAX as i32) as u8;
    }

    /// Override heat sinks (dissipation per turn); `None` for vehicles, which have no heat track.
    pub fn ov_sinks(&self) -> Option<u16> {
        self.ov_card().unit.sinks
    }

    /// Whether the unit shows the 0–5 heat ladder at all (mech/aero; vehicles omit heat).
    pub fn ov_has_heat(&self) -> bool {
        self.ov_card().unit.heat_scale
    }

    /// Shut down in Override: the top box on the heat ladder is **Automatic Shutdown**, or the unit
    /// was shut down by hand. Recoverable by cooling back down / restarting.
    pub fn ov_shutdown(&self) -> bool {
        self.shutdown || self.ov_heat >= OV_HEAT_SHUTDOWN
    }

    /// Mark a TIC as fired this turn and bank its heat onto the 0–5 ladder.
    pub fn ov_fire_tic(&mut self, idx: usize, heat: i32) {
        if self.ov_fired.insert(idx) {
            self.ov_adjust_heat(heat);
        }
    }

    /// Un-fire a TIC: clear its mark and refund its heat.
    pub fn ov_unfire_tic(&mut self, idx: usize, heat: i32) {
        if self.ov_fired.remove(&idx) {
            self.ov_adjust_heat(-heat);
        }
    }

    /// How many times a region's crit-table row has been marked (results stack — e.g. two actuator
    /// hits, or a second engine hit). Each `Vec` entry is one recorded hit of that row index.
    pub fn ov_crit_count(&self, loc: Location, row: u8) -> u8 {
        self.ov_crits.get(&loc).map_or(0, |v| v.iter().filter(|&&r| r == row).count() as u8)
    }

    /// Record one more hit of a region's crit-table row (capped at [`OV_CRIT_MAX`] per row).
    pub fn ov_add_crit(&mut self, loc: Location, row: u8) {
        let list = self.ov_crits.entry(loc).or_default();
        if list.iter().filter(|&&r| r == row).count() < OV_CRIT_MAX as usize {
            list.push(row);
        }
    }

    /// Remove one recorded hit of a region's crit-table row (no-op if none).
    pub fn ov_remove_crit(&mut self, loc: Location, row: u8) {
        if let Some(list) = self.ov_crits.get_mut(&loc) {
            if let Some(pos) = list.iter().position(|&r| r == row) {
                list.remove(pos);
            }
            if list.is_empty() {
                self.ov_crits.remove(&loc);
            }
        }
    }

    /// Aggregate the mechanical effects of every marked Override crit (see [`OvCritEffects`]).
    pub fn ov_crit_effects(&self) -> OvCritEffects {
        let mut e = OvCritEffects::default();
        for (&loc, rows) in &self.ov_crits {
            let Some(table) = override_conv::crit_table(&self.spec, loc) else { continue };
            for &row in rows {
                let Some(entry) = table.get(row as usize) else { continue };
                match entry.kind {
                    OvCritKind::Actuator | OvCritKind::Motive => {
                        e.move_penalty += 2;
                        e.tmm_penalty += 1;
                    }
                    OvCritKind::Engine => e.engine_hits += 1,
                    OvCritKind::Gyro => e.gyro_hits += 1,
                    OvCritKind::Weapon => e.weapon_hits += 1,
                    OvCritKind::CrewHit | OvCritKind::Cockpit => e.crew_pilot_hits += 1,
                    OvCritKind::Stunned => e.stunned = true,
                    OvCritKind::Avionics => e.avionics = true,
                    OvCritKind::Ammo => e.ammo_marked = true,
                    OvCritKind::FuelTank => e.fuel_marked = true,
                    OvCritKind::Bomb => {}
                }
            }
        }
        e
    }

    /// Condition-monitor hits that penalise attacks (+1 each): crew for vehicles, pilot otherwise.
    pub fn ov_condition_hits(&self) -> u8 {
        if self.spec.is_vehicle() {
            self.crew_hits
        } else {
            self.pilot_hits
        }
    }

    /// Override heat-to-hit modifier: +1 once heat reaches the ladder's "+1 Ranged Attack Mod" box
    /// (heat 2), else 0.
    pub fn ov_heat_to_hit(&self) -> i32 {
        i32::from(self.ov_heat >= 2)
    }

    /// The Override to-hit target number for a TIC firing into a range bracket whose printed modifier
    /// is `bracket_mod` (parse the card string with [`override_conv::parse_bracket`]; `None` = no
    /// shot). Folds in gunnery, the bracket, the shot context (attacker move + target), heat, the
    /// condition monitor, and secondary/rear arcs — floored at the 2+ minimum.
    pub fn ov_to_hit(&self, bracket_mod: i32) -> i32 {
        let s = &self.ov_shot;
        let situational = i32::from(s.secondary) + i32::from(s.rear);
        (i32::from(self.gunnery)
            + bracket_mod
            + override_conv::ov_attacker_mod(self.move_mode)
            + override_conv::ov_target_mod(s.target_tmm, s.target_jumped, s.target_immobile)
            + self.ov_heat_to_hit()
            + i32::from(self.ov_condition_hits())
            + situational)
            .max(2)
    }

    /// This-phase move / TMM penalty under Override, from heat (the ladder's heat-1 box is −2 move /
    /// −1 TMM) and stacking actuator/motive crits (−2 move / −1 TMM each). Returns `(move, tmm)`.
    pub fn ov_move_penalty(&self) -> (i32, i32) {
        let fx = self.ov_crit_effects();
        let heat_move = if self.ov_heat >= 1 { 2 } else { 0 };
        let heat_tmm = i32::from(self.ov_heat >= 1);
        (heat_move + fx.move_penalty, heat_tmm + fx.tmm_penalty)
    }

    /// Whether the shot context has been changed from its neutral default (drives the Shot summary).
    pub fn ov_shot_active(&self) -> bool {
        self.ov_shot != OvShot::default()
    }

    /// Whether a 'Mech leg region is destroyed (its structure pips are all gone). Biped legs or any
    /// quad leg; always `false` for non-'Mechs.
    pub fn ov_leg_destroyed(&self) -> bool {
        use Location::*;
        if self.spec.unit_type != UnitType::Mech {
            return false;
        }
        [LeftLeg, RightLeg, FrontLeftLeg, FrontRightLeg, RearLeftLeg, RearRightLeg, CenterLeg]
            .into_iter()
            .any(|l| self.ov_loc_destroyed(l))
    }

    /// Circumstance forcing an automatic PSR failure this phase (the unit falls / loses control),
    /// or `None`. Per the Override QRG: shutdown, unconscious, a destroyed gyro (2nd gyro crit), or
    /// a destroyed leg.
    pub fn ov_psr_auto_fail(&self) -> Option<&'static str> {
        if self.ov_shutdown() {
            Some("shutdown")
        } else if self.pilot_unconscious || self.pilot_dead() {
            Some("unconscious")
        } else if self.ov_crit_effects().gyro_hits >= 2 {
            Some("gyro destroyed")
        } else if self.ov_leg_destroyed() {
            Some("leg destroyed")
        } else {
            None
        }
    }

    /// Override PSR situations in play this phase (each a `+2` modifier): massive damage (10+ taken
    /// this phase), TMM reduced (an actuator/motive crit or a lost leg), and a damaged gyro (1st
    /// gyro crit). Physical-attack triggers (kick/charge/ram/DFA) aren't tracked, so they're not
    /// listed — the player invokes those by hand.
    pub fn ov_psr_situations(&self) -> Vec<&'static str> {
        let fx = self.ov_crit_effects();
        let mut s = Vec::new();
        if self.damage_this_turn >= 10 {
            s.push("massive damage");
        }
        if fx.move_penalty > 0 || self.ov_leg_destroyed() {
            s.push("TMM reduced");
        }
        if fx.gyro_hits == 1 {
            s.push("gyro damaged");
        }
        s
    }

    /// The 2d6 PSR target this phase: piloting + 2 per [`Self::ov_psr_situations`] + the condition
    /// monitor (+1 per pilot/crew hit), floored at 2. Only meaningful when not auto-failing.
    pub fn ov_psr_target(&self) -> i32 {
        let mods = 2 * self.ov_psr_situations().len() as i32 + i32::from(self.ov_condition_hits());
        (i32::from(self.piloting) + mods).max(2)
    }

    /// Whether a PSR is owed this phase (a situation applies or an auto-failure circumstance holds).
    pub fn ov_psr_due(&self) -> bool {
        self.ov_psr_auto_fail().is_some() || !self.ov_psr_situations().is_empty()
    }

    /// Crippled (the morale-test trigger): the core front armor is gone and its structure is ≤ 4.
    /// 'Mech → centre torso, vehicle → front; other types don't morale-test here.
    pub fn ov_crippled(&self) -> bool {
        let core = if self.spec.is_vehicle() {
            Location::Front
        } else if self.spec.unit_type == UnitType::Mech {
            Location::CenterTorso
        } else {
            return false;
        };
        self.ov_armor_remaining(core) == 0 && self.ov_struct_remaining(core) <= 4
    }

    /// The 2d6 morale-test target for a crippled unit (Override base 8, +1 if any weapon is
    /// disabled, +1 for any engine/gyro/actuator/motive crit, +1 per condition-monitor hit). The
    /// force-level commander/NCO modifiers aren't tracked per unit, so they're left to the player.
    pub fn ov_morale_target(&self) -> i32 {
        let fx = self.ov_crit_effects();
        let weapon_down = fx.weapon_hits > 0 || fx.ammo_marked;
        let system_crit = fx.move_penalty > 0 || fx.engine_hits > 0 || fx.gyro_hits > 0;
        8 + i32::from(weapon_down) + i32::from(system_crit) + i32::from(self.ov_condition_hits())
    }

    /// Whether an Override region carries any ammo bin (the merged 'Mech torso covers CT/LT/RT).
    pub fn ov_region_has_ammo(&self, loc: Location) -> bool {
        let in_region = |l: Location| {
            l == loc
                || (loc == Location::CenterTorso
                    && matches!(l, Location::LeftTorso | Location::RightTorso))
        };
        self.spec.ammo.iter().any(|b| in_region(b.location))
    }

    /// Whether a region has *live* ammo: it carries a bin and hasn't been hand-marked spent.
    pub fn ov_ammo_live(&self, loc: Location) -> bool {
        self.ov_region_has_ammo(loc) && !self.ov_ammo_spent.contains(&loc)
    }

    /// Toggle whether a region's ammo is marked spent (Override's optional ammo handling). Returns
    /// the new state (`true` = now spent). No-op for a region with no ammo.
    pub fn ov_toggle_ammo_spent(&mut self, loc: Location) -> bool {
        if !self.ov_region_has_ammo(loc) {
            return false;
        }
        if self.ov_ammo_spent.remove(&loc) {
            false
        } else {
            self.ov_ammo_spent.insert(loc);
            true
        }
    }

    /// Whether a marked ammo crit has detonated: an `Ammo`-kind crit is recorded in a region that
    /// still has live ammo. (No live ammo → the rules make it a weapon result, so no boom.) An ammo
    /// explosion destroys the unit and the pilot/crew takes 2 hits.
    pub fn ov_ammo_exploded(&self) -> bool {
        self.ov_crits.iter().any(|(&loc, rows)| {
            self.ov_ammo_live(loc)
                && override_conv::crit_table(&self.spec, loc).is_some_and(|table| {
                    rows.iter().any(|&r| {
                        table.get(r as usize).is_some_and(|e| e.kind == OvCritKind::Ammo)
                    })
                })
        })
    }

    /// Whether a region's structure is fully gone (its pips are all marked off).
    pub fn ov_loc_destroyed(&self, loc: Location) -> bool {
        self.ov_region(loc).is_some_and(|r| r.structure > 0) && self.ov_struct_remaining(loc) == 0
    }

    /// Out-of-action check for Override: pilot/crew killed on the condition monitor, a 'Mech head or
    /// torso breached to nothing, or every structure-bearing region depleted.
    pub fn ov_destroyed_reason(&self) -> Option<&'static str> {
        // An ammo explosion (ammo crit in a region with live ammo) wrecks the unit outright.
        if self.ov_ammo_exploded() {
            return Some("ammo");
        }
        if self.spec.is_vehicle() {
            if self.crew_hits >= CREW_MAX {
                return Some("crew");
            }
        } else if self.pilot_dead() {
            return Some("pilot");
        }
        if self.spec.unit_type == UnitType::Mech {
            if self.ov_loc_destroyed(Location::Head) {
                return Some("head");
            }
            if self.ov_loc_destroyed(Location::CenterTorso) {
                return Some("torso");
            }
        }
        let regions = self.ov_regions();
        let has_struct = regions.iter().any(|r| r.structure > 0);
        if has_struct && regions.iter().all(|r| self.ov_struct_remaining(r.loc) == 0) {
            return Some("structure");
        }
        None
    }

    /// Whether the unit is out of action under Override rules.
    pub fn ov_destroyed(&self) -> bool {
        self.ov_destroyed_reason().is_some()
    }

    fn loc_state(&mut self, loc: Location) -> &mut LocState {
        self.locations.entry(loc).or_default()
    }

    /// Toggle a critical slot's destroyed mark in `loc`. Returns the new state
    /// (`true` = now marked destroyed).
    pub fn toggle_crit(&mut self, loc: Location, slot: u8) -> bool {
        let set = self.crit_hits.entry(loc).or_default();
        if set.remove(&slot) {
            if set.is_empty() {
                self.crit_hits.remove(&loc);
            }
            false
        } else {
            set.insert(slot);
            true
        }
    }

    /// Whether the given crit slot is marked destroyed.
    pub fn is_crit_hit(&self, loc: Location, slot: u8) -> bool {
        self.crit_hits.get(&loc).is_some_and(|s| s.contains(&slot))
    }

    /// How many crit slots are marked destroyed in `loc`.
    pub fn crit_hits_in(&self, loc: Location) -> usize {
        self.crit_hits.get(&loc).map_or(0, BTreeSet::len)
    }

    /// Count destroyed crit slots across the whole 'Mech whose name satisfies `pred`.
    fn count_crit<F: Fn(&str) -> bool>(&self, pred: F) -> usize {
        self.crit_hits
            .iter()
            .map(|(loc, set)| {
                self.spec.crit_slots.get(loc).map_or(0, |slots| {
                    slots
                        .iter()
                        .filter(|cs| set.contains(&cs.slot) && pred(&cs.name))
                        .count()
                })
            })
            .sum()
    }

    /// Engine critical hits across the whole 'Mech. XL/Light engines spread slots into the side
    /// torsos, so this counts everywhere; the engine is destroyed at 3 hits. Matches every engine
    /// variant by name suffix (`Fusion Engine`, `XL Fusion Engine`, `I.C.E. Engine`, ...) without
    /// catching `…Re-engineered Laser`.
    pub fn engine_hits(&self) -> usize {
        self.count_crit(|n| n.ends_with("Engine"))
    }

    /// Gyro critical hits (destroyed at 2).
    pub fn gyro_hits(&self) -> usize {
        self.count_crit(|n| n.ends_with("Gyro"))
    }

    /// Extra heat generated per turn by engine damage: +5 per 'Mech engine crit, or +2 per
    /// aerospace engine hit (TW p.240, MegaMek `Aero.getEngineCritHeat`).
    pub fn engine_heat(&self) -> i32 {
        if self.spec.is_aerospace() {
            2 * self.aero_engine_hits() as i32
        } else {
            5 * self.engine_hits() as i32
        }
    }

    /// Dissipation lost to destroyed heat-sink crit slots. The slots of one physical sink
    /// share a `uid` (a double heat sink occupies 2–3 slots), so a sink counts once however
    /// many of its slots are marked; each carries its own dissipation in `hs`. Pre-v11 bakes
    /// (empty uid, hs = 0) contribute nothing.
    pub fn sink_dissipation_lost(&self) -> u16 {
        let mut seen: BTreeSet<&str> = BTreeSet::new();
        let mut lost = 0u16;
        for (loc, set) in &self.crit_hits {
            let Some(slots) = self.spec.crit_slots.get(loc) else { continue };
            for cs in slots {
                if cs.hs > 0 && set.contains(&cs.slot) && seen.insert(cs.uid.as_str()) {
                    lost += cs.hs as u16;
                }
            }
        }
        lost
    }

    /// Heat dissipated per turn right now: the sheet value minus destroyed heat sinks.
    pub fn dissipation(&self) -> u16 {
        self.spec.dissipation.saturating_sub(self.sink_dissipation_lost())
    }

    fn cockpit_destroyed(&self) -> bool {
        self.count_crit(|n| n == "Cockpit") > 0
    }

    /// Why the unit is out of action, if it is. Checked in priority order.
    pub fn destroyed_reason(&self) -> Option<&'static str> {
        if self.spec.is_vehicle() {
            return self.vehicle_destroyed_reason();
        }
        if self.spec.is_infantry() {
            return self.infantry_destroyed_reason();
        }
        if self.spec.is_aerospace() {
            // The arcs' armor spills into SI; SI gone (or the pilot dead) = the fighter is out.
            return if self.is_destroyed(Location::AeroSI) {
                Some("structural integrity gone")
            } else if self.aero_engine_hits() >= AERO_CRIT_MAX {
                Some("engine destroyed")
            } else if self.pilot_dead() {
                Some("pilot dead")
            } else {
                None
            };
        }
        if self.is_destroyed(Location::CenterTorso) {
            Some("center torso destroyed")
        } else if self.is_destroyed(Location::Head) {
            Some("head destroyed")
        } else if self.cockpit_destroyed() {
            Some("cockpit hit")
        } else if self.engine_hits() >= 3 {
            Some("engine destroyed")
        } else if self.gyro_hits() >= 2 {
            Some("gyro destroyed")
        } else if self.pilot_dead() {
            Some("pilot dead")
        } else {
            None
        }
    }

    /// Why a combat vehicle is out of action, if it is.
    /// Troopers still standing (Battle Armor: tracks with internal left; conventional
    /// infantry: remaining platoon strength). Equals the headcount on an undamaged unit.
    pub fn troopers_remaining(&self) -> u16 {
        if self.spec.unit_type == UnitType::Infantry {
            return self.internal_remaining(Location::Platoon);
        }
        Location::TROOPERS
            .iter()
            .filter(|&&l| {
                self.spec.armor.contains_key(&l) && self.internal_remaining(l) > 0
            })
            .count() as u16
    }

    /// The squad's suits in order — the Battle Armor trooper tracks this unit actually has.
    pub fn suits(&self) -> Vec<Location> {
        Location::TROOPERS
            .iter()
            .copied()
            .filter(|l| self.spec.armor.contains_key(l))
            .collect()
    }

    /// Number of suits in the squad (full headcount, alive or not).
    pub fn suit_count(&self) -> usize {
        ba_suit_count(&self.spec)
    }

    /// Whether suit `idx` is still standing (its trooper track has internal structure left).
    pub fn suit_alive(&self, idx: usize) -> bool {
        self.suits().get(idx).is_some_and(|&l| self.internal_remaining(l) > 0)
    }

    /// Position of a trooper location among the squad's suits (for syncing the firing suit to the
    /// doll cursor).
    pub fn suit_index_of(&self, loc: Location) -> Option<usize> {
        self.suits().iter().position(|&l| l == loc)
    }

    /// Select the suit to fire from (clamped to the squad size).
    pub fn set_active_suit(&mut self, idx: usize) {
        let n = self.suit_count();
        self.active_suit = if n == 0 { 0 } else { idx.min(n - 1) };
    }

    /// Shots remaining in a bin for a specific suit (0 if the suit is dead or the bin is unknown).
    pub fn suit_ammo_remaining(&self, bin_id: u32, suit: usize) -> u16 {
        if !self.suit_alive(suit) {
            return 0;
        }
        self.suit_ammo.get(&bin_id).and_then(|v| v.get(suit)).copied().unwrap_or(0)
    }

    /// Conventional-infantry damage at the current strength: the full-strength platoon damage
    /// (`spec.dpt`) scaled by surviving troopers. 0 for non-infantry or a wiped platoon. Battle
    /// Armor returns 0 here — its damage is per-suit/per-weapon, not a single platoon value.
    pub fn infantry_damage(&self) -> u16 {
        if self.spec.unit_type != UnitType::Infantry || self.spec.dpt == 0 {
            return 0;
        }
        let full = self.spec.internal.max(1);
        let remaining = self.troopers_remaining();
        ((u32::from(self.spec.dpt) * u32::from(remaining) + u32::from(full) / 2) / u32::from(full))
            as u16
    }

    fn infantry_destroyed_reason(&self) -> Option<&'static str> {
        if self.troopers_remaining() == 0 {
            Some(if self.spec.unit_type == UnitType::BattleArmor {
                "squad wiped out"
            } else {
                "platoon wiped out"
            })
        } else {
            None
        }
    }

    /// Effective infantry movement: ground MP + jump, immobile only when wiped out. (Damage
    /// doesn't slow infantry — it removes troopers/strength, which scales damage instead.)
    fn infantry_movement(&self) -> Movement {
        if self.troopers_remaining() == 0 {
            return Movement { walk: 0, run: 0, jump: 0, immobile: true, note: Some("wiped out") };
        }
        Movement {
            walk: self.spec.walk,
            run: self.spec.walk,
            jump: self.spec.jump,
            immobile: false,
            note: None,
        }
    }

    /// Effective aerospace thrust: `walk` = Safe Thrust, `run` = Max Thrust (no jump). Reduced by
    /// the heat movement penalty; immobile when its SI is gone, shut down, or the pilot is out.
    fn aero_movement(&self) -> Movement {
        let still = |note| Movement { walk: 0, run: 0, jump: 0, immobile: true, note };
        if self.is_destroyed(Location::AeroSI) {
            return still(Some("structural integrity gone"));
        }
        if self.shutdown {
            return still(Some("shutdown"));
        }
        if self.pilot_unconscious || self.pilot_dead() {
            return still(Some("pilot out"));
        }
        let engine = self.aero_engine_hits();
        if engine >= AERO_CRIT_MAX {
            return still(Some("engine destroyed"));
        }
        // Engine damage cuts Safe Thrust 2 per hit; Max Thrust is the usual ⌈1.5 × Safe⌉ off the
        // reduced value (MegaMek `Aero.getWalkMP`, `engineLoss = 2`). Undamaged we keep the baked
        // Safe/Max verbatim so the sheet reads true. Aero heat does NOT reduce thrust — it forces a
        // control roll (`aero_heat_effects().control_avoid`), surfaced on the HEAT panel.
        let (walk, run) = if engine == 0 {
            (self.spec.walk, self.spec.run)
        } else {
            let safe = self.spec.walk.saturating_sub(2 * engine);
            (safe, ((safe as u16 * 3).div_ceil(2)) as u8)
        };
        Movement {
            walk, // Safe Thrust
            run,  // Maximum Thrust
            jump: 0,
            immobile: walk == 0,
            note: if engine > 0 { Some("engine") } else { None },
        }
    }

    fn vehicle_destroyed_reason(&self) -> Option<&'static str> {
        if self.crew_hits >= CREW_MAX {
            return Some("crew killed");
        }
        if self.is_vehicle_crit("Fuel Tank") {
            return Some("fuel tank hit");
        }
        if self.is_vehicle_crit("Ammo") {
            return Some("ammo explosion");
        }
        // Every armored location's internal structure gone = wreck.
        let internal_locs: Vec<Location> = self
            .spec
            .locations()
            .into_iter()
            .filter(|l| self.spec.armor.get(l).is_some_and(|a| a.internal_max > 0))
            .collect();
        if !internal_locs.is_empty() && internal_locs.iter().all(|&l| self.is_destroyed(l)) {
            return Some("wrecked");
        }
        None
    }

    /// Effective vehicle movement: Cruise (= base walk reduced by motive damage) and Flank (= run);
    /// immobilized when wrecked / crew out / motive-immobilized. Stored in the same `Movement`
    /// (walk = Cruise, run = Flank).
    fn vehicle_movement(&self) -> Movement {
        let still = |note| Movement { walk: 0, run: 0, jump: 0, immobile: true, note };
        if self.is_mech_destroyed() {
            return still(Some("wrecked"));
        }
        if self.crew_hits >= CREW_MAX {
            return still(Some("crew out"));
        }
        if self.motive_immobilized() {
            return still(Some("immobilized"));
        }
        let cruise = self.motive_cruise();
        let flank = if cruise == 0 { 0 } else { ((cruise as u16 * 3).div_ceil(2)) as u8 };
        Movement {
            walk: cruise,
            run: flank,
            jump: self.spec.jump,
            immobile: cruise == 0 && self.spec.jump == 0,
            note: if self.motive_damage.is_empty() { None } else { Some("motive") },
        }
    }

    /// Cruise MP after motive damage: fold the table results in the order rolled (MegaMek
    /// `Tank.addMovementDamage`) — Minor no loss, Moderate −1, Heavy halves remaining (round up),
    /// Immobilized → 0.
    pub fn motive_cruise(&self) -> u8 {
        let mut mp = self.spec.walk;
        for r in &self.motive_damage {
            match r {
                MotiveLevel::Minor => {}
                MotiveLevel::Moderate => mp = mp.saturating_sub(1),
                MotiveLevel::Heavy => mp = mp.div_ceil(2),
                MotiveLevel::Immobilized => mp = 0,
            }
        }
        mp
    }

    /// Cruise MP lost to motive damage (base walk − [`Self::motive_cruise`]).
    pub fn motive_mp_lost(&self) -> u8 {
        self.spec.walk.saturating_sub(self.motive_cruise())
    }

    /// Steering (driving-roll) penalty from motive damage — each severity counts once, however many
    /// of its results landed (MegaMek `Tank` `motivePenalty`, "Steering Damage").
    pub fn motive_steering(&self) -> i32 {
        MotiveLevel::ALL
            .iter()
            .filter(|lvl| self.motive_damage.contains(lvl))
            .map(|lvl| lvl.steering())
            .sum()
    }

    /// Whether motive damage has immobilized the vehicle (an Immobilized result, or Cruise reduced
    /// to 0).
    pub fn motive_immobilized(&self) -> bool {
        self.motive_damage.contains(&MotiveLevel::Immobilized) || self.motive_cruise() == 0
    }

    /// Whether a vehicle critical result (by name from [`VEHICLE_CRITS`]) is marked.
    pub fn is_vehicle_crit(&self, name: &str) -> bool {
        VEHICLE_CRITS
            .iter()
            .position(|n| *n == name)
            .is_some_and(|i| self.vehicle_crits.contains(&(i as u8)))
    }

    /// The system crit-result list for this unit's `c` popup: aerospace → [`AEROSPACE_CRITS`]
    /// (graded hits in [`Self::aero_crit_hits`]), combat vehicles → [`VEHICLE_CRITS`] (one-shot
    /// marks in [`Self::vehicle_crits`]). Index into this drives [`Self::crit_hits_at`].
    pub fn unit_crits(&self) -> &'static [&'static str] {
        if self.spec.is_aerospace() {
            &AEROSPACE_CRITS
        } else {
            &VEHICLE_CRITS
        }
    }

    /// Most hits a system crit at `idx` can take: aerospace systems accumulate to [`AERO_CRIT_MAX`];
    /// a combat vehicle's are one-shot (1 = a plain on/off mark).
    pub fn crit_cap(&self) -> u8 {
        if self.spec.is_aerospace() { AERO_CRIT_MAX } else { 1 }
    }

    /// Hits taken by the system crit at `idx` (into [`Self::unit_crits`]). Aerospace reads the
    /// graded [`Self::aero_crit_hits`] count; a vehicle is 0/1 from [`Self::vehicle_crits`].
    pub fn crit_hits_at(&self, idx: usize) -> u8 {
        if self.spec.is_aerospace() {
            self.aero_crit_hits.get(&(idx as u8)).copied().unwrap_or(0)
        } else {
            u8::from(self.vehicle_crits.contains(&(idx as u8)))
        }
    }

    /// Whether the crit at `idx` (into [`Self::unit_crits`]) has any hit.
    pub fn crit_marked(&self, idx: usize) -> bool {
        self.crit_hits_at(idx) > 0
    }

    /// Advance the system crit at `idx` by one hit, wrapping back to 0 once past the cap (so a
    /// single key both marks and clears). Returns the new hit count.
    pub fn bump_crit(&mut self, idx: usize) -> u8 {
        let cap = self.crit_cap();
        let next = if self.crit_hits_at(idx) >= cap { 0 } else { self.crit_hits_at(idx) + 1 };
        let key = idx as u8;
        if self.spec.is_aerospace() {
            if next == 0 {
                self.aero_crit_hits.remove(&key);
            } else {
                self.aero_crit_hits.insert(key, next);
            }
        } else if next == 0 {
            self.vehicle_crits.remove(&key);
        } else {
            self.vehicle_crits.insert(key);
        }
        next
    }

    /// Hits on a named aerospace system (0 for non-aero or an unknown name).
    pub fn aero_crit(&self, name: &str) -> u8 {
        if !self.spec.is_aerospace() {
            return 0;
        }
        AEROSPACE_CRITS
            .iter()
            .position(|n| *n == name)
            .map_or(0, |i| self.crit_hits_at(i))
    }

    /// Aerospace engine hits (each −2 Thrust / +2 heat; destroyed at [`AERO_CRIT_MAX`]).
    pub fn aero_engine_hits(&self) -> u8 {
        self.aero_crit("Engine")
    }
    /// Aerospace sensor hits (to-hit penalty; destroyed at [`AERO_CRIT_MAX`]).
    pub fn aero_sensor_hits(&self) -> u8 {
        self.aero_crit("Sensors")
    }
    /// Aerospace fire-control hits (+2 to-hit each; weapons offline past 2).
    pub fn aero_fcs_hits(&self) -> u8 {
        self.aero_crit("FCS")
    }
    /// Aerospace avionics hits (control-roll penalty; +5 when destroyed).
    pub fn aero_avionics_hits(&self) -> u8 {
        self.aero_crit("Avionics")
    }

    /// Whether fire control is shot out (FCS > 2 → weapons can't fire, per MegaMek
    /// `ComputeToHitIsImpossible`).
    pub fn aero_fire_control_destroyed(&self) -> bool {
        self.aero_fcs_hits() > 2
    }

    /// Aerospace attacker to-hit penalty from sensor + fire-control damage (MegaMek
    /// `ComputeAeroAttackerToHitMods`): Sensors +N (1–2) / +5 (≥3); FCS +2 per hit. 0 for non-aero.
    pub fn aero_weapon_to_hit(&self) -> i32 {
        if !self.spec.is_aerospace() {
            return 0;
        }
        let sensors = self.aero_sensor_hits();
        let sensor_mod = if sensors >= AERO_CRIT_MAX { 5 } else { sensors as i32 };
        let fcs_mod = self.aero_fcs_hits() as i32 * 2;
        sensor_mod + fcs_mod
    }

    /// Aerospace control-roll (PSR) modifier from avionics damage (MegaMek `Aero.addEntityBonuses`):
    /// +N at 1–2 hits, +5 once destroyed. 0 for non-aero.
    pub fn aero_control_modifier(&self) -> i32 {
        let av = self.aero_avionics_hits();
        if av == 0 {
            0
        } else if av >= AERO_CRIT_MAX {
            5
        } else {
            av as i32
        }
    }

    /// Whether the weapon mount `id` has been knocked out by a weapon crit.
    pub fn is_weapon_crit(&self, id: u32) -> bool {
        self.weapon_crits.contains(&id)
    }

    /// Toggle a weapon crit by [`WeaponMount::id`]; returns its new state (`true` = destroyed).
    pub fn toggle_weapon_crit(&mut self, id: u32) -> bool {
        if self.weapon_crits.remove(&id) {
            false
        } else {
            self.weapon_crits.insert(id);
            true
        }
    }

    /// The rows shown in the `c` crit popup: the system crit-result list ([`Self::unit_crits`])
    /// plus, for aerospace fighters, one selectable row per mounted weapon (a rolled aero crit can
    /// destroy a weapon in the hit arc). Combat vehicles get system rows only.
    pub fn crit_rows(&self) -> Vec<CritRow> {
        let max = self.crit_cap();
        let mut rows: Vec<CritRow> = self
            .unit_crits()
            .iter()
            .enumerate()
            .map(|(idx, &label)| CritRow::System {
                idx,
                label,
                hits: self.crit_hits_at(idx),
                max,
            })
            .collect();
        if self.spec.is_aerospace() {
            for &arc in &Location::AEROSPACE {
                for w in self.spec.weapons.iter().filter(|w| w.location == arc) {
                    rows.push(CritRow::Weapon {
                        id: w.id,
                        label: format!("{}: {}", arc.code(), w.name),
                        destroyed: self.is_weapon_crit(w.id),
                    });
                }
            }
        }
        rows
    }

    /// Record / heal a crew hit (clamped to [`CREW_MAX`]).
    pub fn hit_crew(&mut self) {
        self.crew_hits = (self.crew_hits + 1).min(CREW_MAX);
    }
    pub fn heal_crew(&mut self) {
        self.crew_hits = self.crew_hits.saturating_sub(1);
    }

    /// Record a Motive System Damage Table result (appended in roll order).
    pub fn add_motive(&mut self, level: MotiveLevel) {
        self.motive_damage.push(level);
    }

    /// Repair the most recent motive-damage result; returns it (`None` if undamaged).
    pub fn repair_motive(&mut self) -> Option<MotiveLevel> {
        self.motive_damage.pop()
    }

    /// Whether the 'Mech is destroyed by any condition.
    pub fn is_mech_destroyed(&self) -> bool {
        self.destroyed_reason().is_some()
    }

    /// A weapon is knocked out by an explicit weapon crit, by shot-out fire control (aerospace
    /// FCS > 2, MegaMek `ComputeToHitIsImpossible`), or when any crit slot in its location bearing
    /// its name is destroyed ('Mechs/vehicles).
    pub fn is_weapon_disabled(&self, w: &WeaponMount) -> bool {
        self.weapon_crits.contains(&w.id)
            || self.aero_fire_control_destroyed()
            || self.mount_disabled(w.location, &w.name)
    }

    /// Whether a piece of equipment's crit slot has been destroyed.
    pub fn is_equipment_disabled(&self, e: &Equipment) -> bool {
        self.mount_disabled(e.location, &e.name)
    }

    /// Whether any destroyed crit slot at `loc` matches `name` (a mounted item is dead once one
    /// of its slots is hit).
    fn mount_disabled(&self, loc: Location, name: &str) -> bool {
        self.crit_hits.get(&loc).is_some_and(|set| {
            self.spec.crit_slots.get(&loc).is_some_and(|slots| {
                slots
                    .iter()
                    .any(|cs| set.contains(&cs.slot) && cs.name == name)
            })
        })
    }

    /// Destroying a location wrecks everything mounted there, so mark all of that location's crit
    /// slots. This is what makes a lost side torso take its engine slots with it. Idempotent.
    fn sync_destroyed_crits(&mut self) {
        for loc in Location::ALL {
            if !self.is_destroyed(loc) {
                continue;
            }
            let idxs: Vec<u8> = self
                .spec
                .crit_slots
                .get(&loc)
                .map(|slots| slots.iter().map(|c| c.slot).collect())
                .unwrap_or_default();
            if idxs.is_empty() {
                continue;
            }
            let set = self.crit_hits.entry(loc).or_default();
            for i in idxs {
                set.insert(i);
            }
        }
    }

    /// Force a location to destroyed (all internal hits taken).
    fn destroy_location(&mut self, loc: Location) {
        let imax = self
            .spec
            .armor
            .get(&loc)
            .map(|a| a.internal_max)
            .unwrap_or(0);
        self.loc_state(loc).internal_hits = imax;
    }

    /// Losing a side torso also destroys the arm mounted on it.
    fn destroy_attached(&mut self, loc: Location) {
        match loc {
            Location::LeftTorso => self.destroy_location(Location::LeftArm),
            Location::RightTorso => self.destroy_location(Location::RightArm),
            _ => {}
        }
    }

    /// Apply `amount` damage to a location/facing, cascading overflow inward per the transfer
    /// diagram when a location is destroyed. Returns the outcome at the *last* location touched.
    pub fn damage(&mut self, loc: Location, facing: Facing, amount: u16) -> DamageOutcome {
        let outcome = self.apply_damage_cascade(loc, facing, amount);
        // Track total damage this turn; 20+ forces a Piloting Skill Roll.
        self.damage_this_turn = self.damage_this_turn.saturating_add(amount);
        // Any location that just lost its internal structure loses its crit slots too.
        self.sync_destroyed_crits();
        outcome
    }

    fn apply_damage_cascade(&mut self, loc: Location, facing: Facing, amount: u16) -> DamageOutcome {
        let mut cur = loc;
        let mut face = facing;
        let mut amt = amount;
        loop {
            let max = self.spec.armor.get(&cur).copied().unwrap_or_default();
            let outcome = damage::apply_damage(&max, self.loc_state(cur), face, amt);
            match outcome {
                DamageOutcome::Excess(n) => {
                    self.destroy_attached(cur);
                    match damage::transfer_to(cur) {
                        // Transferred damage hits the next location head-on (front facing).
                        Some(next) => {
                            cur = next;
                            face = Facing::Front;
                            amt = n;
                        }
                        None => return DamageOutcome::Excess(n), // mech dead, excess lost
                    }
                }
                DamageOutcome::Destroyed => {
                    self.destroy_attached(cur);
                    return DamageOutcome::Destroyed;
                }
                other => return other,
            }
        }
    }

    /// Repair one or more armor points on a facing.
    pub fn repair_armor(&mut self, loc: Location, facing: Facing, amount: u16) {
        damage::repair_armor(self.loc_state(loc), facing, amount);
    }

    /// Repair one or more internal-structure points.
    pub fn repair_internal(&mut self, loc: Location, amount: u16) {
        damage::repair_internal(self.loc_state(loc), amount);
    }

    pub fn armor_remaining(&self, loc: Location, facing: Facing) -> u16 {
        let max = self.spec.armor.get(&loc).copied().unwrap_or_default();
        let st = self.locations.get(&loc).copied().unwrap_or_default();
        damage::armor_remaining(&max, &st, facing)
    }

    pub fn internal_remaining(&self, loc: Location) -> u16 {
        let max = self.spec.armor.get(&loc).copied().unwrap_or_default();
        let st = self.locations.get(&loc).copied().unwrap_or_default();
        damage::internal_remaining(&max, &st)
    }

    pub fn is_destroyed(&self, loc: Location) -> bool {
        let max = self.spec.armor.get(&loc).copied().unwrap_or_default();
        let st = self.locations.get(&loc).copied().unwrap_or_default();
        damage::is_destroyed(&max, &st)
    }

    /// Adjust heat by `delta`, clamped to >= 0.
    ///
    /// Heat 30+ forces an automatic shutdown; cooling below the lowest shutdown-check
    /// threshold (heat 14) lets the mech restart automatically. In the ambiguous 14–29 band a
    /// shutdown is a die-roll outcome, so the flag is left as-is there (toggle it manually with
    /// [`TrackedMech::toggle_shutdown`]).
    pub fn adjust_heat(&mut self, delta: i32) {
        self.heat = (self.heat + delta).max(0);
        if self.heat >= 30 {
            self.shutdown = true;
        } else if self.heat < 14 {
            self.shutdown = false;
        }
    }

    /// Manually flip the shutdown flag (voluntary shutdown, or a restart-roll result).
    pub fn toggle_shutdown(&mut self) {
        self.shutdown = !self.shutdown;
    }

    /// Apply end-of-turn heat: engine-damage heat is generated, then dissipation removed
    /// (reduced by any destroyed heat sinks).
    pub fn end_turn_heat(&mut self) {
        self.adjust_heat(self.engine_heat() - self.dissipation() as i32);
    }

    /// End the turn: resolve heat and clear the per-turn tallies (damage for PSR triggers,
    /// movement mode/hexes, and the set of weapons marked fired).
    pub fn end_turn(&mut self) {
        self.end_turn_heat();
        self.damage_this_turn = 0;
        self.move_mode = MoveMode::Stationary;
        self.hexes_moved = 0;
        self.ct_target = None;
        self.fired.clear();
        self.suit_fired.clear();
    }

    /// End the turn under Override rules: dissipate heat by sinks on the 0–5 ladder (a shut-down
    /// unit drops to 0), then clear the per-turn TIC-fired marks and movement/PSR tallies.
    pub fn ov_end_turn(&mut self) {
        if self.shutdown || self.ov_heat >= OV_HEAT_SHUTDOWN {
            self.ov_heat = 0;
        } else if let Some(sinks) = self.ov_sinks() {
            // Engine crits add heat each turn before sinks dissipate it (mech/aero only).
            let engine = i32::from(self.ov_crit_effects().engine_hits);
            self.ov_adjust_heat(engine - sinks as i32);
        }
        self.damage_this_turn = 0;
        self.move_mode = MoveMode::Stationary;
        self.hexes_moved = 0;
        self.ct_target = None;
        self.ov_fired.clear();
    }

    pub fn heat_effects(&self) -> HeatEffects {
        heat_effects(self.heat)
    }

    /// Aerospace heat effects at the current heat (control roll / to-hit / shutdown / ammo / pilot
    /// damage — see [`crate::engine::heat::aero_heat_effects`]).
    pub fn aero_heat_effects(&self) -> AeroHeatEffects {
        aero_heat_effects(self.heat)
    }

    /// Hip critical hits (each adds a hefty PSR penalty and cuts leg movement).
    pub fn hip_hits(&self) -> usize {
        self.count_crit(|n| n == "Hip")
    }

    /// Destroyed leg actuators (upper/lower leg + foot — not arm actuators).
    pub fn leg_actuator_hits(&self) -> usize {
        self.count_crit(|n| n.ends_with("Leg Actuator") || n == "Foot Actuator")
    }

    /// Number of fully destroyed legs (config-appropriate leg locations).
    pub fn destroyed_legs(&self) -> usize {
        self.spec
            .config
            .locations()
            .iter()
            .filter(|&&l| l.is_leg() && self.is_destroyed(l))
            .count()
    }

    /// Adjust the Gunnery skill by `delta`, clamped to 0..=[`SKILL_MAX`].
    pub fn adjust_gunnery(&mut self, delta: i32) {
        self.gunnery = (self.gunnery as i32 + delta).clamp(0, SKILL_MAX as i32) as u8;
    }

    /// Adjust the Piloting skill by `delta`, clamped to 0..=[`SKILL_MAX`].
    pub fn adjust_piloting(&mut self, delta: i32) {
        self.piloting = (self.piloting as i32 + delta).clamp(0, SKILL_MAX as i32) as u8;
    }

    /// This unit's skill-adjusted point cost in the given game system: skill-adjusted Battle
    /// Value for Classic, skill-adjusted Alpha Strike PV for Alpha Strike. The AS "Skill" is the
    /// single [`Self::gunnery`] value (4 = neutral); piloting is unused in Alpha Strike. Default
    /// 4/5 skills leave the baked cost unchanged.
    pub fn point_cost(&self, mode: GameMode) -> u64 {
        match mode {
            // Override uses Battle Value like Classic (per the rulebook's force-balancing note).
            GameMode::Classic | GameMode::Override => {
                skill::skill_adjusted_bv(self.spec.bv, self.gunnery, self.piloting)
            }
            // SBF shares AS point values at the element level; the derived formation PV is a
            // separate, near-but-not-identical figure surfaced in the UI (see spec §3.5).
            // Standard BF shares them too — the BF Skill PV table IS the AS one (IO:BF p.50).
            GameMode::AlphaStrike
            | GameMode::StrategicBattleForce
            | GameMode::BattleForce
            | GameMode::AbstractCombatSystem => {
                skill::skill_adjusted_pv(self.spec.as_stats.pv.into(), self.gunnery)
            }
        }
    }

    /// The 2d6 target for a Piloting Skill Roll right now: piloting skill + [`Self::psr_modifier`].
    pub fn psr_target(&self) -> i32 {
        self.piloting as i32 + self.psr_modifier()
    }

    /// The roll modifier applied to every Piloting Skill Roll, from current damage: gyro hits
    /// (+3 each), hip hits (+2 each), destroyed leg actuators (+1 each), pilot hits (+1 each), and
    /// **+1 per full 20 points of damage taken this turn** (the optional cumulative-damage PSR rule
    /// — standard Total Warfare forces a single roll at +0, but +1 per 20 is the house rule we play
    /// with; not in MekBay).
    pub fn psr_modifier(&self) -> i32 {
        3 * self.gyro_hits() as i32
            + 2 * self.hip_hits() as i32
            + self.leg_actuator_hits() as i32
            + self.pilot_hits as i32
            + (self.damage_this_turn / 20) as i32
    }

    /// Reasons a Piloting Skill Roll is owed *this turn* (empty = none pending). Covers the
    /// easy-to-forget automatic triggers: 20+ damage taken this turn, a destroyed leg, and
    /// moving hard on damaged kit (run with gyro/hip damage; jump with gyro/hip/leg-actuator
    /// damage — the landing roll).
    /// A biped that has lost a leg falls **automatically** (Total Warfare auto-fall) — there is no
    /// roll to stay upright, so this is reported separately from [`Self::psr_due`]. The fall still
    /// owes a pilot-damage PSR (at [`Self::psr_target`], with all the usual modifiers) and a
    /// stand-up PSR next turn. Quads stay up on a single leg loss, so this is biped-only. `None`
    /// when no auto-fall applies.
    pub fn auto_fall(&self) -> Option<&'static str> {
        let biped = matches!(self.spec.config, MechConfig::Biped);
        if biped && self.destroyed_legs() > 0 {
            Some("leg destroyed")
        } else {
            None
        }
    }

    /// Triggers that owe a Piloting Skill Roll **to avoid falling** this turn. A destroyed leg is
    /// not here — that's an automatic fall (see [`Self::auto_fall`]).
    pub fn psr_due(&self) -> Vec<&'static str> {
        let mut reasons = Vec::new();
        if self.damage_this_turn >= 20 {
            reasons.push("20+ dmg");
        }
        let gyro_or_hip = self.gyro_hits() > 0 || self.hip_hits() > 0;
        if self.move_mode == MoveMode::Ran && gyro_or_hip {
            reasons.push("ran w/ gyro/hip dmg");
        }
        if self.move_mode == MoveMode::Jumped && (gyro_or_hip || self.leg_actuator_hits() > 0) {
            reasons.push("jumped w/ leg dmg");
        }
        reasons
    }

    /// Cycle this turn's movement mode (the order in [`MoveMode::ALL`]), skipping modes the
    /// unit can't use: Jumped needs jump MP, and an immobile unit stays stationary. Hexes are
    /// re-clamped to the new mode's MP.
    pub fn cycle_move_mode(&mut self, delta: i32) {
        let mv = self.movement();
        if mv.immobile {
            self.move_mode = MoveMode::Stationary;
            self.hexes_moved = 0;
            return;
        }
        let all = MoveMode::ALL;
        let mut i = all.iter().position(|&m| m == self.move_mode).unwrap_or(0) as i32;
        for _ in 0..all.len() {
            i = (i + delta).rem_euclid(all.len() as i32);
            let mode = all[i as usize];
            // Jumped needs jump MP; infantry have no run, so skip Ran for them.
            let jump_ok = mode != MoveMode::Jumped || mv.jump > 0;
            let run_ok = !(mode == MoveMode::Ran && self.spec.is_infantry());
            if jump_ok && run_ok {
                self.move_mode = mode;
                break;
            }
        }
        self.hexes_moved = self.hexes_moved.min(self.max_hexes());
    }

    /// The most hexes the unit can have moved in its current mode (effective MP after heat,
    /// crits, and leg damage). Stationary means it didn't move.
    pub fn max_hexes(&self) -> u8 {
        let mv = self.movement();
        match self.move_mode {
            MoveMode::Stationary => 0,
            MoveMode::Walked => mv.walk,
            MoveMode::Ran => mv.run,
            MoveMode::Jumped => mv.jump,
        }
    }

    /// Adjust this turn's hexes-moved count, clamped to what the current mode allows.
    pub fn adjust_hexes_moved(&mut self, delta: i32) {
        let max = self.max_hexes() as i32;
        self.hexes_moved = (self.hexes_moved as i32 + delta).clamp(0, max) as u8;
    }

    /// Aerospace: adjust current velocity (in hexes), clamped to 0..=60.
    pub fn adjust_velocity(&mut self, delta: i32) {
        self.velocity = (self.velocity as i32 + delta).clamp(0, 60) as u8;
    }

    /// Aerospace: adjust current altitude level, clamped to 0..=10 (low-altitude map).
    pub fn adjust_altitude(&mut self, delta: i32) {
        self.altitude = (self.altitude as i32 + delta).clamp(0, 10) as u8;
    }

    /// The to-hit modifier this unit's *own attacks* take from how it moved this turn.
    pub fn attack_move_modifier(&self) -> i32 {
        attacker_movement_modifier(self.move_mode)
    }

    /// The Target Movement Modifier *opponents* take when shooting at this unit: from hexes
    /// moved (+1 if it jumped), or −4 while immobile.
    pub fn tmm(&self) -> i32 {
        if self.movement().immobile {
            return -4;
        }
        target_movement_modifier(self.hexes_moved, self.move_mode)
    }

    /// Current *effective* movement after heat, leg-actuator/hip crits, and leg loss. Walking MP
    /// is reduced by destroyed leg actuators (-1 each) and halved per hip hit, then the heat
    /// penalty is removed; running is recomputed (ceil 1.5× walk) only when something changed, so
    /// undamaged values keep any MASC/Supercharger bonus baked into the sheet. A biped on one leg
    /// hobbles at 1 MP; lose both (or be shut down / pilot out) and it's immobile. Jump MP is left
    /// at the sheet value unless the 'Mech can't move at all.
    pub fn movement(&self) -> Movement {
        let s = &self.spec;
        let still = |note| Movement { walk: 0, run: 0, jump: 0, immobile: true, note };
        if s.is_vehicle() {
            return self.vehicle_movement();
        }
        if s.is_infantry() {
            return self.infantry_movement();
        }
        if s.is_aerospace() {
            return self.aero_movement();
        }
        if self.is_mech_destroyed() {
            return still(Some("wrecked"));
        }
        if self.shutdown {
            return still(Some("shutdown"));
        }
        if self.pilot_unconscious || self.pilot_dead() {
            return still(Some("pilot out"));
        }
        let legs_gone = self.destroyed_legs();
        let biped = matches!(s.config, MechConfig::Biped);
        if legs_gone >= 2 || (!biped && legs_gone >= 3) {
            return still(Some("legs gone"));
        }
        if biped && legs_gone == 1 {
            return Movement { walk: 1, run: 0, jump: 0, immobile: false, note: Some("leg gone") };
        }
        let heat_pen = self.heat_effects().movement_penalty;
        let actuator = self.leg_actuator_hits() as u8;
        let hips = self.hip_hits();
        // Quad/tripod with a leg gone: treat as a flat -1 per missing leg (an approximation).
        let leg_loss = if biped { 0 } else { legs_gone as u8 };
        if heat_pen == 0 && actuator == 0 && hips == 0 && leg_loss == 0 {
            return Movement { walk: s.walk, run: s.run, jump: s.jump, immobile: false, note: None };
        }
        // Hip crits: a single hip halves Walking MP (round up); two or more zero it out
        // (MegaMek `Mech.getWalkMP` / TW). Repeated halving would wrongly leave a 'Mech mobile.
        let mut w = s.walk.saturating_sub(actuator).saturating_sub(leg_loss);
        w = match hips {
            0 => w,
            1 => w.div_ceil(2),
            _ => 0,
        };
        let w = w.saturating_sub(heat_pen);
        let run = if w == 0 { 0 } else { ((w as u16 * 3).div_ceil(2)) as u8 };
        // 0 walking MP from heat/crits isn't "immobile" (the 'Mech is still functional) — only
        // shutdown / pilot-out / lost legs set that banner.
        Movement { walk: w, run, jump: s.jump, immobile: false, note: None }
    }

    /// Fire a weapon: add its heat and, if it uses ammo, spend one shot from a compatible bin.
    /// Returns `None` if the weapon id is unknown.
    pub fn fire_weapon(&mut self, weapon_id: u32) -> Option<FireResult> {
        let w = self.spec.weapons.iter().find(|w| w.id == weapon_id)?;
        let heat = w.heat;
        let key = w.ammo_key.clone();
        self.adjust_heat(heat as i32);
        // Battle Armor fires per suit (the active suit marks this weapon); everything else uses
        // the squad-wide shot count (Ultra/Rotary fire several times).
        if self.spec.unit_type == UnitType::BattleArmor {
            self.suit_fired.entry(weapon_id).or_default().insert(self.active_suit);
        } else {
            *self.fired.entry(weapon_id).or_insert(0) += 1;
        }

        let mut result = FireResult {
            heat,
            ammo_spent: false,
            out_of_ammo: false,
        };
        if let Some(k) = key {
            let bins = self.compatible_bins(&k);
            if !bins.is_empty() {
                // Prefer the hand-chosen active bin while it still has shots; otherwise
                // fall back to the first compatible non-empty bin.
                let active = self
                    .active_bin
                    .get(&k)
                    .copied()
                    .filter(|id| bins.contains(id) && self.ammo_remaining(*id) > 0);
                let chosen =
                    active.or_else(|| bins.iter().copied().find(|&id| self.ammo_remaining(id) > 0));
                match chosen {
                    Some(id) => {
                        self.fire_ammo(id, 1);
                        result.ammo_spent = true;
                    }
                    None => result.out_of_ammo = true,
                }
            }
        }
        Some(result)
    }

    /// Shots fired from a weapon this turn (0 if none).
    pub fn shots_fired(&self, weapon_id: u32) -> u8 {
        self.fired.get(&weapon_id).copied().unwrap_or(0)
    }

    /// Whether a weapon has fired at all this turn (any suit, for Battle Armor).
    pub fn is_fired(&self, weapon_id: u32) -> bool {
        if self.spec.unit_type == UnitType::BattleArmor {
            return self.suit_fired.get(&weapon_id).is_some_and(|s| !s.is_empty());
        }
        self.shots_fired(weapon_id) > 0
    }

    /// Battle Armor: whether the active suit has already fired this weapon this turn.
    pub fn active_suit_fired(&self, weapon_id: u32) -> bool {
        self.suit_fired.get(&weapon_id).is_some_and(|s| s.contains(&self.active_suit))
    }

    /// Battle Armor: how many suits have fired this weapon this turn.
    pub fn suit_fired_count(&self, weapon_id: u32) -> usize {
        self.suit_fired.get(&weapon_id).map_or(0, BTreeSet::len)
    }

    /// Battle Armor: how many *living* suits have fired this weapon this turn (for the `✓N/M`
    /// marker, so the numerator never exceeds the living-suit denominator).
    pub fn living_suits_fired(&self, weapon_id: u32) -> usize {
        self.suit_fired
            .get(&weapon_id)
            .map_or(0, |s| s.iter().filter(|&&i| self.suit_alive(i)).count())
    }

    /// Un-fire one shot of a weapon fired this turn: drop the last shot's mark and remove the heat
    /// it added. No-op (returns `None`) if it hasn't fired. Ammo is not refunded (adjust by hand).
    /// For Battle Armor this un-marks the *active suit's* shot of the weapon.
    pub fn unfire_weapon(&mut self, weapon_id: u32) -> Option<u8> {
        if self.spec.unit_type == UnitType::BattleArmor {
            let suit = self.active_suit;
            let set = self.suit_fired.get_mut(&weapon_id)?;
            if !set.remove(&suit) {
                return None;
            }
            if set.is_empty() {
                self.suit_fired.remove(&weapon_id);
            }
        } else {
            match self.fired.get_mut(&weapon_id) {
                Some(n) if *n > 0 => {
                    *n -= 1;
                    if *n == 0 {
                        self.fired.remove(&weapon_id);
                    }
                }
                _ => return None,
            }
        }
        let heat = self.spec.weapons.iter().find(|w| w.id == weapon_id).map_or(0, |w| w.heat);
        self.adjust_heat(-(heat as i32));
        Some(heat)
    }

    /// Ids of all ammo bins whose `ammo_key` matches `key`, in spec order.
    pub fn compatible_bins(&self, key: &str) -> Vec<u32> {
        self.spec
            .ammo
            .iter()
            .filter(|b| b.ammo_key.as_deref() == Some(key))
            .map(|b| b.id)
            .collect()
    }

    /// The bin a weapon will draw from next (for display/highlight): the hand-chosen active
    /// bin if set and still compatible, else the first compatible non-empty bin, else the
    /// first compatible bin. `None` for energy weapons or weapons with no compatible bin.
    pub fn weapon_bin(&self, weapon_id: u32) -> Option<u32> {
        let key = self
            .spec
            .weapons
            .iter()
            .find(|w| w.id == weapon_id)?
            .ammo_key
            .as_deref()?;
        let bins = self.compatible_bins(key);
        if bins.is_empty() {
            return None;
        }
        self.active_bin
            .get(key)
            .copied()
            .filter(|id| bins.contains(id))
            .or_else(|| bins.iter().copied().find(|&id| self.ammo_remaining(id) > 0))
            .or_else(|| bins.first().copied())
    }

    /// How a weapon resolves on the Cluster Hits Table, given the munition currently loaded in the
    /// bin it would fire from. Energy / single-projectile weapons return [`ClusterProfile::Single`].
    /// `None` only when the weapon id isn't on this unit. Used by the dice-reference popup (§18).
    pub fn weapon_cluster_profile(&self, weapon_id: u32) -> Option<ClusterProfile> {
        let w = self.spec.weapons.iter().find(|w| w.id == weapon_id)?;
        let Some(ty) = w.ammo_type() else {
            return Some(ClusterProfile::Single);
        };
        let munition = self
            .weapon_bin(weapon_id)
            .map_or("", |bin| self.bin_munition(bin));
        Some(cluster_profile(ty, w.rack_size().unwrap_or(0), munition))
    }

    /// The ammo bin occupying a crit slot, identified by its location and display name (crit
    /// slots and bins share both). Returns the first match. `None` if the slot isn't ammo.
    pub fn bin_at(&self, loc: Location, name: &str) -> Option<u32> {
        self.spec
            .ammo
            .iter()
            .find(|b| b.location == loc && b.name == name)
            .map(|b| b.id)
    }

    /// The munition currently loaded in a bin: the hand-chosen one if set, else the bin's
    /// baked default (`"Standard"` when none was recorded). Empty string if the id is unknown.
    pub fn bin_munition(&self, bin_id: u32) -> &str {
        if let Some(m) = self.munition_choice.get(&bin_id) {
            return m;
        }
        self.spec
            .ammo
            .iter()
            .find(|b| b.id == bin_id)
            .map_or("", AmmoBin::munition_name)
    }

    /// Load `munition` into a bin (no validation; callers pass a name from the bin's catalog).
    /// Clears the override when it equals the baked default, keeping saved state minimal.
    pub fn set_bin_munition(&mut self, bin_id: u32, munition: &str) {
        let default = self
            .spec
            .ammo
            .iter()
            .find(|b| b.id == bin_id)
            .map_or("", AmmoBin::munition_name);
        if munition == default {
            self.munition_choice.remove(&bin_id);
        } else {
            self.munition_choice.insert(bin_id, munition.to_string());
        }
    }

    /// Whether `bin_id` is the hand-chosen active bin for its ammo type.
    pub fn is_active_bin(&self, bin_id: u32) -> bool {
        self.spec
            .ammo
            .iter()
            .find(|b| b.id == bin_id)
            .and_then(|b| b.ammo_key.as_deref())
            .and_then(|k| self.active_bin.get(k))
            .copied()
            == Some(bin_id)
    }

    /// Mark `bin_id` as the active bin for its ammo type. No-op if the id is unknown or the
    /// bin has no `ammo_key`. Returns the bin's display name on success.
    pub fn set_active_bin(&mut self, bin_id: u32) -> Option<String> {
        let bin = self.spec.ammo.iter().find(|b| b.id == bin_id)?;
        let key = bin.ammo_key.clone()?;
        let name = bin.name.clone();
        self.active_bin.insert(key, bin_id);
        Some(name)
    }

    /// Shots remaining in a bin.
    pub fn ammo_remaining(&self, bin_id: u32) -> u16 {
        if self.spec.unit_type == UnitType::BattleArmor {
            return self.suit_ammo_remaining(bin_id, self.active_suit);
        }
        self.ammo.get(&bin_id).copied().unwrap_or(0)
    }

    /// A bin's full capacity (0 if the id is unknown).
    pub fn ammo_max(&self, bin_id: u32) -> u16 {
        self.spec
            .ammo
            .iter()
            .find(|b| b.id == bin_id)
            .map_or(0, AmmoBin::shots_max)
    }

    /// Spend `shots` from a bin (saturating at 0). Returns shots actually spent. For Battle Armor
    /// the shots come from the active suit's copy of the bin.
    pub fn fire_ammo(&mut self, bin_id: u32, shots: u16) -> u16 {
        if self.spec.unit_type == UnitType::BattleArmor {
            let suit = self.active_suit;
            let Some(entry) = self.suit_ammo.get_mut(&bin_id).and_then(|v| v.get_mut(suit)) else {
                return 0;
            };
            let spent = shots.min(*entry);
            *entry -= spent;
            return spent;
        }
        let entry = self.ammo.entry(bin_id).or_insert(0);
        let spent = shots.min(*entry);
        *entry -= spent;
        spent
    }

    /// Adjust a bin by `delta` shots, clamped to `[0, shots_max]`.
    pub fn adjust_ammo(&mut self, bin_id: u32, delta: i32) {
        let cap = self
            .spec
            .ammo
            .iter()
            .find(|b| b.id == bin_id)
            .map(|b| b.shots_max())
            .unwrap_or(0);
        // Battle Armor adjusts the active suit's copy of the bin.
        if self.spec.unit_type == UnitType::BattleArmor {
            let suit = self.active_suit;
            if let Some(cur) = self.suit_ammo.get_mut(&bin_id).and_then(|v| v.get_mut(suit)) {
                *cur = (*cur as i32 + delta).clamp(0, cap as i32) as u16;
            }
            return;
        }
        let cur = self.ammo.entry(bin_id).or_insert(0);
        let next = (*cur as i32 + delta).clamp(0, cap as i32);
        *cur = next as u16;
    }
}

/// The whole tracking session: a roster of mechs and which one is active.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    pub version: u32,
    pub mechs: Vec<TrackedMech>,
    pub active: usize,
    /// Game system this session is played under (chosen at creation). Defaulted to Classic so
    /// sessions saved before AS support still load.
    #[serde(default)]
    pub mode: GameMode,
    /// Game-log snapshot counter — the number of times the session has been snapshotted (`L`).
    /// Purely a label for the optional log; imposes no turn/phase structure. Defaulted for old
    /// sessions.
    #[serde(default)]
    pub turn: u32,
    /// Optional force-building point ceiling in the session's own system (BV for Classic, PV for
    /// Alpha Strike). `None` = no limit. Compared against [`Self::force_total`] to flag a busted
    /// budget while adding units. Defaulted for old sessions.
    #[serde(default)]
    pub limit: Option<u64>,
    /// Alpha Strike only: play at 1:1 **ground (hex) scale** — movement and ranges shown in hexes
    /// (halved, rounded up; 2" = 1 hex) instead of inches. Display preference; defaulted off.
    #[serde(default)]
    pub as_ground_scale: bool,
    /// Strategic BattleForce grouping + live combat state. Empty/ignored unless
    /// `mode == StrategicBattleForce`. Defaulted so non-SBF sessions still load.
    #[serde(default)]
    pub sbf: SbfState,
    /// Standard BattleForce Unit (lance) grouping + round state. Empty/ignored unless
    /// `mode == BattleForce`. Defaulted so pre-BF sessions still load.
    #[serde(default)]
    pub bf: BfState,
    /// Abstract Combat System grouping + live combat state. Empty/ignored unless
    /// `mode == AbstractCombatSystem`. Defaulted so pre-ACS sessions still load.
    #[serde(default)]
    pub acs: AcsState,
}

impl Default for Session {
    fn default() -> Self {
        Session {
            version: SESSION_VERSION,
            mechs: Vec::new(),
            active: 0,
            mode: GameMode::Classic,
            turn: 0,
            limit: None,
            as_ground_scale: false,
            sbf: SbfState::default(),
            bf: BfState::default(),
            acs: AcsState::default(),
        }
    }
}

// ============================ Strategic BattleForce state ============================
// Phase 3 of docs/sbf-implementation-spec.md. Single-force (the record-sheet model): the tracker
// holds only *your* formations; the opponent is hand-entered at to-hit time (Phase 4), exactly as
// AlphaStrike/Override do. The shared `Session.mechs` pool is the AS-element store; a formation's
// units reference pool indices. Derived SBF stats (SbfUnit/SbfFormation) are recomputed on demand
// from the pool + the converter (Phase 2), never persisted — only the live counters below are.

/// SBF grouping + live combat state for a session.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SbfState {
    /// Your formations (single-force; no OpFor is tracked).
    pub formations: Vec<SbfFormationState>,
    #[serde(default)]
    pub active_formation: usize,
    /// Selected unit within the active formation.
    #[serde(default)]
    pub active_unit: usize,
    /// Round counter (advanced when every formation is done); the interleave engine is a v2 non-goal.
    #[serde(default)]
    pub round: u32,
}

/// One SBF Formation's grouping + live state. Derived stats are recomputed via [`Session::sbf_formation`].
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SbfFormationState {
    pub name: String,
    pub units: Vec<SbfUnitState>,
    /// Live morale rung (default Normal); player-set by hand (no simulated checks — see [`MoraleStatus`]).
    #[serde(default)]
    pub morale: MoraleStatus,
    /// Jump distance used this turn; §4.1 booleanizes it via `> 0`.
    #[serde(default)]
    pub jump_used_this_turn: u8,
    /// Whether this formation has activated this round.
    #[serde(default)]
    pub is_done: bool,
}

/// One SBF Unit's grouping + live combat counters — the only persisted per-unit slice.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SbfUnitState {
    pub name: String,
    /// Indices into [`Session::mechs`] (the AS elements composing this Unit).
    pub elements: Vec<usize>,
    /// Armor points lost; current armor = derived armor − `armor_hits` (AS/Override convention).
    #[serde(default)]
    pub armor_hits: u16,
    #[serde(default)]
    pub damage_crits: u8,
    #[serde(default)]
    pub targeting_crits: u8,
    #[serde(default)]
    pub mp_crits: u8,
    /// Force Commander (the COM special, IO:BF p.165) — at most one unit in the whole force.
    /// Tracked as a designation only; its Tactics-check role (+2 defender, Step 5b p.172) is a
    /// table concern the detail pane hints at.
    #[serde(default)]
    pub is_commander: bool,
    /// Formation Leader (the LEAD special) — at most one unit per formation.
    #[serde(default)]
    pub is_leader: bool,
}

/// Morale rung (IO:BF p.175 states). Manual, player-set — neurohelmet does **not** simulate morale
/// checks/recovery/triggers (decided 2026-07-03); the rung is a settable label the player advances
/// by hand off their record sheet. Four rulebook states: MegaMek ACAR's extra `Unsteady` step is a
/// simulator artifact and is deliberately omitted. Ordinals 0→3; Routed is the worst.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum MoraleStatus {
    #[default]
    Normal,
    Shaken,
    Broken,
    Routed,
}

/// A target for manually reassigning one pool element ([`Session::sbf_assign_element`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SbfAssign {
    /// Into an existing unit: `(formation index, unit index)`.
    Unit(usize, usize),
    /// Into a fresh unit appended to this formation (splitting).
    NewUnit(usize),
    /// Into a fresh single-unit formation.
    NewFormation,
    /// Out of every unit (back to the ungrouped pool).
    Unassign,
}

/// A target for manually reassigning one pool element in ACS ([`Session::acs_assign_element`]). ACS
/// nests four tiers, so each "new" variant creates the target tier and everything below it down to
/// a fresh single-element SBF Unit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AcsAssign {
    /// Into an existing SBF Unit: `(formation, combat unit, team, unit)`.
    Unit(usize, usize, usize, usize),
    /// A fresh SBF Unit in an existing Combat Team: `(formation, combat unit, team)`.
    NewUnit(usize, usize, usize),
    /// A fresh Combat Team (+ SBF Unit) in an existing Combat Unit: `(formation, combat unit)`.
    NewTeam(usize, usize),
    /// A fresh Combat Unit (+ Team + SBF Unit) in an existing Formation: `(formation)`.
    NewCombatUnit(usize),
    /// A fresh Formation (+ Combat Unit + Team + SBF Unit).
    NewFormation,
    /// Out of every SBF Unit (back to the ungrouped pool).
    Unassign,
}

/// Force-organization doctrine for auto-grouping (IO:BF p.165) — an *option*, never applied
/// implicitly; manual grouping is the primary flow.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SbfDoctrine {
    /// Lances of 4 → Companies (3 lances); Flights of 2 → Squadrons (3 flights).
    InnerSphere,
    /// Stars of 5 → Binary (2) / Trinary (3); aerospace as Flights/Squadrons.
    Clan,
    /// Level IIs of 6 → Level III (capped at 3 by the ≤20-element formation rule).
    ComStar,
}

impl MoraleStatus {
    /// One rung worse (player-marked; Routed is the floor). No roll — morale is manual (§4.3).
    pub fn worsened(self) -> Self {
        match self {
            Self::Normal => Self::Shaken,
            Self::Shaken => Self::Broken,
            Self::Broken | Self::Routed => Self::Routed,
        }
    }

    /// One rung better (player-marked; Normal is the ceiling).
    pub fn improved(self) -> Self {
        match self {
            Self::Normal | Self::Shaken => Self::Normal,
            Self::Broken => Self::Shaken,
            Self::Routed => Self::Broken,
        }
    }

    /// Display label (record-sheet wording).
    pub fn label(self) -> &'static str {
        match self {
            Self::Normal => "Normal",
            Self::Shaken => "Shaken",
            Self::Broken => "Broken",
            Self::Routed => "Routed",
        }
    }
}

impl SbfUnitState {
    /// Current armor = derived armor − hits, floored at 0.
    pub fn armor_remaining(&self, derived: &SbfUnit) -> i64 {
        (derived.armor - self.armor_hits as i64).max(0)
    }

    /// Mark `dmg` points of SBF damage: fills the armor pool (no per-element structure at unit
    /// scale) and returns the **overflow** — SBF damage spills over to another player-chosen unit
    /// rather than being discarded (§4.2 Spillover). `0` = fully absorbed; negative input returns 0.
    #[must_use = "the overflow must spill to another unit, not vanish (§4.2)"]
    pub fn apply_damage(&mut self, derived: &SbfUnit, dmg: i64) -> i64 {
        let dmg = dmg.max(0);
        let applied = dmg.min(self.armor_remaining(derived));
        self.armor_hits = self.armor_hits.saturating_add(applied as u16);
        dmg - applied
    }

    /// Repair `n` armor points (undo hits).
    pub fn repair(&mut self, n: i64) {
        self.armor_hits = self.armor_hits.saturating_sub(n.max(0) as u16);
    }

    pub fn add_damage_crit(&mut self) {
        self.damage_crits = self.damage_crits.saturating_add(1);
    }

    pub fn add_targeting_crit(&mut self) {
        self.targeting_crits = self.targeting_crits.saturating_add(1);
    }

    pub fn add_mp_crit(&mut self) {
        self.mp_crits = self.mp_crits.saturating_add(1);
    }

    /// `getCurrentDamage` = `damage.reducedBy(damage_crits)`: each crit −1 at every band, floored 0.
    pub fn current_damage(&self, derived: &SbfUnit) -> DamageVector {
        sbf::reduced_by(derived.damage, self.damage_crits)
    }

    /// `getBaseGunnery` = skill + targeting crits (`SBFUnit.java:332-334`).
    pub fn base_gunnery(&self, derived: &SbfUnit) -> i64 {
        derived.skill + self.targeting_crits as i64
    }

    /// Destroyed once no armor remains (Phase 4.2).
    pub fn is_destroyed(&self, derived: &SbfUnit) -> bool {
        self.armor_remaining(derived) == 0
    }

    /// Whether a crit roll is owed: current armor below half of full (§4.2 trigger,
    /// `StandardUnitAttackHandler.java:167`) — and the unit still alive; a destroyed unit has no
    /// crits left to take, so the UI never prompts for one.
    pub fn crit_check_due(&self, derived: &SbfUnit) -> bool {
        let remaining = self.armor_remaining(derived);
        remaining > 0 && remaining * 2 < derived.armor
    }

    /// Apply one SBF crit-table result (§4.2; the player rolls, [`sbf::sbf_crit`] reads the table,
    /// this marks it). Applies immediately — no End-Phase staging (decided 2026-07-03). `Destroyed`
    /// zeroes the armor pool, which is the destruction marker (`is_destroyed`).
    pub fn apply_crit(&mut self, derived: &SbfUnit, crit: sbf::SbfCrit) {
        match crit {
            sbf::SbfCrit::None => {}
            sbf::SbfCrit::Targeting => self.add_targeting_crit(),
            sbf::SbfCrit::Damage => self.add_damage_crit(),
            sbf::SbfCrit::Both => {
                self.add_targeting_crit();
                self.add_damage_crit();
            }
            sbf::SbfCrit::Destroyed => self.armor_hits = derived.armor.max(0) as u16,
        }
    }

    /// Current MP after Movement crits: −1 each, floored at **0** (immobile). There is no
    /// minimum-move floor of 1 — IO:BF p.166 "Minimum Movement" only applies to a unit that still
    /// has ≥1 MP (verified 2026-07-03).
    pub fn current_movement(&self, derived: &SbfUnit) -> i64 {
        (derived.movement - self.mp_crits as i64).max(0)
    }
}

impl Session {
    /// The typed AS element for pool index `i` (skill = the tracked mech's gunnery).
    fn sbf_element(&self, i: usize) -> AsElement {
        let tm = &self.mechs[i];
        as_element::as_element(&tm.spec.as_stats, &tm.spec.display_name(), tm.gunnery)
    }

    /// Derived SBF Unit for a unit state — recomputed on demand (cheap), like `ov_card`.
    pub fn sbf_unit(&self, u: &SbfUnitState) -> SbfUnit {
        let els: Vec<AsElement> = u.elements.iter().map(|&i| self.sbf_element(i)).collect();
        sbf::convert_unit(&u.name, &els)
    }

    /// Derived SBF Formation for a formation state.
    pub fn sbf_formation(&self, f: &SbfFormationState) -> SbfFormation {
        let units: Vec<SbfUnit> = f.units.iter().map(|u| self.sbf_unit(u)).collect();
        sbf::convert_formation(&f.name, &units)
    }

    /// The resolved AS elements of a formation (needed by the Phase-4 crippling test).
    pub fn sbf_formation_elements(&self, f: &SbfFormationState) -> Vec<AsElement> {
        f.units
            .iter()
            .flat_map(|u| u.elements.iter().map(|&i| self.sbf_element(i)))
            .collect()
    }

    /// The force's SBF point value: the sum of each formation's derived PV (see spec §3.5 — this
    /// differs slightly from `force_total`, which sums per-element AS PV as a forcegen budget
    /// proxy). Empty formations contribute nothing (and aren't converted).
    pub fn sbf_force_pv(&self) -> i64 {
        self.sbf
            .formations
            .iter()
            .filter(|f| !f.units.is_empty())
            .map(|f| self.sbf_formation(f).point_value)
            .sum()
    }

    /// Create a formation from a contiguous run of pool indices, auto-split into Units of ≤6 elements
    /// (a Lance/Star/Level II becomes one Unit; a Company splits into two). Returns its index.
    pub fn sbf_new_formation(&mut self, name: &str, pool: std::ops::Range<usize>) -> usize {
        let indices: Vec<usize> = pool.collect();
        let units: Vec<SbfUnitState> = indices
            .chunks(6)
            .enumerate()
            .map(|(i, chunk)| SbfUnitState {
                name: format!("{name} {}", i + 1),
                elements: chunk.to_vec(),
                ..Default::default()
            })
            .collect();
        self.sbf.formations.push(SbfFormationState {
            name: name.to_string(),
            units,
            ..Default::default()
        });
        self.sbf.formations.len() - 1
    }

    /// Rename a formation (no-op if the index is out of range).
    pub fn sbf_rename_formation(&mut self, fi: usize, name: &str) {
        if let Some(f) = self.sbf.formations.get_mut(fi) {
            f.name = name.to_string();
        }
    }

    /// Remove a formation, keeping `active_formation` in range.
    pub fn sbf_remove_formation(&mut self, fi: usize) {
        if fi < self.sbf.formations.len() {
            self.sbf.formations.remove(fi);
            self.sbf.active_formation = self.sbf.active_formation.min(self.sbf.formations.len().saturating_sub(1));
        }
    }

    /// Whether a formation is a structurally valid SBF Formation (`BaseFormationConverter.canConvert`,
    /// :77-107): 1–4 units, ≤20 total elements, 1–6 elements per unit, no `LG` in a unit of >2, no
    /// `VLG`/`SLG` in a unit of >1, ground units hold no aerospace element, and an aerospace unit's
    /// ground elements all have `SOA`/`LAM`/`BIM`. An all-aerospace formation is a Strategic
    /// Aerospace **Squadron** instead (IO:BF p.177): Flights of ≤2 elements, ≤6 Flights and ≤12
    /// elements, replacing the ground caps. The UI surfaces violations as a warning rather
    /// than hard-rejecting a hand-built force.
    pub fn sbf_can_convert(&self, f: &SbfFormationState) -> bool {
        if f.units.is_empty() {
            return false;
        }
        // SAS structure (p.177): every unit is an aerospace Flight — the NARROW As/La unit test,
        // like the formation-movement rule (spec §2.4), so `Unknown`-typed junk keeps the ground
        // caps. (Airship "Flights of 1 element" are unreachable: airship SVs bake as V/ground.)
        let aero = f.units.iter().all(|u| {
            matches!(self.sbf_unit(u).sbf_type, SbfElementType::As | SbfElementType::La)
        });
        let (max_units, max_elements, max_per_unit) = if aero { (6, 12, 2) } else { (4, 20, 6) };
        if f.units.len() > max_units {
            return false;
        }
        if f.units.iter().map(|u| u.elements.len()).sum::<usize>() > max_elements {
            return false;
        }
        for u in &f.units {
            let k = u.elements.len();
            if k == 0 || k > max_per_unit {
                return false;
            }
            let els: Vec<AsElement> = u.elements.iter().map(|&i| self.sbf_element(i)).collect();
            if k > 2 && els.iter().any(|e| e.has_sua("LG")) {
                return false;
            }
            if k > 1 && els.iter().any(|e| e.has_any_sua(&["VLG", "SLG"])) {
                return false;
            }
            let unit_ground = self.sbf_unit(u).sbf_type.is_ground();
            if unit_ground && els.iter().any(|e| e.sbf_type.is_aerospace()) {
                return false;
            }
            if !unit_ground
                && els
                    .iter()
                    .filter(|e| e.sbf_type.is_ground())
                    .any(|e| !e.has_any_sua(&["SOA", "LAM", "BIM"]))
            {
                return false;
            }
        }
        true
    }

    // ---- Grouping: manual assignment + doctrine auto-group (Phase 5 rework, 2026-07-03) ----

    /// Where a pool element currently sits: `(formation, unit)` indices, or `None` if ungrouped.
    pub fn sbf_element_assignment(&self, elem: usize) -> Option<(usize, usize)> {
        self.sbf.formations.iter().enumerate().find_map(|(fi, f)| {
            f.units
                .iter()
                .position(|u| u.elements.contains(&elem))
                .map(|ui| (fi, ui))
        })
    }

    /// Move a pool element between units: detach it from wherever it is, then attach per
    /// `target` (creating a new unit or formation as needed). Pruning of emptied
    /// units/formations is left to [`Self::sbf_prune_empty`] so callers control the timing
    /// (the grouping editor prunes immediately after each move — an empty group would render
    /// as destroyed).
    pub fn sbf_assign_element(&mut self, elem: usize, target: SbfAssign) {
        if elem >= self.mechs.len() {
            return;
        }
        for f in &mut self.sbf.formations {
            for u in &mut f.units {
                u.elements.retain(|&e| e != elem);
            }
        }
        match target {
            SbfAssign::Unit(fi, ui) => {
                if let Some(u) =
                    self.sbf.formations.get_mut(fi).and_then(|f| f.units.get_mut(ui))
                {
                    u.elements.push(elem);
                }
            }
            SbfAssign::NewUnit(fi) => {
                if let Some(f) = self.sbf.formations.get_mut(fi) {
                    let name = format!("Unit {}", f.units.len() + 1);
                    f.units.push(SbfUnitState { name, elements: vec![elem], ..Default::default() });
                }
            }
            SbfAssign::NewFormation => {
                // "Formation", not "Force": a Force is the whole side (IO:BF p.25); the
                // record-sheet hierarchy is Formation → Unit → Element.
                let n = self.sbf.formations.len() + 1;
                self.sbf.formations.push(SbfFormationState {
                    name: format!("Formation {n}"),
                    units: vec![SbfUnitState {
                        name: "Unit 1".into(),
                        elements: vec![elem],
                        ..Default::default()
                    }],
                    ..Default::default()
                });
            }
            SbfAssign::Unassign => {}
        }
    }

    /// Toggle the Force Commander (COM, IO:BF p.165) on a unit: at most one across the whole
    /// force — marking a new one clears the old; marking the current one clears it.
    pub fn sbf_set_commander(&mut self, fi: usize, ui: usize) {
        let was = self
            .sbf
            .formations
            .get(fi)
            .and_then(|f| f.units.get(ui))
            .is_some_and(|u| u.is_commander);
        for f in &mut self.sbf.formations {
            for u in &mut f.units {
                u.is_commander = false;
            }
        }
        if !was {
            if let Some(u) = self.sbf.formations.get_mut(fi).and_then(|f| f.units.get_mut(ui)) {
                u.is_commander = true;
            }
        }
    }

    /// Toggle the Formation Leader (LEAD, IO:BF p.165) on a unit: at most one per formation.
    pub fn sbf_set_leader(&mut self, fi: usize, ui: usize) {
        let Some(f) = self.sbf.formations.get_mut(fi) else { return };
        let was = f.units.get(ui).is_some_and(|u| u.is_leader);
        for u in &mut f.units {
            u.is_leader = false;
        }
        if !was {
            if let Some(u) = f.units.get_mut(ui) {
                u.is_leader = true;
            }
        }
    }

    /// Whether a formation holds the COM or LEAD unit — the defender adds +2 to the Step-5b
    /// damage-allocation Tactics roll (p.172); surfaced as a hint, rolled at the table.
    pub fn sbf_has_com_or_lead(&self, f: &SbfFormationState) -> bool {
        f.units.iter().any(|u| u.is_commander || u.is_leader)
    }

    /// Whether a unit is a drone for the p.172 "Is a Drone +1" attacker row. Derived from the
    /// elements directly: the golden-locked converter never aggregates DRO into unit SUAs
    /// (a ported MegaMek bug — Open Q 6), so the tracker checks the composition itself.
    pub fn sbf_unit_is_drone(&self, u: &SbfUnitState) -> bool {
        !u.elements.is_empty()
            && u.elements
                .iter()
                .all(|&i| i < self.mechs.len() && self.sbf_element(i).has_sua("DRO"))
    }

    /// Whether a unit is a support vehicle, for the SAS "Attacker is Support Vehicle with: … No
    /// AFC or BFC special +2" fire-control row (IO:BF p.179 Misc): true when every element is an
    /// SV. Same composition-derived shape as [`Self::sbf_unit_is_drone`] — the raw AS type never
    /// survives conversion (`SV` collapses into the `V` unit type).
    pub fn sbf_unit_is_sv(&self, u: &SbfUnitState) -> bool {
        !u.elements.is_empty()
            && u.elements
                .iter()
                .all(|&i| i < self.mechs.len() && self.sbf_element(i).as_type == "SV")
    }

    /// Drop units emptied by reassignment, keeping the cursors in range. Formations are NOT
    /// pruned — an empty formation is a first-class workspace (decided 2026-07-04): it renders
    /// as "(no units)" and stays available as a grouping target until explicitly deleted.
    pub fn sbf_prune_empty_units(&mut self) {
        for f in &mut self.sbf.formations {
            f.units.retain(|u| !u.elements.is_empty());
        }
        self.sbf.active_formation = self
            .sbf
            .active_formation
            .min(self.sbf.formations.len().saturating_sub(1));
        let units = self
            .sbf
            .formations
            .get(self.sbf.active_formation)
            .map_or(0, |f| f.units.len());
        self.sbf.active_unit = self.sbf.active_unit.min(units.saturating_sub(1));
    }

    /// Rebuild all formations from the whole pool under a force-organization doctrine
    /// (IO:BF p.165 Standard Force Organization Schemes, fitted to the SBF structural caps of
    /// ≤6 elements/unit, ≤4 units and ≤20 elements per formation). Ground and aerospace
    /// elements are never mixed (the `can_convert` rule); aerospace groups as Flights of 2 into
    /// Squadrons (ComStar: Level IIs of 6), ground per the doctrine. Discards live marks —
    /// callers should warn (the app makes it one undo step).
    pub fn sbf_group_doctrine(&mut self, doctrine: SbfDoctrine) {
        let (mut ground, mut aero): (Vec<usize>, Vec<usize>) = (Vec::new(), Vec::new());
        for i in 0..self.mechs.len() {
            if self.sbf_element(i).sbf_type.is_aerospace() {
                aero.push(i);
            } else {
                ground.push(i);
            }
        }
        self.sbf.formations.clear();

        // (unit size, unit name, formation name by unit count) per doctrine, ground arm.
        let ground_scheme: (usize, &str, fn(usize) -> String) = match doctrine {
            SbfDoctrine::InnerSphere => (4, "Lance", |k| {
                if k == 1 { "Lance".into() } else { "Company".into() }
            }),
            SbfDoctrine::Clan => (5, "Star", |k| match k {
                1 => "Star".into(),
                2 => "Binary".into(),
                _ => "Trinary".into(),
            }),
            SbfDoctrine::ComStar => (6, "Level II", |k| {
                if k == 1 { "Level II".into() } else { "Level III".into() }
            }),
        };
        let aero_scheme: (usize, &str, fn(usize) -> String) = match doctrine {
            // Flight = 2 fighters, Squadron = 3 flights (IO:BF p.165 aerospace formations).
            SbfDoctrine::InnerSphere | SbfDoctrine::Clan => (2, "Flight", |k| {
                if k == 1 { "Flight".into() } else { "Squadron".into() }
            }),
            SbfDoctrine::ComStar => (6, "Level II", |k| {
                if k == 1 { "Level II".into() } else { "Level III".into() }
            }),
        };

        let mut formation_no = 0;
        for (pool, (unit_size, unit_name, formation_name)) in
            [(ground, ground_scheme), (aero, aero_scheme)]
        {
            // Formations take up to 3 units (Company = 3-4 lances, Trinary = 3 Stars, Level III
            // capped by the ≤20-element rule; 3 is the common shape for all three doctrines).
            for chunk in pool.chunks(unit_size * 3) {
                formation_no += 1;
                let units: Vec<SbfUnitState> = chunk
                    .chunks(unit_size)
                    .enumerate()
                    .map(|(k, els)| SbfUnitState {
                        name: format!("{unit_name} {}", k + 1),
                        elements: els.to_vec(),
                        ..Default::default()
                    })
                    .collect();
                let name = format!("{} {formation_no}", formation_name(units.len()));
                self.sbf.formations.push(SbfFormationState {
                    name,
                    units,
                    ..Default::default()
                });
            }
        }
        self.sbf.active_formation = 0;
        self.sbf.active_unit = 0;
    }

    // ---- Phase 4: damage spillover, crippling, elimination (spec §4.2/§4.6) ----

    /// Apply `dmg` to the formation's units in the player-chosen `order`, chaining spillover:
    /// each unit absorbs up to its remaining armor and the overflow moves to the next (§4.2 —
    /// SBF damage carries over like TW arm→torso; it is never discarded). Returns whatever the
    /// listed units could not absorb (0 = fully placed). Out-of-range indices are skipped.
    #[must_use = "a nonzero remainder means the formation could not absorb the damage"]
    pub fn sbf_apply_damage_chain(&mut self, fi: usize, order: &[usize], dmg: i64) -> i64 {
        let mut rem = dmg.max(0);
        for &ui in order {
            if rem == 0 {
                break;
            }
            let Some(u) = self.sbf.formations.get(fi).and_then(|f| f.units.get(ui)) else {
                continue;
            };
            let derived = self.sbf_unit(u);
            rem = self.sbf.formations[fi].units[ui].apply_damage(&derived, rem);
        }
        rem
    }

    /// A formation is eliminated when its last unit is destroyed (§4.2 rollup; ACAR
    /// `EndPhase.destroyUnits:159-161`). Surfaced as state — removal stays with the player.
    /// Element-less units don't count (an empty grouping workspace is not a casualty).
    pub fn sbf_formation_eliminated(&self, f: &SbfFormationState) -> bool {
        let mut manned = f.units.iter().filter(|u| !u.elements.is_empty()).peekable();
        manned.peek().is_some() && manned.all(|u| u.is_destroyed(&self.sbf_unit(u)))
    }

    /// The SBF crippling test (§4.6; `Formation.java:213-258`, IO BETA p.242). Crippled if ANY:
    /// 1. ≥ half of all elements that had damage are reduced to zero damage (element base vector
    ///    reduced by the containing unit's `damage_crits`);
    /// 2. ≥ half of the armored non-infantry units are gutted (unit-scale approximation of the
    ///    per-element structure test — Open Question 22): `armor_remaining == 0`;
    /// 3. ≥ half of the units carry ≥2 targeting crits.
    ///
    /// Thresholds are `ceil(n/2)`; a test with an empty denominator is skipped.
    pub fn sbf_is_crippled(&self, f: &SbfFormationState) -> bool {
        let ceil_half = |n: usize| n.div_ceil(2);

        // 1: elements whose (originally nonzero) damage is now all-zero after the unit's crits.
        let mut total_elements = 0usize;
        let mut zeroed = 0usize;
        for u in &f.units {
            for &i in &u.elements {
                let base = self.sbf_element(i).std_damage;
                total_elements += 1;
                let all_zero = |v: &DamageVector| {
                    v.s == 0.0 && v.m == 0.0 && v.l.unwrap_or(0.0) == 0.0 && v.e.unwrap_or(0.0) == 0.0
                };
                if !all_zero(&base) && all_zero(&sbf::reduced_by(base, u.damage_crits)) {
                    zeroed += 1;
                }
            }
        }
        if total_elements > 0 && zeroed >= ceil_half(total_elements) {
            return true;
        }

        // 2: gutted armored non-infantry units (unit-scale approximation).
        let mut units_with_armor = 0usize;
        let mut gutted = 0usize;
        for u in &f.units {
            let derived = self.sbf_unit(u);
            if derived.armor > 0 && !matches!(derived.sbf_type, SbfElementType::Ci | SbfElementType::Ba) {
                units_with_armor += 1;
                if u.armor_remaining(&derived) == 0 {
                    gutted += 1;
                }
            }
        }
        if units_with_armor > 0 && gutted >= ceil_half(units_with_armor) {
            return true;
        }

        // 3: units with ≥2 targeting crits.
        let heavy_tgt = f.units.iter().filter(|u| u.targeting_crits >= 2).count();
        !f.units.is_empty() && heavy_tgt >= ceil_half(f.units.len())
    }

    /// Non-triggering withdrawal hint (§4.3): a Routed or crippled formation *would* withdraw
    /// under forced-withdrawal rules — except BA/CI (infantry), which are exempt. The tracker only
    /// flags it; the player decides.
    pub fn sbf_would_withdraw(&self, f: &SbfFormationState) -> bool {
        // An empty workspace formation has nothing to withdraw.
        if f.units.iter().all(|u| u.elements.is_empty()) {
            return false;
        }
        if matches!(self.sbf_formation(f).sbf_type, SbfElementType::Ci | SbfElementType::Ba) {
            return false;
        }
        f.morale == MoraleStatus::Routed || self.sbf_is_crippled(f)
    }
}

// ============================ Abstract Combat System state ============================
// Phase 2 of docs/acs-implementation-spec.md. Single-force / boardless / manual-first, exactly like
// SBF. The tracked atom is the **Combat Unit** (a single armor pool + fatigue + morale — no
// per-element crits at this scale). The grouping nests three tiers below the Combat Unit, because
// the converter's ÷3-per-tier rounding makes the tier boundaries load-bearing: pool elements →
// SBF Unit → Combat Team → Combat Unit → Formation. Only the live counters (armor_hits / fatigue /
// morale) are persisted; every derived stat is recomputed on demand from the pool + the converter.

/// One SBF Unit's worth of pool elements — the deepest ACS grouping tier (→ [`sbf::convert_unit`]).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcsUnitGrouping {
    pub name: String,
    /// Indices into [`Session::mechs`] (the AS elements composing this SBF Unit).
    pub elements: Vec<usize>,
}

/// One Combat Team = 1–4 SBF Units (→ [`acs::convert_combat_team`]).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcsTeamGrouping {
    pub name: String,
    pub units: Vec<AcsUnitGrouping>,
}

/// One Combat Unit's grouping + live combat state — the tracked atom. Derived stats recomputed via
/// [`Session::acs_combat_unit`]; only the counters below persist.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcsCombatUnitState {
    pub name: String,
    pub teams: Vec<AcsTeamGrouping>,
    /// Armor points lost; current armor = derived armor − `armor_hits` (AS/Override/SBF convention).
    #[serde(default)]
    pub armor_hits: u32,
    /// Fatigue Points ×2 (FP accrue in halves, IO:BF p.249 — stored doubled to keep serde integer).
    #[serde(default)]
    pub fatigue_points_x2: u16,
    /// Live morale rung (default Normal); player-set by hand (no simulated checks — see [`AcsMorale`]).
    #[serde(default)]
    pub morale: AcsMorale,
    /// Force Commander (the COM ability, IO:BF p.239) — at most one Combat Unit in the whole force.
    #[serde(default)]
    pub is_commander: bool,
    /// Formation Leader (the LEAD ability) — at most one Combat Unit per Formation.
    #[serde(default)]
    pub is_leader: bool,
}

/// One ACS Formation's grouping + rollup morale. Derived stats via [`Session::acs_formation`].
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcsFormationState {
    pub name: String,
    pub units: Vec<AcsCombatUnitState>,
    /// Formation-level morale rung (the rollup; player-set — the calculator uses the LEAD/COM unit).
    #[serde(default)]
    pub morale: AcsMorale,
    /// Whether this Formation has activated this round.
    #[serde(default)]
    pub is_done: bool,
}

/// ACS grouping + live combat state for a session (single-force; no OpFor is tracked).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcsState {
    pub formations: Vec<AcsFormationState>,
    #[serde(default)]
    pub active_formation: usize,
    /// Selected Combat Unit within the active Formation.
    #[serde(default)]
    pub active_unit: usize,
    #[serde(default)]
    pub round: u32,
    /// Force Leadership Rating (IO:BF p.239) — a static, player-set number feeding the Phase-3
    /// morale/adjustment readouts. 0 = unset. The live ISW economy behind it is out of v1 scope.
    #[serde(default)]
    pub leadership_rating: i64,
}

impl AcsCombatUnitState {
    /// Current armor = derived armor − hits, floored at 0.
    pub fn armor_remaining(&self, derived: &AcsCombatUnit) -> i64 {
        (derived.armor - self.armor_hits as i64).max(0)
    }

    /// Mark `dmg` points of ACS damage against the Combat Unit's armor pool and return **how many
    /// Damage Thresholds were newly crossed** by this hit (0–3) — each crossing triggers a Morale
    /// Check (§3, p.249–250). Unlike SBF there is **no spillover**: damage targets one Combat Unit
    /// and stops (a Combat Unit at 0 armor is destroyed). Negative input is a no-op.
    pub fn apply_damage(&mut self, derived: &AcsCombatUnit, dmg: i64) -> u8 {
        let dmg = dmg.max(0);
        let before = self.armor_remaining(derived);
        let applied = dmg.min(before);
        self.armor_hits = self.armor_hits.saturating_add(applied as u32);
        let after = self.armor_remaining(derived);
        derived
            .damage_thresholds
            .iter()
            .filter(|&&t| before > t && after <= t)
            .count() as u8
    }

    /// Repair `n` armor points (undo hits).
    pub fn repair(&mut self, n: i64) {
        self.armor_hits = self.armor_hits.saturating_sub(n.max(0) as u32);
    }

    /// Destroyed once no armor remains.
    pub fn is_destroyed(&self, derived: &AcsCombatUnit) -> bool {
        self.armor_remaining(derived) <= 0
    }

    /// Fatigue Points as a real number (the stored value is doubled).
    pub fn fatigue_points(&self) -> f32 {
        self.fatigue_points_x2 as f32 / 2.0
    }

    /// Add `fp` Fatigue Points (half-integers allowed; stored ×2). Negative input is a no-op.
    pub fn add_fatigue(&mut self, fp: f32) {
        if fp > 0.0 {
            self.fatigue_points_x2 = self
                .fatigue_points_x2
                .saturating_add((fp * 2.0).round() as u16);
        }
    }
}

impl Session {
    // ---- Derive-on-demand: rebuild the converter tiers from the grouping + pool (never persisted).

    /// Derived SBF Unit for the deepest grouping tier.
    fn acs_sbf_unit(&self, g: &AcsUnitGrouping) -> SbfUnit {
        let els: Vec<AsElement> = g.elements.iter().map(|&i| self.sbf_element(i)).collect();
        sbf::convert_unit(&g.name, &els)
    }

    /// Derived Combat Team (conversion Phase 2).
    pub fn acs_combat_team(&self, g: &AcsTeamGrouping) -> AcsCombatTeam {
        let units: Vec<SbfUnit> = g.units.iter().map(|u| self.acs_sbf_unit(u)).collect();
        acs::convert_combat_team(&g.name, &units)
    }

    /// Derived Combat Unit (conversion Phase 3).
    pub fn acs_combat_unit(&self, cu: &AcsCombatUnitState) -> AcsCombatUnit {
        let teams: Vec<AcsCombatTeam> = cu.teams.iter().map(|t| self.acs_combat_team(t)).collect();
        acs::convert_combat_unit(&cu.name, &teams)
    }

    /// Derived Formation (conversion Phase 4).
    pub fn acs_formation(&self, f: &AcsFormationState) -> AcsFormation {
        let units: Vec<AcsCombatUnit> = f.units.iter().map(|u| self.acs_combat_unit(u)).collect();
        acs::convert_formation_acs(&f.name, &units)
    }

    /// The force's ACS point value: the sum of each Formation's derived PV. Empty Formations (and
    /// Combat Units with no teams) contribute nothing.
    pub fn acs_force_pv(&self) -> i64 {
        self.acs
            .formations
            .iter()
            .filter(|f| !f.units.is_empty())
            .map(|f| self.acs_formation(f).point_value)
            .sum()
    }

    /// Whether an ACS Formation is aerospace (its converted type is `As`/`La`). Abstract Combat
    /// Aerospace is a v1 non-goal (see `engine/acs.rs`); the UI flags aerospace Formations as
    /// unsupported rather than silently running them through the ground converter. Empty
    /// Formations are never aerospace.
    pub fn acs_formation_is_aerospace(&self, f: &AcsFormationState) -> bool {
        !f.units.is_empty() && self.acs_formation(f).is_aerospace()
    }

    // ---- Grouping (coarse, Phase 2): build/rename/remove Formations. The fine per-element manual
    //      editor across the three sub-tiers is Phase 4 (mirrors how SBF split state vs editor).

    /// Create a Formation from a contiguous run of pool indices, auto-nested into a default
    /// hierarchy: SBF Units of ≤4 elements → Combat Teams of ≤4 Units → Combat Units of ≤3 Teams,
    /// all under one Formation. Returns its index. (ACS is regiment-scale, so realistic small pools
    /// yield a single Combat Unit; the manual editor refines the tiers.)
    pub fn acs_new_formation(&mut self, name: &str, pool: std::ops::Range<usize>) -> usize {
        let elems: Vec<usize> = pool.collect();
        let sbf_units: Vec<AcsUnitGrouping> = elems
            .chunks(4)
            .enumerate()
            .map(|(i, c)| AcsUnitGrouping {
                name: format!("{name} U{}", i + 1),
                elements: c.to_vec(),
            })
            .collect();
        let teams: Vec<AcsTeamGrouping> = sbf_units
            .chunks(4)
            .enumerate()
            .map(|(i, c)| AcsTeamGrouping {
                name: format!("Team {}", i + 1),
                units: c.to_vec(),
            })
            .collect();
        let units: Vec<AcsCombatUnitState> = teams
            .chunks(3)
            .enumerate()
            .map(|(i, c)| AcsCombatUnitState {
                name: format!("Combat Unit {}", i + 1),
                teams: c.to_vec(),
                ..Default::default()
            })
            .collect();
        self.acs.formations.push(AcsFormationState {
            name: name.to_string(),
            units,
            ..Default::default()
        });
        self.acs.formations.len() - 1
    }

    /// Rename a Formation (no-op if out of range).
    pub fn acs_rename_formation(&mut self, fi: usize, name: &str) {
        if let Some(f) = self.acs.formations.get_mut(fi) {
            f.name = name.to_string();
        }
    }

    /// Remove a Formation, keeping `active_formation` in range.
    pub fn acs_remove_formation(&mut self, fi: usize) {
        if fi < self.acs.formations.len() {
            self.acs.formations.remove(fi);
            self.acs.active_formation = self
                .acs
                .active_formation
                .min(self.acs.formations.len().saturating_sub(1));
        }
    }

    /// Drop empty SBF Units, then empty Combat Teams, then empty Combat Units; keep empty Formations
    /// (first-class workspaces — deleted explicitly). Keeps the active indices in range.
    pub fn acs_prune_empty(&mut self) {
        for f in &mut self.acs.formations {
            for cu in &mut f.units {
                for t in &mut cu.teams {
                    t.units.retain(|u| !u.elements.is_empty());
                }
                cu.teams.retain(|t| !t.units.is_empty());
            }
            f.units.retain(|cu| !cu.teams.is_empty());
        }
        self.acs.active_formation = self
            .acs
            .active_formation
            .min(self.acs.formations.len().saturating_sub(1));
        let units = self
            .acs
            .formations
            .get(self.acs.active_formation)
            .map(|f| f.units.len())
            .unwrap_or(0);
        self.acs.active_unit = self.acs.active_unit.min(units.saturating_sub(1));
    }

    /// Designate one Combat Unit as Force Commander (COM) — unique across the whole force.
    pub fn acs_set_commander(&mut self, fi: usize, ui: usize) {
        for f in &mut self.acs.formations {
            for cu in &mut f.units {
                cu.is_commander = false;
            }
        }
        if let Some(cu) = self.acs.formations.get_mut(fi).and_then(|f| f.units.get_mut(ui)) {
            cu.is_commander = true;
        }
    }

    /// Designate one Combat Unit as Formation Leader (LEAD) — unique within its Formation.
    pub fn acs_set_leader(&mut self, fi: usize, ui: usize) {
        let Some(f) = self.acs.formations.get_mut(fi) else {
            return;
        };
        for cu in &mut f.units {
            cu.is_leader = false;
        }
        if let Some(cu) = f.units.get_mut(ui) {
            cu.is_leader = true;
        }
    }

    // ---- Fine grouping editor (the manual-first flow; the four-tier analogue of sbf_assign) ----

    /// Where a pool element currently sits: `(formation, combat unit, team, unit)`, or `None` if
    /// ungrouped.
    pub fn acs_element_assignment(&self, elem: usize) -> Option<(usize, usize, usize, usize)> {
        for (fi, f) in self.acs.formations.iter().enumerate() {
            for (cui, cu) in f.units.iter().enumerate() {
                for (ti, t) in cu.teams.iter().enumerate() {
                    if let Some(ui) = t.units.iter().position(|u| u.elements.contains(&elem)) {
                        return Some((fi, cui, ti, ui));
                    }
                }
            }
        }
        None
    }

    /// Every existing SBF-Unit path, in Formation/Combat-Unit/Team/Unit order — the `←/→` cycle
    /// stops for the grouping editor. A Formation, Combat Unit or Team with nothing below it still
    /// contributes one virtual "new tier here" stop so empty workspaces stay reachable.
    pub fn acs_unit_stops(&self) -> Vec<AcsAssign> {
        let mut stops = Vec::new();
        for (fi, f) in self.acs.formations.iter().enumerate() {
            if f.units.is_empty() {
                stops.push(AcsAssign::NewCombatUnit(fi));
                continue;
            }
            for (cui, cu) in f.units.iter().enumerate() {
                if cu.teams.is_empty() {
                    stops.push(AcsAssign::NewTeam(fi, cui));
                    continue;
                }
                for (ti, t) in cu.teams.iter().enumerate() {
                    if t.units.is_empty() {
                        stops.push(AcsAssign::NewUnit(fi, cui, ti));
                        continue;
                    }
                    for ui in 0..t.units.len() {
                        stops.push(AcsAssign::Unit(fi, cui, ti, ui));
                    }
                }
            }
        }
        stops
    }

    /// Move a pool element to `target`: detach it from every SBF Unit, then attach per `target`,
    /// creating the target tier (and everything below it, down to a fresh single-element SBF Unit)
    /// as needed. Pruning of emptied tiers is left to the caller ([`Self::acs_prune_empty`]).
    pub fn acs_assign_element(&mut self, elem: usize, target: AcsAssign) {
        if elem >= self.mechs.len() {
            return;
        }
        for f in &mut self.acs.formations {
            for cu in &mut f.units {
                for t in &mut cu.teams {
                    for u in &mut t.units {
                        u.elements.retain(|&e| e != elem);
                    }
                }
            }
        }
        let leaf = |elem: usize| AcsUnitGrouping { name: "U1".into(), elements: vec![elem] };
        match target {
            AcsAssign::Unit(fi, cui, ti, ui) => {
                if let Some(u) = self
                    .acs
                    .formations
                    .get_mut(fi)
                    .and_then(|f| f.units.get_mut(cui))
                    .and_then(|cu| cu.teams.get_mut(ti))
                    .and_then(|t| t.units.get_mut(ui))
                {
                    u.elements.push(elem);
                }
            }
            AcsAssign::NewUnit(fi, cui, ti) => {
                if let Some(t) = self
                    .acs
                    .formations
                    .get_mut(fi)
                    .and_then(|f| f.units.get_mut(cui))
                    .and_then(|cu| cu.teams.get_mut(ti))
                {
                    let name = format!("U{}", t.units.len() + 1);
                    t.units.push(AcsUnitGrouping { name, elements: vec![elem] });
                }
            }
            AcsAssign::NewTeam(fi, cui) => {
                if let Some(cu) =
                    self.acs.formations.get_mut(fi).and_then(|f| f.units.get_mut(cui))
                {
                    let name = format!("Team {}", cu.teams.len() + 1);
                    cu.teams.push(AcsTeamGrouping { name, units: vec![leaf(elem)] });
                }
            }
            AcsAssign::NewCombatUnit(fi) => {
                if let Some(f) = self.acs.formations.get_mut(fi) {
                    let name = format!("Combat Unit {}", f.units.len() + 1);
                    f.units.push(AcsCombatUnitState {
                        name,
                        teams: vec![AcsTeamGrouping { name: "Team 1".into(), units: vec![leaf(elem)] }],
                        ..Default::default()
                    });
                }
            }
            AcsAssign::NewFormation => {
                let n = self.acs.formations.len() + 1;
                self.acs.formations.push(AcsFormationState {
                    name: format!("Formation {n}"),
                    units: vec![AcsCombatUnitState {
                        name: "Combat Unit 1".into(),
                        teams: vec![AcsTeamGrouping { name: "Team 1".into(), units: vec![leaf(elem)] }],
                        ..Default::default()
                    }],
                    ..Default::default()
                });
            }
            AcsAssign::Unassign => {}
        }
    }
}

impl AcsState {
    /// Start a new round: bump the counter and clear every Formation's `is_done`. Armor/fatigue/
    /// morale persist (they are the record sheet).
    pub fn begin_round(&mut self) {
        self.round += 1;
        for f in &mut self.formations {
            f.is_done = false;
        }
    }

    /// Start a new phase within the round: re-arm `is_done`.
    pub fn begin_phase(&mut self) {
        for f in &mut self.formations {
            f.is_done = false;
        }
    }
}

impl SbfState {
    // ---- Phase 4: turn/round tracker (spec §4.5, single-force — no initiative/interleave) ----

    /// Start a new round: bump the counter, clear every formation's `is_done`, reset jump.
    /// Morale/armor/crits persist (they are the record sheet).
    pub fn begin_round(&mut self) {
        self.round += 1;
        for f in &mut self.formations {
            f.is_done = false;
            f.jump_used_this_turn = 0;
        }
    }

    /// Start a new phase within the round: re-arm `is_done` (jump persists into firing for TMM,
    /// `SBFMovementProcessor.java:34`).
    pub fn begin_phase(&mut self) {
        for f in &mut self.formations {
            f.is_done = false;
        }
    }

    /// Mark one formation's activation finished this phase.
    pub fn end_turn(&mut self, fi: usize) {
        if let Some(f) = self.formations.get_mut(fi) {
            f.is_done = true;
        }
    }
}

// ============================ Standard BattleForce state ============================
// Phase 2 of docs/standard-bf-implementation-spec.md. Single-force, like every mode: the tracker
// holds only *your* elements; the opponent is hand-entered at to-hit time (Phase 3). The shared
// `Session.mechs` pool is the element roster (the per-element live state is the AS block +
// `TrackedMech.bf`); a BF Unit (lance) is only a movement grouping over pool indices. Every stat
// line is derived per frame (the `ov_card()` doctrine) — only grouping + live counters persist.

/// Standard BF Unit grouping + round state for a session (spec §2.3).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BfState {
    /// Your Units (lances; single-force — no OpFor is tracked). Ungrouped elements are legal
    /// (single-element Units are first-class in BF, p.51) and render under an implicit
    /// "Unassigned" section rather than being forced into a group.
    pub units: Vec<BfUnitState>,
    #[serde(default)]
    pub active_unit: usize,
    /// Round counter, advanced by `n` ([`Session::bf_begin_round`]).
    #[serde(default)]
    pub round: u32,
}

/// One BF Unit's (lance's) grouping state — the printed sheet's per-Unit wrapper (p.322).
/// Live Unit MV is derived per frame ([`battleforce::bf_unit_mv`], Phase 3); only what the
/// player writes on the sheet persists.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BfUnitState {
    pub name: String,
    /// Indices into [`Session::mechs`] (the pool IS the roster — the SBF precedent).
    pub elements: Vec<usize>,
    /// Static Unit Size (p.53): jround-mean of element sizes, stamped at grouping time —
    /// "determined at the start of play, and is not adjusted for destroyed Elements" — and
    /// restamped only on membership edits ([`Session::bf_restamp_sizes`]).
    #[serde(default)]
    pub size: i64,
    /// Manual morale rung (per the SBF morale ruling; IO:BF pp.97–99 checks stay at the table).
    #[serde(default)]
    pub morale: BfMorale,
    /// The printed sheet's per-Unit Notes field (p.322).
    #[serde(default)]
    pub notes: String,
}

/// Standard BF per-Unit morale rung (IO:BF pp.97–99): Normal / Broken / Routed. Manual,
/// player-set — neurohelmet does not simulate morale checks/recovery (the SBF [`MoraleStatus`]
/// ruling inherited; the check tables live in the spec's Appendix A). Unlike SBF's four-rung
/// ladder, BF prints three states; the `m` key cycles the rung (spec §3.3).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum BfMorale {
    #[default]
    Normal,
    Broken,
    Routed,
}

impl BfMorale {
    /// The next rung on the `m`-key cycle: Normal → Broken → Routed → Normal (spec §3.3).
    pub fn cycled(self) -> Self {
        match self {
            Self::Normal => Self::Broken,
            Self::Broken => Self::Routed,
            Self::Routed => Self::Normal,
        }
    }

    /// Display label (record-sheet wording).
    pub fn label(self) -> &'static str {
        match self {
            Self::Normal => "Normal",
            Self::Broken => "Broken",
            Self::Routed => "Routed",
        }
    }
}

/// A target for manually reassigning one pool element ([`Session::bf_assign_element`]) — the
/// [`SbfAssign`] shape with one level less nesting (BF has no Formation tier).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BfAssign {
    /// Into an existing unit (index into [`BfState::units`]).
    Unit(usize),
    /// Into a fresh unit appended to the sheet.
    NewUnit,
    /// Out of every unit (back to the implicit "Unassigned" section).
    Unassign,
}

impl Session {
    // ---- Standard BF: per-element live readouts (spec §2.2) ----
    // Computed each frame from the pool + the Phase-1 rules, never stored (the `ov_card()`
    // doctrine). All take a pool index into `Session.mechs`.

    /// Live available MP (aerospace: TP) for pool element `i`: base hexes (AS inches ÷ 2 —
    /// aerospace thrust passes through unconverted) through [`battleforce::bf_current_mp`] with
    /// the element's heat, accumulated MP-crit loss, motive flags, TSM (detected from the
    /// element's specials — the movement effect is not pre-baked into AS stats, spec §1.2), and
    /// the Engine-crit hit count + crit column — the vehicle MV halving and aero thrust loss
    /// derive live from `bf.engine`, never from an `mp_lost` snapshot (§1.4, as built
    /// 2026-07-05).
    pub fn bf_current_mp(&self, i: usize) -> u32 {
        let el = self.sbf_element(i);
        let tm = &self.mechs[i];
        let base = if battleforce::bf_is_aero(&el) {
            el.primary_move
        } else {
            inches_to_hexes(el.primary_move)
        };
        battleforce::bf_current_mp(
            base,
            tm.as_heat,
            tm.bf.mp_lost,
            tm.bf.motive,
            el.has_sua("TSM"),
            tm.bf.engine,
            battleforce::bf_crit_col(&el),
        )
    }

    /// Live TMM for pool element `i`: the Available-MP bracket table over
    /// [`Self::bf_current_mp`] (p.347 quick-ref; basis p.86 fn1). Ground only — the bracket
    /// "does not apply to aerospace Elements"; Phase 3 renders the airborne-aero target-type
    /// rows instead (§1.3). Immobile is not a TMM: it is the flat −4 to-hit row.
    pub fn bf_live_tmm(&self, i: usize) -> i32 {
        battleforce::bf_tmm(self.bf_current_mp(i))
    }

    /// Live damage at a bracket for pool element `i`: [`battleforce::bf_shot_damage`] with a
    /// zero OV commit — −1 per Weapon crit, ground E derived as L−1, then the vehicle
    /// Engine-crit halving (1st hit: all damage values × 0.5, round down, min 0 — §1.4) after
    /// the Weapon-crit subtraction. The shot modal's previews go through the same engine leg,
    /// so the card and the modal can never disagree. `None` = no attack at this bracket.
    pub fn bf_current_damage(&self, i: usize, range: BfRange) -> Option<f32> {
        let el = self.sbf_element(i);
        let tm = &self.mechs[i];
        battleforce::bf_shot_damage(&el, range, tm.bf.weapon, 0, tm.bf.engine)
    }

    /// Shut down for pool element `i` — the shared AS heat scale's S box ([`TrackedMech::as_shutdown`];
    /// BF heat is the same 0–4/S ladder, p.26). A shutdown member also pins its Unit (p.49) — a
    /// Phase-3 badge, not an MP change.
    pub fn bf_shutdown(&self, i: usize) -> bool {
        self.mechs[i].as_shutdown()
    }

    /// Apply one MP crit to pool element `i` (p.43): remove half of CURRENT MP, rounded
    /// normally, minimum 1 lost — computed now from the live MP and accumulated into
    /// [`BfLive::mp_lost`] (multiplicative at apply time, never `count × k` — spec §1.2).
    pub fn bf_apply_mp_crit(&mut self, i: usize) {
        let loss = battleforce::bf_mp_crit_loss(self.bf_current_mp(i));
        if let Some(tm) = self.mechs.get_mut(i) {
            tm.bf.mp_lost = tm.bf.mp_lost.saturating_add(loss);
        }
    }

    // ---- Standard BF: Unit grouping ops (spec §2.3; the sbf_* family, one tier flatter) ----

    /// Create a Unit from a contiguous run of pool indices and stamp its static Size. No
    /// auto-split: BF Unit capacity (ground 4/5/6 by doctrine, aero 2, advanced hard cap 9) is
    /// advisory — understrength and oversize Units are the player's call (spec §1.7). Returns
    /// the new unit's index.
    pub fn bf_new_unit(&mut self, name: &str, pool: std::ops::Range<usize>) -> usize {
        self.bf.units.push(BfUnitState {
            name: name.to_string(),
            elements: pool.collect(),
            ..Default::default()
        });
        let ui = self.bf.units.len() - 1;
        self.bf_restamp_sizes();
        ui
    }

    /// Rename a Unit (no-op if the index is out of range).
    pub fn bf_rename_unit(&mut self, ui: usize, name: &str) {
        if let Some(u) = self.bf.units.get_mut(ui) {
            u.name = name.to_string();
        }
    }

    /// Remove a Unit, keeping `active_unit` in range. Its elements return to the implicit
    /// "Unassigned" section (they live in the pool; only the grouping is dropped).
    pub fn bf_remove_unit(&mut self, ui: usize) {
        if ui < self.bf.units.len() {
            self.bf.units.remove(ui);
            self.bf.active_unit = self.bf.active_unit.min(self.bf.units.len().saturating_sub(1));
        }
    }

    /// Where a pool element currently sits: its unit index, or `None` if unassigned.
    pub fn bf_element_assignment(&self, elem: usize) -> Option<usize> {
        self.bf.units.iter().position(|u| u.elements.contains(&elem))
    }

    /// Move a pool element between Units: detach it from wherever it is, then attach per
    /// `target` (creating a new unit as needed). Unit Sizes are restamped — a membership edit
    /// is exactly what invalidates the static stamp (p.53). Pruning of emptied units is left to
    /// [`Self::bf_prune_empty_units`] so callers control the timing (the SBF precedent).
    pub fn bf_assign_element(&mut self, elem: usize, target: BfAssign) {
        if elem >= self.mechs.len() {
            return;
        }
        for u in &mut self.bf.units {
            u.elements.retain(|&e| e != elem);
        }
        match target {
            BfAssign::Unit(ui) => {
                if let Some(u) = self.bf.units.get_mut(ui) {
                    u.elements.push(elem);
                }
            }
            BfAssign::NewUnit => {
                let name = format!("Unit {}", self.bf.units.len() + 1);
                self.bf.units.push(BfUnitState {
                    name,
                    elements: vec![elem],
                    ..Default::default()
                });
            }
            BfAssign::Unassign => {}
        }
        self.bf_restamp_sizes();
    }

    /// Drop Units emptied by reassignment, keeping the cursor in range (the SBF
    /// `sbf_prune_empty_units` mirror — the grouping editor prunes after each move).
    pub fn bf_prune_empty_units(&mut self) {
        self.bf.units.retain(|u| !u.elements.is_empty());
        self.bf.active_unit = self.bf.active_unit.min(self.bf.units.len().saturating_sub(1));
    }

    /// Restamp every Unit's static Size from its current membership (p.53: Size is fixed at
    /// grouping time and "not adjusted for destroyed Elements" — destruction never lands here,
    /// only membership edits do). Idempotent for untouched Units, so restamping all is safe.
    fn bf_restamp_sizes(&mut self) {
        for ui in 0..self.bf.units.len() {
            let sizes: Vec<u8> = self.bf.units[ui]
                .elements
                .iter()
                .map(|&i| self.mechs[i].spec.as_stats.size)
                .collect();
            self.bf.units[ui].size = battleforce::bf_unit_size(&sizes);
        }
    }

    /// Rebuild all Units from the whole pool under a force-organization doctrine — an option,
    /// never applied implicitly (manual grouping is the primary flow). Ground Units per the
    /// doctrine (IS Lances of 4 / Clan Stars of 5 / CS-WoB Level IIs of 6 — the printed ground
    /// sheets' capacities, pp.322–324); aerospace pairs off at 2 per Unit (Air Lance / Point,
    /// p.325; Force Distribution p.51 — spec §1.7), never mixed with ground (p.52). Discards
    /// hand-built grouping (morale/notes included) — callers warn first (the itemized
    /// destructive-regroup confirmation, the SBF precedent).
    pub fn bf_group_doctrine(&mut self, doctrine: SbfDoctrine) {
        let (mut ground, mut aero): (Vec<usize>, Vec<usize>) = (Vec::new(), Vec::new());
        for i in 0..self.mechs.len() {
            if battleforce::bf_is_aero(&self.sbf_element(i)) {
                aero.push(i);
            } else {
                ground.push(i);
            }
        }
        self.bf.units.clear();

        let (ground_size, ground_name) = match doctrine {
            SbfDoctrine::InnerSphere => (4, "Lance"),
            SbfDoctrine::Clan => (5, "Star"),
            SbfDoctrine::ComStar => (6, "Level II"),
        };
        // The aerospace sheet holds 2 per Unit for all three doctrines (p.51, p.325).
        let aero_name = match doctrine {
            SbfDoctrine::InnerSphere | SbfDoctrine::ComStar => "Air Lance",
            SbfDoctrine::Clan => "Point",
        };
        for (pool, unit_size, unit_name) in
            [(ground, ground_size, ground_name), (aero, 2, aero_name)]
        {
            for (k, chunk) in pool.chunks(unit_size).enumerate() {
                self.bf.units.push(BfUnitState {
                    name: format!("{unit_name} {}", k + 1),
                    elements: chunk.to_vec(),
                    ..Default::default()
                });
            }
        }
        self.bf_restamp_sizes();
        self.bf.active_unit = 0;
    }

    /// Start a new BF round (`n`): bump the counter and clear every element's Crew Stunned flag
    /// — the one piece of turn-scoped BF state (spec §2.3; the SBF `begin_round` precedent).
    /// Everything else (armor/heat/crits/morale) persists — it is the record sheet.
    pub fn bf_begin_round(&mut self) {
        self.bf.round += 1;
        for tm in &mut self.mechs {
            tm.bf.crew_stunned = false;
        }
    }
}

impl Session {
    pub fn new() -> Self {
        Session::default()
    }

    /// A fresh session under the given game system. SBF sessions start with one empty formation
    /// on the sheet (like a blank Formation Record Sheet) — elements added later are assigned
    /// into it via the grouping editor. BattleForce sessions likewise start with one empty Unit
    /// (the blank ground record sheet's first Unit wrapper, p.322).
    pub fn new_with_mode(mode: GameMode) -> Self {
        let mut s = Session {
            mode,
            ..Session::default()
        };
        if mode == GameMode::StrategicBattleForce {
            s.sbf.formations.push(SbfFormationState {
                name: "Formation 1".into(),
                ..Default::default()
            });
        }
        if mode == GameMode::BattleForce {
            s.bf.units.push(BfUnitState {
                name: "Unit 1".into(),
                ..Default::default()
            });
        }
        if mode == GameMode::AbstractCombatSystem {
            s.acs.formations.push(AcsFormationState {
                name: "Formation 1".into(),
                ..Default::default()
            });
        }
        s
    }

    /// The force's point total in this session's game system: skill-adjusted Battle Value summed
    /// for Classic, skill-adjusted Alpha Strike PV summed for AS (see [`TrackedMech::point_cost`]).
    /// Default 4/5 skills leave each unit at its baked cost; 0-valued units (pre-v12 specs) add
    /// nothing.
    pub fn force_total(&self) -> u64 {
        self.mechs.iter().map(|m| m.point_cost(self.mode)).sum()
    }

    /// Whether the force is over its point limit (always `false` when no limit is set).
    pub fn over_limit(&self) -> bool {
        matches!(self.limit, Some(l) if self.force_total() > l)
    }

    /// Points left under the limit (negative if busted); `None` when no limit is set.
    pub fn remaining(&self) -> Option<i64> {
        self.limit.map(|l| l as i64 - self.force_total() as i64)
    }

    /// Migrate a session loaded from disk to the current data format: refresh each tracked
    /// mech's immutable `spec` from the current `bundle` (matched by display name), pulling in
    /// baked fields added since the session was saved (equipment, munition data, …) while
    /// preserving all live play state (damage, heat, ammo counts, crits, pilot, AS, choices).
    ///
    /// Live state is keyed by stable ids/locations (ammo bins keep their ids across bakes; crit
    /// slots come from the unchanged record sheet), so a whole-spec swap is safe; any bin that's
    /// new to the refreshed spec is initialized to full. A mech whose unit is no longer in the
    /// bundle keeps its old spec. Returns the number of specs actually changed.
    pub fn relink_specs(&mut self, bundle: &Bundle) -> usize {
        let mut updated = 0;
        for tm in &mut self.mechs {
            let name = tm.spec.display_name();
            let Some(fresh) = bundle.mechs.iter().find(|m| m.display_name() == name) else {
                continue;
            };
            if *fresh != tm.spec {
                tm.spec = fresh.clone();
                updated += 1;
            }
            // A bin new to the refreshed spec has no live entry yet — start it full.
            for b in &tm.spec.ammo {
                tm.ammo.entry(b.id).or_insert_with(|| b.shots_max());
            }
            // Battle Armor: seed each suit's copy of any bin missing one (e.g. a session saved
            // before BA weapons/ammo were baked — see the squad-wide-location fix), and re-fit the
            // per-suit length if a re-bake changed the squad size.
            if tm.spec.unit_type == UnitType::BattleArmor {
                let suits = ba_suit_count(&tm.spec);
                let bins: Vec<(u32, u16)> =
                    tm.spec.ammo.iter().map(|b| (b.id, b.shots_max())).collect();
                for (id, max) in bins {
                    let v = tm.suit_ammo.entry(id).or_insert_with(|| vec![max; suits]);
                    if v.len() != suits {
                        v.resize(suits, max);
                    }
                }
            }
        }
        // A hand-edited / older session could carry an out-of-range active index; keep it valid so
        // the roster never indexes past the end.
        if !self.mechs.is_empty() {
            self.active = self.active.min(self.mechs.len() - 1);
        }
        self.version = SESSION_VERSION;
        updated
    }

    /// The roster cap for a game mode: `Some(MAX_MECHS)` for Classic/Override, `None` (uncapped) for
    /// Alpha Strike, which is played at much larger scale.
    pub fn mech_cap(mode: GameMode) -> Option<usize> {
        match mode {
            GameMode::AlphaStrike
            | GameMode::StrategicBattleForce
            | GameMode::BattleForce
            | GameMode::AbstractCombatSystem => None,
            GameMode::Classic | GameMode::Override => Some(MAX_MECHS),
        }
    }

    /// Add a mech to the roster (subject to [`Session::mech_cap`]) and make it active. Returns false
    /// if the mode's cap is reached.
    pub fn add_mech(&mut self, spec: Mech) -> bool {
        if Self::mech_cap(self.mode).is_some_and(|cap| self.mechs.len() >= cap) {
            return false;
        }
        self.mechs.push(TrackedMech::new(spec));
        self.active = self.mechs.len() - 1;
        true
    }

    /// Remove the mech at `idx`, keeping `active` valid. SBF and BF groupings reference the pool
    /// by index, so every `SbfUnitState.elements` / `BfUnitState.elements` entry is remapped (the
    /// removed element is dropped, higher indices shift down); units and formations emptied by
    /// the removal are dropped too — otherwise every grouping consumer would read (or panic on)
    /// the wrong mech.
    pub fn remove_mech(&mut self, idx: usize) {
        if idx >= self.mechs.len() {
            return;
        }
        self.mechs.remove(idx);
        if self.active >= self.mechs.len() {
            self.active = self.mechs.len().saturating_sub(1);
        }
        for f in &mut self.sbf.formations {
            for u in &mut f.units {
                u.elements.retain(|&e| e != idx);
                for e in &mut u.elements {
                    if *e > idx {
                        *e -= 1;
                    }
                }
            }
        }
        // Emptied units go; emptied formations stay (first-class empty workspaces — the player
        // deletes formations explicitly with D).
        self.sbf_prune_empty_units();
        // The same walk for the BF lance groupings (spec §2.3 — the SBF review finding must not
        // recur): remap, drop emptied units, restamp Sizes for the membership change.
        for u in &mut self.bf.units {
            u.elements.retain(|&e| e != idx);
            for e in &mut u.elements {
                if *e > idx {
                    *e -= 1;
                }
            }
        }
        self.bf_prune_empty_units();
        self.bf_restamp_sizes();
        // ACS grouping nests three tiers below the Combat Unit; remap element indices at the deepest
        // (SBF Unit) tier, then prune empties up the tiers (same review finding as SBF/BF).
        for f in &mut self.acs.formations {
            for cu in &mut f.units {
                for t in &mut cu.teams {
                    for u in &mut t.units {
                        u.elements.retain(|&e| e != idx);
                        for e in &mut u.elements {
                            if *e > idx {
                                *e -= 1;
                            }
                        }
                    }
                }
            }
        }
        self.acs_prune_empty();
    }

    pub fn active_mech(&self) -> Option<&TrackedMech> {
        self.mechs.get(self.active)
    }

    pub fn active_mech_mut(&mut self) -> Option<&mut TrackedMech> {
        self.mechs.get_mut(self.active)
    }

    /// Cycle the active mech by `delta` (wrapping).
    pub fn switch(&mut self, delta: i32) {
        if self.mechs.is_empty() {
            return;
        }
        let n = self.mechs.len() as i32;
        self.active = (((self.active as i32 + delta) % n + n) % n) as usize;
    }
}

/// Load a session from disk; `Ok(None)` if the file does not exist.
pub fn load(path: &Path) -> Result<Option<Session>> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Save a session atomically (temp file + rename) so a crash mid-write can't corrupt it.
pub fn save(path: &Path, session: &Session) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_vec_pretty(session)?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, &json)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

// ---------- named sessions ----------

/// Where all session/log data lives. `NEUROHELMET_DIR` overrides it (used to relocate the data
/// directory, and by tests to avoid touching the real one); otherwise `<data_dir>/neurohelmet`.
///
/// Back-compat with the pre-rename `mechdoll` name: the legacy `MECHDOLL_DIR` env is still honored,
/// and if the new default dir doesn't exist yet but the old `<data_dir>/mechdoll` does, that legacy
/// location is used — so data written before the rename keeps loading without a manual move.
pub fn neurohelmet_dir() -> PathBuf {
    // Explicit override wins: new env name, then the pre-rename name.
    if let Some(dir) =
        std::env::var_os("NEUROHELMET_DIR").or_else(|| std::env::var_os("MECHDOLL_DIR"))
    {
        return PathBuf::from(dir);
    }
    let base = dirs::data_dir().unwrap_or_else(|| PathBuf::from("."));
    pick_data_dir(base.join("neurohelmet"), base.join("mechdoll"), |p| p.exists())
}

/// Pure resolution of the default data dir: prefer the current (`neurohelmet`) location, but fall
/// back to the legacy (`mechdoll`) one when the current doesn't exist yet and the legacy does — so a
/// pre-rename install's data keeps loading. Split out (no env, no real fs) so it's unit-testable.
fn pick_data_dir(current: PathBuf, legacy: PathBuf, exists: impl Fn(&Path) -> bool) -> PathBuf {
    if !exists(&current) && exists(&legacy) {
        legacy
    } else {
        current
    }
}

/// Directory holding all named session files.
pub fn sessions_dir() -> PathBuf {
    neurohelmet_dir().join("sessions")
}

/// The name of the most recently active session is remembered here.
fn pointer_file() -> PathBuf {
    neurohelmet_dir().join("current")
}

/// Make a filesystem-safe session name.
pub fn sanitize_name(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == ' ' || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let s = s.trim().to_string();
    if s.is_empty() {
        "session".to_string()
    } else {
        s
    }
}

/// Path to a named session file.
pub fn session_file(name: &str) -> PathBuf {
    sessions_dir().join(format!("{}.json", sanitize_name(name)))
}

/// The name of the currently-active session (from the pointer file), if any.
pub fn read_current() -> Option<String> {
    std::fs::read_to_string(pointer_file())
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Record which session is active.
pub fn write_current(name: &str) -> Result<()> {
    let p = pointer_file();
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(p, sanitize_name(name))?;
    Ok(())
}

pub fn save_named(name: &str, session: &Session) -> Result<()> {
    save(&session_file(name), session)
}

pub fn load_named(name: &str) -> Result<Option<Session>> {
    load(&session_file(name))
}

pub fn delete_named(name: &str) -> Result<()> {
    match std::fs::remove_file(session_file(name)) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}

pub fn rename_session(old: &str, new: &str) -> Result<()> {
    let from = session_file(old);
    let to = session_file(new);
    if from.exists() {
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::rename(from, to)?;
    }
    Ok(())
}

/// A short description of a saved session for the browser list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionMeta {
    pub name: String,
    pub mech_count: usize,
    /// Up to three chassis names, e.g. "Atlas, Locust, Shadow Hawk +2".
    pub summary: String,
    /// The session's game system (Classic or Alpha Strike).
    pub mode: GameMode,
    /// The force's point total in its game system (BV for Classic, PV for Alpha Strike).
    pub force_total: u64,
    /// The session's optional point ceiling (BV/PV), shown alongside the total.
    pub limit: Option<u64>,
}

fn list_sessions_in(dir: &Path) -> Vec<SessionMeta> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|x| x.to_str()) != Some("json") {
            continue;
        }
        let Some(name) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let session = std::fs::read(&path)
            .ok()
            .and_then(|b| serde_json::from_slice::<Session>(&b).ok());
        let (mech_count, summary, mode, force_total, limit) = match session {
            Some(s) => {
                let chassis: Vec<String> =
                    s.mechs.iter().map(|m| m.spec.chassis.clone()).collect();
                let summary = if chassis.is_empty() {
                    "empty".to_string()
                } else {
                    let shown = chassis
                        .iter()
                        .take(3)
                        .map(String::as_str)
                        .collect::<Vec<_>>()
                        .join(", ");
                    let extra = chassis.len().saturating_sub(3);
                    if extra > 0 {
                        format!("{shown} +{extra}")
                    } else {
                        shown
                    }
                };
                (s.mechs.len(), summary, s.mode, s.force_total(), s.limit)
            }
            None => (0, "unreadable".to_string(), GameMode::Classic, 0, None),
        };
        out.push(SessionMeta {
            name: name.to_string(),
            mech_count,
            summary,
            mode,
            force_total,
            limit,
        });
    }
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    out
}

/// All saved sessions, sorted by name.
pub fn list_sessions() -> Vec<SessionMeta> {
    list_sessions_in(&sessions_dir())
}

/// One-time migration: move a legacy single `session.json` into `sessions/default.json`.
pub fn migrate_legacy() -> Result<()> {
    let legacy = neurohelmet_dir().join("session.json");
    if !legacy.exists() {
        return Ok(());
    }
    std::fs::create_dir_all(sessions_dir())?;
    let dest = session_file("default");
    if dest.exists() {
        let _ = std::fs::remove_file(&legacy);
    } else {
        std::fs::rename(&legacy, &dest)?;
    }
    if read_current().is_none() {
        write_current("default")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{
        AmmoBin, AsStats, CritSlot, Equipment, HeatSinkType, LocationArmor, MechConfig, UnitType,
        WeaponMount,
    };

    #[test]
    fn data_dir_falls_back_to_legacy_mechdoll_dir() {
        let cur = PathBuf::from("/data/neurohelmet");
        let leg = PathBuf::from("/data/mechdoll");
        // Only the legacy `mechdoll` dir exists → use it, so a pre-rename install's data loads.
        assert_eq!(pick_data_dir(cur.clone(), leg.clone(), |p| *p == leg), leg);
        // The new dir exists → always prefer it (even if the legacy one also lingers).
        assert_eq!(pick_data_dir(cur.clone(), leg.clone(), |_| true), cur);
        // Fresh install (neither exists yet) → the new `neurohelmet` dir.
        assert_eq!(pick_data_dir(cur.clone(), leg.clone(), |_| false), cur);
    }

    fn atlas() -> Mech {
        let mut armor = BTreeMap::new();
        armor.insert(
            Location::CenterTorso,
            LocationArmor {
                armor_max: 47,
                rear_max: 14,
                internal_max: 31,
            },
        );
        armor.insert(
            Location::LeftArm,
            LocationArmor {
                armor_max: 2,
                rear_max: 0,
                internal_max: 3,
            },
        );
        armor.insert(
            Location::LeftTorso,
            LocationArmor {
                armor_max: 2,
                rear_max: 1,
                internal_max: 4,
            },
        );
        Mech {
            chassis: "Atlas".into(),
            model: "AS7-D".into(),
            tonnage: 100,
            tech_base: "Inner Sphere".into(),
            role: "Juggernaut".into(),
            weight_class: "Assault".into(),
            subtype: "BattleMek".into(),
            year: 2755,
            bv: 0,
            cost: 0,
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
            }],
            ammo: vec![AmmoBin {
                id: 0,
                name: "AC/20 Ammo".into(),
                location: Location::CenterTorso,
                shots_per_ton: 5,
                tons: 2,
                ammo_key: Some("AC:20".into()),
                munition: String::new(),
                base_ammo: None,
            }],
            crit_slots: BTreeMap::from([(
                Location::CenterTorso,
                vec![
                    CritSlot { slot: 0, name: "Fusion Engine".into(), system: true, hittable: true, ..Default::default() },
                    CritSlot { slot: 1, name: "Gyro".into(), system: true, hittable: true, ..Default::default() },
                    CritSlot { slot: 2, name: "Autocannon/20".into(), system: false, hittable: true, ..Default::default() },
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
                specials: vec!["AC2/2/-".into(), "IF1".into()],
                arcs: None,
                ..Default::default()
            },
            availability: BTreeMap::new(),
        }
    }

    /// A 4-suit Battle Armor squad: per-suit armor tracks + an SRM-2 with a 2-shot ammo bin.
    fn elemental_squad() -> Mech {
        let mut m = atlas();
        m.chassis = "Elemental".into();
        m.model = "[SRM]".into();
        m.unit_type = UnitType::BattleArmor;
        m.crit_slots = BTreeMap::new();
        let mut armor = BTreeMap::new();
        for &l in &Location::TROOPERS[..4] {
            armor.insert(l, LocationArmor { armor_max: 10, rear_max: 0, internal_max: 1 });
        }
        m.armor = armor;
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

    #[test]
    fn ba_ammo_is_per_suit_and_fires_from_the_active_suit() {
        let mut tm = TrackedMech::new(elemental_squad());
        // Four suits, each with its own 2-shot SRM bin.
        assert_eq!(tm.suit_count(), 4);
        assert_eq!(tm.suit_ammo[&0], vec![2, 2, 2, 2]);

        // Firing from suit 0 drains only suit 0.
        tm.set_active_suit(0);
        assert_eq!(tm.ammo_remaining(0), 2);
        tm.fire_weapon(0);
        assert_eq!(tm.suit_ammo[&0], vec![1, 2, 2, 2]);
        assert_eq!(tm.ammo_remaining(0), 1); // active suit's view

        // Switch to suit 2 — it still has its full load.
        tm.set_active_suit(2);
        assert_eq!(tm.ammo_remaining(0), 2);
        tm.fire_weapon(0);
        tm.fire_weapon(0);
        assert_eq!(tm.suit_ammo[&0], vec![1, 2, 0, 2]);
        assert_eq!(tm.ammo_remaining(0), 0);
    }

    #[test]
    fn infantry_damage_scales_with_troopers() {
        let mut m = atlas();
        m.unit_type = UnitType::Infantry;
        m.dpt = 13;
        m.internal = 20;
        m.crit_slots = BTreeMap::new();
        m.armor = BTreeMap::from([(
            Location::Platoon,
            LocationArmor { armor_max: 0, rear_max: 0, internal_max: 20 },
        )]);
        let mut tm = TrackedMech::new(m);
        assert_eq!(tm.troopers_remaining(), 20);
        assert_eq!(tm.infantry_damage(), 13, "full strength = dpt");
        tm.damage(Location::Platoon, Facing::Front, 10); // half the platoon down
        assert_eq!(tm.troopers_remaining(), 10);
        assert_eq!(tm.infantry_damage(), 7, "round(13×10/20)");
        tm.damage(Location::Platoon, Facing::Front, 10); // wiped
        assert_eq!(tm.infantry_damage(), 0);
    }

    #[test]
    fn ov_crit_effects_aggregate_and_apply() {
        // Actuator (mech-leg table row 1) and motive crits stack into a move/TMM penalty.
        let mut tm = TrackedMech::new(atlas());
        tm.ov_add_crit(Location::LeftLeg, 1);
        let fx = tm.ov_crit_effects();
        assert_eq!((fx.move_penalty, fx.tmm_penalty, fx.engine_hits), (2, 1, 0));
        tm.ov_add_crit(Location::RightLeg, 1); // a second actuator stacks
        assert_eq!(tm.ov_crit_effects().move_penalty, 4);

        // Engine crits (torso table row 3) bank +1 heat each at end-turn, before sinks dissipate.
        let mut m = atlas();
        m.dissipation = 5; // sinks = round(5/5) = 1
        let mut hot = TrackedMech::new(m);
        hot.ov_add_crit(Location::CenterTorso, 3);
        hot.ov_add_crit(Location::CenterTorso, 3);
        assert_eq!(hot.ov_crit_effects().engine_hits, 2);
        hot.ov_end_turn(); // 0 + 2 engine − 1 sink = 1
        assert_eq!(hot.ov_heat, 1, "engine crits add heat past dissipation");

        // Removing a hit walks the count back down.
        hot.ov_remove_crit(Location::CenterTorso, 3);
        assert_eq!(hot.ov_crit_count(Location::CenterTorso, 3), 1);
    }

    #[test]
    fn ov_to_hit_assembles_modifiers() {
        use crate::engine::MoveMode;
        let mut tm = TrackedMech::new(atlas()); // gunnery 4; move_mode defaults to Stationary (−1)
        assert_eq!(tm.ov_to_hit(0), 3, "gunnery 4 + standstill −1 (the attacker move comes from move_mode)");
        tm.move_mode = MoveMode::Walked; // ground, 0
        assert_eq!(tm.ov_to_hit(0), 4);
        assert_eq!(tm.ov_to_hit(2), 6, "range bracket adds in");
        assert!(!tm.ov_shot_active(), "default shot (target fields) is neutral");
        // Target TMM 2 + jumped (+1); attacker jumped (+2).
        tm.ov_shot.target_tmm = 2;
        tm.ov_shot.target_jumped = true;
        tm.move_mode = MoveMode::Jumped;
        assert!(tm.ov_shot_active());
        assert_eq!(tm.ov_to_hit(0), 4 + 2 + 1 + 2);
        // Immobile overrides target movement (−2); heat ≥2 adds +1; a pilot hit adds +1.
        tm.ov_shot.target_immobile = true;
        tm.ov_heat = 2;
        tm.hit_pilot();
        assert_eq!(tm.ov_to_hit(0), 4 + 2 - 2 + 1 + 1, "immobile −2, jump +2, heat +1, pilot +1");
        // Floors at the 2+ minimum (gunnery 0, standstill −1).
        let mut easy = TrackedMech::new(atlas());
        easy.gunnery = 0;
        assert_eq!(easy.ov_to_hit(0), 2, "clamped to 2+");
    }

    #[test]
    fn ov_psr_and_morale() {
        let mut tm = TrackedMech::new(atlas()); // piloting 5
        assert!(!tm.ov_psr_due(), "no PSR owed at full health");
        assert_eq!(tm.ov_psr_auto_fail(), None);

        // A gyro crit (torso table row 2) is a +2 PSR situation.
        tm.ov_add_crit(Location::CenterTorso, 2);
        assert_eq!(tm.ov_psr_situations(), vec!["gyro damaged"]);
        assert_eq!(tm.ov_psr_target(), 7, "piloting 5 + gyro 2");
        assert!(tm.ov_psr_due());
        // A pilot hit adds +1 to the PSR target.
        tm.hit_pilot();
        assert_eq!(tm.ov_psr_target(), 8);
        // A second gyro crit destroys the gyro → automatic failure.
        tm.ov_add_crit(Location::CenterTorso, 2);
        assert_eq!(tm.ov_psr_auto_fail(), Some("gyro destroyed"));

        // Massive damage: 10 pips in a phase forces a PSR.
        let mut hot = TrackedMech::new(atlas());
        for _ in 0..10 {
            hot.ov_damage(Location::CenterTorso, false);
        }
        assert!(hot.ov_psr_situations().contains(&"massive damage"));

        // Crippling damage (core front armor gone + structure ≤ 4) triggers a morale test.
        let mut crip = TrackedMech::new(atlas());
        for _ in 0..60 {
            crip.ov_damage(Location::CenterTorso, false);
        }
        assert!(crip.ov_crippled(), "torso stripped → crippled");
        assert_eq!(crip.ov_morale_target(), 8, "base morale, no crits/condition");
        crip.ov_add_crit(Location::CenterTorso, 3); // engine crit (system) → +1
        crip.hit_pilot(); // condition hit → +1
        assert_eq!(crip.ov_morale_target(), 10);
    }

    #[test]
    fn ov_ammo_explosion_and_spent_gate() {
        // The atlas test mech carries an AC/20 ammo bin in the right torso → the merged Override
        // torso region has ammo.
        let mut tm = TrackedMech::new(atlas());
        assert!(tm.ov_region_has_ammo(Location::CenterTorso), "torso carries the AC bin");
        assert!(!tm.ov_region_has_ammo(Location::Head), "head has no ammo");
        assert!(!tm.ov_ammo_exploded());

        // An ammo crit (torso table row 0) in a live region detonates → unit destroyed.
        tm.ov_add_crit(Location::CenterTorso, 0);
        assert!(tm.ov_ammo_exploded());
        assert_eq!(tm.ov_destroyed_reason(), Some("ammo"));

        // Marking the bin spent makes the crit a dud (becomes a weapon result) — no boom.
        assert!(tm.ov_toggle_ammo_spent(Location::CenterTorso), "now spent");
        assert!(!tm.ov_ammo_exploded());
        assert_eq!(tm.ov_destroyed_reason(), None);
        // Toggling back to live re-arms it.
        assert!(!tm.ov_toggle_ammo_spent(Location::CenterTorso), "now live");
        assert!(tm.ov_ammo_exploded());

        // An ammo crit in a region with no ammo never explodes.
        let mut dry = TrackedMech::new(atlas());
        dry.ov_add_crit(Location::Head, 0); // head has no ammo bin
        assert!(!dry.ov_ammo_exploded());
    }

    /// A bare weapon mount; tests override `id`/`name`/`location` via `..weapon_stub()`.
    fn weapon_stub() -> WeaponMount {
        WeaponMount {
            id: 0,
            name: String::new(),
            location: Location::Nose,
            rear: false,
            heat: 0,
            damage: "1".into(),
            range: "1/2/3".into(),
            crit_slots: 1,
            ammo_key: None,
            to_hit: 0,
            tc_eligible: false,
            count: 1,
        }
    }

    fn aero_fighter() -> Mech {
        let mut m = atlas();
        m.chassis = "Visigoth".into();
        m.unit_type = UnitType::Aerospace;
        m.config = MechConfig::Biped; // unused for aero
        m.crit_slots = BTreeMap::new();
        m.walk = 5; // safe thrust
        m.run = 8; // max thrust
        m.jump = 0;
        m.heat_sinks = 10;
        m.dissipation = 10;
        m.armor = BTreeMap::from([
            (Location::Nose, LocationArmor { armor_max: 12, rear_max: 0, internal_max: 0 }),
            (Location::LeftWing, LocationArmor { armor_max: 9, rear_max: 0, internal_max: 0 }),
            (Location::RightWing, LocationArmor { armor_max: 9, rear_max: 0, internal_max: 0 }),
            (Location::Aft, LocationArmor { armor_max: 7, rear_max: 0, internal_max: 0 }),
            (Location::AeroSI, LocationArmor { armor_max: 0, rear_max: 0, internal_max: 5 }),
        ]);
        m
    }

    #[test]
    fn aero_arcs_spill_into_shared_si_then_destroyed() {
        let mut tm = TrackedMech::new(aero_fighter());
        assert_eq!((tm.movement().walk, tm.movement().run), (5, 8), "safe/max thrust");
        assert!(tm.destroyed_reason().is_none());
        // Arcs aren't "destroyed" (no per-arc internal); SI is the structure pool.
        assert!(!tm.is_destroyed(Location::Nose));

        // Strip the nose armor exactly — SI untouched.
        tm.damage(Location::Nose, Facing::Front, 12);
        assert_eq!(tm.internal_remaining(Location::AeroSI), 5);

        // More into the (armorless) nose spills into the shared SI.
        tm.damage(Location::Nose, Facing::Front, 3);
        assert_eq!(tm.internal_remaining(Location::AeroSI), 2);

        // A hit on a DIFFERENT arc draws the SAME SI pool → SI gone → destroyed.
        tm.damage(Location::Aft, Facing::Front, 7 + 2);
        assert_eq!(tm.internal_remaining(Location::AeroSI), 0);
        assert_eq!(tm.destroyed_reason(), Some("structural integrity gone"));

        // Aero heat does NOT cut thrust (it forces a control roll instead) — thrust stays put.
        let mut hot = TrackedMech::new(aero_fighter());
        hot.adjust_heat(5);
        assert_eq!(hot.movement().walk, 5);
        assert_eq!(hot.aero_heat_effects().control_avoid, Some(5));
    }

    #[test]
    fn motive_damage_table_grades_mp_and_steering() {
        let mut m = atlas();
        m.unit_type = UnitType::Vehicle;
        m.motive = Some(crate::domain::MotiveType::Tracked);
        m.walk = 6; // Cruise 6
        let mut tm = TrackedMech::new(m);
        assert_eq!(tm.movement().walk, 6);
        assert_eq!(tm.motive_steering(), 0);

        // Minor: no MP loss, +1 steering.
        tm.add_motive(MotiveLevel::Minor);
        assert_eq!(tm.motive_cruise(), 6);
        assert_eq!(tm.motive_steering(), 1);

        // Moderate: −1 MP, steering now +1+2.
        tm.add_motive(MotiveLevel::Moderate);
        assert_eq!(tm.motive_cruise(), 5);
        assert_eq!(tm.motive_steering(), 3);
        assert_eq!(tm.movement().run, 8); // Flank = ⌈1.5 × 5⌉

        // Heavy: halves remaining (⌈5/2⌉ = 3), steering +1+2+3 = 6.
        tm.add_motive(MotiveLevel::Heavy);
        assert_eq!(tm.motive_cruise(), 3);
        assert_eq!(tm.motive_steering(), 6);
        assert_eq!(tm.motive_mp_lost(), 3);
        assert!(!tm.motive_immobilized());

        // A second Heavy halves again (⌈3/2⌉ = 2); steering doesn't double-count a severity.
        tm.add_motive(MotiveLevel::Heavy);
        assert_eq!(tm.motive_cruise(), 2);
        assert_eq!(tm.motive_steering(), 6);

        // Repair pops the most recent result (LIFO).
        assert_eq!(tm.repair_motive(), Some(MotiveLevel::Heavy));
        assert_eq!(tm.motive_cruise(), 3);

        // Immobilized zeroes MP and immobilizes.
        tm.add_motive(MotiveLevel::Immobilized);
        assert!(tm.motive_immobilized());
        assert!(tm.movement().immobile);
        assert_eq!(tm.movement().note, Some("immobilized"));
    }

    #[test]
    fn aero_graded_crits_apply_tw_effects() {
        let mut m = aero_fighter();
        m.weapons = vec![WeaponMount {
            id: 1,
            name: "ER PPC".into(),
            location: Location::Nose,
            ..weapon_stub()
        }];
        let mut tm = TrackedMech::new(m);
        let ppc = tm.spec.weapons[0].clone();
        assert_eq!((tm.movement().walk, tm.movement().run), (5, 8));

        // Aero systems accumulate hits (cap 3), and one key cycles 0→1→2→3→0.
        let engine_idx = AEROSPACE_CRITS.iter().position(|n| *n == "Engine").unwrap();
        assert_eq!(tm.bump_crit(engine_idx), 1);
        // Engine −2 Thrust / +2 heat per hit; Max Thrust = ⌈1.5 × Safe⌉ off the reduced value.
        assert_eq!((tm.movement().walk, tm.movement().run), (3, 5));
        assert_eq!(tm.engine_heat(), 2);
        assert_eq!(tm.bump_crit(engine_idx), 2);
        assert_eq!((tm.movement().walk, tm.movement().run), (1, 2));
        assert_eq!(tm.engine_heat(), 4);
        // Third hit destroys the engine — thrust 0, unit out.
        assert_eq!(tm.bump_crit(engine_idx), 3);
        assert!(tm.movement().immobile);
        assert_eq!(tm.destroyed_reason(), Some("engine destroyed"));
        // Wrap back to 0 clears it.
        assert_eq!(tm.bump_crit(engine_idx), 0);
        assert!(tm.destroyed_reason().is_none());

        // Sensors: +N at 1–2, +5 at 3. FCS: +2 per hit; >2 takes weapons offline.
        let sensors = AEROSPACE_CRITS.iter().position(|n| *n == "Sensors").unwrap();
        tm.bump_crit(sensors); // 1 hit
        assert_eq!(tm.aero_weapon_to_hit(), 1);
        tm.bump_crit(sensors);
        tm.bump_crit(sensors); // 3 hits → +5
        assert_eq!(tm.aero_weapon_to_hit(), 5);

        let fcs = AEROSPACE_CRITS.iter().position(|n| *n == "FCS").unwrap();
        tm.bump_crit(fcs); // +2 → total to-hit 7
        assert_eq!(tm.aero_weapon_to_hit(), 7);
        assert!(!tm.is_weapon_disabled(&ppc));
        tm.bump_crit(fcs);
        tm.bump_crit(fcs); // 3 FCS hits (>2) → fire control gone
        assert!(tm.aero_fire_control_destroyed());
        assert!(tm.is_weapon_disabled(&ppc), "weapons offline with FCS shot out");

        // Avionics: control-roll modifier +N (1–2), +5 destroyed.
        let avi = AEROSPACE_CRITS.iter().position(|n| *n == "Avionics").unwrap();
        assert_eq!(tm.aero_control_modifier(), 0);
        tm.bump_crit(avi);
        assert_eq!(tm.aero_control_modifier(), 1);
        tm.bump_crit(avi);
        tm.bump_crit(avi);
        assert_eq!(tm.aero_control_modifier(), 5);

        // Landing Gear has no on-map combat effect — purely a mark.
        let gear = AEROSPACE_CRITS.iter().position(|n| *n == "Landing Gear").unwrap();
        tm.bump_crit(gear);
        assert!(tm.crit_marked(gear));
    }

    #[test]
    fn aero_weapon_crit_destroys_a_specific_mount() {
        let mut m = aero_fighter();
        m.weapons = vec![
            WeaponMount {
                id: 1,
                name: "ER PPC".into(),
                location: Location::Nose,
                ..weapon_stub()
            },
            WeaponMount {
                id: 2,
                name: "Medium Laser".into(),
                location: Location::LeftWing,
                ..weapon_stub()
            },
        ];
        let mut tm = TrackedMech::new(m);

        // crit_rows = the 5 system crits, then a Weapons section (Nose before LeftWing in doll order).
        let rows = tm.crit_rows();
        assert_eq!(rows.len(), AEROSPACE_CRITS.len() + 2);
        let weapon_rows: Vec<_> = rows
            .iter()
            .filter(|r| matches!(r, CritRow::Weapon { .. }))
            .collect();
        assert_eq!(weapon_rows[0].label(), "NOS: ER PPC");
        assert_eq!(weapon_rows[1].label(), "LWG: Medium Laser");

        let ppc = tm.spec.weapons[0].clone();
        let mlas = tm.spec.weapons[1].clone();
        assert!(!tm.is_weapon_disabled(&ppc));

        // A rolled weapon crit knocks out just that mount, not the other.
        assert!(tm.toggle_weapon_crit(1));
        assert!(tm.is_weapon_disabled(&ppc));
        assert!(!tm.is_weapon_disabled(&mlas));

        // Toggling again repairs it.
        assert!(!tm.toggle_weapon_crit(1));
        assert!(!tm.is_weapon_disabled(&ppc));
    }

    #[test]
    fn aero_velocity_altitude_persist_across_turns() {
        let mut tm = TrackedMech::new(aero_fighter());
        assert_eq!((tm.velocity, tm.altitude), (0, 0));
        tm.adjust_velocity(8);
        tm.adjust_altitude(5);
        assert_eq!((tm.velocity, tm.altitude), (8, 5));
        // Both persist across end-turn (unlike hexes_moved / move_mode).
        tm.end_turn();
        assert_eq!((tm.velocity, tm.altitude), (8, 5));
        // Clamped: altitude 0..=10, velocity 0..=60.
        tm.adjust_altitude(20);
        assert_eq!(tm.altitude, 10);
        tm.adjust_velocity(-100);
        assert_eq!(tm.velocity, 0);
    }

    #[test]
    fn ba_each_suit_fires_each_weapon_once() {
        // An energy weapon (no ammo) must still be firable once per suit, not once per squad.
        let mut m = elemental_squad();
        m.weapons.push(WeaponMount {
            id: 1,
            name: "Small Laser".into(),
            location: Location::Trooper1,
            rear: false,
            heat: 0,
            damage: "3".into(),
            range: "1/2/3".into(),
            crit_slots: 0,
            ammo_key: None,
            to_hit: 0,
            tc_eligible: false,
            count: 1,
        });
        let mut tm = TrackedMech::new(m);

        // Suit 0 fires the laser, then can't fire it again.
        tm.set_active_suit(0);
        assert!(!tm.active_suit_fired(1));
        tm.fire_weapon(1);
        assert!(tm.active_suit_fired(1));
        assert_eq!(tm.suit_fired_count(1), 1);

        // Suit 1 still can.
        tm.set_active_suit(1);
        assert!(!tm.active_suit_fired(1));
        tm.fire_weapon(1);
        assert_eq!(tm.suit_fired_count(1), 2);

        // Un-firing suit 1 clears only that suit's mark.
        tm.unfire_weapon(1);
        assert!(!tm.active_suit_fired(1));
        assert_eq!(tm.suit_fired_count(1), 1);

        // End of turn clears everyone.
        tm.end_turn();
        assert_eq!(tm.suit_fired_count(1), 0);
    }

    #[test]
    fn ba_dead_suit_loses_its_ammo() {
        let mut tm = TrackedMech::new(elemental_squad());
        // Kill suit 1 (Trooper2): 10 armor + 1 internal.
        tm.damage(Location::Trooper2, Facing::Front, 11);
        assert!(!tm.suit_alive(1));
        // Its ammo is no longer usable, even though stored shots remain.
        tm.set_active_suit(1);
        assert_eq!(tm.ammo_remaining(0), 0);
        // Firing from it spends nothing.
        let before = tm.suit_ammo[&0].clone();
        tm.fire_ammo(0, 1);
        // (fire_ammo still decrements the stored slot, but it's unreachable — the suit is dead.)
        assert_eq!(tm.ammo_remaining(0), 0);
        // Living suits are unaffected.
        tm.set_active_suit(0);
        assert_eq!(tm.ammo_remaining(0), 2);
        let _ = before;
    }

    #[test]
    fn crit_toggle_roundtrip() {
        let mut tm = TrackedMech::new(atlas());
        assert!(!tm.is_crit_hit(Location::CenterTorso, 0));
        assert_eq!(tm.crit_hits_in(Location::CenterTorso), 0);

        assert!(tm.toggle_crit(Location::CenterTorso, 0)); // -> destroyed
        assert!(tm.toggle_crit(Location::CenterTorso, 2)); // -> destroyed
        assert!(tm.is_crit_hit(Location::CenterTorso, 0));
        assert_eq!(tm.crit_hits_in(Location::CenterTorso), 2);

        assert!(!tm.toggle_crit(Location::CenterTorso, 0)); // -> repaired
        assert!(!tm.is_crit_hit(Location::CenterTorso, 0));
        assert_eq!(tm.crit_hits_in(Location::CenterTorso), 1);

        // Clearing the last hit drops the location's entry entirely.
        assert!(!tm.toggle_crit(Location::CenterTorso, 2));
        assert!(!tm.crit_hits.contains_key(&Location::CenterTorso));
    }

    fn crit_consequence_mech() -> Mech {
        let mut armor = BTreeMap::new();
        for loc in [
            Location::CenterTorso,
            Location::Head,
            Location::RightTorso,
            Location::LeftArm,
        ] {
            armor.insert(loc, LocationArmor { armor_max: 10, rear_max: 0, internal_max: 5 });
        }
        let cs = |slot: u8, name: &str, system: bool| CritSlot {
            slot,
            name: name.into(),
            system,
            hittable: true, ..Default::default()
        };
        let crit_slots = BTreeMap::from([
            (
                Location::CenterTorso,
                vec![
                    cs(0, "Fusion Engine", true),
                    cs(1, "Fusion Engine", true),
                    cs(2, "Fusion Engine", true),
                    cs(3, "Gyro", true),
                    cs(4, "Gyro", true),
                ],
            ),
            (
                Location::Head,
                vec![cs(0, "Life Support", true), cs(1, "Sensors", true), cs(2, "Cockpit", true)],
            ),
            (
                Location::RightTorso,
                // A side-torso weapon plus XL engine slots (lost together when the torso goes).
                vec![cs(0, "AC/20", false), cs(1, "XL Fusion Engine", true), cs(2, "XL Fusion Engine", true)],
            ),
        ]);
        Mech {
            chassis: "Test".into(),
            model: "X".into(),
            tonnage: 50,
            tech_base: "IS".into(),
            role: String::new(),
            weight_class: "Medium".into(),
            subtype: "BattleMek".into(),
            year: 3025,
            bv: 0,
            cost: 0,
            armor_type: "Ferro-Fibrous".into(),
            structure_type: "Standard".into(),
            walk: 4,
            run: 6,
            jump: 0,
            heat_sinks: 10,
            heat_sink_type: HeatSinkType::Single,
            dissipation: 10,
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
                name: "AC/20".into(),
                location: Location::RightTorso,
                rear: false,
                heat: 7,
                damage: "20".into(),
                range: String::new(),
                crit_slots: 10,
                ammo_key: None,
                to_hit: 0,
                tc_eligible: false,
                count: 1,
            }],
            ammo: vec![],
            crit_slots,
            as_stats: AsStats::default(),
            availability: BTreeMap::new(),
        }
    }

    #[test]
    fn engine_hits_ramp_heat_then_destroy() {
        let mut tm = TrackedMech::new(crit_consequence_mech());
        tm.toggle_crit(Location::CenterTorso, 0);
        assert_eq!(tm.engine_hits(), 1);
        assert_eq!(tm.engine_heat(), 5);
        assert!(tm.destroyed_reason().is_none());
        tm.toggle_crit(Location::CenterTorso, 1);
        assert_eq!(tm.engine_heat(), 10);
        assert!(tm.destroyed_reason().is_none());
        tm.toggle_crit(Location::CenterTorso, 2);
        assert_eq!(tm.engine_hits(), 3);
        assert_eq!(tm.destroyed_reason(), Some("engine destroyed"));
    }

    #[test]
    fn gyro_and_cockpit_destroy() {
        let mut tm = TrackedMech::new(crit_consequence_mech());
        tm.toggle_crit(Location::CenterTorso, 3);
        assert!(tm.destroyed_reason().is_none());
        tm.toggle_crit(Location::CenterTorso, 4);
        assert_eq!(tm.gyro_hits(), 2);
        assert_eq!(tm.destroyed_reason(), Some("gyro destroyed"));

        let mut tm = TrackedMech::new(crit_consequence_mech());
        tm.toggle_crit(Location::Head, 2); // Cockpit
        assert_eq!(tm.destroyed_reason(), Some("cockpit hit"));
    }

    #[test]
    fn weapon_disabled_by_crit_and_by_torso_loss() {
        let mut tm = TrackedMech::new(crit_consequence_mech());
        let ac20 = tm.spec.weapons[0].clone();
        assert!(!tm.is_weapon_disabled(&ac20));
        tm.toggle_crit(Location::RightTorso, 0); // direct hit on the gun's slot
        assert!(tm.is_weapon_disabled(&ac20));
        tm.toggle_crit(Location::RightTorso, 0); // un-mark
        assert!(!tm.is_weapon_disabled(&ac20));

        // Destroying just the side torso (10 armor + 5 internal, no excess to cascade) wrecks the
        // gun and its 2 XL engine slots.
        tm.damage(Location::RightTorso, Facing::Front, 15);
        assert!(tm.is_destroyed(Location::RightTorso));
        assert!(!tm.is_destroyed(Location::CenterTorso), "no excess cascaded inward");
        assert!(tm.is_weapon_disabled(&ac20));
        assert_eq!(tm.engine_hits(), 2, "two XL engine slots lost with the torso");
    }

    #[test]
    fn pilot_hits_track_and_kill() {
        let mut tm = TrackedMech::new(atlas());
        assert_eq!(tm.pilot_hits, 0);
        assert_eq!(tm.consciousness_avoid(), None);
        assert!(!tm.pilot_dead());

        tm.hit_pilot();
        assert_eq!(tm.consciousness_avoid(), Some(3));
        tm.hit_pilot();
        assert_eq!(tm.consciousness_avoid(), Some(5));
        tm.heal_pilot();
        assert_eq!(tm.pilot_hits, 1);

        for _ in 0..10 {
            tm.hit_pilot(); // clamps at 6
        }
        assert_eq!(tm.pilot_hits, 6);
        assert!(tm.pilot_dead());
        assert_eq!(tm.consciousness_avoid(), None); // dead -> no roll
        assert_eq!(tm.destroyed_reason(), Some("pilot dead"));

        tm.heal_pilot();
        assert_eq!(tm.pilot_hits, 5);
        assert!(!tm.pilot_dead());
        assert_eq!(tm.destroyed_reason(), None);
    }

    #[test]
    fn pilot_consciousness_toggle() {
        let mut tm = TrackedMech::new(atlas());
        assert!(!tm.pilot_unconscious);
        tm.toggle_unconscious();
        assert!(tm.pilot_unconscious);
        tm.toggle_unconscious();
        assert!(!tm.pilot_unconscious);

        // A dead pilot can't be toggled conscious/unconscious.
        for _ in 0..6 {
            tm.hit_pilot();
        }
        assert!(tm.pilot_dead());
        tm.toggle_unconscious();
        assert!(!tm.pilot_unconscious, "no toggle once dead");
    }

    #[test]
    fn alpha_strike_damage_heat_crits() {
        let mut tm = TrackedMech::new(atlas()); // AS: Arm 10, Str 8
        assert_eq!((tm.as_armor_remaining(), tm.as_struct_remaining()), (10, 8));
        assert!(!tm.as_destroyed());

        for _ in 0..10 {
            tm.as_damage();
        }
        assert_eq!(tm.as_armor_remaining(), 0);
        assert_eq!(tm.as_struct_remaining(), 8);
        tm.as_damage(); // spills into structure
        assert_eq!(tm.as_struct_remaining(), 7);
        tm.as_repair(); // pulls structure back first
        assert_eq!(tm.as_struct_remaining(), 8);
        for _ in 0..8 {
            tm.as_damage();
        }
        assert!(tm.as_destroyed(), "structure gone");

        tm.as_adjust_heat(10);
        assert_eq!(tm.as_heat, 4); // clamps at the S box
        assert!(tm.as_shutdown(), "heat 4 (the S box) is a shutdown");
        tm.as_adjust_heat(-1);
        assert!(!tm.as_shutdown(), "heat 3 is not shut down");
        tm.as_adjust_heat(-10);
        assert_eq!(tm.as_heat, 0);
        assert!(!tm.as_shutdown());

        let mut tm2 = TrackedMech::new(atlas());
        tm2.as_crit_inc(AsCritKind::Engine);
        assert!(!tm2.as_destroyed());
        tm2.as_crit_inc(AsCritKind::Engine);
        assert!(tm2.as_destroyed(), "2 engine crits");
        tm2.as_crit_inc(AsCritKind::Engine); // caps at 2
        assert_eq!(tm2.as_crit(AsCritKind::Engine), 2);
        tm2.as_crit_dec(AsCritKind::Engine);
        assert_eq!(tm2.as_crit(AsCritKind::Engine), 1);
    }

    #[test]
    fn as_target_tmm_none_some_round_trip() {
        let mut tm = TrackedMech::new(atlas());
        assert!(!tm.as_shot_active());
        tm.as_adjust_target_tmm(-1); // no-op from None
        assert_eq!(tm.as_target, None);
        tm.as_adjust_target_tmm(1); // None -> Some(0)
        assert_eq!(tm.as_target, Some(AsTarget::default()));
        assert!(tm.as_shot_active());
        tm.as_adjust_target_tmm(5); // climbs, then clamps at the cap
        tm.as_adjust_target_tmm(5);
        assert_eq!(tm.as_target.unwrap().tmm, AS_TARGET_TMM_MAX);
        tm.as_toggle_target_jumped();
        assert!(tm.as_target.unwrap().jumped);
        // Stepping below 0 clears the whole target back to None.
        tm.as_adjust_target_tmm(-(AS_TARGET_TMM_MAX as i32) - 1);
        assert_eq!(tm.as_target, None);
        // Attacker jump alone also activates the shot context.
        tm.as_toggle_attacker_jumped();
        assert!(tm.as_shot_active());
    }

    #[test]
    fn ct_target_distance_none_some_round_trip() {
        let mut tm = TrackedMech::new(atlas());
        assert!(!tm.ct_shot_active());
        tm.ct_adjust_distance(-1); // no-op from None
        assert_eq!(tm.ct_target, None);
        tm.ct_adjust_hexes(3); // no-op without a target
        assert_eq!(tm.ct_target, None);
        tm.ct_adjust_distance(1); // None -> Some(distance 1)
        assert_eq!(tm.ct_target, Some(CtTarget { distance: 1, ..CtTarget::default() }));
        assert!(tm.ct_shot_active());
        tm.ct_adjust_distance(CT_TARGET_DISTANCE_MAX as i32 + 5); // climbs, then clamps
        assert_eq!(tm.ct_target.unwrap().distance, CT_TARGET_DISTANCE_MAX);
        tm.ct_adjust_hexes(CT_TARGET_HEXES_MAX as i32 + 5); // hexes clamp too
        assert_eq!(tm.ct_target.unwrap().hexes_moved, CT_TARGET_HEXES_MAX);
        tm.ct_toggle_target_jumped();
        assert!(tm.ct_target.unwrap().jumped);
        tm.ct_toggle_target_immobile();
        assert!(tm.ct_target.unwrap().immobile);
        // Stepping distance below 1 clears the whole target.
        tm.ct_adjust_distance(-(CT_TARGET_DISTANCE_MAX as i32));
        assert_eq!(tm.ct_target, None);
        // End-turn clears any live target.
        tm.ct_adjust_distance(1);
        assert!(tm.ct_shot_active());
        tm.end_turn();
        assert!(!tm.ct_shot_active());
    }

    #[test]
    fn as_crit_kinds_by_unit_type() {
        let mech = TrackedMech::new(atlas());
        assert_eq!(mech.as_crit_kinds(), &AsCritKind::ALL, "'Mech uses the full crit set");
        assert!(mech.as_crit_kinds().contains(&AsCritKind::Mp));

        let mut aero_spec = atlas();
        aero_spec.unit_type = UnitType::Aerospace;
        let aero = TrackedMech::new(aero_spec);
        assert_eq!(
            aero.as_crit_kinds(),
            &[AsCritKind::Engine, AsCritKind::FireControl, AsCritKind::Weapon],
            "aerospace omits MP"
        );
        assert!(!aero.as_crit_kinds().contains(&AsCritKind::Mp));

        // Combat vehicles swap MP for a Motive track; 2 engine crits don't destroy them.
        let mut veh_spec = atlas();
        veh_spec.unit_type = UnitType::Vehicle;
        let mut veh = TrackedMech::new(veh_spec);
        assert_eq!(
            veh.as_crit_kinds(),
            &[AsCritKind::Engine, AsCritKind::FireControl, AsCritKind::Weapon, AsCritKind::Motive]
        );
        veh.as_crit_inc(AsCritKind::Engine);
        veh.as_crit_inc(AsCritKind::Engine);
        assert!(!veh.as_destroyed(), "2 engine crits don't kill a vehicle (only a 'Mech)");
        for _ in 0..6 {
            veh.as_crit_inc(AsCritKind::Motive);
        }
        assert_eq!(veh.as_crit(AsCritKind::Motive), 5, "Motive track caps at 5");

        // Emplacements (AS type BD, baked as Vehicle) are weapons-only.
        let mut bd_spec = atlas();
        bd_spec.unit_type = UnitType::Vehicle;
        bd_spec.as_stats.tp = "BD".into();
        let bd = TrackedMech::new(bd_spec);
        assert_eq!(bd.as_crit_kinds(), &[AsCritKind::Weapon]);
    }

    #[test]
    fn engine_heat_applied_at_end_turn() {
        let mut tm = TrackedMech::new(crit_consequence_mech());
        tm.toggle_crit(Location::CenterTorso, 0);
        tm.toggle_crit(Location::CenterTorso, 1); // +10 heat/turn
        tm.heat = 5;
        tm.end_turn_heat(); // +10 engine, -10 dissipation -> net 0 (without engine heat it would clamp to 0)
        assert_eq!(tm.heat, 5);
    }

    #[test]
    fn destroyed_heat_sinks_reduce_dissipation() {
        let mut spec = crit_consequence_mech();
        // A 2-slot double heat sink (slots share a uid → counts once) plus a single.
        let hs = |slot: u8, uid: &str, hs: u8| CritSlot {
            slot,
            name: "Double Heat Sink".into(),
            hittable: true,
            uid: uid.into(),
            hs,
            ..Default::default()
        };
        spec.crit_slots.insert(
            Location::LeftArm,
            vec![hs(0, "DHS@LA#0", 2), hs(1, "DHS@LA#0", 2), hs(2, "HS@LA#2", 1)],
        );
        let mut tm = TrackedMech::new(spec);
        assert_eq!(tm.dissipation(), 10);

        tm.toggle_crit(Location::LeftArm, 0); // one slot of the double
        assert_eq!(tm.sink_dissipation_lost(), 2);
        tm.toggle_crit(Location::LeftArm, 1); // its other slot: same sink, no extra loss
        assert_eq!(tm.sink_dissipation_lost(), 2);
        tm.toggle_crit(Location::LeftArm, 2); // the single sink
        assert_eq!(tm.sink_dissipation_lost(), 3);
        assert_eq!(tm.dissipation(), 7);

        tm.heat = 10;
        tm.end_turn_heat(); // -7 instead of -10
        assert_eq!(tm.heat, 3);

        // Repairing the crit restores the sink.
        tm.toggle_crit(Location::LeftArm, 2);
        assert_eq!(tm.dissipation(), 8);
    }

    #[test]
    fn pre_v11_specs_without_sink_data_lose_nothing() {
        // Older bakes have empty uid / hs = 0 on every slot; marking crits must not
        // touch dissipation.
        let mut tm = TrackedMech::new(crit_consequence_mech());
        tm.toggle_crit(Location::RightTorso, 0);
        tm.toggle_crit(Location::CenterTorso, 3);
        assert_eq!(tm.sink_dissipation_lost(), 0);
        assert_eq!(tm.dissipation(), 10);
    }

    #[test]
    fn damage_and_repair_flow() {
        let mut tm = TrackedMech::new(atlas());
        assert_eq!(tm.armor_remaining(Location::CenterTorso, Facing::Front), 47);
        tm.damage(Location::CenterTorso, Facing::Front, 50);
        // 47 armor gone, 3 into internal
        assert_eq!(tm.armor_remaining(Location::CenterTorso, Facing::Front), 0);
        assert_eq!(tm.internal_remaining(Location::CenterTorso), 28);
        tm.repair_armor(Location::CenterTorso, Facing::Front, 10);
        assert_eq!(tm.armor_remaining(Location::CenterTorso, Facing::Front), 10);
    }

    #[test]
    fn heat_and_ammo() {
        let mut tm = TrackedMech::new(atlas());
        tm.adjust_heat(31);
        assert!(tm.shutdown);
        assert!(tm.heat_effects().auto_shutdown);
        tm.end_turn_heat();
        assert_eq!(tm.heat, 11); // 31 - 20 dissipation
        assert!(!tm.shutdown, "cooling below 14 restarts the mech");

        // In the 14-29 band shutdown is a roll, so it stays put until toggled.
        tm.adjust_heat(9); // -> 20
        assert!(!tm.shutdown);
        tm.toggle_shutdown();
        assert!(tm.shutdown);
        tm.adjust_heat(-10); // -> 10, below 14, auto-restart
        assert!(!tm.shutdown);

        assert_eq!(tm.ammo_remaining(0), 10);
        assert_eq!(tm.fire_ammo(0, 3), 3);
        assert_eq!(tm.ammo_remaining(0), 7);
        tm.adjust_ammo(0, 100); // clamps to max 10
        assert_eq!(tm.ammo_remaining(0), 10);
    }

    #[test]
    fn damage_cascades_inward() {
        let mut tm = TrackedMech::new(atlas());
        // LA: 2 armor + 3 internal = 5. Hit for 6 -> LA destroyed, 1 transfers to LT armor.
        let out = tm.damage(Location::LeftArm, Facing::Front, 6);
        assert_eq!(out, DamageOutcome::Absorbed); // landed in LT armor
        assert!(tm.is_destroyed(Location::LeftArm));
        assert!(!tm.is_destroyed(Location::LeftTorso));
        assert_eq!(tm.armor_remaining(Location::LeftTorso, Facing::Front), 1);
    }

    #[test]
    fn full_cascade_to_center_torso() {
        let mut tm = TrackedMech::new(atlas());
        // Overkill the left arm: LA(5) + LT(2+1+4=7 front uses 2+4=6) + CT(47+31=78).
        let out = tm.damage(Location::LeftArm, Facing::Front, 200);
        assert!(tm.is_destroyed(Location::LeftArm));
        assert!(tm.is_destroyed(Location::LeftTorso));
        assert!(tm.is_destroyed(Location::CenterTorso));
        // CT destroyed -> mech dead, leftover returned as Excess.
        assert!(matches!(out, DamageOutcome::Excess(_)));
    }

    #[test]
    fn side_torso_loss_takes_the_arm() {
        let mut tm = TrackedMech::new(atlas());
        tm.damage(Location::LeftTorso, Facing::Front, 7); // 2 armor + 4 internal = destroyed
        assert!(tm.is_destroyed(Location::LeftTorso));
        assert!(tm.is_destroyed(Location::LeftArm), "arm goes with the side torso");
    }

    #[test]
    fn fired_marks_track_until_unfire_or_end_turn() {
        let mut tm = TrackedMech::new(atlas()); // weapon 0 = AC/20, heat 7
        assert!(!tm.is_fired(0));

        tm.fire_weapon(0).unwrap();
        assert!(tm.is_fired(0));
        assert_eq!(tm.heat, 7);

        // Un-fire clears the mark and the heat it added.
        assert_eq!(tm.unfire_weapon(0), Some(7));
        assert!(!tm.is_fired(0));
        assert_eq!(tm.heat, 0);
        assert_eq!(tm.unfire_weapon(0), None, "un-firing an unfired weapon is a no-op");

        // End-turn clears all fired marks.
        tm.fire_weapon(0);
        assert!(tm.is_fired(0));
        tm.end_turn();
        assert!(!tm.is_fired(0));
    }

    #[test]
    fn ultra_rotary_fire_multiple_shots() {
        let wpn = |id: u32, key: &str, heat: u8| WeaponMount {
            id,
            name: "AC".into(),
            location: Location::RightTorso,
            rear: false,
            heat,
            damage: "5".into(),
            range: "6/12/18".into(),
            crit_slots: 1,
            ammo_key: Some(key.into()),
            to_hit: 0,
            tc_eligible: false,
            count: 1,
        };
        // max_shots is keyed on the ammo type (MegaMek Mounted.getNumShots).
        assert_eq!(wpn(0, "AC_ULTRA:5", 1).max_shots(), 2);
        assert_eq!(wpn(0, "AC_ROTARY:5", 1).max_shots(), 6);
        assert_eq!(wpn(0, "AC:20", 7).max_shots(), 1);

        // Firing accumulates shots and base heat per shot (getCurrentHeat = heat × shots).
        let mut m = atlas();
        m.weapons = vec![wpn(0, "AC_ROTARY:5", 1)];
        let mut tm = TrackedMech::new(m);
        tm.fire_weapon(0);
        tm.fire_weapon(0);
        tm.fire_weapon(0);
        assert_eq!(tm.shots_fired(0), 3);
        assert_eq!(tm.heat, 3); // 1 heat × 3 shots

        // Un-fire removes one shot's heat; end-turn clears the rest.
        assert_eq!(tm.unfire_weapon(0), Some(1));
        assert_eq!(tm.shots_fired(0), 2);
        assert_eq!(tm.heat, 2);
        tm.end_turn();
        assert_eq!(tm.shots_fired(0), 0);
    }

    #[test]
    fn firing_weapon_spends_linked_ammo() {
        let mut tm = TrackedMech::new(atlas());
        assert_eq!(tm.ammo_remaining(0), 10);
        let r = tm.fire_weapon(0).unwrap();
        assert_eq!(r.heat, 7);
        assert!(r.ammo_spent);
        assert!(!r.out_of_ammo);
        assert_eq!(tm.heat, 7);
        assert_eq!(tm.ammo_remaining(0), 9);

        // Drain the bin, then the next shot reports out-of-ammo (but still adds heat).
        for _ in 0..9 {
            tm.fire_weapon(0);
        }
        assert_eq!(tm.ammo_remaining(0), 0);
        let r = tm.fire_weapon(0).unwrap();
        assert!(r.out_of_ammo);
        assert!(!r.ammo_spent);
    }

    #[test]
    fn active_bin_selection_directs_firing() {
        // Atlas + a second compatible AC/20 bin in the right torso.
        let mut spec = atlas();
        spec.ammo.push(AmmoBin {
            id: 1,
            name: "AC/20 Ammo".into(),
            location: Location::RightTorso,
            shots_per_ton: 5,
            tons: 1,
            ammo_key: Some("AC:20".into()),
            munition: String::new(),
            base_ammo: None,
        });
        let mut tm = TrackedMech::new(spec);

        // With no choice made, firing/display use the first compatible bin (id 0).
        assert_eq!(tm.weapon_bin(0), Some(0));
        assert!(!tm.is_active_bin(1));

        // Pick the right-torso bin (found via its crit-slot location + name).
        assert_eq!(tm.bin_at(Location::RightTorso, "AC/20 Ammo"), Some(1));
        assert_eq!(tm.set_active_bin(1).as_deref(), Some("AC/20 Ammo"));
        assert!(tm.is_active_bin(1));
        assert_eq!(tm.weapon_bin(0), Some(1));

        // Firing now spends from the active bin, leaving bin 0 untouched.
        tm.fire_weapon(0).unwrap();
        assert_eq!(tm.ammo_remaining(0), 10);
        assert_eq!(tm.ammo_remaining(1), 4);

        // Drain the active bin; firing falls back to the other non-empty bin.
        for _ in 0..4 {
            tm.fire_weapon(0);
        }
        assert_eq!(tm.ammo_remaining(1), 0);
        let r = tm.fire_weapon(0).unwrap();
        assert!(r.ammo_spent && !r.out_of_ammo);
        assert_eq!(tm.ammo_remaining(0), 9);
    }

    #[test]
    fn relink_refreshes_specs_preserving_state() {
        use crate::data::bundle::Bundle;
        // Old session: an Atlas saved before equipment support (no gear), banged up a bit.
        let mut old = atlas();
        old.equipment.clear();
        let mut session = Session::new();
        session.add_mech(old);
        {
            let tm = session.active_mech_mut().unwrap();
            tm.damage(Location::CenterTorso, Facing::Front, 4);
            tm.fire_ammo(0, 3); // spend 3 of 10 AC/20 shots
        }
        // A second mech the current bundle no longer carries — must be left untouched.
        session.add_mech({
            let mut m = atlas();
            m.model = "PHANTOM".into();
            m
        });

        // Fresh bundle: same Atlas, now WITH gear + a brand-new second ammo bin.
        let mut fresh = atlas();
        fresh.equipment = vec![Equipment { name: "Jump Jet".into(), location: Location::LeftLeg }];
        fresh.ammo.push(AmmoBin {
            id: 1,
            name: "AC/20 Ammo".into(),
            location: Location::LeftTorso,
            shots_per_ton: 5,
            tons: 1,
            ammo_key: Some("AC:20".into()),
            munition: String::new(),
            base_ammo: None,
        });
        let bundle = Bundle::new(vec![fresh]);

        let n = session.relink_specs(&bundle);
        assert_eq!(n, 1, "only the Atlas (in the bundle) is refreshed");

        let atlas_tm = &session.mechs[0];
        assert_eq!(atlas_tm.spec.equipment.len(), 1, "gear pulled in from the bundle");
        assert_eq!(atlas_tm.ammo_remaining(0), 7, "spent ammo count survived");
        assert_eq!(atlas_tm.ammo_remaining(1), 5, "a new bin starts full");
        assert_eq!(
            atlas_tm.locations[&Location::CenterTorso].armor_hits, 4,
            "battle damage survived"
        );
        assert!(session.mechs[1].spec.equipment.is_empty(), "unknown unit kept its old spec");

        // Idempotent: a second pass changes nothing.
        assert_eq!(session.relink_specs(&bundle), 0);
    }

    /// A biped (Walk 6 / Run 9 / Jump 4) with internal everywhere and leg + gyro crit slots, so
    /// leg loss, hip/actuator crits, and gyro crits can be exercised.
    fn mover() -> Mech {
        let mut m = atlas();
        let mut armor = BTreeMap::new();
        for loc in Location::ALL.into_iter().filter(|l| !l.is_vehicle() && !l.is_infantry()) {
            armor.insert(
                loc,
                LocationArmor { armor_max: 8, rear_max: 0, internal_max: 6 },
            );
        }
        m.armor = armor;
        m.walk = 6;
        m.run = 9;
        m.jump = 4;
        let leg_slots = vec![
            CritSlot { slot: 0, name: "Hip".into(), system: true, hittable: true, ..Default::default() },
            CritSlot { slot: 1, name: "Upper Leg Actuator".into(), system: true, hittable: true, ..Default::default() },
            CritSlot { slot: 2, name: "Lower Leg Actuator".into(), system: true, hittable: true, ..Default::default() },
            CritSlot { slot: 3, name: "Foot Actuator".into(), system: true, hittable: true, ..Default::default() },
        ];
        m.crit_slots = BTreeMap::from([
            (
                Location::CenterTorso,
                vec![CritSlot { slot: 0, name: "Gyro".into(), system: true, hittable: true, ..Default::default() }],
            ),
            (Location::LeftLeg, leg_slots.clone()),
            (Location::RightLeg, leg_slots),
        ]);
        m
    }

    #[test]
    fn psr_modifier_and_triggers() {
        let mut tm = TrackedMech::new(mover());
        assert_eq!(tm.psr_modifier(), 0);
        assert!(tm.psr_due().is_empty());

        // 20+ damage in a turn owes a PSR; end-turn clears the tally.
        tm.damage(Location::RightTorso, Facing::Front, 20);
        assert!(tm.psr_due().contains(&"20+ dmg"));
        tm.end_turn();
        assert!(!tm.psr_due().contains(&"20+ dmg"));

        // Damage PSR scales +1 per full 20 points taken this turn (house rule). Set the tally
        // directly to isolate the formula from cascade crits.
        let mut dmg = TrackedMech::new(mover());
        dmg.damage_this_turn = 19;
        assert_eq!(dmg.psr_modifier(), 0, "<20 → no PSR");
        dmg.damage_this_turn = 20;
        assert_eq!(dmg.psr_modifier(), 1, "20 → +1");
        dmg.damage_this_turn = 45;
        assert_eq!(dmg.psr_modifier(), 2, "45 → +2");
        dmg.damage_this_turn = 65;
        assert_eq!(dmg.psr_modifier(), 3, "65 → +3");

        // Modifiers stack: gyro +3, hip +2, leg actuator +1, pilot +1.
        tm.toggle_crit(Location::LeftLeg, 0); // Hip
        tm.toggle_crit(Location::LeftLeg, 1); // Upper Leg Actuator
        tm.toggle_crit(Location::CenterTorso, 0); // Gyro
        tm.hit_pilot();
        assert_eq!(tm.psr_modifier(), 3 + 2 + 1 + 1);
    }

    #[test]
    fn movement_mode_tracks_modifiers_and_clears_on_end_turn() {
        let mut tm = TrackedMech::new(mover());
        assert_eq!(tm.attack_move_modifier(), 0);
        assert_eq!(tm.tmm(), 0);

        // Cycle stationary -> walked -> ran; set 7 hexes (run 9 allows it).
        tm.cycle_move_mode(1);
        tm.cycle_move_mode(1);
        tm.adjust_hexes_moved(7);
        assert_eq!(tm.move_mode, MoveMode::Ran);
        assert_eq!(tm.attack_move_modifier(), 2);
        assert_eq!(tm.tmm(), 3); // 7 hexes

        // Jumping adds +1 TMM on top of the hex bracket — and switching mode re-clamps
        // hexes to jump MP (4), so the bracket drops too.
        tm.cycle_move_mode(1);
        assert_eq!(tm.move_mode, MoveMode::Jumped);
        assert_eq!(tm.hexes_moved, 4);
        assert_eq!(tm.attack_move_modifier(), 3);
        assert_eq!(tm.tmm(), 2); // 4 hexes (+1) + jumped (+1)

        // End turn resets to stationary.
        tm.end_turn();
        assert_eq!(tm.move_mode, MoveMode::Stationary);
        assert_eq!(tm.hexes_moved, 0);
    }

    #[test]
    fn hexes_clamp_to_the_modes_effective_mp() {
        let mut tm = TrackedMech::new(mover()); // walk 6 / run 9 / jump 4
        // Stationary means it didn't move — hexes stay 0.
        tm.adjust_hexes_moved(5);
        assert_eq!(tm.hexes_moved, 0);

        tm.cycle_move_mode(1); // walked
        tm.adjust_hexes_moved(20);
        assert_eq!(tm.hexes_moved, 6, "capped at walk MP");

        tm.cycle_move_mode(1); // ran
        tm.adjust_hexes_moved(20);
        assert_eq!(tm.hexes_moved, 9, "capped at run MP");

        // Effective MP, not sheet MP: heat slows the cap down too.
        tm.heat = 5; // -1 MP
        tm.cycle_move_mode(-1); // back to walked (re-clamps)
        assert_eq!(tm.hexes_moved, 5, "walk 6 - 1 heat");
    }

    #[test]
    fn non_jumpers_skip_the_jumped_mode() {
        let mut m = mover();
        m.jump = 0;
        let mut tm = TrackedMech::new(m);
        // ran -> next skips jumped, wraps to stationary.
        tm.cycle_move_mode(1);
        tm.cycle_move_mode(1);
        assert_eq!(tm.move_mode, MoveMode::Ran);
        tm.cycle_move_mode(1);
        assert_eq!(tm.move_mode, MoveMode::Stationary, "no jump MP -> no jumped mode");
        // And backwards from stationary lands on ran, not jumped.
        tm.cycle_move_mode(-1);
        assert_eq!(tm.move_mode, MoveMode::Ran);
    }

    #[test]
    fn immobile_units_stay_stationary() {
        let mut tm = TrackedMech::new(mover());
        tm.cycle_move_mode(1);
        tm.adjust_hexes_moved(3);
        tm.toggle_shutdown();
        tm.cycle_move_mode(1);
        assert_eq!(tm.move_mode, MoveMode::Stationary);
        assert_eq!(tm.hexes_moved, 0);
    }

    #[test]
    fn force_total_follows_the_game_mode() {
        let mut a = atlas();
        a.bv = 1897;
        a.as_stats.pv = 52;
        let mut b = atlas();
        b.bv = 500;
        b.as_stats.pv = 20;

        let mut s = Session::new_with_mode(GameMode::Classic);
        s.mechs = vec![TrackedMech::new(a.clone()), TrackedMech::new(b.clone())];
        assert_eq!(s.force_total(), 1897 + 500, "Classic sums BV");

        let mut s = Session::new_with_mode(GameMode::AlphaStrike);
        s.mechs = vec![TrackedMech::new(a), TrackedMech::new(b)];
        assert_eq!(s.force_total(), 52 + 20, "AS sums PV");
    }

    #[test]
    fn force_total_is_skill_adjusted_and_respects_the_limit() {
        let mut a = atlas();
        a.bv = 1897;
        let mut s = Session::new_with_mode(GameMode::Classic);
        let mut tm = TrackedMech::new(a);
        // Elite 2/3 -> BV table 1.68; round(1897 * 1.68) = 3187.
        tm.gunnery = 2;
        tm.piloting = 3;
        assert_eq!(tm.point_cost(GameMode::Classic), 3187);
        s.mechs = vec![tm];
        assert_eq!(s.force_total(), 3187, "force totals the skill-adjusted cost");

        // No limit -> never over.
        assert!(!s.over_limit());
        assert_eq!(s.remaining(), None);

        s.limit = Some(4000);
        assert!(!s.over_limit());
        assert_eq!(s.remaining(), Some(4000 - 3187));

        s.limit = Some(3000);
        assert!(s.over_limit(), "3187 busts a 3000 limit");
        assert_eq!(s.remaining(), Some(3000 - 3187));
    }

    #[test]
    fn moving_hard_on_damaged_kit_owes_a_psr() {
        let mut tm = TrackedMech::new(mover());
        tm.toggle_crit(Location::LeftLeg, 0); // Hip

        // Walking with a damaged hip owes nothing; running does.
        tm.cycle_move_mode(1); // walked
        assert!(tm.psr_due().is_empty());
        tm.cycle_move_mode(1); // ran
        assert!(tm.psr_due().contains(&"ran w/ gyro/hip dmg"));

        // Jumping with any leg/gyro damage owes the landing roll.
        tm.cycle_move_mode(1); // jumped
        assert!(tm.psr_due().contains(&"jumped w/ leg dmg"));

        // A healthy 'Mech can run/jump freely.
        let mut ok = TrackedMech::new(mover());
        ok.cycle_move_mode(1);
        ok.cycle_move_mode(1);
        assert!(ok.psr_due().is_empty());
    }

    #[test]
    fn destroyed_leg_is_an_auto_fall_not_an_avoid_fall_psr() {
        let mut tm = TrackedMech::new(mover()); // biped
        assert!(tm.auto_fall().is_none());
        // Destroy a leg (8 armor + 6 internal).
        tm.damage(Location::LeftLeg, Facing::Front, 14);
        assert!(tm.is_destroyed(Location::LeftLeg));
        // Reported as an automatic fall, NOT as an avoid-fall PSR trigger.
        assert_eq!(tm.auto_fall(), Some("leg destroyed"));
        assert!(!tm.psr_due().iter().any(|r| r.contains("leg")));
        // The fall still owes a PSR (pilot damage / stand-up) at the full modified target.
        assert!(tm.psr_target() > tm.piloting as i32);
    }

    #[test]
    fn immobile_unit_has_negative_tmm() {
        let mut tm = TrackedMech::new(mover());
        tm.toggle_shutdown();
        assert_eq!(tm.tmm(), -4);
    }

    #[test]
    fn weapon_to_hit_from_data_plus_tc() {
        let mut m = atlas();
        m.weapons = vec![
            WeaponMount {
                id: 0,
                name: "Medium Pulse Laser".into(),
                location: Location::RightArm,
                rear: false,
                heat: 4,
                damage: "6".into(),
                range: "2/4/6".into(),
                crit_slots: 1,
                ammo_key: None,
                to_hit: -2,        // from stats.toHitModifier
                tc_eligible: true, // direct-fire
                count: 1,
            },
            WeaponMount {
                id: 1,
                name: "LRM 20".into(),
                location: Location::LeftTorso,
                rear: false,
                heat: 6,
                damage: "1/msl".into(),
                range: "7/14/21".into(),
                crit_slots: 5,
                ammo_key: Some("LRM:20".into()),
                to_hit: 0,
                tc_eligible: false, // missiles aren't direct-fire -> no TC bonus
                count: 1,
            },
        ];

        // Inherent modifier from the data; no Targeting Computer yet.
        assert!(!m.has_targeting_computer());
        assert_eq!(m.weapon_to_hit(&m.weapons[0]), -2);
        assert_eq!(m.weapon_to_hit(&m.weapons[1]), 0);

        // Mount a Targeting Computer: -1 to the eligible (direct-fire) weapon only.
        m.equipment.push(Equipment { name: "Targeting Computer".into(), location: Location::Head });
        assert!(m.has_targeting_computer());
        assert_eq!(m.weapon_to_hit(&m.weapons[0]), -3); // pulse -2 + TC -1
        assert_eq!(m.weapon_to_hit(&m.weapons[1]), 0); // LRM ineligible
    }

    #[test]
    fn skills_default_clamp_and_psr_target() {
        let mut tm = TrackedMech::new(atlas());
        assert_eq!((tm.gunnery, tm.piloting), (4, 5));
        assert_eq!(tm.psr_target(), 5); // piloting 5 + 0 modifier

        tm.adjust_gunnery(-5); // clamps at 0 (best)
        assert_eq!(tm.gunnery, 0);
        tm.adjust_piloting(10); // clamps at SKILL_MAX (worst)
        assert_eq!(tm.piloting, SKILL_MAX);

        tm.adjust_piloting(-4); // 8 -> 4
        tm.hit_pilot(); // +1 PSR modifier
        assert_eq!(tm.psr_target(), 4 + 1);
    }

    #[test]
    fn movement_reflects_heat_and_leg_loss() {
        let mut tm = TrackedMech::new(mover());
        let m = tm.movement();
        assert_eq!((m.walk, m.run, m.jump), (6, 9, 4));
        assert!(!m.immobile && m.note.is_none());

        // Heat 5 -> -1 walking MP; run recomputed (ceil 1.5x), jump untouched.
        tm.adjust_heat(5);
        let m = tm.movement();
        assert_eq!((m.walk, m.run, m.jump), (5, 8, 4));
        tm.adjust_heat(-5);

        // A hip hit halves walking MP; a second hip (other leg) zeroes it out.
        tm.toggle_crit(Location::LeftLeg, 0); // Hip
        assert_eq!(tm.movement().walk, 3);
        tm.toggle_crit(Location::RightLeg, 0); // second Hip
        assert_eq!(tm.movement().walk, 0, "two hips = 0 walking MP, not halved-again");
        tm.toggle_crit(Location::RightLeg, 0); // repair
        tm.toggle_crit(Location::LeftLeg, 0); // repair

        // One leg gone -> hobble at 1 MP, no run/jump; a destroyed leg doesn't cascade here
        // because the damage exactly matches armor (8) + internal (6).
        tm.damage(Location::LeftLeg, Facing::Front, 14);
        assert!(tm.is_destroyed(Location::LeftLeg));
        let m = tm.movement();
        assert_eq!((m.walk, m.run, m.jump), (1, 0, 0));
        assert_eq!(m.note, Some("leg gone"));

        // Both legs gone -> immobile.
        tm.damage(Location::RightLeg, Facing::Front, 14);
        assert!(tm.movement().immobile);

        // Shutdown immobilizes regardless of legs.
        let mut up = TrackedMech::new(mover());
        up.shutdown = true;
        assert!(up.movement().immobile);
    }

    #[test]
    fn equipment_disabled_by_crit() {
        let mut spec = atlas();
        spec.equipment.push(Equipment {
            name: "ECM Suite (Guardian)".into(),
            location: Location::CenterTorso,
        });
        // Give it a crit slot so a hit there can disable it.
        spec.crit_slots.get_mut(&Location::CenterTorso).unwrap().push(CritSlot {
            slot: 5,
            name: "ECM Suite (Guardian)".into(),
            system: false,
            hittable: true, ..Default::default()
        });
        let mut tm = TrackedMech::new(spec);
        let ecm = tm.spec.equipment[0].clone();
        assert!(!tm.is_equipment_disabled(&ecm));
        tm.toggle_crit(Location::CenterTorso, 5); // destroy the ECM slot
        assert!(tm.is_equipment_disabled(&ecm));
    }

    #[test]
    fn munition_choice_overrides_baked_default() {
        let mut spec = atlas();
        // Bin 0 defaults to a non-standard load baked in; it's in a choice group.
        spec.ammo[0].munition = "Semi-Guided".into();
        spec.ammo[0].base_ammo = Some("AC20".into());
        let mut tm = TrackedMech::new(spec);

        // With no override, the baked munition applies.
        assert_eq!(tm.bin_munition(0), "Semi-Guided");
        assert!(tm.munition_choice.is_empty());

        // Loading a different munition records an override.
        tm.set_bin_munition(0, "Standard");
        assert_eq!(tm.bin_munition(0), "Standard");
        assert_eq!(tm.munition_choice.get(&0).map(String::as_str), Some("Standard"));

        // Loading back the baked default clears the override (keeps saved state minimal).
        tm.set_bin_munition(0, "Semi-Guided");
        assert_eq!(tm.bin_munition(0), "Semi-Guided");
        assert!(tm.munition_choice.is_empty());
    }

    #[test]
    fn roster_cap_and_switch() {
        let mut s = Session::new();
        for _ in 0..MAX_MECHS {
            assert!(s.add_mech(atlas()));
        }
        assert!(!s.add_mech(atlas())); // full at 12
        assert_eq!(s.active, 11);
        s.switch(1);
        assert_eq!(s.active, 0); // wraps
        s.switch(-1);
        assert_eq!(s.active, 11);
    }

    #[test]
    fn alpha_strike_roster_is_uncapped() {
        assert_eq!(Session::mech_cap(GameMode::Classic), Some(MAX_MECHS));
        assert_eq!(Session::mech_cap(GameMode::Override), Some(MAX_MECHS));
        assert_eq!(Session::mech_cap(GameMode::AlphaStrike), None);

        // Alpha Strike keeps accepting past the Classic cap.
        let mut s = Session::new();
        s.mode = GameMode::AlphaStrike;
        for _ in 0..MAX_MECHS + 8 {
            assert!(s.add_mech(atlas()));
        }
        assert_eq!(s.mechs.len(), MAX_MECHS + 8);
    }

    #[test]
    fn session_save_load_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session.json");
        let mut s = Session::new();
        s.add_mech(atlas());
        s.active_mech_mut()
            .unwrap()
            .damage(Location::CenterTorso, Facing::Rear, 5);
        s.active_mech_mut().unwrap().adjust_heat(12);
        save(&path, &s).unwrap();

        let back = load(&path).unwrap().unwrap();
        assert_eq!(back.mechs.len(), 1);
        assert_eq!(
            back.active_mech().unwrap().armor_remaining(Location::CenterTorso, Facing::Rear),
            9
        );
        assert_eq!(back.active_mech().unwrap().heat, 12);
    }

    #[test]
    fn load_missing_is_none() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nope.json");
        assert!(load(&path).unwrap().is_none());
    }

    #[test]
    fn sanitize_names() {
        assert_eq!(sanitize_name("Tuesday Game"), "Tuesday Game");
        assert_eq!(sanitize_name("   "), "session");
        assert_eq!(sanitize_name("ok-name_1"), "ok-name_1");
        // Path-traversal characters are stripped so a name can't escape the sessions dir.
        let evil = sanitize_name("../../etc/passwd");
        assert!(!evil.contains('/') && !evil.contains('.'));
    }

    #[test]
    fn list_sessions_reads_dir() {
        let dir = tempfile::tempdir().unwrap();
        let mut a = Session::new();
        a.add_mech(atlas());
        a.add_mech(atlas());
        save(&dir.path().join("lance.json"), &a).unwrap();
        save(&dir.path().join("empty.json"), &Session::new()).unwrap();
        std::fs::write(dir.path().join("notes.txt"), b"ignore me").unwrap();

        let metas = list_sessions_in(dir.path());
        assert_eq!(metas.len(), 2); // .txt ignored
        let lance = metas.iter().find(|m| m.name == "lance").unwrap();
        assert_eq!(lance.mech_count, 2);
        assert!(lance.summary.contains("Atlas"));
        let empty = metas.iter().find(|m| m.name == "empty").unwrap();
        assert_eq!(empty.mech_count, 0);
        assert_eq!(empty.summary, "empty");
    }

    // ---- Strategic BattleForce (Phase 3) ----

    fn sbf_atlas() -> crate::domain::Mech {
        crate::domain::Mech {
            chassis: "Atlas".into(),
            model: "AS7-D".into(),
            as_stats: crate::domain::AsStats {
                tp: "BM".into(),
                size: 4,
                movement: "6\"".into(),
                armor: 10,
                structure: 8,
                dmg_s: "5".into(),
                dmg_m: "5".into(),
                dmg_l: "2".into(),
                dmg_e: "0".into(),
                pv: 52,
                specials: ["AC2/2/-", "IF1", "LRM1/1/1", "REAR1/1/-"].map(String::from).to_vec(),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    /// An SBF session with `n` Atlas elements in the pool and one Lance-shaped formation over
    /// them. (The seeded empty starter formation is dropped so `formations[0]` is the Lance.)
    fn sbf_session(n: usize) -> Session {
        let mut s = Session::new_with_mode(GameMode::StrategicBattleForce);
        s.sbf.formations.clear();
        for _ in 0..n {
            s.mechs.push(TrackedMech::new(sbf_atlas()));
        }
        s.sbf_new_formation("Lance", 0..n);
        s
    }

    #[test]
    fn sbf_serde_round_trip() {
        let s = sbf_session(4);
        let json = serde_json::to_string(&s).unwrap();
        let back: Session = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);
        assert_eq!(back.mode, GameMode::StrategicBattleForce);
        assert_eq!(back.sbf.formations.len(), 1);
        assert_eq!(back.sbf.formations[0].units[0].elements, vec![0, 1, 2, 3]);
    }

    #[test]
    fn sbf_derived_stats_are_pure() {
        let mut s = sbf_session(4);
        let unit_before = s.sbf_unit(&s.sbf.formations[0].units[0]);
        // Damaging the live state must NOT change the derived (immutable) stat line.
        let u = &mut s.sbf.formations[0].units[0];
        let _ = u.apply_damage(&unit_before, 10);
        u.add_damage_crit();
        u.add_targeting_crit();
        let unit_after = s.sbf_unit(&s.sbf.formations[0].units[0]);
        assert_eq!(unit_before, unit_after);
    }

    #[test]
    fn sbf_damage_repair_and_crit_counters() {
        let s = sbf_session(4);
        let derived = s.sbf_unit(&s.sbf.formations[0].units[0]); // Atlas lance: armor 25, damage 7/7/3
        let mut u = SbfUnitState::default();
        assert_eq!(derived.armor, 25);
        // apply_damage fills the armor pool and returns the overflow (§4.2 spillover).
        assert_eq!(u.apply_damage(&derived, 10), 0);
        assert_eq!(u.armor_remaining(&derived), 15);
        assert_eq!(u.apply_damage(&derived, 100), 85);
        assert_eq!(u.armor_remaining(&derived), 0);
        assert!(u.is_destroyed(&derived));
        u.repair(5);
        assert_eq!(u.armor_remaining(&derived), 5);
        // A damage crit steps every band down by 1; a targeting crit worsens base gunnery.
        u.add_damage_crit();
        assert_eq!(u.current_damage(&derived), DamageVector { s: 6.0, m: 6.0, l: Some(2.0), e: None });
        assert_eq!(u.base_gunnery(&derived), 4);
        u.add_targeting_crit();
        assert_eq!(u.base_gunnery(&derived), 5);
    }

    #[test]
    fn sbf_structural_validity() {
        // A valid Lance (1 unit of 4) passes.
        let s = sbf_session(4);
        assert!(s.sbf_can_convert(&s.sbf.formations[0]));

        // 21 total elements → fails (≤20).
        let big = sbf_session(21);
        assert!(!big.sbf_can_convert(&big.sbf.formations[0]));

        // 5 units → fails (1–4 units).
        let mut five = sbf_session(4);
        five.sbf.formations[0].units = (0..5)
            .map(|i| SbfUnitState { name: format!("U{i}"), elements: vec![i % 4], ..Default::default() })
            .collect();
        assert!(!five.sbf_can_convert(&five.sbf.formations[0]));

        // A 7-element unit → fails (1–6 per unit).
        let mut seven = sbf_session(7);
        seven.sbf.formations[0].units = vec![SbfUnitState {
            name: "U".into(),
            elements: (0..7).collect(),
            ..Default::default()
        }];
        assert!(!seven.sbf_can_convert(&seven.sbf.formations[0]));
    }

    #[test]
    fn sbf_absent_from_old_session_json() {
        // A pre-SBF session JSON (no `sbf` key, no `mode`) loads with an empty default SbfState.
        let json = r#"{"version":1,"mechs":[],"active":0}"#;
        let s: Session = serde_json::from_str(json).unwrap();
        assert_eq!(s.mode, GameMode::Classic);
        assert_eq!(s.sbf, SbfState::default());
        assert!(s.sbf.formations.is_empty());
    }

    // ---- Strategic BattleForce (Phase 4) ----

    /// A conventional-infantry element (formation-level CI is exempt from the withdrawal hint).
    fn sbf_ci() -> crate::domain::Mech {
        crate::domain::Mech {
            chassis: "Rifle Platoon".into(),
            model: "".into(),
            as_stats: crate::domain::AsStats {
                tp: "CI".into(),
                size: 1,
                movement: "4\"f".into(),
                armor: 4,
                structure: 1,
                dmg_s: "2".into(),
                dmg_m: "0".into(),
                dmg_l: "0".into(),
                dmg_e: "0".into(),
                pv: 8,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn sbf_spillover_chains_across_units() {
        // 8 elements → two units (6 + 2). Overflow from the first spills into the second and only
        // an un-absorbable remainder comes back (§4.2 — damage carries over, never discarded).
        let mut s = sbf_session(8);
        let a0 = s.sbf_unit(&s.sbf.formations[0].units[0]).armor;
        let a1 = s.sbf_unit(&s.sbf.formations[0].units[1]).armor;
        assert!(a0 > 0 && a1 > 3);

        // Packet bigger than unit 0: fills it, chains 3 into unit 1, nothing comes back.
        assert_eq!(s.sbf_apply_damage_chain(0, &[0, 1], a0 + 3), 0);
        let f = &s.sbf.formations[0];
        assert!(f.units[0].is_destroyed(&s.sbf_unit(&f.units[0])));
        assert_eq!(f.units[1].armor_remaining(&s.sbf_unit(&f.units[1])), a1 - 3);

        // Both units cap: the exact un-absorbable remainder returns (out-of-range index skipped).
        assert_eq!(s.sbf_apply_damage_chain(0, &[0, 1, 9], (a1 - 3) + 7), 7);
        let f = &s.sbf.formations[0];
        assert!(s.sbf_formation_eliminated(f));
        // Elimination is a state, not a removal — the formation is still on the sheet.
        assert_eq!(s.sbf.formations.len(), 1);
    }

    #[test]
    fn sbf_crit_gate_and_application() {
        let s = sbf_session(4);
        let derived = s.sbf_unit(&s.sbf.formations[0].units[0]); // armor 25
        let mut u = SbfUnitState::default();
        // Gate: a crit roll is owed only once current armor is below half of full (§4.2).
        assert!(!u.crit_check_due(&derived));
        let _ = u.apply_damage(&derived, 12); // 13 remaining, 13*2 >= 25
        assert!(!u.crit_check_due(&derived));
        let _ = u.apply_damage(&derived, 1); // 12 remaining, 24 < 25
        assert!(u.crit_check_due(&derived));

        // Each table result maps onto exactly its counters; effects are immediate (§4.2 timing).
        u.apply_crit(&derived, sbf::SbfCrit::None);
        assert_eq!((u.targeting_crits, u.damage_crits), (0, 0));
        u.apply_crit(&derived, sbf::SbfCrit::Targeting);
        assert_eq!((u.targeting_crits, u.damage_crits), (1, 0));
        u.apply_crit(&derived, sbf::SbfCrit::Damage);
        assert_eq!((u.targeting_crits, u.damage_crits), (1, 1));
        u.apply_crit(&derived, sbf::SbfCrit::Both);
        assert_eq!((u.targeting_crits, u.damage_crits), (2, 2));
        u.apply_crit(&derived, sbf::SbfCrit::Destroyed);
        assert!(u.is_destroyed(&derived));
        // A destroyed unit owes no further crit rolls.
        assert!(!u.crit_check_due(&derived));

        // MP crits: −1 MP each, floored at 0 → immobile (no minimum-move floor of 1).
        let mut m = SbfUnitState::default();
        assert_eq!(m.current_movement(&derived), derived.movement);
        for _ in 0..derived.movement + 2 {
            m.add_mp_crit();
        }
        assert_eq!(m.current_movement(&derived), 0);
    }

    #[test]
    fn sbf_morale_is_a_manual_rung() {
        // The ladder cycles by hand: no roll, no TN, terminal ends both ways.
        let mut m = MoraleStatus::Normal;
        let down = [MoraleStatus::Shaken, MoraleStatus::Broken, MoraleStatus::Routed, MoraleStatus::Routed];
        for want in down {
            m = m.worsened();
            assert_eq!(m, want);
        }
        let up = [MoraleStatus::Broken, MoraleStatus::Shaken, MoraleStatus::Normal, MoraleStatus::Normal];
        for want in up {
            m = m.improved();
            assert_eq!(m, want);
        }

        // Nothing automatic touches the rung: even a crit-12 (destroyed) leaves morale alone (§4.3).
        let mut s = sbf_session(4);
        let derived = s.sbf_unit(&s.sbf.formations[0].units[0]);
        s.sbf.formations[0].units[0].apply_crit(&derived, sbf::SbfCrit::Destroyed);
        assert_eq!(s.sbf.formations[0].morale, MoraleStatus::Normal);
    }

    #[test]
    fn sbf_crippled_thresholds() {
        // Prong 1 — ≥ half the elements reduced to zero damage. Atlas 5/5/2: 4 damage crits leave
        // 1/1/0 (not zero); the 5th zeroes every element in the unit → 4 of 4 ≥ ceil(4/2).
        let mut s = sbf_session(4);
        s.sbf.formations[0].units[0].damage_crits = 4;
        assert!(!s.sbf_is_crippled(&s.sbf.formations[0]));
        s.sbf.formations[0].units[0].damage_crits = 5;
        assert!(s.sbf_is_crippled(&s.sbf.formations[0]));

        // Prong 2 — ≥ half the armored non-infantry units gutted; 3 units → threshold ceil(3/2)=2.
        let mut s = sbf_session(6);
        s.sbf.formations[0].units = (0..3)
            .map(|i| SbfUnitState {
                name: format!("U{i}"),
                elements: vec![i * 2, i * 2 + 1],
                ..Default::default()
            })
            .collect();
        let gut = |s: &mut Session, ui: usize| {
            let d = s.sbf_unit(&s.sbf.formations[0].units[ui]);
            assert_eq!(s.sbf.formations[0].units[ui].apply_damage(&d, d.armor), 0);
        };
        gut(&mut s, 0);
        assert!(!s.sbf_is_crippled(&s.sbf.formations[0]), "1 of 3 gutted is under threshold");
        gut(&mut s, 1);
        assert!(s.sbf_is_crippled(&s.sbf.formations[0]), "2 of 3 gutted crosses ceil(3/2)");

        // Prong 3 — ≥ half the units with ≥2 targeting crits; 2 units → threshold 1.
        let mut s = sbf_session(8); // units of 6 + 2
        s.sbf.formations[0].units[0].targeting_crits = 1;
        assert!(!s.sbf_is_crippled(&s.sbf.formations[0]));
        s.sbf.formations[0].units[0].targeting_crits = 2;
        assert!(s.sbf_is_crippled(&s.sbf.formations[0]));
    }

    #[test]
    fn sbf_withdrawal_hint_flags_only() {
        // Routed or crippled non-infantry → hint on; it triggers nothing.
        let mut s = sbf_session(4);
        assert!(!s.sbf_would_withdraw(&s.sbf.formations[0]));
        s.sbf.formations[0].morale = MoraleStatus::Routed;
        assert!(s.sbf_would_withdraw(&s.sbf.formations[0]));
        s.sbf.formations[0].morale = MoraleStatus::Normal;
        s.sbf.formations[0].units[0].damage_crits = 5; // crippled via prong 1
        assert!(s.sbf_would_withdraw(&s.sbf.formations[0]));
        assert_eq!(s.sbf.formations.len(), 1, "a hint, not a removal");

        // BA/CI formations are exempt even when Routed.
        let mut ci = Session::new_with_mode(GameMode::StrategicBattleForce);
        ci.sbf.formations.clear(); // drop the seeded starter; [0] must be the CI formation
        for _ in 0..4 {
            ci.mechs.push(TrackedMech::new(sbf_ci()));
        }
        ci.sbf_new_formation("Foot", 0..4);
        ci.sbf.formations[0].morale = MoraleStatus::Routed;
        assert!(!ci.sbf_would_withdraw(&ci.sbf.formations[0]));
    }

    #[test]
    fn sbf_remove_mech_remaps_element_indices() {
        // The pool is index-referenced; deleting a mech must remap every SBF grouping or the
        // consumers panic on / silently read the wrong mech (review finding, 2026-07-03).
        let mut s = sbf_session(8); // units: [0..6], [6,7]
        s.remove_mech(3);
        let f = &s.sbf.formations[0];
        assert_eq!(f.units[0].elements, vec![0, 1, 2, 3, 4]);
        assert_eq!(f.units[1].elements, vec![5, 6]);
        // Every Phase-4 consumer walks the pool without panicking.
        assert!(!s.sbf_is_crippled(&s.sbf.formations[0]));
        assert!(!s.sbf_formation_eliminated(&s.sbf.formations[0]));
        let _ = s.sbf_force_pv();
        assert_eq!(s.sbf_apply_damage_chain(0, &[0], 1), 0);

        // Removing a unit's last element drops the unit; the formation stays as an empty
        // workspace (first-class — deleted only explicitly).
        let mut s = sbf_session(1);
        s.remove_mech(0);
        assert_eq!(s.sbf.formations.len(), 1);
        assert!(s.sbf.formations[0].units.is_empty());
        assert_eq!(s.sbf.active_formation, 0);
    }

    #[test]
    fn sbf_session_starts_with_an_empty_formation() {
        // A fresh SBF session has one formation on the sheet before any elements exist.
        let s = Session::new_with_mode(GameMode::StrategicBattleForce);
        assert_eq!(s.sbf.formations.len(), 1);
        assert_eq!(s.sbf.formations[0].name, "Formation 1");
        assert!(s.sbf.formations[0].units.is_empty());
        assert!(!s.sbf_formation_eliminated(&s.sbf.formations[0]));
        assert_eq!(s.sbf_force_pv(), 0);
        // Other modes are unaffected.
        assert!(Session::new_with_mode(GameMode::AlphaStrike).sbf.formations.is_empty());
    }

    // ---- Abstract Combat System (Phase 2) ----

    /// An ACS session with `n` Atlas elements pooled and one default-nested Formation over them.
    fn acs_session(n: usize) -> Session {
        let mut s = Session::new_with_mode(GameMode::AbstractCombatSystem);
        s.acs.formations.clear();
        for _ in 0..n {
            s.mechs.push(TrackedMech::new(sbf_atlas()));
        }
        s.acs_new_formation("Regiment", 0..n);
        s
    }

    #[test]
    fn acs_session_starts_with_an_empty_formation() {
        let s = Session::new_with_mode(GameMode::AbstractCombatSystem);
        assert_eq!(s.acs.formations.len(), 1);
        assert_eq!(s.acs.formations[0].name, "Formation 1");
        assert!(s.acs.formations[0].units.is_empty());
        assert_eq!(s.acs_force_pv(), 0);
        // Other modes carry no ACS state.
        assert!(Session::new_with_mode(GameMode::AlphaStrike).acs.formations.is_empty());
    }

    #[test]
    fn acs_formation_is_aerospace_flags_aero_and_not_ground() {
        // A ground Formation (Atlas elements) is not aerospace.
        let ground = acs_session(4);
        assert!(!ground.acs_formation_is_aerospace(&ground.acs.formations[0]));
        // A Formation of aerospace fighters (AF) converts to an As-typed Formation → flagged.
        let mut aero = Session::new_with_mode(GameMode::AbstractCombatSystem);
        aero.acs.formations.clear();
        for _ in 0..4 {
            aero.mechs.push(TrackedMech::new(sbf_aero()));
        }
        aero.acs_new_formation("Wing", 0..4);
        assert!(aero.acs_formation_is_aerospace(&aero.acs.formations[0]));
        // The seeded empty Formation is never aerospace.
        let empty = Session::new_with_mode(GameMode::AbstractCombatSystem);
        assert!(!empty.acs_formation_is_aerospace(&empty.acs.formations[0]));
    }

    #[test]
    fn acs_serde_round_trip_preserves_nested_grouping() {
        let s = acs_session(6);
        let json = serde_json::to_string(&s).unwrap();
        let back: Session = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);
        assert_eq!(back.mode, GameMode::AbstractCombatSystem);
        // 6 elements → SBF Units of ≤4 → [4,2], one Team, one Combat Unit, one Formation.
        let f = &back.acs.formations[0];
        assert_eq!(f.units.len(), 1);
        assert_eq!(f.units[0].teams.len(), 1);
        assert_eq!(f.units[0].teams[0].units.len(), 2);
        assert_eq!(f.units[0].teams[0].units[0].elements, vec![0, 1, 2, 3]);
        assert_eq!(f.units[0].teams[0].units[1].elements, vec![4, 5]);
    }

    #[test]
    fn acs_derived_stats_are_pure() {
        let mut s = acs_session(4);
        let cu_before = s.acs_combat_unit(&s.acs.formations[0].units[0]);
        // Mutating the live counters must not change the derived (immutable) stat line.
        let cu = &mut s.acs.formations[0].units[0];
        let _ = cu.apply_damage(&cu_before, 5);
        cu.add_fatigue(2.5);
        cu.morale = AcsMorale::Broken;
        let cu_after = s.acs_combat_unit(&s.acs.formations[0].units[0]);
        assert_eq!(cu_before, cu_after);
    }

    #[test]
    fn acs_damage_fills_armor_and_flags_threshold_crossings() {
        let mut s = acs_session(4);
        let cu = s.acs_combat_unit(&s.acs.formations[0].units[0]);
        let thresholds = cu.damage_thresholds;
        let armor = cu.armor;
        assert!(armor > 0 && thresholds[0] < armor, "sane derived armor/thresholds");
        let st = &mut s.acs.formations[0].units[0];
        // One big hit that crosses the first threshold exactly once.
        let dmg = armor - thresholds[0] + 1;
        let crossed = st.apply_damage(&cu, dmg);
        assert_eq!(crossed, 1, "crossed the 75% mark once");
        assert_eq!(st.armor_remaining(&cu), armor - dmg);
        assert!(!st.is_destroyed(&cu));
        // Overkill destroys and never underflows.
        let crossed2 = st.apply_damage(&cu, armor * 10);
        assert!(st.is_destroyed(&cu));
        assert_eq!(st.armor_remaining(&cu), 0);
        assert!(crossed2 >= 1, "the remaining thresholds fall on destruction");
    }

    #[test]
    fn acs_fatigue_is_stored_doubled() {
        let mut st = AcsCombatUnitState::default();
        st.add_fatigue(0.5);
        st.add_fatigue(2.0);
        assert_eq!(st.fatigue_points(), 2.5);
        assert_eq!(st.fatigue_points_x2, 5);
    }

    #[test]
    fn acs_remove_mech_remaps_nested_grouping() {
        let mut s = acs_session(6); // Units [0,1,2,3] and [4,5]
        s.remove_mech(2); // drop element 2; higher indices shift down
        let u = &s.acs.formations[0].units[0].teams[0].units;
        assert_eq!(u[0].elements, vec![0, 1, 2], "3→2, and 2 removed");
        assert_eq!(u[1].elements, vec![3, 4], "4→3, 5→4");
    }

    #[test]
    fn acs_assign_element_moves_across_all_four_tiers() {
        let mut s = Session::new_with_mode(GameMode::AbstractCombatSystem);
        s.acs.formations.clear();
        for _ in 0..3 {
            s.mechs.push(TrackedMech::new(sbf_atlas()));
        }
        // Each element into its own Formation → three Formations.
        for e in 0..3 {
            s.acs_assign_element(e, AcsAssign::NewFormation);
        }
        assert_eq!(s.acs.formations.len(), 3);
        assert_eq!(s.acs_element_assignment(1), Some((1, 0, 0, 0)));
        // Merge element 2 into element 0's SBF Unit (they now share one Unit — affects ÷3 rounding).
        s.acs_assign_element(2, AcsAssign::Unit(0, 0, 0, 0));
        s.acs_prune_empty();
        // The vacated Formation stays as an empty workspace (first-class, deleted only via D), but
        // its emptied Combat Unit/Team/SBF-Unit are pruned away.
        assert_eq!(s.acs.formations.len(), 3);
        assert!(s.acs.formations[2].units.is_empty(), "vacated Formation is an empty workspace");
        let u = &s.acs.formations[0].units[0].teams[0].units[0];
        assert_eq!(u.elements, vec![0, 2]);
        // Split element 2 into a NEW Combat Team within element 0's Combat Unit.
        s.acs_assign_element(2, AcsAssign::NewTeam(0, 0));
        assert_eq!(s.acs.formations[0].units[0].teams.len(), 2);
        assert_eq!(s.acs_element_assignment(2), Some((0, 0, 1, 0)));
        // Unassign it entirely.
        s.acs_assign_element(2, AcsAssign::Unassign);
        s.acs_prune_empty();
        assert_eq!(s.acs_element_assignment(2), None);
    }

    #[test]
    fn acs_commander_is_unique_across_the_force() {
        let mut s = acs_session(4);
        s.acs_new_formation("Second", 0..0); // an empty second formation
        s.acs.formations[0].units.push(AcsCombatUnitState {
            name: "cu2".into(),
            teams: vec![AcsTeamGrouping {
                name: "t".into(),
                units: vec![AcsUnitGrouping { name: "u".into(), elements: vec![0] }],
            }],
            ..Default::default()
        });
        s.acs_set_commander(0, 0);
        s.acs_set_commander(0, 1); // reassigning clears the first
        assert!(!s.acs.formations[0].units[0].is_commander);
        assert!(s.acs.formations[0].units[1].is_commander);
    }

    #[test]
    fn sbf_skill_drives_unit_pv_through_the_average() {
        // Step 1H/p.260: unit PV = round(sum base PV / 3) scaled by the UNIT skill — which is the
        // jround-average of element skills (Step 1G). A single-element unit shows the scaling
        // directly; one bump inside a 4-element lance vanishes in the rounding (4.25 → 4).
        let mut solo = Session::new_with_mode(GameMode::StrategicBattleForce);
        solo.mechs.push(TrackedMech::new(sbf_atlas()));
        solo.sbf_assign_element(0, SbfAssign::NewUnit(0));
        let pv_at = |s: &Session| s.sbf_unit(&s.sbf.formations[0].units[0]).point_value;
        assert_eq!(pv_at(&solo), 17, "round(52/3) at skill 4");
        solo.mechs[0].gunnery = 5;
        assert_eq!(pv_at(&solo), 15, "×0.9 per skill point above 4");
        solo.mechs[0].gunnery = 3;
        assert_eq!(pv_at(&solo), 20, "×1.2 per point below 4 (on the rounded intermediate)");

        let mut lance = sbf_session(4); // one unit of 4, PV 69
        let before = lance.sbf_unit(&lance.sbf.formations[0].units[0]).point_value;
        lance.mechs[0].gunnery = 5; // avg 4.25 → jround 4 → unchanged (rules-correct)
        let after = lance.sbf_unit(&lance.sbf.formations[0].units[0]).point_value;
        assert_eq!(before, after, "single bump in a lance rounds away");
        for tm in &mut lance.mechs {
            tm.gunnery = 5; // avg 5 → the whole unit worsens
        }
        let all = lance.sbf_unit(&lance.sbf.formations[0].units[0]).point_value;
        assert_eq!(all, 62, "69 × 0.9 once every element worsens");
    }

    /// An aerospace-fighter element for the ground/aero partition tests.
    fn sbf_aero() -> crate::domain::Mech {
        crate::domain::Mech {
            chassis: "Visigoth".into(),
            model: "C".into(),
            as_stats: crate::domain::AsStats {
                tp: "AF".into(),
                size: 2,
                movement: "10a".into(),
                armor: 6,
                structure: 2,
                dmg_s: "3".into(),
                dmg_m: "3".into(),
                dmg_l: "2".into(),
                dmg_e: "1".into(),
                pv: 30,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn sbf_manual_assignment() {
        // 8 elements, one formation (units 6+2). Move element 0 around the org chart.
        let mut s = sbf_session(8);
        assert_eq!(s.sbf_element_assignment(0), Some((0, 0)));

        // Into the second unit.
        s.sbf_assign_element(0, SbfAssign::Unit(0, 1));
        assert_eq!(s.sbf_element_assignment(0), Some((0, 1)));
        assert_eq!(s.sbf.formations[0].units[1].elements, vec![6, 7, 0]);

        // Split into a new unit of the same formation.
        s.sbf_assign_element(0, SbfAssign::NewUnit(0));
        assert_eq!(s.sbf_element_assignment(0), Some((0, 2)));

        // Into a brand-new formation.
        s.sbf_assign_element(0, SbfAssign::NewFormation);
        assert_eq!(s.sbf.formations.len(), 2);
        assert_eq!(s.sbf_element_assignment(0), Some((1, 0)));

        // Unassign entirely; the emptied unit/formation survive until pruned.
        s.sbf_assign_element(0, SbfAssign::Unassign);
        assert_eq!(s.sbf_element_assignment(0), None);
        assert_eq!(s.sbf.formations.len(), 2, "no pruning mid-edit");
        s.sbf_prune_empty_units();
        assert_eq!(s.sbf.formations.len(), 2, "empty FORMATIONS persist (first-class workspaces)");
        assert!(s.sbf.formations[1].units.is_empty());
        assert_eq!(s.sbf.formations[0].units.len(), 2, "empty split unit pruned");
        // Empty formations are not casualties and carry no PV.
        assert!(!s.sbf_formation_eliminated(&s.sbf.formations[1]));
        let _ = s.sbf_force_pv(); // no panic, empties skipped

        // Out-of-range element index is a no-op.
        s.sbf_assign_element(99, SbfAssign::NewFormation);
        assert_eq!(s.sbf.formations.len(), 2);
    }

    #[test]
    fn sbf_doctrine_grouping() {
        // Inner Sphere: 8 ground → one Company of two Lances of 4.
        let mut s = sbf_session(8);
        s.sbf_group_doctrine(SbfDoctrine::InnerSphere);
        assert_eq!(s.sbf.formations.len(), 1);
        assert_eq!(s.sbf.formations[0].name, "Company 1");
        let sizes: Vec<usize> =
            s.sbf.formations[0].units.iter().map(|u| u.elements.len()).collect();
        assert_eq!(sizes, vec![4, 4]);
        assert_eq!(s.sbf.formations[0].units[0].name, "Lance 1");

        // Clan: 8 → Star 5 + Star 3 = a Binary. 15 → a Trinary of three full Stars.
        s.sbf_group_doctrine(SbfDoctrine::Clan);
        assert_eq!(s.sbf.formations[0].name, "Binary 1");
        let sizes: Vec<usize> =
            s.sbf.formations[0].units.iter().map(|u| u.elements.len()).collect();
        assert_eq!(sizes, vec![5, 3]);
        let mut s15 = sbf_session(15);
        s15.sbf_group_doctrine(SbfDoctrine::Clan);
        assert_eq!(s15.sbf.formations[0].name, "Trinary 1");
        assert_eq!(s15.sbf.formations[0].units.len(), 3);

        // ComStar: 8 → Level III of Level II (6) + Level II (2).
        s.sbf_group_doctrine(SbfDoctrine::ComStar);
        assert_eq!(s.sbf.formations[0].name, "Level III 1");
        let sizes: Vec<usize> =
            s.sbf.formations[0].units.iter().map(|u| u.elements.len()).collect();
        assert_eq!(sizes, vec![6, 2]);

        // 13 IS ground → Company (3 lances: 4+4+4) + a lone Lance formation.
        let mut s13 = sbf_session(13);
        s13.sbf_group_doctrine(SbfDoctrine::InnerSphere);
        assert_eq!(s13.sbf.formations.len(), 2);
        assert_eq!(s13.sbf.formations[0].units.len(), 3);
        assert_eq!(s13.sbf.formations[1].name, "Lance 2");
        assert_eq!(s13.sbf.formations[1].units[0].elements, vec![12]);
    }

    #[test]
    fn sbf_doctrine_separates_aerospace() {
        // 4 ground + 4 aero: ground and aerospace never share a formation (can_convert rule);
        // aero groups as Flights of 2 under a Squadron.
        let mut s = sbf_session(4);
        for _ in 0..4 {
            s.mechs.push(TrackedMech::new(sbf_aero()));
        }
        s.sbf_group_doctrine(SbfDoctrine::InnerSphere);
        assert_eq!(s.sbf.formations.len(), 2);
        assert_eq!(s.sbf.formations[0].name, "Lance 1");
        assert_eq!(s.sbf.formations[1].name, "Squadron 2");
        let aero_sizes: Vec<usize> =
            s.sbf.formations[1].units.iter().map(|u| u.elements.len()).collect();
        assert_eq!(aero_sizes, vec![2, 2]);
        assert_eq!(s.sbf.formations[1].units[0].name, "Flight 1");
        assert!(s.sbf_can_convert(&s.sbf.formations[0]));
        assert!(s.sbf_can_convert(&s.sbf.formations[1]));
    }

    /// An all-aero session with one formation of `flights` units, `per_flight` elements each.
    fn sbf_squadron(flights: usize, per_flight: usize) -> Session {
        let mut s = Session::new_with_mode(GameMode::StrategicBattleForce);
        s.sbf.formations.clear();
        for _ in 0..flights * per_flight {
            s.mechs.push(TrackedMech::new(sbf_aero()));
        }
        let units: Vec<SbfUnitState> = (0..flights)
            .map(|k| SbfUnitState {
                name: format!("Flight {}", k + 1),
                elements: (k * per_flight..(k + 1) * per_flight).collect(),
                ..Default::default()
            })
            .collect();
        s.sbf.formations.push(SbfFormationState {
            name: "Squadron".into(),
            units,
            ..Default::default()
        });
        s
    }

    #[test]
    fn sbf_aero_squadron_caps() {
        // SAS structure (IO:BF p.177): an all-aerospace formation validates as a Squadron —
        // ≤6 Flights of ≤2 elements, ≤12 total — where the same shape would bust the ground
        // 4-unit cap.
        let full = sbf_squadron(6, 2);
        assert!(full.sbf_can_convert(&full.sbf.formations[0]), "6 Flights / 12 elements pass");

        // A 3-element Flight is over the ≤2-per-Flight cap, even though 3 ≤ the ground 6.
        let fat = sbf_squadron(2, 3);
        assert!(!fat.sbf_can_convert(&fat.sbf.formations[0]), "3-element Flights are rejected");

        // A 7th Flight busts the ≤6-Flight (and ≤12-element) Squadron cap.
        let wing = sbf_squadron(7, 2);
        assert!(!wing.sbf_can_convert(&wing.sbf.formations[0]));

        // Ground formations keep the ground caps exactly as-is: 5 units still fail (1–4) even
        // though a Squadron could hold 5 Flights…
        let mut five = sbf_session(5);
        five.sbf.formations[0].units = (0..5)
            .map(|i| SbfUnitState { name: format!("U{i}"), elements: vec![i], ..Default::default() })
            .collect();
        assert!(!five.sbf_can_convert(&five.sbf.formations[0]));
        // …and a 4-element ground Lance still passes (>2 per unit is aero-only law).
        let lance = sbf_session(4);
        assert!(lance.sbf_can_convert(&lance.sbf.formations[0]));
    }

    #[test]
    fn sbf_commander_and_leader_are_unique() {
        // 8 elements → units (6+2). COM is force-unique, LEAD formation-unique, both toggle.
        let mut s = sbf_session(8);
        s.sbf_assign_element(7, SbfAssign::NewFormation); // a second formation to cross
        s.sbf_set_commander(0, 0);
        assert!(s.sbf.formations[0].units[0].is_commander);
        s.sbf_set_commander(1, 0); // moves force-wide
        assert!(!s.sbf.formations[0].units[0].is_commander);
        assert!(s.sbf.formations[1].units[0].is_commander);
        s.sbf_set_commander(1, 0); // toggle off
        assert!(!s.sbf.formations[1].units[0].is_commander);

        s.sbf_set_leader(0, 0);
        s.sbf_set_leader(0, 1); // moves within the formation
        assert!(!s.sbf.formations[0].units[0].is_leader);
        assert!(s.sbf.formations[0].units[1].is_leader);
        s.sbf_set_leader(0, 1); // toggle off
        assert!(!s.sbf.formations[0].units[1].is_leader);

        // The defender +2 Tactics hint keys off either flag being present.
        assert!(!s.sbf_has_com_or_lead(&s.sbf.formations[0]));
        s.sbf_set_leader(0, 0);
        assert!(s.sbf_has_com_or_lead(&s.sbf.formations[0]));

        // Round-trips (serde defaults keep old sessions loading).
        let json = serde_json::to_string(&s).unwrap();
        let back: Session = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);
    }

    #[test]
    fn sbf_turn_round_tracker() {
        // begin_round bumps the counter and re-arms every formation; morale/armor/crits persist.
        let mut s = sbf_session(8);
        s.sbf_new_formation("Second", 6..8);
        s.sbf.formations[0].morale = MoraleStatus::Shaken;
        s.sbf.formations[0].units[0].damage_crits = 1;
        s.sbf.formations[0].jump_used_this_turn = 2;
        s.sbf.end_turn(0);
        s.sbf.end_turn(1);
        s.sbf.end_turn(9); // out of range: no-op
        assert!(s.sbf.formations.iter().all(|f| f.is_done));

        s.sbf.begin_round();
        assert_eq!(s.sbf.round, 1);
        assert!(s.sbf.formations.iter().all(|f| !f.is_done));
        assert_eq!(s.sbf.formations[0].jump_used_this_turn, 0);
        assert_eq!(s.sbf.formations[0].morale, MoraleStatus::Shaken);
        assert_eq!(s.sbf.formations[0].units[0].damage_crits, 1);

        // begin_phase re-arms is_done mid-round without touching the round counter.
        s.sbf.end_turn(0);
        s.sbf.begin_phase();
        assert!(!s.sbf.formations[0].is_done);
        assert_eq!(s.sbf.round, 1);
    }

    // ---- Standard BattleForce (spec §2.4) ----

    /// A Standard-BF test element: type / movement / specials vary per test; the fixed stats are
    /// size 2, armor 5, structure 3, damage 3/3/1/−, PV 30.
    fn bf_mech(tp: &str, movement: &str, specials: &[&str]) -> crate::domain::Mech {
        crate::domain::Mech {
            chassis: "BF Test".into(),
            model: tp.into(),
            as_stats: crate::domain::AsStats {
                tp: tp.into(),
                size: 2,
                movement: movement.into(),
                armor: 5,
                structure: 3,
                dmg_s: "3".into(),
                dmg_m: "3".into(),
                dmg_l: "1".into(),
                dmg_e: "-".into(),
                pv: 30,
                specials: specials.iter().map(|s| s.to_string()).collect(),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    /// A BF session with `n` [`bf_mech`] BattleMechs (MV 16″ → 8 hexes) in the pool and one Unit
    /// over them. (The seeded empty starter "Unit 1" is dropped so `units[0]` is the Unit.)
    fn bf_session(n: usize) -> Session {
        let mut s = Session::new_with_mode(GameMode::BattleForce);
        s.bf.units.clear();
        for _ in 0..n {
            s.mechs.push(TrackedMech::new(bf_mech("BM", "16\"", &[])));
        }
        s.bf_new_unit("Unit A", 0..n);
        s
    }

    #[test]
    fn bf_serde_round_trip() {
        // A populated BfState + per-element BfLive survives the JSON round trip intact.
        let mut s = bf_session(2);
        s.bf.units[0].morale = BfMorale::Broken;
        s.bf.units[0].notes = "holding the ridge".into();
        s.bf.round = 4;
        s.mechs[0].bf = BfLive {
            engine: 1,
            fire_control: 2,
            mp_lost: 3,
            weapon: 1,
            crew_stunned: true,
            motive: BfMotive { half: true, ..Default::default() },
            arm_spent: true,
            killed: Some(BfKill::Ammo),
            crew_hit: 2,
            kf_drive: 3,
            dock_hits: 1,
            door_hits: 4,
            kf_boom: true,
            docking_collar: true,
            thruster: true,
        };
        let json = serde_json::to_string(&s).unwrap();
        let back: Session = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);
        assert_eq!(back.mode, GameMode::BattleForce);
        assert_eq!(back.bf.units[0].elements, vec![0, 1]);
        assert_eq!(back.mechs[0].bf.killed, Some(BfKill::Ammo));
    }

    #[test]
    fn bf_absent_from_old_session_json() {
        // A pre-BF session JSON (no `bf` key at either level) loads with default BF state.
        let mut s = Session::new_with_mode(GameMode::AlphaStrike);
        s.mechs.push(TrackedMech::new(sbf_atlas()));
        let mut v = serde_json::to_value(&s).unwrap();
        assert!(v.as_object_mut().unwrap().remove("bf").is_some());
        assert!(v["mechs"][0].as_object_mut().unwrap().remove("bf").is_some());
        let back: Session = serde_json::from_value(v).unwrap();
        assert_eq!(back.bf, BfState::default());
        assert_eq!(back.mechs[0].bf, BfLive::default());
        assert_eq!(back, s);
    }

    #[test]
    fn bf_session_starts_with_an_empty_unit() {
        // A fresh BF session has one Unit on the sheet before any elements exist (§2.1).
        let s = Session::new_with_mode(GameMode::BattleForce);
        assert_eq!(s.bf.units.len(), 1);
        assert_eq!(s.bf.units[0].name, "Unit 1");
        assert!(s.bf.units[0].elements.is_empty());
        assert_eq!(s.bf.units[0].size, 0);
        assert!(s.sbf.formations.is_empty(), "no SBF starter in a BF session");
        // Other modes are unaffected.
        assert!(Session::new_with_mode(GameMode::AlphaStrike).bf.units.is_empty());
        assert!(Session::new_with_mode(GameMode::StrategicBattleForce).bf.units.is_empty());
    }

    #[test]
    fn bf_point_cost_and_mech_cap() {
        // BF shares the AS PV arm (the BF Skill PV table IS the AS one, p.50) and is uncapped.
        let mut tm = TrackedMech::new(bf_mech("BM", "16\"", &[]));
        assert_eq!(tm.point_cost(GameMode::BattleForce), 30, "default skill 4 = baked PV");
        for g in 0..=SKILL_MAX {
            tm.gunnery = g;
            assert_eq!(
                tm.point_cost(GameMode::BattleForce),
                tm.point_cost(GameMode::AlphaStrike),
                "BF PV tracks the AS arm at skill {g}"
            );
        }
        assert_eq!(Session::mech_cap(GameMode::BattleForce), None);
    }

    #[test]
    fn bf_remove_mech_remaps_unit_indices() {
        // The pool is index-referenced; deleting a mech must remap every BF Unit (the SBF review
        // finding must not recur) and drop Units emptied by the removal.
        let mut s = bf_session(6);
        s.mechs.push(TrackedMech::new(bf_mech("BM", "16\"", &[])));
        s.mechs.push(TrackedMech::new(bf_mech("BM", "16\"", &[])));
        s.bf_new_unit("Unit B", 6..8);
        s.remove_mech(3);
        assert_eq!(s.bf.units[0].elements, vec![0, 1, 2, 3, 4]);
        assert_eq!(s.bf.units[1].elements, vec![5, 6]);
        // Every consumer walks the remapped pool without panicking.
        for i in 0..s.mechs.len() {
            let _ = s.bf_current_mp(i);
            let _ = s.bf_live_tmm(i);
            let _ = s.bf_current_damage(i, BfRange::Short);
        }

        // Removing a Unit's last element drops the Unit and keeps the cursor in range.
        let mut s = bf_session(1);
        s.bf.active_unit = 0;
        s.remove_mech(0);
        assert!(s.bf.units.is_empty());
        assert_eq!(s.bf.active_unit, 0);
    }

    #[test]
    fn bf_derived_stats_are_pure() {
        // Damage/heat/crit marks live on TrackedMech/BfLive; the immutable spec (and with it
        // every per-frame derived readout base) must never change (the SBF purity pattern).
        let mut s = bf_session(1);
        let spec_before = s.mechs[0].spec.clone();
        s.mechs[0].as_damage();
        s.mechs[0].as_adjust_heat(2);
        s.mechs[0].bf.weapon += 1;
        s.mechs[0].bf.engine += 1;
        s.bf_apply_mp_crit(0);
        s.mechs[0].bf_mark_motive(BfMotive { minus_one: true, ..Default::default() });
        assert_eq!(s.mechs[0].spec, spec_before);
        // A fresh element of the same spec still reads the full baseline.
        let fresh = Session {
            mechs: vec![TrackedMech::new(spec_before)],
            mode: GameMode::BattleForce,
            ..Session::default()
        };
        assert_eq!(fresh.bf_current_mp(0), 8);
        assert_eq!(fresh.bf_current_damage(0, BfRange::Short), Some(3.0));
    }

    #[test]
    fn bf_mp_crit_accumulates_multiplicatively() {
        // p.43: each MP crit removes half of CURRENT MP (round normally, min 1 lost) — computed
        // at apply time, so the accumulated loss is NOT `count × k` (spec §1.2). MV 16″ = 8 hexes.
        let mut s = bf_session(1);
        assert_eq!(s.bf_current_mp(0), 8);
        s.bf_apply_mp_crit(0); // loses 8/2 = 4
        assert_eq!(s.mechs[0].bf.mp_lost, 4);
        assert_eq!(s.bf_current_mp(0), 4);
        s.bf_apply_mp_crit(0); // loses 4/2 = 2 (not another 4)
        assert_eq!(s.mechs[0].bf.mp_lost, 6);
        assert_eq!(s.bf_current_mp(0), 2);
        s.bf_apply_mp_crit(0); // loses 2/2 = 1
        assert_eq!(s.bf_current_mp(0), 1);
        s.bf_apply_mp_crit(0); // at current 1 the loss floors at 1 → 0 = immobile
        assert_eq!(s.bf_current_mp(0), 0);
        s.bf_apply_mp_crit(0); // already immobile: stays floored at 0
        assert_eq!(s.bf_current_mp(0), 0);
        // Heat subtracts from MP before the bracket (p.49): repairs aside, the readout is live.
        assert_eq!(s.bf_live_tmm(0), 0);
    }

    #[test]
    fn bf_motive_flags_are_once_per_game_and_stack() {
        // p.43: "a vehicle may only suffer each effect once per game" — each effect is an
        // independent spent-flag; marking the same effect again is a no-op, but DIFFERENT
        // effects stack (−1 MV and ½ MV together).
        let m1 = BfMotive { minus_one: true, ..Default::default() };
        let mh = BfMotive { half: true, ..Default::default() };
        let mi = BfMotive { immobile: true, ..Default::default() };
        let mut s = Session::new_with_mode(GameMode::BattleForce);
        s.mechs.push(TrackedMech::new(bf_mech("CV", "16\"", &[])));
        let tm = &mut s.mechs[0];
        tm.bf_mark_motive(m1);
        assert_eq!(tm.bf.motive, m1);
        tm.bf_mark_motive(BfMotive::default()); // no effect rolled: no-op
        assert_eq!(tm.bf.motive, m1);
        tm.bf_mark_motive(m1); // same effect again: once per game, no-op
        assert_eq!(tm.bf.motive, m1);
        assert_eq!(s.bf_current_mp(0), 7, "−1 MV");
        s.mechs[0].bf_mark_motive(mh);
        assert!(s.mechs[0].bf.motive.minus_one && s.mechs[0].bf.motive.half, "flags stack");
        assert_eq!(s.bf_current_mp(0), 3, "(8 − 1) / 2, round down (p.44)");
        s.mechs[0].bf_mark_motive(mi);
        assert_eq!(s.bf_current_mp(0), 0);
        assert!(s.mechs[0].bf.motive.minus_one, "immobile never clears the spent flags");
    }

    #[test]
    fn bf_vehicle_engine_mv_halving_is_chronology_free() {
        // The vehicle Engine-crit MV halving derives live from bf.engine (never an mp_lost
        // snapshot), so Engine-then-motive and motive-then-Engine read the same — the
        // sequential table value (8 → 4 → 2) either way (§1.2/§1.4 as built).
        let mh = BfMotive { half: true, ..Default::default() };
        let mut a = Session::new_with_mode(GameMode::BattleForce);
        a.mechs.push(TrackedMech::new(bf_mech("CV", "16\"", &[])));
        a.mechs[0].bf.engine = 1; // Engine crit first…
        assert_eq!(a.bf_current_mp(0), 4);
        a.mechs[0].bf_mark_motive(mh); // …then ½ MV
        assert_eq!(a.bf_current_mp(0), 2);

        let mut b = Session::new_with_mode(GameMode::BattleForce);
        b.mechs.push(TrackedMech::new(bf_mech("CV", "16\"", &[])));
        b.mechs[0].bf_mark_motive(mh); // ½ MV first…
        assert_eq!(b.bf_current_mp(0), 4);
        b.mechs[0].bf.engine = 1; // …then the Engine crit
        assert_eq!(b.bf_current_mp(0), 2);
        assert_eq!(b.mechs[0].bf.mp_lost, 0, "nothing engine-related is snapshotted");
    }

    #[test]
    fn bf_aero_engine_tp_derives_live() {
        // Aero Engine hits derive TP live from bf.engine: the 2nd hit is TP 0 + shutdown,
        // permanent (p.42) — repairing heat afterwards must NOT resurrect thrust (the old
        // mp_lost snapshot baked the transient heat in and did).
        let mut s = Session::new_with_mode(GameMode::BattleForce);
        s.mechs.push(TrackedMech::new(bf_mech("AF", "7a", &[])));
        s.mechs[0].as_adjust_heat(2);
        assert_eq!(s.bf_current_mp(0), 5);
        s.mechs[0].bf.engine = 1;
        assert_eq!(s.bf_current_mp(0), 3, "thrust −50% of current, round down, min 1");
        s.mechs[0].bf.engine = 2;
        assert_eq!(s.bf_current_mp(0), 0, "2nd hit: TP 0");
        s.mechs[0].as_adjust_heat(-2); // cool back down
        assert_eq!(s.bf_current_mp(0), 0, "TP 0 survives cooling");
        assert_eq!(s.mechs[0].bf.mp_lost, 0, "mp_lost stays MP-crits-only");
        assert!(!s.mechs[0].bf_destroyed(), "aero engine hits shut down, never destroy");
    }

    #[test]
    fn bf_unit_size_restamps_on_membership_change() {
        // Unit Size is static (p.53) — stamped at grouping time, invalidated only by membership
        // edits. Two size-4 elements → 4; adding a size-2 element → jround(10/3) = 3.
        let mut s = Session::new_with_mode(GameMode::BattleForce);
        s.bf.units.clear();
        s.mechs.push(TrackedMech::new(sbf_atlas())); // size 4
        s.mechs.push(TrackedMech::new(sbf_atlas())); // size 4
        s.mechs.push(TrackedMech::new(bf_mech("BM", "16\"", &[]))); // size 2
        s.bf_new_unit("Heavies", 0..2);
        assert_eq!(s.bf.units[0].size, 4);
        s.bf_assign_element(2, BfAssign::Unit(0));
        assert_eq!(s.bf.units[0].size, 3);
        s.bf_assign_element(2, BfAssign::Unassign);
        assert_eq!(s.bf.units[0].size, 4);
        // A fresh single-element Unit stamps its own size.
        s.bf_assign_element(2, BfAssign::NewUnit);
        assert_eq!(s.bf.units[1].elements, vec![2]);
        assert_eq!(s.bf.units[1].size, 2);
        // Assignment lookup + rename + remove round out the sbf_* mirror surface.
        assert_eq!(s.bf_element_assignment(2), Some(1));
        assert_eq!(s.bf_element_assignment(0), Some(0));
        s.bf_rename_unit(1, "Recon");
        assert_eq!(s.bf.units[1].name, "Recon");
        s.bf_remove_unit(1);
        assert_eq!(s.bf.units.len(), 1);
        assert_eq!(s.bf_element_assignment(2), None, "elements return to Unassigned");
    }

    #[test]
    fn bf_morale_is_a_manual_cycle() {
        // The `m`-key ladder wraps: Normal → Broken → Routed → Normal (§3.3); labels are the
        // record-sheet wording. Nothing automatic touches the rung.
        let mut m = BfMorale::default();
        assert_eq!(m, BfMorale::Normal);
        for (want, label) in
            [(BfMorale::Broken, "Broken"), (BfMorale::Routed, "Routed"), (BfMorale::Normal, "Normal")]
        {
            m = m.cycled();
            assert_eq!(m, want);
            assert_eq!(m.label(), label);
        }
    }

    #[test]
    fn bf_destroyed_predicate() {
        // Structure gone.
        let mut tm = TrackedMech::new(bf_mech("BM", "16\"", &[]));
        assert!(!tm.bf_destroyed());
        tm.as_struct_hits = tm.spec.as_stats.structure;
        assert!(tm.bf_destroyed());

        // An outright-kill crit marked on the sheet.
        let mut tm = TrackedMech::new(bf_mech("BM", "16\"", &[]));
        tm.bf.killed = Some(BfKill::Fuel);
        assert!(tm.bf_destroyed());

        // 2 Engine hits destroy on the destroy-at-2 columns ('Mech/Vehicle, §1.4)…
        for tp in ["BM", "CV"] {
            let mut tm = TrackedMech::new(bf_mech(tp, "16\"", &[]));
            tm.bf.engine = 1;
            assert!(!tm.bf_destroyed(), "{tp}: 1 engine hit is not a kill");
            tm.bf.engine = 2;
            assert!(tm.bf_destroyed(), "{tp}: 2 engine hits destroy");
        }
        // …but not aerospace (2nd hit = TP 0 + shutdown) or infantry (no crit column at all).
        for tp in ["AF", "CI"] {
            let mut tm = TrackedMech::new(bf_mech(tp, "7a", &[]));
            tm.bf.engine = 2;
            assert!(!tm.bf_destroyed(), "{tp}: engine hits never destroy");
        }
    }

    #[test]
    fn bf_live_mp_tmm_and_tsm() {
        // MV 16″ → 8 hexes → TMM +3 (7–9 bracket); heat subtracts from MP directly (p.49).
        let mut s = bf_session(1);
        assert_eq!(s.bf_current_mp(0), 8);
        assert_eq!(s.bf_live_tmm(0), 3);
        s.mechs[0].as_adjust_heat(2);
        assert_eq!(s.bf_current_mp(0), 6);
        assert_eq!(s.bf_live_tmm(0), 2, "5–6 available MP → +2");

        // TSM (p.154, detected from the element's specials): +1 MP at heat ≥ 1, and heat 1's
        // MP loss is ignored entirely; heat 2+ subtracts normally.
        let mut s = Session::new_with_mode(GameMode::BattleForce);
        s.mechs.push(TrackedMech::new(bf_mech("BM", "16\"", &["TSM"])));
        assert_eq!(s.bf_current_mp(0), 8, "no TSM bonus at heat 0");
        s.mechs[0].as_adjust_heat(1);
        assert_eq!(s.bf_current_mp(0), 9, "heat 1: +1 MP and the heat loss is ignored");
        s.mechs[0].as_adjust_heat(1);
        assert_eq!(s.bf_current_mp(0), 7, "heat 2: 8 + 1 − 2");

        // Aerospace thrust passes through unconverted (7a → 7 TP) and shutdown reads the shared
        // AS heat scale's S box.
        let mut s = Session::new_with_mode(GameMode::BattleForce);
        s.mechs.push(TrackedMech::new(bf_mech("AF", "7a", &[])));
        assert_eq!(s.bf_current_mp(0), 7);
        assert!(!s.bf_shutdown(0));
        s.mechs[0].as_adjust_heat(4);
        assert!(s.bf_shutdown(0));
    }

    #[test]
    fn bf_current_damage_readout() {
        // Card 3/3/1/−: Weapon crits subtract 1 per hit (p.43); ground Extreme derives as
        // L − 1 min 0 (p.84), never the baked E.
        let mut s = bf_session(1);
        assert_eq!(s.bf_current_damage(0, BfRange::Short), Some(3.0));
        assert_eq!(s.bf_current_damage(0, BfRange::Long), Some(1.0));
        assert_eq!(s.bf_current_damage(0, BfRange::Extreme), None, "ground E = L−1 = 0");
        s.mechs[0].bf.weapon = 1;
        assert_eq!(s.bf_current_damage(0, BfRange::Short), Some(2.0));
        assert_eq!(s.bf_current_damage(0, BfRange::Long), None, "reduced to nothing");

        // Vehicle Engine crit, 1st hit: damage values × 0.5 round down (§1.4), after the
        // Weapon-crit subtraction.
        let mut s = Session::new_with_mode(GameMode::BattleForce);
        s.mechs.push(TrackedMech::new(bf_mech("CV", "16\"", &[])));
        s.mechs[0].bf.engine = 1;
        assert_eq!(s.bf_current_damage(0, BfRange::Short), Some(1.0), "3 → 1 (round down)");
        assert_eq!(s.bf_current_damage(0, BfRange::Long), None, "1 → 0");
        s.mechs[0].bf.weapon = 1;
        assert_eq!(s.bf_current_damage(0, BfRange::Short), Some(1.0), "(3−1)/2 = 1");
        // A 'Mech's engine hits never touch its damage line.
        let mut s = bf_session(1);
        s.mechs[0].bf.engine = 1;
        assert_eq!(s.bf_current_damage(0, BfRange::Short), Some(3.0));
    }

    #[test]
    fn bf_group_doctrine_and_new_round() {
        // 5 ground + 3 aero: IS doctrine → Lances of 4, aero pairs of 2 (Air Lance), never mixed.
        let mut s = Session::new_with_mode(GameMode::BattleForce);
        for _ in 0..5 {
            s.mechs.push(TrackedMech::new(bf_mech("BM", "16\"", &[])));
        }
        for _ in 0..3 {
            s.mechs.push(TrackedMech::new(bf_mech("AF", "7a", &[])));
        }
        s.bf_group_doctrine(SbfDoctrine::InnerSphere);
        let names: Vec<&str> = s.bf.units.iter().map(|u| u.name.as_str()).collect();
        assert_eq!(names, vec!["Lance 1", "Lance 2", "Air Lance 1", "Air Lance 2"]);
        assert_eq!(s.bf.units[0].elements, vec![0, 1, 2, 3]);
        assert_eq!(s.bf.units[1].elements, vec![4], "understrength Units are legal");
        assert_eq!(s.bf.units[2].elements, vec![5, 6]);
        assert_eq!(s.bf.units[3].elements, vec![7]);
        assert!(s.bf.units.iter().all(|u| u.size == 2), "sizes stamped at grouping time");

        // Clan: Stars of 5, aero Points of 2.
        s.bf_group_doctrine(SbfDoctrine::Clan);
        let names: Vec<&str> = s.bf.units.iter().map(|u| u.name.as_str()).collect();
        assert_eq!(names, vec!["Star 1", "Point 1", "Point 2"]);

        // `n` new round: bump the counter, clear every element's Crew Stunned flag — everything
        // else persists (it is the record sheet).
        s.mechs[0].bf.crew_stunned = true;
        s.mechs[7].bf.crew_stunned = true;
        s.mechs[0].bf.fire_control = 2;
        s.bf.units[0].morale = BfMorale::Broken;
        s.bf_begin_round();
        assert_eq!(s.bf.round, 1);
        assert!(s.mechs.iter().all(|tm| !tm.bf.crew_stunned));
        assert_eq!(s.mechs[0].bf.fire_control, 2);
        assert_eq!(s.bf.units[0].morale, BfMorale::Broken);
    }
}
