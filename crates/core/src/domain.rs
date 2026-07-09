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

//! Shared vocabulary: the immutable "spec" of a mech. Pure serde data, no behavior.
//!
//! Mutable per-game state (damage/heat/ammo counters) lives in [`crate::session`], never here.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// A BattleMech hit location. Order matches the canonical record-sheet ordering. The biped
/// set comes first; quad legs (four-legged 'Mechs use these instead of arms/legs) and the
/// tripod center leg follow. A given 'Mech only uses the subset for its [`MechConfig`].
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize)]
pub enum Location {
    Head,
    CenterTorso,
    LeftTorso,
    RightTorso,
    LeftArm,
    RightArm,
    LeftLeg,
    RightLeg,
    // Quad legs.
    FrontLeftLeg,
    FrontRightLeg,
    RearLeftLeg,
    RearRightLeg,
    // Tripod center leg.
    CenterLeg,
    // Combat-vehicle locations (tanks/VTOL/naval).
    Front,
    Rear,
    LeftSide,
    RightSide,
    Turret,
    Body,
    Rotor,
    FrontTurret,
    FrontLeftSide,
    FrontRightSide,
    RearLeftSide,
    RearRightSide,
    // Battle Armor troopers (one armor track per suit, `loc="T1".."T6"` on the sheet).
    Trooper1,
    Trooper2,
    Trooper3,
    Trooper4,
    Trooper5,
    Trooper6,
    // Conventional infantry: one synthesized troop-strength track (not a Mekbay sheet code).
    Platoon,
    // Aerospace fighters: four armor arcs + one shared Structural Integrity pool (`loc="SI"`),
    // which the arcs cascade into.
    Nose,
    LeftWing,
    RightWing,
    Aft,
    AeroSI,
}

impl Location {
    /// Every hit location across all configurations, for iteration. Use [`MechConfig::locations`]
    /// for the subset a given 'Mech actually has.
    pub const ALL: [Location; 37] = [
        Location::Head,
        Location::CenterTorso,
        Location::LeftTorso,
        Location::RightTorso,
        Location::LeftArm,
        Location::RightArm,
        Location::LeftLeg,
        Location::RightLeg,
        Location::FrontLeftLeg,
        Location::FrontRightLeg,
        Location::RearLeftLeg,
        Location::RearRightLeg,
        Location::CenterLeg,
        Location::Front,
        Location::Rear,
        Location::LeftSide,
        Location::RightSide,
        Location::Turret,
        Location::Body,
        Location::Rotor,
        Location::FrontTurret,
        Location::FrontLeftSide,
        Location::FrontRightSide,
        Location::RearLeftSide,
        Location::RearRightSide,
        Location::Trooper1,
        Location::Trooper2,
        Location::Trooper3,
        Location::Trooper4,
        Location::Trooper5,
        Location::Trooper6,
        Location::Platoon,
        Location::Nose,
        Location::LeftWing,
        Location::RightWing,
        Location::Aft,
        Location::AeroSI,
    ];

    /// Aerospace fighter locations, in doll order: the four armor arcs plus the shared SI pool the
    /// arcs cascade into (rendered as a center "SI" box).
    pub const AEROSPACE: [Location; 5] = [
        Location::Nose,
        Location::LeftWing,
        Location::RightWing,
        Location::Aft,
        Location::AeroSI,
    ];

    /// Battle Armor trooper tracks, in squad order (a squad uses the first N).
    pub const TROOPERS: [Location; 6] = [
        Location::Trooper1,
        Location::Trooper2,
        Location::Trooper3,
        Location::Trooper4,
        Location::Trooper5,
        Location::Trooper6,
    ];

    /// The combat-vehicle hit locations, in doll order (front/turret across the top, sides + body,
    /// rear at the bottom). Vehicles only ever use this subset; mechs never touch it.
    pub const VEHICLE: [Location; 12] = [
        Location::Front,
        Location::Turret,
        Location::FrontTurret,
        Location::LeftSide,
        Location::RightSide,
        Location::Body,
        Location::Rotor,
        Location::Rear,
        Location::FrontLeftSide,
        Location::FrontRightSide,
        Location::RearLeftSide,
        Location::RearRightSide,
    ];

    /// The code used in MegaMek/Mekbay data (`HD`, `CT`, `LT`, `FLL`, ...).
    pub fn code(self) -> &'static str {
        match self {
            Location::Head => "HD",
            Location::CenterTorso => "CT",
            Location::LeftTorso => "LT",
            Location::RightTorso => "RT",
            Location::LeftArm => "LA",
            Location::RightArm => "RA",
            Location::LeftLeg => "LL",
            Location::RightLeg => "RL",
            Location::FrontLeftLeg => "FLL",
            Location::FrontRightLeg => "FRL",
            Location::RearLeftLeg => "RLL",
            Location::RearRightLeg => "RRL",
            Location::CenterLeg => "CL",
            Location::Front => "FR",
            Location::Rear => "RR",
            Location::LeftSide => "LS",
            Location::RightSide => "RS",
            Location::Turret => "TU",
            Location::Body => "BD",
            Location::Rotor => "RO",
            Location::FrontTurret => "FT",
            Location::FrontLeftSide => "FRLS",
            Location::FrontRightSide => "FRRS",
            Location::RearLeftSide => "RRLS",
            Location::RearRightSide => "RRRS",
            Location::Trooper1 => "T1",
            Location::Trooper2 => "T2",
            Location::Trooper3 => "T3",
            Location::Trooper4 => "T4",
            Location::Trooper5 => "T5",
            Location::Trooper6 => "T6",
            Location::Platoon => "PLT",
            Location::Nose => "NOS",
            Location::LeftWing => "LWG",
            Location::RightWing => "RWG",
            Location::Aft => "AFT",
            Location::AeroSI => "SI",
        }
    }

    /// A short human label for the UI.
    pub fn label(self) -> &'static str {
        match self {
            Location::Head => "Head",
            Location::CenterTorso => "Center Torso",
            Location::LeftTorso => "Left Torso",
            Location::RightTorso => "Right Torso",
            Location::LeftArm => "Left Arm",
            Location::RightArm => "Right Arm",
            Location::LeftLeg => "Left Leg",
            Location::RightLeg => "Right Leg",
            Location::FrontLeftLeg => "Front Left Leg",
            Location::FrontRightLeg => "Front Right Leg",
            Location::RearLeftLeg => "Rear Left Leg",
            Location::RearRightLeg => "Rear Right Leg",
            Location::CenterLeg => "Center Leg",
            Location::Front => "Front",
            Location::Rear => "Rear",
            Location::LeftSide => "Left Side",
            Location::RightSide => "Right Side",
            Location::Turret => "Turret",
            Location::Body => "Body",
            Location::Rotor => "Rotor",
            Location::FrontTurret => "Front Turret",
            Location::FrontLeftSide => "Front Left Side",
            Location::FrontRightSide => "Front Right Side",
            Location::RearLeftSide => "Rear Left Side",
            Location::RearRightSide => "Rear Right Side",
            Location::Trooper1 => "Trooper 1",
            Location::Trooper2 => "Trooper 2",
            Location::Trooper3 => "Trooper 3",
            Location::Trooper4 => "Trooper 4",
            Location::Trooper5 => "Trooper 5",
            Location::Trooper6 => "Trooper 6",
            Location::Platoon => "Platoon",
            Location::Nose => "Nose",
            Location::LeftWing => "Left Wing",
            Location::RightWing => "Right Wing",
            Location::Aft => "Aft",
            Location::AeroSI => "Structural Integrity",
        }
    }

    /// Whether this is a combat-vehicle location (rather than a 'Mech one).
    pub fn is_vehicle(self) -> bool {
        Location::VEHICLE.contains(&self)
    }

    /// Whether this is an infantry location (a BA trooper track or the CI platoon pool).
    pub fn is_infantry(self) -> bool {
        Location::TROOPERS.contains(&self) || self == Location::Platoon
    }

    /// Whether this is an aerospace location (an armor arc or the shared SI pool).
    pub fn is_aerospace(self) -> bool {
        Location::AEROSPACE.contains(&self)
    }

    /// Whether this location is a leg (any configuration's leg variant).
    pub fn is_leg(self) -> bool {
        matches!(
            self,
            Location::LeftLeg
                | Location::RightLeg
                | Location::FrontLeftLeg
                | Location::FrontRightLeg
                | Location::RearLeftLeg
                | Location::RearRightLeg
                | Location::CenterLeg
        )
    }

    /// Parse a data-file location code.
    pub fn from_code(s: &str) -> Option<Location> {
        Some(match s.trim().to_ascii_uppercase().as_str() {
            "HD" | "HEAD" => Location::Head,
            "CT" => Location::CenterTorso,
            "LT" => Location::LeftTorso,
            "RT" => Location::RightTorso,
            "LA" => Location::LeftArm,
            "RA" => Location::RightArm,
            "LL" => Location::LeftLeg,
            "RL" => Location::RightLeg,
            "FLL" => Location::FrontLeftLeg,
            "FRL" => Location::FrontRightLeg,
            "RLL" => Location::RearLeftLeg,
            "RRL" => Location::RearRightLeg,
            "CL" => Location::CenterLeg,
            "FR" | "FRONT" => Location::Front,
            "RR" | "REAR" => Location::Rear,
            "LS" => Location::LeftSide,
            "RS" => Location::RightSide,
            "TU" | "TURRET" => Location::Turret,
            "BD" | "BODY" => Location::Body,
            "RO" | "ROTOR" => Location::Rotor,
            "FT" => Location::FrontTurret,
            "FRLS" => Location::FrontLeftSide,
            "FRRS" => Location::FrontRightSide,
            "RRLS" => Location::RearLeftSide,
            "RRRS" => Location::RearRightSide,
            "T1" => Location::Trooper1,
            "T2" => Location::Trooper2,
            "T3" => Location::Trooper3,
            "T4" => Location::Trooper4,
            "T5" => Location::Trooper5,
            "T6" => Location::Trooper6,
            "PLT" => Location::Platoon,
            // Aerospace fighter arcs + the shared SI pool (SI pips are `class="pip structure"`,
            // so they land in AeroSI's internal via the normal armor parser).
            "NOS" => Location::Nose,
            "LWG" => Location::LeftWing,
            "RWG" => Location::RightWing,
            "AFT" => Location::Aft,
            "SI" => Location::AeroSI,
            _ => return None,
        })
    }

    /// Only the three torsos carry rear armor.
    pub fn has_rear(self) -> bool {
        matches!(
            self,
            Location::CenterTorso | Location::LeftTorso | Location::RightTorso
        )
    }
}

/// Which armor facing is being addressed.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Facing {
    Front,
    Rear,
}

/// Immutable maximum armor/structure points for one location.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocationArmor {
    pub armor_max: u16,
    /// Rear armor; always 0 for non-torso locations.
    pub rear_max: u16,
    pub internal_max: u16,
}

/// Heat sink technology, determining dissipation per sink.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum HeatSinkType {
    #[default]
    Single,
    Double,
}

impl HeatSinkType {
    /// Heat dissipated per individual sink.
    pub fn per_sink(self) -> u16 {
        match self {
            HeatSinkType::Single => 1,
            HeatSinkType::Double => 2,
        }
    }

    /// Short display label, e.g. for the HEAT panel.
    pub fn label(self) -> &'static str {
        match self {
            HeatSinkType::Single => "Single",
            HeatSinkType::Double => "Double",
        }
    }
}

/// A mounted weapon (or other heat-generating equipment we can "fire").
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WeaponMount {
    pub id: u32,
    pub name: String,
    pub location: Location,
    pub rear: bool,
    pub heat: u8,
    /// Damage as printed on the sheet (e.g. "20" or "1/msl"); kept as text for display.
    pub damage: String,
    /// Range bracket as printed (e.g. "7/14/21").
    pub range: String,
    pub crit_slots: u8,
    /// Ammo compatibility key (`"<ammoType>:<rackSize>"`, e.g. `"AC:20"`). `None` for energy
    /// weapons. Links a weapon to its [`AmmoBin`]s with the matching key.
    #[serde(default)]
    pub ammo_key: Option<String>,
    /// Inherent to-hit modifier from `equipment2.json` `stats.toHitModifier` (e.g. pulse −2,
    /// heavy laser +1). Applied as MegaMek's `weaponType.getToHitModifier`. Defaulted to 0.
    #[serde(default)]
    pub to_hit: i8,
    /// Whether a Targeting Computer's −1 applies to this weapon: `F_DIRECT_FIRE` and not
    /// `F_CWS`/`F_TASER` (MegaMek `ComputeAttackerToHitMods`). Defaulted to false.
    #[serde(default)]
    pub tc_eligible: bool,
    /// How many copies are carried. For conventional infantry this is the number of troopers
    /// wielding the weapon, so the group's damage is `count × damage` (Mekbay's `d` is *per
    /// trooper*); for 'Mechs/vehicles/BA it's 1 (identical mounts are listed separately). Defaults
    /// to 1 for bundles/sessions baked before this field.
    #[serde(default = "one_u16")]
    pub count: u16,
}

fn one_u16() -> u16 {
    1
}

impl WeaponMount {
    /// Maximum shots this weapon can fire in one turn. Per MegaMek `Mounted.getNumShots`, this is
    /// keyed on the ammo type: Ultra ACs fire 2, Rotary ACs up to 6, everything else 1. Each shot
    /// costs the weapon's base heat and one round (`getCurrentHeat = heat × shots`). Derived from
    /// the baked `ammo_key` (`"<ammoType>:<rackSize>"`), so no extra baked field is needed.
    pub fn max_shots(&self) -> u8 {
        match self.ammo_key.as_deref() {
            Some(k) if k.starts_with("AC_ULTRA") => 2,
            Some(k) if k.starts_with("AC_ROTARY") => 6,
            _ => 1,
        }
    }

    /// The ammo-type prefix of the [`Self::ammo_key`] (`"LRM:20"` → `"LRM"`), or `None` for an
    /// energy weapon with no ammo key.
    pub fn ammo_type(&self) -> Option<&str> {
        self.ammo_key.as_deref().map(|k| match k.split_once(':') {
            Some((t, _)) => t,
            None => k,
        })
    }

    /// The rack size encoded in the [`Self::ammo_key`] (`"LRM:20"` → `20`), or `None` when there is
    /// no ammo key or the suffix isn't numeric.
    pub fn rack_size(&self) -> Option<u16> {
        self.ammo_key
            .as_deref()
            .and_then(|k| k.split_once(':'))
            .and_then(|(_, rs)| rs.parse().ok())
    }
}

/// A bin of ammunition.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AmmoBin {
    pub id: u32,
    pub name: String,
    pub location: Location,
    pub shots_per_ton: u16,
    pub tons: u16,
    /// Ammo compatibility key (`"<type>:<rackSize>"`); matches [`WeaponMount::ammo_key`].
    #[serde(default)]
    pub ammo_key: Option<String>,
    /// The loaded munition's display name as recorded for this unit, e.g. `"Standard"`,
    /// `"Semi-Guided"`, `"Inferno"`. Empty string is treated as `"Standard"`. Defaulted for
    /// bundles/sessions baked before munition support.
    #[serde(default)]
    pub munition: String,
    /// Key into [`crate::data::bundle::Bundle::munitions`] for the set of munitions this bin can
    /// load (the launcher's `baseAmmo` group). `None` when the bin has no munition choice (only
    /// standard ammo exists for its type).
    #[serde(default)]
    pub base_ammo: Option<String>,
}

/// Display name for a bin with no specific munition recorded.
pub const STANDARD_MUNITION: &str = "Standard";

impl AmmoBin {
    /// Total shots this bin holds when full.
    pub fn shots_max(&self) -> u16 {
        self.shots_per_ton.saturating_mul(self.tons)
    }

    /// The recorded munition display name, normalizing an empty string to `"Standard"`.
    pub fn munition_name(&self) -> &str {
        if self.munition.is_empty() {
            STANDARD_MUNITION
        } else {
            &self.munition
        }
    }
}

/// A piece of mounted equipment that isn't a weapon, ammo bin, or heat sink — e.g. a jump jet,
/// CASE, ECM suite, TAG, C3 unit, targeting computer, or MASC. One entry per mounted instance
/// (record sheets list each separately); the UI groups identical names for display.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Equipment {
    pub name: String,
    pub location: Location,
}

/// One row of a location's critical-hit table. Fixed systems (engine, gyro, actuators,
/// cockpit) and mounted equipment (weapons, ammo bins, heat sinks) each occupy slots;
/// taking a crit destroys the equipment/system in that slot.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CritSlot {
    /// Slot index within the location (0-based, as printed on the sheet).
    pub slot: u8,
    /// Display name, e.g. "Fusion Engine", "Medium Laser", "Heat Sink".
    pub name: String,
    /// True for fixed systems (engine/gyro/actuators/cockpit/sensors/life support),
    /// false for mounted equipment.
    pub system: bool,
    /// Whether this slot can take a critical hit. Empty / roll-again slots are not hittable.
    pub hittable: bool,
    /// Groups the slots of one physical multi-slot item (e.g. both slots of a Clan double
    /// heat sink share a uid like `CLDoubleHeatSink@RT#0`). Empty for pre-v11 bakes.
    #[serde(default)]
    pub uid: String,
    /// Heat dissipated per turn by the sink this slot belongs to (0 = not a heat sink).
    #[serde(default)]
    pub hs: u8,
}

/// A 'Mech chassis configuration, which determines its hit-location set and doll layout.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum MechConfig {
    #[default]
    Biped,
    Quad,
    Tripod,
}

/// What kind of unit a [`Mech`] record actually describes. (`Mech` is the historical name for the
/// unit container; it also holds combat vehicles and infantry, branched by this field.)
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum UnitType {
    #[default]
    Mech,
    Vehicle,
    /// Conventional infantry (foot/jump/motorized/mechanized platoons). In Classic they track
    /// as a single troop-strength pool ([`Location::Platoon`], armor 0) — their sheet is a
    /// strength/damage table, not an armor doll.
    Infantry,
    /// Battle Armor squads. In Classic each suit is its own armor track
    /// ([`Location::Trooper1`]…, from the sheet's `loc="T1".."T6"` pips).
    BattleArmor,
    /// Aerospace / conventional fighters. Alpha Strike only — they render as an AS card and have
    /// no Classic doll (Classic aero needs structural integrity / altitude / arcs, deferred). Added
    /// last to keep the earlier serde discriminants stable for old sessions/bundles.
    Aerospace,
}

impl UnitType {
    /// The two crew-skill labels for this unit type (Gunnery first). Vehicles drive, infantry
    /// make anti-'Mech attacks; everything else pilots. Mirrors the per-unit skill panels.
    pub fn skill_labels(self) -> (&'static str, &'static str) {
        match self {
            UnitType::Vehicle => ("Gunnery", "Driving"),
            UnitType::Infantry | UnitType::BattleArmor => ("Gunnery", "Anti-Mech"),
            _ => ("Gunnery", "Piloting"),
        }
    }
}

/// A combat vehicle's motive system — determines movement flavor (and, in Classic, motive-damage
/// effects). Display only for now.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum MotiveType {
    Tracked,
    Wheeled,
    Hover,
    Vtol,
    Naval,
    Wige,
}

impl MotiveType {
    /// Parse the unit JSON `moveType` string.
    pub fn from_move_type(s: &str) -> Option<MotiveType> {
        Some(match s.trim().to_ascii_lowercase().as_str() {
            "tracked" => MotiveType::Tracked,
            "wheeled" => MotiveType::Wheeled,
            "hover" => MotiveType::Hover,
            "vtol" => MotiveType::Vtol,
            "naval" | "submarine" | "hydrofoil" => MotiveType::Naval,
            "wige" => MotiveType::Wige,
            _ => return None,
        })
    }

    pub fn label(self) -> &'static str {
        match self {
            MotiveType::Tracked => "Tracked",
            MotiveType::Wheeled => "Wheeled",
            MotiveType::Hover => "Hover",
            MotiveType::Vtol => "VTOL",
            MotiveType::Naval => "Naval",
            MotiveType::Wige => "WiGE",
        }
    }
}

impl MechConfig {
    /// The hit locations this configuration uses, in record-sheet order.
    pub fn locations(self) -> &'static [Location] {
        use Location::*;
        match self {
            MechConfig::Biped => {
                &[Head, CenterTorso, LeftTorso, RightTorso, LeftArm, RightArm, LeftLeg, RightLeg]
            }
            MechConfig::Quad => &[
                Head,
                CenterTorso,
                LeftTorso,
                RightTorso,
                FrontLeftLeg,
                FrontRightLeg,
                RearLeftLeg,
                RearRightLeg,
            ],
            MechConfig::Tripod => &[
                Head,
                CenterTorso,
                LeftTorso,
                RightTorso,
                LeftArm,
                RightArm,
                LeftLeg,
                RightLeg,
                CenterLeg,
            ],
        }
    }
}

/// Which game system a session is played under. Chosen when a session is created.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum GameMode {
    #[default]
    Classic,
    AlphaStrike,
    /// BattleTech: Override — DFA Wargaming's streamlined ruleset (converted pip armor/structure,
    /// a 0–5 heat scale, TIC weapon groups, a pilot condition monitor). Uses BV like Classic.
    Override,
    /// Strategic BattleForce — the formation-scale game: the AS-element roster is grouped into
    /// Units (1–6 elements) and Formations (1–4 units), tracked at unit scale. Single-force, like
    /// AlphaStrike; uses AS PV. See `docs/sbf-implementation-spec.md`.
    StrategicBattleForce,
    /// Standard BattleForce (IO:BF pp.24–55): per-element AS-card tracking at hex scale, lance
    /// Units, BF crit table. Single-force; uses AS PV. See
    /// `docs/standard-bf-implementation-spec.md`.
    BattleForce,
    /// Abstract Combat System (IO:BF pp.236–264): the planetary-invasion / multi-regiment scale —
    /// AS elements fuse up through SBF Units into Combat Teams → Combat Units → Formations, tracked
    /// at Combat-Unit scale (armor pool + damage thresholds + fatigue + morale). Ground-only v1;
    /// single-force; uses AS PV. See `docs/acs-implementation-spec.md`.
    AbstractCombatSystem,
}

/// One weapon class's S/M/L/E damage within a large-craft firing arc, as printed strings
/// (`"0*"` = minimal damage). Absent/empty bands bake as `"0"`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArcDamage {
    pub s: String,
    pub m: String,
    pub l: String,
    pub e: String,
}

/// One firing arc of a large-craft (DropShip→WarShip) AS/BF card: per-weapon-class damage plus
/// arc-level specials (e.g. `ENE`, `PNT1`). Weapon classes are STD (standard), CAP (capital),
/// SCAP (sub-capital), and MSL (capital/sub-capital missiles); a class absent from the arc is
/// all-zero. The capital classes drive the to-hit weapon-class modifier and the crit
/// weapon-class selection — not a damage rescale (arc values are already BF-scale).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FiringArc {
    pub std: ArcDamage,
    pub cap: ArcDamage,
    pub scap: ArcDamage,
    pub msl: ArcDamage,
    pub specials: Vec<String>,
}

/// A large-craft multi-arc AS/BF card: four firing arcs over the single Armor/Structure/Threshold
/// pool carried on the parent [`AsStats`]. `front/left/right/rear` mirror the source
/// `frontArc/leftArc/rightArc/rearArc`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArcCard {
    pub front: FiringArc,
    pub left: FiringArc,
    pub right: FiringArc,
    pub rear: FiringArc,
}

/// A unit's Alpha Strike card stats, baked from the Mekbay `as` block. Damage values and special
/// tags are kept as printed strings (`"0*"` = minimal damage; `"AC2/2/-"` = per-range special).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AsStats {
    pub pv: u16,
    pub size: u8,
    /// AS unit type code, e.g. "BM" (BattleMech), "IM" (IndustrialMech).
    pub tp: String,
    /// Movement as printed, e.g. `6"` or `8"/6"j`.
    pub movement: String,
    pub tmm: u8,
    pub armor: u8,
    pub structure: u8,
    pub dmg_s: String,
    pub dmg_m: String,
    pub dmg_l: String,
    pub dmg_e: String,
    /// Overheat value.
    pub overheat: u8,
    /// Aerospace armor **Threshold** (TH): an attack doing damage ≥ this triggers a crit roll.
    /// 0 for non-aerospace units (which use TMM instead).
    #[serde(default)]
    pub threshold: u8,
    pub specials: Vec<String>,
    /// Large-craft multi-arc card (DropShips→WarShips): four firing arcs × weapon classes over the
    /// single Arm/Str/Th pool above. `None` for single-arc units (fighters, ground). Optional +
    /// `#[serde(default)]` so pre-arc session snapshots still load.
    #[serde(default)]
    pub arcs: Option<ArcCard>,
}

/// The immutable specification of a single mech, as baked from MegaMek/Mekbay data.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Mech {
    pub chassis: String,
    pub model: String,
    pub tonnage: u16,
    pub tech_base: String,
    pub role: String,
    /// Mekbay weight class, e.g. "Light"/"Medium"/"Heavy"/"Assault" (also "Ultra Light/PA(L)/…",
    /// "Colossal/Super-Heavy", support-vehicle classes). A picker filter facet; defaulted (empty)
    /// for old bundles.
    #[serde(default)]
    pub weight_class: String,
    /// Mekbay unit subtype / family, e.g. "BattleMek", "BattleMek Omni", "Industrial Mek",
    /// "Combat Vehicle", "Battle Armor". A picker filter facet; defaulted (empty) for old bundles.
    #[serde(default)]
    pub subtype: String,
    /// Introduction year (e.g. 2755). Defaulted so older bundles still load.
    #[serde(default)]
    pub year: u16,
    /// Battle Value (the Classic point cost, as computed by MegaMek). Defaulted for old bundles.
    #[serde(default)]
    pub bv: u32,
    /// Purchase cost in C-bills. Defaulted for old bundles.
    #[serde(default)]
    pub cost: u64,
    /// Armor technology, e.g. "Standard Armor", "Ferro-Fibrous", "Stealth".
    #[serde(default)]
    pub armor_type: String,
    /// Internal-structure technology, e.g. "Standard", "Endo Steel".
    #[serde(default)]
    pub structure_type: String,
    pub walk: u8,
    pub run: u8,
    pub jump: u8,
    pub heat_sinks: u16,
    pub heat_sink_type: HeatSinkType,
    /// Total heat dissipation per turn (as resolved by the data source).
    pub dissipation: u16,
    /// Biped / quad / tripod. Determines the hit-location set and doll layout (mechs only).
    #[serde(default)]
    pub config: MechConfig,
    /// Whether this record is a 'Mech or a combat vehicle. Defaulted to `Mech` for old bundles.
    #[serde(default)]
    pub unit_type: UnitType,
    /// Combat-vehicle motive system (`None` for 'Mechs).
    #[serde(default)]
    pub motive: Option<MotiveType>,
    /// Combat-vehicle internal structure — a single shared pool (not per-location like a 'Mech's).
    /// 0 / unused for 'Mechs (whose internal lives in `armor` per location).
    #[serde(default)]
    pub internal: u16,
    /// Conventional-infantry full-strength damage (Mekbay's `dpt`, "damage per turn"). The platoon
    /// has one combined damage value, not per-weapon — it scales down with surviving troopers (see
    /// [`crate::session::TrackedMech::infantry_damage`]). 0 for 'Mechs/vehicles/Battle Armor.
    #[serde(default)]
    pub dpt: u16,
    /// Transport / storage bays carried, as printed on the record sheet's "Features" line, e.g.
    /// `["Infantry Compartment (1 ton)", "Cargo (8 tons)"]`. Empty for units that carry nothing.
    /// Defaulted for bundles baked before transport support.
    #[serde(default)]
    pub transport: Vec<String>,
    pub armor: BTreeMap<Location, LocationArmor>,
    pub weapons: Vec<WeaponMount>,
    pub ammo: Vec<AmmoBin>,
    /// Non-weapon, non-ammo gear: jump jets, CASE, ECM, TAG, C3, targeting computer, MASC, etc.
    /// Heat sinks are summarized by [`Mech::heat_sinks`]/[`Mech::heat_sink_type`] instead.
    /// Defaulted for bundles/sessions baked before equipment support.
    #[serde(default)]
    pub equipment: Vec<Equipment>,
    /// Critical-slot layout per location (record-sheet crit table). Defaulted so bundles
    /// and saved sessions baked before crit support still deserialize.
    #[serde(default)]
    pub crit_slots: BTreeMap<Location, Vec<CritSlot>>,
    /// Alpha Strike card stats. Defaulted for bundles baked before AS support.
    #[serde(default)]
    pub as_stats: AsStats,
    /// Faction/era availability (§35): `era_id -> faction_id -> rarity score (1..=100)`, the
    /// `max(requisition, salvage)` weight from Mekbay's RATGenerator-derived table. Sparse —
    /// only entries with a nonzero score are stored, and an empty map means "no RAT data"
    /// (the "unknown rarity" tier). Era/faction ids resolve via [`crate::data::bundle::Bundle`]'s
    /// `eras`/`factions`. Defaulted for bundles baked before availability support.
    #[serde(default)]
    pub availability: BTreeMap<u16, BTreeMap<u16, u8>>,
}

/// Availability rarity tier for a unit at a chosen era/faction (§35). Buckets the 0..=100
/// RATGenerator score the way Mekbay does, plus an `Unknown` tier for units with no RAT data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rarity {
    /// No RAT data for this unit at all (a coverage gap, not a canon signal).
    Unknown,
    /// Has data, but not available to the selected faction/era.
    NotAvailable,
    VeryRare,
    Rare,
    Uncommon,
    Common,
    VeryCommon,
}

impl Rarity {
    /// Bucket a nonzero RATGenerator score (1..=100) into a rarity tier.
    pub fn from_score(score: u8) -> Rarity {
        match score {
            0 => Rarity::NotAvailable,
            s if s < 20 => Rarity::VeryRare,
            s if s < 40 => Rarity::Rare,
            s if s < 60 => Rarity::Uncommon,
            s if s < 80 => Rarity::Common,
            _ => Rarity::VeryCommon,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Rarity::Unknown => "Unknown",
            Rarity::NotAvailable => "Not Available",
            Rarity::VeryRare => "Very Rare",
            Rarity::Rare => "Rare",
            Rarity::Uncommon => "Uncommon",
            Rarity::Common => "Common",
            Rarity::VeryCommon => "Very Common",
        }
    }

    /// Sort rank, higher = more common/available (Unknown and NotAvailable sort last).
    pub fn rank(self) -> u8 {
        match self {
            Rarity::Unknown => 0,
            Rarity::NotAvailable => 1,
            Rarity::VeryRare => 2,
            Rarity::Rare => 3,
            Rarity::Uncommon => 4,
            Rarity::Common => 5,
            Rarity::VeryCommon => 6,
        }
    }
}

impl Mech {
    /// Convenience: "Atlas AS7-D".
    pub fn display_name(&self) -> String {
        if self.model.is_empty() {
            self.chassis.clone()
        } else {
            format!("{} {}", self.chassis, self.model)
        }
    }

    /// Best availability score for an era/faction selection (§35). `None` for an axis means "any"
    /// (take the max over it). Returns `None` when the unit has no matching RAT entry. Since only
    /// nonzero scores are baked, a returned value is always `>= 1`.
    pub fn avail_score(&self, era: Option<u16>, faction: Option<u16>) -> Option<u8> {
        // A pinned axis is a direct map lookup; `None` ("any") takes the max across that axis. This
        // is on the hot path — the picker re-sorts the whole catalog by rarity on every faction
        // change — so avoid scanning a unit's entire availability map when both axes are known.
        let score_in = |fmap: &BTreeMap<u16, u8>| -> Option<u8> {
            match faction {
                Some(f) => fmap.get(&f).copied(),
                None => fmap.values().copied().max(),
            }
        };
        match era {
            Some(e) => self.availability.get(&e).and_then(score_in),
            None => self.availability.values().filter_map(score_in).max(),
        }
    }

    /// Rarity tier of this unit for the chosen era/faction (§35). Distinguishes a true coverage
    /// gap (`Unknown` — no data at all) from "exists in the RATs but not for this selection"
    /// (`NotAvailable`).
    pub fn rarity(&self, era: Option<u16>, faction: Option<u16>) -> Rarity {
        if self.availability.is_empty() {
            return Rarity::Unknown;
        }
        match self.avail_score(era, faction) {
            Some(s) => Rarity::from_score(s),
            None => Rarity::NotAvailable,
        }
    }

    /// Total armor points (front + rear) across all locations.
    pub fn total_armor(&self) -> u16 {
        self.armor.values().map(|a| a.armor_max + a.rear_max).sum()
    }

    /// Total internal-structure points across all locations.
    pub fn total_internal(&self) -> u16 {
        self.armor.values().map(|a| a.internal_max).sum()
    }

    /// Whether this record is a combat vehicle (rather than a 'Mech).
    pub fn is_vehicle(&self) -> bool {
        self.unit_type == UnitType::Vehicle
    }

    /// Whether this record is infantry (conventional or Battle Armor). They play on the AS
    /// card or the Classic trooper/strength tracks; they have no crit slots or heat.
    pub fn is_infantry(&self) -> bool {
        matches!(self.unit_type, UnitType::Infantry | UnitType::BattleArmor)
    }

    /// Whether this record is an aerospace / conventional fighter (Alpha Strike only, no Classic
    /// doll).
    pub fn is_aerospace(&self) -> bool {
        self.unit_type == UnitType::Aerospace
    }

    /// Whether this unit has **only** Alpha Strike data and no Classic record sheet — i.e. the
    /// hand-entered units (gun emplacements / Battlefield Support assets) that aren't in Mekbay,
    /// signalled by an empty per-location `armor` map. Every real baked unit (mech/vehicle/aero/
    /// infantry) has at least one armor location, so these render on the AS card but have no
    /// Classic doll and are restricted to Alpha Strike sessions.
    pub fn is_as_only(&self) -> bool {
        self.armor.is_empty()
    }

    /// The hit locations this unit uses, in doll order: a vehicle's location set (only those it
    /// actually has armor in) for vehicles, trooper tracks / the platoon for infantry, else the
    /// 'Mech config's set. Aerospace has none (AS-only — no Classic doll).
    pub fn locations(&self) -> Vec<Location> {
        if self.is_vehicle() {
            Location::VEHICLE.iter().copied().filter(|l| self.armor.contains_key(l)).collect()
        } else if self.unit_type == UnitType::Infantry {
            vec![Location::Platoon]
        } else if self.unit_type == UnitType::BattleArmor {
            Location::TROOPERS.iter().copied().filter(|l| self.armor.contains_key(l)).collect()
        } else if self.is_aerospace() {
            Location::AEROSPACE.iter().copied().filter(|l| self.armor.contains_key(l)).collect()
        } else {
            self.config.locations().to_vec()
        }
    }

    /// Whether the 'Mech mounts a Targeting Computer (from the baked equipment list).
    pub fn has_targeting_computer(&self) -> bool {
        self.equipment.iter().any(|e| e.name.contains("Targeting Computer"))
    }

    /// A weapon's total to-hit modifier: its inherent [`WeaponMount::to_hit`] plus the −1 from a
    /// Targeting Computer when the weapon is eligible (see [`WeaponMount::tc_eligible`]). This is
    /// the *equipment-derived* part only — range, movement, heat, and terrain are the player's.
    pub fn weapon_to_hit(&self, w: &WeaponMount) -> i32 {
        let tc = if self.has_targeting_computer() && w.tc_eligible { -1 } else { 0 };
        w.to_hit as i32 + tc
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn location_code_roundtrip() {
        for loc in Location::ALL {
            assert_eq!(Location::from_code(loc.code()), Some(loc));
        }
        assert_eq!(Location::from_code("head"), Some(Location::Head));
        assert_eq!(Location::from_code("zz"), None);
    }

    #[test]
    fn availability_rarity_lookup() {
        // era 13 -> { faction 27: 50 (Uncommon), faction 30: 5 (Very Rare) }; era 10 -> { 27: 90 }.
        let mut av: BTreeMap<u16, BTreeMap<u16, u8>> = BTreeMap::new();
        av.insert(13, BTreeMap::from([(27, 50u8), (30, 5u8)]));
        av.insert(10, BTreeMap::from([(27, 90u8)]));
        let m = Mech { availability: av, ..Default::default() };

        // Specific era+faction.
        assert_eq!(m.avail_score(Some(13), Some(27)), Some(50));
        assert_eq!(m.rarity(Some(13), Some(27)), Rarity::Uncommon);
        assert_eq!(m.rarity(Some(13), Some(30)), Rarity::VeryRare);
        // "Any faction" in an era = max over factions; "any era" for a faction = max over eras.
        assert_eq!(m.avail_score(Some(13), None), Some(50));
        assert_eq!(m.avail_score(None, Some(27)), Some(90));
        assert_eq!(m.rarity(None, Some(27)), Rarity::VeryCommon);
        // Has data but not for this selection -> NotAvailable (distinct from no-data Unknown).
        assert_eq!(m.avail_score(Some(10), Some(30)), None);
        assert_eq!(m.rarity(Some(10), Some(30)), Rarity::NotAvailable);
        // A unit with no RAT data at all is Unknown, never NotAvailable.
        let blank = Mech::default();
        assert_eq!(blank.rarity(Some(13), Some(27)), Rarity::Unknown);
        // Sort ranks order most-common highest, unknown/not-available lowest.
        assert!(Rarity::VeryCommon.rank() > Rarity::Rare.rank());
        assert!(Rarity::NotAvailable.rank() > Rarity::Unknown.rank());
        assert!(Rarity::VeryRare.rank() > Rarity::NotAvailable.rank());
    }

    #[test]
    fn only_torsos_have_rear() {
        assert!(Location::CenterTorso.has_rear());
        assert!(Location::LeftTorso.has_rear());
        assert!(Location::RightTorso.has_rear());
        assert!(!Location::Head.has_rear());
        assert!(!Location::LeftArm.has_rear());
    }

    fn weapon(ammo_key: Option<&str>) -> WeaponMount {
        WeaponMount {
            id: 0,
            name: "test".into(),
            location: Location::RightTorso,
            rear: false,
            heat: 0,
            damage: String::new(),
            range: String::new(),
            crit_slots: 1,
            ammo_key: ammo_key.map(Into::into),
            to_hit: 0,
            tc_eligible: false,
            count: 1,
        }
    }

    #[test]
    fn aerospace_has_no_doll_locations() {
        let mech = Mech { unit_type: UnitType::Aerospace, ..Default::default() };
        assert!(mech.is_aerospace());
        assert!(mech.locations().is_empty(), "aerospace is AS-only — no Classic doll");
    }

    #[test]
    fn weapon_ammo_type_and_rack_size() {
        let lrm = weapon(Some("LRM:20"));
        assert_eq!(lrm.ammo_type(), Some("LRM"));
        assert_eq!(lrm.rack_size(), Some(20));

        let ultra = weapon(Some("AC_ULTRA:5"));
        assert_eq!(ultra.ammo_type(), Some("AC_ULTRA"));
        assert_eq!(ultra.rack_size(), Some(5));

        let energy = weapon(None);
        assert_eq!(energy.ammo_type(), None);
        assert_eq!(energy.rack_size(), None);
    }

    #[test]
    fn mech_serde_roundtrip() {
        let mut armor = BTreeMap::new();
        armor.insert(
            Location::CenterTorso,
            LocationArmor {
                armor_max: 47,
                rear_max: 14,
                internal_max: 31,
            },
        );
        let mech = Mech {
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
                location: Location::RightTorso,
                shots_per_ton: 5,
                tons: 2,
                ammo_key: Some("AC:20".into()),
                munition: String::new(),
                base_ammo: None,
            }],
            crit_slots: BTreeMap::from([(
                Location::RightTorso,
                vec![CritSlot {
                    slot: 0,
                    name: "Autocannon/20".into(),
                    system: false,
                    hittable: true, ..Default::default()
                }],
            )]),
            as_stats: AsStats::default(),
            availability: BTreeMap::new(),
        };
        let json = serde_json::to_string(&mech).unwrap();
        let back: Mech = serde_json::from_str(&json).unwrap();
        assert_eq!(mech, back);
        assert_eq!(back.ammo[0].shots_max(), 10);
        assert_eq!(back.display_name(), "Atlas AS7-D");
        assert_eq!(back.crit_slots[&Location::RightTorso][0].name, "Autocannon/20");
    }
}
