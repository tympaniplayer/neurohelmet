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

//! Application state and keyboard handling (the Elm-ish model + update).

use super::filters::{self, Facet, FacetValues, Filters};
use super::forcegen;
use super::icons::{icons, set_icons, IconSet};
use super::picker::Picker;
use super::profile::{profile, set_profile, DisplayProfile};
use super::theme::{set_theme, theme, Theme, THEMES};
use neurohelmet_core::data::bundle::Bundle;
use neurohelmet_core::domain::{Facing, GameMode, Location, Mech, UnitType};
use neurohelmet_core::engine::as_element::{self, AsElement};
use neurohelmet_core::engine::battleforce::{
    self, BfA2G, BfAeroAngle, BfAttackKind, BfMotive, BfMove, BfPhysical, BfRange, BfShot,
    BfTargetKind, BfTargetMove,
};
use neurohelmet_core::engine::large_craft;
use neurohelmet_core::engine::sbf::{self, SbfAeroTarget, SbfRange};
use neurohelmet_core::engine::{inches_to_hexes, override_conv, ClusterProfile, PILOT_MAX};
use neurohelmet_core::session::SbfDoctrine;
use neurohelmet_core::session::{
    self, AsCritKind, BfAssign, BfKill, CritRow, MotiveLevel, Session, SessionMeta, CREW_MAX,
    DEFAULT_GUNNERY, DEFAULT_PILOTING, OV_TARGET_TMM_MAX, SKILL_MAX,
};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Picker,
    Tracker,
    /// The Alpha Strike card view (used when the session's mode is AlphaStrike).
    AlphaStrike,
    /// The read-only BattleTech Override reference card for the active unit (toggled with `O` from
    /// either the Classic tracker or the Alpha Strike card; converts the unit on the fly).
    Override,
    /// The Strategic BattleForce formation tracker (used when the session's mode is
    /// StrategicBattleForce): formations → units → live armor/crit/morale state.
    Sbf,
    /// The Standard BattleForce sheet (used when the session's mode is BattleForce): the AS card
    /// grid grouped under lance-Unit header rows, at hex scale with BF live tracking (spec §3.2).
    BattleForce,
    /// The Abstract Combat System tracker (used when the session's mode is AbstractCombatSystem):
    /// Formations → Combat Units → detail, with live armor/threshold/fatigue/morale + calculators.
    Acs,
    Sessions,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Doll,
    Equipment,
}

/// A selectable row in the equipment panel.
#[derive(Clone, Copy)]
pub enum EquipRow {
    Weapon(u32),
    Ammo(u32),
    /// Index into `spec.equipment` (jump jets, CASE, ECM, …). Display-only — no fire/spend action.
    Equip(usize),
}

/// A deferred action awaiting confirmation or a typed name.
#[derive(Clone)]
pub enum PendingAction {
    DeleteActiveMech,
    /// Remove the active SBF formation and its pool elements (indices remap via `remove_mech`).
    DeleteActiveFormation,
    /// Rename the active SBF formation from typed input.
    RenameFormation,
    /// Rename the active ACS Formation from typed input.
    RenameAcsFormation,
    /// Apply `n` points of damage to the active ACS Combat Unit (announced amount, typed in).
    AcsDamage,
    /// Rename the active SBF unit from typed input.
    RenameUnit,
    /// Rename the active Standard BF Unit (lance) from typed input.
    RenameBfUnit,
    /// Apply a doctrine auto-group after the player confirmed the itemized losses.
    ApplyDoctrine(SbfDoctrine),
    /// Rebuild the Standard BF Unit grouping under a doctrine after the itemized confirm.
    ApplyBfDoctrine(SbfDoctrine),
    /// Rebuild the whole ACS grouping from the pool after the player confirmed the itemized losses.
    AcsAutoGroup,
    /// CASEP ammo crit (IO:BF p.151): the player rolled the 1D6 and it came up ≤ 2 — the ammo
    /// detonates and the element is destroyed (`y`); `n` = the 3+ ignore outcome.
    BfAmmoDetonate,
    NewSession(GameMode),
    RenameSession(String),
    DeleteSession(String),
    /// Set (or clear, if blank) the current session's force point limit from typed input.
    SetSessionLimit,
    /// Quit the app (confirmed via `q`; Ctrl+C still quits without asking).
    Quit,
}

/// A modal overlay on top of the current screen.
pub enum Modal {
    Confirm {
        prompt: String,
        action: PendingAction,
    },
    Input {
        prompt: String,
        buffer: String,
        action: PendingAction,
    },
    /// Critical-slot marker for one location. `sel` indexes into that location's
    /// `spec.crit_slots` list (every listed slot is occupied and hittable).
    Crit { loc: Location, sel: usize },
    /// Munition picker for one ammo bin, opened from the crit popup with `t`. `crit_sel` is the
    /// crit-slot index to restore when closing; `sel` indexes the bin's munition catalog list.
    Munition {
        loc: Location,
        crit_sel: usize,
        bin: u32,
        sel: usize,
    },
    /// Alpha Strike crit popup; `sel` indexes [`AsCritKind::ALL`].
    AsCrit { sel: usize },
    /// Pilot skills editor; `sel` 0 = Gunnery, 1 = Piloting.
    Skills { sel: usize },
    /// Pre-add skill + cost preview, opened from the picker with Enter. `idx` is the bundle index
    /// of the highlighted unit; `gunnery`/`piloting` are the skills to add it at; `sel` 0 = the
    /// first skill (Gunnery / the AS Skill), 1 = the second (Piloting; unused in Alpha Strike).
    /// Shows the skill-adjusted point cost and whether adding busts the session's budget; Enter
    /// commits the add at the chosen skills.
    AddUnit {
        idx: usize,
        gunnery: u8,
        piloting: u8,
        sel: usize,
    },
    /// This turn's movement editor; `sel` 0 = mode (stationary/walked/ran/jumped), 1 = hexes.
    Move { sel: usize },
    /// Alpha Strike to-hit shot context (§33 Phase 2). `sel` indexes the rows: 0 = attacker jumped,
    /// 1 = target TMM, 2 = target jumped, 3 = target immobile.
    Shot { sel: usize },
    /// Classic GATOR to-hit target (§24). `sel` indexes the rows: 0 = distance, 1 = target hexes
    /// moved, 2 = target jumped, 3 = target immobile.
    Gator { sel: usize },
    /// Combat-vehicle / aerospace crit popup; `sel` indexes the unit's [`CritRow`] list.
    VehicleCrit { sel: usize },
    /// Override per-region crit popup: `loc` is the region, `sel` indexes its crit table rows.
    /// Space toggles whether the highlighted result is marked as taken.
    OvCrit { loc: Location, sel: usize },
    /// Override to-hit shot editor (attacker move + target movement/state + arc). `sel` indexes the
    /// rows: 0 = attacker move, 1 = target TMM, 2 = target jumped, 3 = target immobile, 4 = secondary,
    /// 5 = rear.
    OvShot { sel: usize },
    /// Combat-vehicle Motive System Damage popup; `sel` indexes [`MotiveLevel::ALL`].
    Motive { sel: usize },
    /// Read-only dice-reference popup: cluster-hits column for the selected weapon + the 'Mech
    /// hit-location table. `Tab` toggles `tab`; rolls/changes nothing.
    Dice { tab: DiceTab },
    /// Picker facet-filter editor; `sel` indexes [`Facet::ALL`]. ←→ cycle the value, Esc applies.
    Filters { sel: usize },
    /// Type-to-filter faction picker (combo box) for the §35 availability lens, opened from the
    /// Filters modal's Faction row with Enter. `query` substring-filters the 82-faction catalog;
    /// `sel` indexes the filtered list (with an empty query, index 0 is the "(any)" / clear entry).
    /// Enter sets the lens faction and returns to Filters; Esc returns without changing it.
    FactionPick { query: String, sel: usize },
    /// §35 weighted random force generator. Config stage (`!rolled`) edits the parameters; rolling
    /// moves to the result stage (a preview the player Accepts/Rerolls).
    GenerateForce(ForceGen),
    /// In-app display picker (Ctrl-T): theme + layout profile + icon set. Rows `0..THEMES.len()` are
    /// the themes (highlighting one live-previews it via `set_theme`); the next row toggles the layout
    /// profile (Pi/Modern) and the last toggles the icon set (Text/Nerd) with ←→/Space, live via
    /// `set_profile` / `set_icons`. Enter keeps the choice; Esc restores `original` /
    /// `original_profile` / `original_icons`.
    ThemePicker {
        sel: usize,
        original: Theme,
        original_profile: DisplayProfile,
        original_icons: IconSet,
    },
    /// SBF grouping editor — the PRIMARY grouping flow (manual; auto-group is the opt-in `a`
    /// inside it). `sel` indexes the element pool; keys move the selected element between units,
    /// split it to a new unit, start a new formation, or unassign it.
    SbfGroup { sel: usize },
    /// ACS grouping editor — the four-tier analogue of [`Modal::SbfGroup`]. `sel` indexes the pool;
    /// `←/→` cycle the element through existing SBF Units, `n/t/c/F` split it off into a new SBF
    /// Unit / Team / Combat Unit / Formation, `u` unassigns, `a` auto-groups the whole pool.
    AcsGroup { sel: usize },
    /// SBF doctrine picker for the opt-in auto-group (IO:BF p.165): `sel` 0 = Inner Sphere,
    /// 1 = Clan, 2 = ComStar. Enter rebuilds all formations under that scheme.
    SbfDoctrine { sel: usize },
    /// SBF crit-counter popup for the active unit; `sel` indexes the Damage/Targeting/MP rows.
    /// The single SBF crit table (spec §4.2) is shown as a dim reference below the counters.
    SbfCrit { sel: usize },
    /// SBF to-hit shot editor (spec §4.1). `sel` indexes the rows: 0 = range bracket,
    /// 1 = formation jumped (persisted `jump_used_this_turn`), 2 = target TMM, 3 = target jumped,
    /// 4 = number of targets. Target legs are hand-entered (single-force; no tracked OpFor).
    SbfShot { sel: usize },
    /// Standard BF crit modal (spec §3.3): the active element's column of the p.42 crit table as
    /// the pick list — `sel` indexes the 2D6 rows 2..=12 (enter the roll by picking its row, or
    /// direct-pick the effect you were dealt), plus the motive-damage rungs for vehicles.
    /// Enter applies the effect to the element's `TrackedMech.bf` live state.
    BfCrit { sel: usize },
    /// Standard BF to-hit shot editor (spec §3.3): the p.39 To-Hit Modifiers Table as rows over
    /// the ephemeral [`BfShotUi`]; `sel` indexes [`App::BF_SHOT_ROWS`].
    BfShot { sel: usize },
    /// Standard BF grouping editor — the SbfGroup clone one tier flatter (Units only, no
    /// formation level). `sel` indexes the element pool.
    BfGroup { sel: usize },
    /// Standard BF doctrine picker for the opt-in auto-group (ground 4/5/6 by doctrine, aero
    /// pairs of 2 — spec §1.7): `sel` 0 = Inner Sphere, 1 = Clan, 2 = ComStar.
    BfDoctrine { sel: usize },
    /// Full keybinding reference. Any key dismisses it.
    Help,
}

/// State for the §35 force-generator modal ([`Modal::GenerateForce`]).
pub struct ForceGen {
    /// Availability lens, `(id, name)` — `None` = any.
    pub faction: Option<(u16, String)>,
    pub era: Option<(u16, String)>,
    /// Target force size (the count floor), 1..=[`session::MAX_MECHS`].
    pub count: usize,
    /// Whether to cap the roll at the session's BV/PV limit (only honored if a limit is set).
    pub use_budget: bool,
    /// Relax the availability floor to include rare/off-table (but never future) units.
    pub allow_rare: bool,
    /// Optional weight-class skew.
    pub class_bias: Option<String>,
    /// Seed for the (reproducible) draw.
    pub seed: u64,
    /// Config-stage cursor, indexes [`ForceGen::ROWS`].
    pub field: usize,
    /// `true` once rolled — switches the modal to the result/preview stage.
    pub rolled: bool,
    /// The rolled force (bundle indices, may repeat); empty after a roll = nothing qualified.
    pub preview: Vec<usize>,
    /// A status/empty-result note shown under the preview.
    pub note: String,
}

impl ForceGen {
    /// Config rows, in display order (also the `field` cursor range).
    pub const ROWS: [&'static str; 6] = [
        "Faction",
        "Era",
        "Size",
        "Budget",
        "Allow rare",
        "Class bias",
    ];
}

/// Which page of the dice-reference popup ([`Modal::Dice`]) is showing.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DiceTab {
    /// Cluster Hits Table column for the selected weapon's rack size.
    Cluster,
    /// The full Cluster Hits Table (every rack size 2–30 + 40), as a reference.
    Table,
    /// 'Mech Hit Location Table (front/left/right/rear).
    HitLoc,
}

impl DiceTab {
    /// Tab-cycle order.
    pub const ALL: [DiceTab; 3] = [DiceTab::Cluster, DiceTab::Table, DiceTab::HitLoc];
}

pub struct App {
    pub bundle: Bundle,
    pub names: Vec<String>,
    pub session: Session,
    pub current_name: String,
    pub screen: Screen,
    pub picker: Picker,
    /// Active picker facet filters (composed with the fuzzy query).
    pub filters: Filters,
    /// The cyclable filter values present in the bundle (built once at startup).
    pub facet_values: FacetValues,
    pub cursor: Location,
    pub facing: Facing,
    pub focus: Focus,
    pub equip_sel: usize,
    /// Override mode: selected TIC row (index into the packed weapons table). The armor-diagram
    /// selection reuses the shared doll [`Self::cursor`].
    pub ov_tic: usize,
    /// SBF mode: the hand-entered to-hit context (spec §4.1, the printed p.172 table). Ephemeral
    /// App state (the opponent is read off the other player's sheet each shot, not tracked);
    /// the formation's jump count (`jump_used_this_turn`) IS persisted session state, and the
    /// BFC/DRO specials are derived from the formation, not entered here.
    pub sbf_shot: SbfShotUi,
    /// Standard BF mode: the hand-entered shot context (the p.39 table, spec §3.3). Ephemeral
    /// App state per the SBF precedent — the opponent is read off the other sheet each shot;
    /// the attacker-side terms (heat, FC crits, specials) derive from the active element live.
    pub bf_shot: BfShotUi,
    /// ACS mode: the hand-entered range + target TMM for the detail-pane to-hit/damage readout
    /// (Phase 3 calculators). Ephemeral like the SBF/BF shot UIs; the attacker-side terms
    /// (experience, tactics, fatigue, morale) derive from the active Combat Unit's live state.
    pub acs_shot: AcsShotUi,
    pub sessions: Vec<SessionMeta>,
    pub sessions_sel: usize,
    pub modal: Option<Modal>,
    pub dirty: bool,
    pub should_quit: bool,
    pub status: String,
    /// Picker: whether the unit-preview popup is open.
    pub show_preview: bool,
    undo_stack: Vec<Session>,
}

/// Hand-entered ACS shot legs for the detail-pane readout (IO:BF p.248 range + the hand-set target
/// TMM). Ephemeral like [`SbfShotUi`]; the attacker-side terms derive from the active Combat Unit.
#[derive(Clone, Copy, Default)]
pub struct AcsShotUi {
    pub range: neurohelmet_core::engine::acs::AcsRange,
    pub target_tmm: i64,
    pub secondary: bool,
    // ---- aerospace (IO:BF p.250 Aerospace To-Hit + p.241 cross-type), read only for an aero
    // Formation. The aero range ladder has an Extreme bracket, so it is `SbfRange`, not `AcsRange`.
    pub aero_range: neurohelmet_core::engine::sbf::SbfRange,
    pub weapon_class: large_craft::WeaponClass,
    pub firing_arc: large_craft::Arc,
    pub matchup: neurohelmet_core::engine::acs::AcsAeroMatchup,
    /// The target is itself a large craft (DropShip/JumpShip/station/WarShip) — waives the
    /// capital weapon-class penalty (p.191 Notes). Auto-true for the large-craft `matchup` cases;
    /// the toggle covers same-type (WS→WS) and Space-Station targets the 6-row matchup can't encode.
    pub target_large_craft: bool,
    /// The hand-entered aerospace Ground-Support mission the readout previews.
    pub aero_mission: AcsAeroMission,
}

/// The ACS aerospace Ground-Support missions (IO:BF folio pp.251-252), a readout selector.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum AcsAeroMission {
    /// Space combat (the aero to-hit table); no ground-support mission.
    #[default]
    SpaceCombat,
    Cap,
    GroundStrike,
    AerialRecon,
    OrbitToSurface,
    CombatDrop,
}

impl AcsAeroMission {
    pub const ALL: [AcsAeroMission; 6] = [
        Self::SpaceCombat,
        Self::Cap,
        Self::GroundStrike,
        Self::AerialRecon,
        Self::OrbitToSurface,
        Self::CombatDrop,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::SpaceCombat => "space combat",
            Self::Cap => "CAP / close air support",
            Self::GroundStrike => "ground strike",
            Self::AerialRecon => "aerial recon",
            Self::OrbitToSurface => "orbit-to-surface",
            Self::CombatDrop => "combat drop",
        }
    }
}

/// Hand-entered SBF shot legs (IO:BF p.172 To-Hit Modifiers Table; the aero rows are the p.179
/// Strategic Aerospace table) not derivable from tracked state. Ephemeral — reset on restart,
/// like reading the opponent's sheet fresh each shot.
#[derive(Clone, Copy, Default)]
pub struct SbfShotUi {
    pub range: SbfRange,
    pub indirect: bool,
    pub withheld: u8,
    pub spotting: bool,
    pub secondary: bool,
    pub target_tmm: i64,
    pub target_jump: u8,
    pub target_evaded: bool,
    pub terrain: i64,
    /// Aero attack kind ([`SbfAeroUiKind::Off`] = a plain ground p.172 shot).
    pub aero_kind: SbfAeroUiKind,
    /// Aero target-type row (only read when `aero_kind` is on).
    pub aero_target: SbfAeroTarget,
    /// Attacker is "behind" the target: −2 (p.179 Misc), or −1 for a Large-Aerospace tailer.
    pub behind_target: bool,
    /// Cluster Bomb −1, folded into the bombing kinds at ctx build (inert for other kinds).
    pub cluster: bool,
    // ---- Large-Aerospace capital-scale legs (IO:BF p.191); only read when the firing Unit is a
    // large craft carrying an arc card. Each (firing_arc, weapon_class) is its own to-hit roll.
    pub firing_arc: large_craft::Arc,
    pub weapon_class: large_craft::WeaponClass,
    /// The target is itself a large craft (DropShip/JumpShip/station/WarShip) — waives the
    /// weapon-class penalty (p.191 Notes).
    pub target_large_craft: bool,
    pub high_speed: bool,
    pub atmospheric: bool,
    /// Defender point-defense damage assigned vs a missile attack (0/1/2+ — 2+ auto-fails).
    pub point_defense: u8,
    /// Screen-launcher rating in play (SCR#): +SCR to-hit, capped at +4.
    pub screen: u8,
    pub naval_c3: bool,
    pub teleoperated: bool,
    pub crippled: bool,
    pub grappled: bool,
    pub acm: sbf::SbfAcm,
}

/// The shot modal's aero attack-kind cycle: Off (ground shot) plus the SAS kinds (IO:BF
/// pp.179–180). Flat so `←→` can walk it; the engine's nested [`SbfAeroKind`] (with the cluster
/// toggle folded into the bombing kinds) is built in [`App::sbf_to_hit_ctx`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SbfAeroUiKind {
    #[default]
    Off,
    AirToAir,
    GroundToAir,
    AltitudeBombing,
    DiveBombing,
    Strafing,
    Striking,
}

impl SbfAeroUiKind {
    /// One step along the cycle, clamped at the ends (the range-row convention).
    fn cycled(self, fwd: bool) -> Self {
        use SbfAeroUiKind::*;
        const ORDER: [SbfAeroUiKind; 7] = [
            Off,
            AirToAir,
            GroundToAir,
            AltitudeBombing,
            DiveBombing,
            Strafing,
            Striking,
        ];
        let i = ORDER.iter().position(|&k| k == self).unwrap_or(0);
        let j = if fwd {
            (i + 1).min(ORDER.len() - 1)
        } else {
            i.saturating_sub(1)
        };
        ORDER[j]
    }

    /// The engine kind (with the cluster toggle folded in); `None` = a ground shot.
    pub fn to_engine(self, cluster: bool) -> Option<sbf::SbfAeroKind> {
        Some(match self {
            Self::Off => return None,
            Self::AirToAir => sbf::SbfAeroKind::AirToAir,
            Self::GroundToAir => sbf::SbfAeroKind::GroundToAir,
            Self::AltitudeBombing => {
                sbf::SbfAeroKind::A2G(sbf::SbfA2G::AltitudeBombing { cluster })
            }
            Self::DiveBombing => sbf::SbfAeroKind::A2G(sbf::SbfA2G::DiveBombing { cluster }),
            Self::Strafing => sbf::SbfAeroKind::A2G(sbf::SbfA2G::Strafing),
            Self::Striking => sbf::SbfAeroKind::A2G(sbf::SbfA2G::Striking),
        })
    }

    /// Whether the cluster toggle has any effect (the −1 rides on bombing attacks only).
    pub fn is_bombing(self) -> bool {
        matches!(self, Self::AltitudeBombing | Self::DiveBombing)
    }
}

/// Hand-entered Standard BF shot legs (the p.39 To-Hit Modifiers Table, spec §3.3) — everything
/// [`battleforce::BfShot`] needs that is not derived from the attacking element. Ephemeral App
/// state like [`SbfShotUi`]; applied to whichever element is active (an ineligible attack kind
/// falls back to Standard for that card — see [`App::bf_shot_for`]).
#[derive(Clone, Copy, Default, PartialEq)]
pub struct BfShotUi {
    pub attacker_move: BfMove,
    pub range: BfRange,
    /// Attack kind; the Indirect spotter legs live inside the variant.
    pub kind: BfAttackKind,
    /// Overheat committed at declaration (bounded live by [`battleforce::bf_max_ov_commit`]).
    pub ov: u8,
    pub area_effect: bool,
    pub secondary: bool,
    pub also_spotting: bool,
    /// Grounded aerospace element making a ground-to-ground weapon attack (p.46).
    pub grounded: bool,
    // ---- target side (hand-entered) ----
    pub target_tmm: u8,
    pub target_move: BfTargetMove,
    /// ±JMPS/JMPW (jumped) or ±SUBS/SUBW (submersible) adjustment.
    pub target_move_adj: i32,
    pub target_immobile: bool,
    pub target_kind: BfTargetKind,
    /// 0 = none, 1 = target has MAS (+3 if it stood still), 2 = LMAS (+2).
    pub target_mas: u8,
    pub target_woods: bool,
    pub target_partial_cover: bool,
    pub target_underwater: bool,
    pub target_stealth: bool,
    pub target_carrying_ba: bool,
    /// A2G strafing/striking strikes the target's rear: +1 damage, added before the strafing
    /// halving (p.41, §1.5). Damage-side only (no TN row); bombing never strikes the rear
    /// (p.48), so the row is active for Strafing/Striking only.
    pub strike_rear: bool,
    /// Large-craft per-arc shot (only meaningful when the element carries a multi-arc card): which
    /// firing arc + weapon class the preview resolves.
    pub firing_arc: large_craft::Arc,
    pub weapon_class: large_craft::WeaponClass,
}

/// The typed AS element a tracked mech fields in Standard BF — the BF element IS the AS card
/// (spec §"Data fidelity" 1; same derivation as `Session::sbf_element`).
pub(crate) fn bf_element_of(tm: &neurohelmet_core::session::TrackedMech) -> AsElement {
    as_element::as_element(&tm.spec.as_stats, &tm.spec.display_name(), tm.gunnery)
}

/// Whether an element may declare this attack kind (spec §1.3/§1.5): Indirect needs IF, a
/// REAR-weapons attack needs the REAR ability, physicals go by [`battleforce::bf_physical_eligible`],
/// and air-to-ground attacks need an aerospace element.
pub(crate) fn bf_kind_eligible(el: &AsElement, kind: BfAttackKind) -> bool {
    match kind {
        BfAttackKind::Standard => true,
        BfAttackKind::Indirect { .. } => el.has_sua("IF"),
        BfAttackKind::RearWeapons => el.sua_dmg("REAR").is_some(),
        BfAttackKind::Physical(p) => battleforce::bf_physical_eligible(p, el),
        BfAttackKind::AirToGround(_) => battleforce::bf_is_aero(el),
    }
}

/// The attack-kind cycle order for the BF shot modal (Indirect's spotter legs are separate rows).
pub(crate) const BF_KIND_CYCLE: [BfAttackKind; 12] = [
    BfAttackKind::Standard,
    BfAttackKind::Indirect {
        spotter_also_attacked: false,
        spotter_is_remote_sensor: false,
    },
    BfAttackKind::RearWeapons,
    BfAttackKind::Physical(BfPhysical::Standard),
    BfAttackKind::Physical(BfPhysical::Melee),
    BfAttackKind::Physical(BfPhysical::Charge),
    BfAttackKind::Physical(BfPhysical::Dfa),
    BfAttackKind::Physical(BfPhysical::AntiMech),
    BfAttackKind::AirToGround(BfA2G::AltitudeBombing),
    BfAttackKind::AirToGround(BfA2G::DiveBombing),
    BfAttackKind::AirToGround(BfA2G::Strafing),
    BfAttackKind::AirToGround(BfA2G::Striking),
];

/// Whether two attack kinds are the same cycle stop (the Indirect spotter legs vary within one
/// stop; the physical/A2G subtypes are distinct stops).
pub(crate) fn bf_kind_same(a: BfAttackKind, b: BfAttackKind) -> bool {
    match (a, b) {
        (BfAttackKind::Physical(x), BfAttackKind::Physical(y)) => x == y,
        (BfAttackKind::AirToGround(x), BfAttackKind::AirToGround(y)) => x == y,
        (BfAttackKind::Standard, BfAttackKind::Standard)
        | (BfAttackKind::Indirect { .. }, BfAttackKind::Indirect { .. })
        | (BfAttackKind::RearWeapons, BfAttackKind::RearWeapons) => true,
        _ => false,
    }
}

/// Compact display label for an attack kind (the shot modal's kind row + the card's To-Hit tag).
pub(crate) fn bf_kind_label(kind: BfAttackKind) -> &'static str {
    match kind {
        BfAttackKind::Standard => "Standard",
        BfAttackKind::Indirect { .. } => "Indirect (IF)",
        BfAttackKind::RearWeapons => "Rear weapons (REAR)",
        BfAttackKind::Physical(BfPhysical::Standard) => "Physical: standard",
        BfAttackKind::Physical(BfPhysical::Melee) => "Physical: melee (MEL)",
        BfAttackKind::Physical(BfPhysical::Charge) => "Physical: charge",
        BfAttackKind::Physical(BfPhysical::Dfa) => "Physical: DFA",
        BfAttackKind::Physical(BfPhysical::AntiMech) => "Physical: anti-'Mech",
        BfAttackKind::AirToGround(BfA2G::AltitudeBombing) => "A2G: altitude bombing",
        BfAttackKind::AirToGround(BfA2G::DiveBombing) => "A2G: dive bombing",
        BfAttackKind::AirToGround(BfA2G::Strafing) => "A2G: strafing",
        BfAttackKind::AirToGround(BfA2G::Striking) => "A2G: striking",
    }
}

/// Whether an SBF formation/unit name looks tool-generated (doctrine or editor default) rather
/// than hand-entered. Used to itemize what a doctrine rebuild would discard; a false negative
/// (user typed "Lance 1" by hand) merely skips one line of warning.
fn sbf_default_name(name: &str) -> bool {
    let base = name
        .trim_end_matches(|c: char| c.is_ascii_digit())
        .trim_end();
    matches!(
        base,
        "Formation"
            | "Unit"
            | "Lance"
            | "Star"
            | "Binary"
            | "Trinary"
            | "Company"
            | "Level II"
            | "Level III"
            | "Flight"
            | "Squadron"
            // Standard BF aero-unit doctrine names (spec §1.7).
            | "Air Lance"
            | "Point"
    )
}

/// Whether an ACS Formation / Combat Unit name is one the app generated (see
/// [`Session::acs_new_formation`]) rather than hand-entered. Nested SBF-unit names are seeded as
/// `"<formation> U<n>"`, so a renamed Formation propagates — the check strips that suffix too.
/// A false negative merely skips one line of the rebuild warning.
fn acs_default_name(name: &str) -> bool {
    let base = name
        .trim_end_matches(|c: char| c.is_ascii_digit())
        .trim_end();
    // "Formation 1 U2" -> "Formation 1" -> "Formation": the per-SBF-unit seed inside a Formation.
    let base = base
        .strip_suffix('U')
        .map(|b| {
            b.trim_end()
                .trim_end_matches(|c: char| c.is_ascii_digit())
                .trim_end()
        })
        .unwrap_or(base);
    matches!(base, "Formation" | "Combat Unit" | "Team")
}

/// How many actions can be undone.
const UNDO_DEPTH: usize = 50;

/// A fresh force-generator seed from the wall clock (the draw is otherwise deterministic).
fn fresh_seed() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0x1234_5678_9ABC_DEF0)
}

/// Derive the next seed for a reroll (a plain LCG step — reproducible, unlike the wall clock).
fn next_seed(s: u64) -> u64 {
    s.wrapping_mul(6_364_136_223_846_793_005)
        .wrapping_add(1_442_695_040_888_963_407)
}

impl App {
    pub fn new(bundle: Bundle, mut session: Session, current_name: String) -> Self {
        // Migrate a session loaded from disk to the current data format (refresh baked specs).
        let migrated = session.relink_specs(&bundle);
        let names = bundle
            .index()
            .iter()
            .map(|s| {
                let model = if s.model.is_empty() {
                    String::new()
                } else {
                    format!(" {}", s.model)
                };
                let year = if s.year > 0 {
                    format!("  {}", s.year)
                } else {
                    String::new()
                };
                let bv = if s.bv > 0 {
                    format!("  BV {}", s.bv)
                } else {
                    String::new()
                };
                format!(
                    "{}{}  {}t{}{}  {}",
                    s.chassis, model, s.tonnage, year, bv, s.role
                )
            })
            .collect::<Vec<_>>();
        let total = names.len();
        let screen = if session.mechs.is_empty() {
            Screen::Picker
        } else {
            match session.mode {
                GameMode::AlphaStrike => Screen::AlphaStrike,
                GameMode::BattleForce => Screen::BattleForce,
                GameMode::StrategicBattleForce => Screen::Sbf,
                GameMode::AbstractCombatSystem => Screen::Acs,
                GameMode::Override => Screen::Override,
                GameMode::Classic => Screen::Tracker,
            }
        };
        let facet_values = FacetValues::from_bundle(&bundle);
        let mut app = App {
            bundle,
            names,
            session,
            current_name,
            screen,
            picker: Picker::new(total),
            filters: Filters::default(),
            facet_values,
            cursor: Location::CenterTorso,
            facing: Facing::Front,
            focus: Focus::Doll,
            equip_sel: 0,
            ov_tic: 0,
            sbf_shot: SbfShotUi::default(),
            bf_shot: BfShotUi::default(),
            acs_shot: AcsShotUi::default(),
            sessions: Vec::new(),
            sessions_sel: 0,
            modal: None,
            // Persist the refreshed specs if anything was migrated on load.
            dirty: migrated > 0,
            should_quit: false,
            status: if migrated > 0 {
                format!("Updated {migrated} mech spec(s) to the latest data")
            } else {
                String::new()
            },
            show_preview: false,
            undo_stack: Vec::new(),
        };
        // Snap the doll cursor onto the active unit's location set (vehicles and infantry
        // don't have a Center Torso).
        app.clamp_selection();
        app
    }

    /// Equipment rows for the active mech (weapons then ammo bins).
    pub fn equip_rows(&self) -> Vec<EquipRow> {
        let Some(tm) = self.session.active_mech() else {
            return Vec::new();
        };
        tm.spec
            .weapons
            .iter()
            .map(|w| EquipRow::Weapon(w.id))
            .chain(tm.spec.ammo.iter().map(|b| EquipRow::Ammo(b.id)))
            .chain((0..tm.spec.equipment.len()).map(EquipRow::Equip))
            .collect()
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        self.status.clear();
        // Universal quit.
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.should_quit = true;
            return;
        }
        // A modal captures all input until dismissed.
        if self.modal.is_some() {
            self.modal_key(key);
            return;
        }
        // Universal display picker (Ctrl-T) — theme + layout profile — available on every screen.
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('t') {
            let original = theme();
            self.modal = Some(Modal::ThemePicker {
                sel: original.preset_index(),
                original,
                original_profile: profile(),
                original_icons: icons(),
            });
            return;
        }
        // Undo is handled up front so it never snapshots itself. Every play screen gets it —
        // exclude only the two screens where `z` means something else (the picker's search box)
        // or nothing at all (the Sessions browser), so a future mode is correct by default
        // rather than repeating the dead-key bug Override had.
        if !matches!(self.screen, Screen::Picker | Screen::Sessions)
            && key.code == KeyCode::Char('z')
        {
            self.undo();
            return;
        }
        // Snapshot before dispatch; keep it only if the action actually changed state.
        let before = self.session.clone();
        match self.screen {
            Screen::Picker => self.picker_key(key),
            Screen::Tracker => self.tracker_key(key),
            Screen::AlphaStrike => self.alpha_strike_key(key),
            Screen::Override => self.override_key(key),
            Screen::Sbf => self.sbf_key(key),
            Screen::BattleForce => self.bf_key(key),
            Screen::Acs => self.acs_key(key),
            Screen::Sessions => self.sessions_key(key),
        }
        if self.session != before {
            self.push_undo(before);
        }
    }

    /// The screen to show when a unit is loaded — the AS card or the Classic record sheet,
    /// per the session's game mode.
    fn tracker_screen(&self) -> Screen {
        match self.session.mode {
            GameMode::AlphaStrike => Screen::AlphaStrike,
            GameMode::BattleForce => Screen::BattleForce,
            GameMode::StrategicBattleForce => Screen::Sbf,
            GameMode::AbstractCombatSystem => Screen::Acs,
            GameMode::Classic => Screen::Tracker,
            GameMode::Override => Screen::Override,
        }
    }

    /// Picker when the roster is empty, otherwise the mode-appropriate tracker screen.
    fn loaded_screen(&self) -> Screen {
        if self.session.mechs.is_empty() {
            Screen::Picker
        } else {
            self.tracker_screen()
        }
    }

    fn push_undo(&mut self, snapshot: Session) {
        self.undo_stack.push(snapshot);
        if self.undo_stack.len() > UNDO_DEPTH {
            self.undo_stack.remove(0);
        }
    }

    fn undo(&mut self) {
        match self.undo_stack.pop() {
            Some(prev) => {
                self.session = prev;
                self.clamp_selection();
                self.dirty = true;
                self.status = "Undo".into();
            }
            None => self.status = "Nothing to undo".into(),
        }
    }

    fn clamp_selection(&mut self) {
        let n = self.equip_rows().len();
        if self.equip_sel >= n {
            self.equip_sel = n.saturating_sub(1);
        }
        // SBF selection is session state; keep it in range after undo/regroup/removal.
        let sbf = &mut self.session.sbf;
        sbf.active_formation = sbf
            .active_formation
            .min(sbf.formations.len().saturating_sub(1));
        let units = sbf
            .formations
            .get(sbf.active_formation)
            .map_or(0, |f| f.units.len());
        sbf.active_unit = sbf.active_unit.min(units.saturating_sub(1));
        // Same for the Standard BF Unit cursor.
        let bf = &mut self.session.bf;
        bf.active_unit = bf.active_unit.min(bf.units.len().saturating_sub(1));
        // Keep the doll cursor on a location the active unit actually has (e.g. after
        // switching from a biped to a quad / vehicle / BA squad, the cursor must snap back). In
        // Override mode the doll's locations are the converted regions (merged torso), so snap to
        // those instead of the raw spec locations.
        if let Some(tm) = self.session.active_mech() {
            if self.session.mode == GameMode::Override {
                let regions = tm.ov_region_locs();
                if !regions.contains(&self.cursor) {
                    self.cursor = regions.first().copied().unwrap_or(Location::CenterTorso);
                }
                let tics = tm.ov_card().tics.len();
                if self.ov_tic >= tics {
                    self.ov_tic = tics.saturating_sub(1);
                }
            } else {
                let locs = tm.spec.locations();
                if !locs.contains(&self.cursor) {
                    self.cursor = locs.first().copied().unwrap_or(Location::CenterTorso);
                }
            }
        }
        self.sync_firing_suit();
    }

    /// Keep the Battle Armor firing suit in step with the doll cursor — the trooper the cursor is
    /// on is the suit you fire from. No-op for every other unit type.
    fn sync_firing_suit(&mut self) {
        let cursor = self.cursor;
        if let Some(tm) = self.session.active_mech_mut() {
            if tm.spec.unit_type == UnitType::BattleArmor {
                if let Some(idx) = tm.suit_index_of(cursor) {
                    tm.active_suit = idx;
                }
            }
        }
    }

    // ----- Modals -----

    fn modal_key(&mut self, key: KeyEvent) {
        match self.modal.take() {
            Some(Modal::Confirm { prompt, action }) => match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => self.execute(action),
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                    self.status = "Cancelled".into();
                }
                _ => self.modal = Some(Modal::Confirm { prompt, action }),
            },
            Some(Modal::Input {
                prompt,
                mut buffer,
                action,
            }) => match key.code {
                KeyCode::Enter => self.execute_input(action, buffer.trim().to_string()),
                KeyCode::Esc => self.status = "Cancelled".into(),
                KeyCode::Backspace => {
                    buffer.pop();
                    self.modal = Some(Modal::Input {
                        prompt,
                        buffer,
                        action,
                    });
                }
                KeyCode::Char(c) => {
                    buffer.push(c);
                    self.modal = Some(Modal::Input {
                        prompt,
                        buffer,
                        action,
                    });
                }
                _ => {
                    self.modal = Some(Modal::Input {
                        prompt,
                        buffer,
                        action,
                    })
                }
            },
            Some(Modal::Crit { loc, sel }) => self.crit_modal_key(loc, sel, key),
            Some(Modal::Munition {
                loc,
                crit_sel,
                bin,
                sel,
            }) => self.munition_modal_key(loc, crit_sel, bin, sel, key),
            Some(Modal::AsCrit { sel }) => self.as_crit_modal_key(sel, key),
            Some(Modal::Skills { sel }) => self.skills_modal_key(sel, key),
            Some(Modal::AddUnit {
                idx,
                gunnery,
                piloting,
                sel,
            }) => self.add_unit_modal_key(idx, gunnery, piloting, sel, key),
            Some(Modal::Move { sel }) => self.move_modal_key(sel, key),
            Some(Modal::Shot { sel }) => self.shot_modal_key(sel, key),
            Some(Modal::Gator { sel }) => self.gator_modal_key(sel, key),
            Some(Modal::VehicleCrit { sel }) => self.vehicle_crit_modal_key(sel, key),
            Some(Modal::OvCrit { loc, sel }) => self.ov_crit_modal_key(loc, sel, key),
            Some(Modal::OvShot { sel }) => self.ov_shot_modal_key(sel, key),
            Some(Modal::SbfGroup { sel }) => self.sbf_group_modal_key(sel, key),
            Some(Modal::AcsGroup { sel }) => self.acs_group_modal_key(sel, key),
            Some(Modal::SbfDoctrine { sel }) => self.sbf_doctrine_modal_key(sel, key),
            Some(Modal::SbfCrit { sel }) => self.sbf_crit_modal_key(sel, key),
            Some(Modal::SbfShot { sel }) => self.sbf_shot_modal_key(sel, key),
            Some(Modal::BfCrit { sel }) => self.bf_crit_modal_key(sel, key),
            Some(Modal::BfShot { sel }) => self.bf_shot_modal_key(sel, key),
            Some(Modal::BfGroup { sel }) => self.bf_group_modal_key(sel, key),
            Some(Modal::BfDoctrine { sel }) => self.bf_doctrine_modal_key(sel, key),
            Some(Modal::Motive { sel }) => self.motive_modal_key(sel, key),
            Some(Modal::Dice { tab }) => self.dice_modal_key(tab, key),
            Some(Modal::Filters { sel }) => self.filters_modal_key(sel, key),
            Some(Modal::FactionPick { query, sel }) => self.faction_pick_key(query, sel, key),
            Some(Modal::GenerateForce(fg)) => self.force_gen_modal_key(fg, key),
            Some(Modal::ThemePicker {
                sel,
                original,
                original_profile,
                original_icons,
            }) => self.theme_picker_key(sel, original, original_profile, original_icons, key),
            Some(Modal::Help) => {} // any key dismisses (modal already taken)
            None => {}
        }
    }

    /// Drive the critical-slot popup: navigate the location's slots and toggle hits.
    fn crit_modal_key(&mut self, loc: Location, sel: usize, key: KeyEvent) {
        let count = self
            .session
            .active_mech()
            .and_then(|tm| tm.spec.crit_slots.get(&loc))
            .map_or(0, Vec::len);
        match key.code {
            KeyCode::Esc | KeyCode::Char('c') => {} // close (modal already taken)
            KeyCode::Up | KeyCode::Char('k') => {
                self.modal = Some(Modal::Crit {
                    loc,
                    sel: sel.saturating_sub(1),
                });
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let max = count.saturating_sub(1);
                self.modal = Some(Modal::Crit {
                    loc,
                    sel: (sel + 1).min(max),
                });
            }
            KeyCode::Char(' ') | KeyCode::Enter => {
                let before = self.session.clone();
                if let Some(tm) = self.session.active_mech_mut() {
                    if let Some(slot) = tm.spec.crit_slots.get(&loc).and_then(|s| s.get(sel)) {
                        let (idx, name) = (slot.slot, slot.name.clone());
                        let destroyed = tm.toggle_crit(loc, idx);
                        self.status = format!(
                            "{} {} {}",
                            loc.code(),
                            name,
                            if destroyed { "destroyed" } else { "repaired" }
                        );
                    }
                }
                if self.session != before {
                    self.push_undo(before);
                    self.dirty = true;
                }
                self.modal = Some(Modal::Crit { loc, sel });
            }
            KeyCode::Char('a') => {
                let before = self.session.clone();
                // Resolve the selected slot's name first (immutable borrow), then mutate.
                let slot_name = self
                    .session
                    .active_mech()
                    .and_then(|tm| tm.spec.crit_slots.get(&loc))
                    .and_then(|s| s.get(sel))
                    .map(|cs| cs.name.clone());
                if let (Some(name), Some(tm)) = (slot_name, self.session.active_mech_mut()) {
                    match tm.bin_at(loc, &name).and_then(|id| tm.set_active_bin(id)) {
                        Some(bin_name) => {
                            self.status = format!("Active bin: {} {}", loc.code(), bin_name);
                        }
                        None => self.status = "Not an ammo slot".into(),
                    }
                }
                if self.session != before {
                    self.push_undo(before);
                    self.dirty = true;
                }
                self.modal = Some(Modal::Crit { loc, sel });
            }
            KeyCode::Char('t') => {
                // Open the munition picker if the selected slot is an ammo bin with choices.
                let target = self.session.active_mech().and_then(|tm| {
                    let cs = tm.spec.crit_slots.get(&loc)?.get(sel)?;
                    let bin = tm.bin_at(loc, &cs.name)?;
                    let b = tm.spec.ammo.iter().find(|b| b.id == bin)?;
                    let list = self.bundle.munitions_for(b.base_ammo.as_deref());
                    if list.is_empty() {
                        return None;
                    }
                    let cur = tm.bin_munition(bin);
                    let msel = list.iter().position(|m| m == cur).unwrap_or(0);
                    Some((bin, msel))
                });
                self.modal = match target {
                    Some((bin, msel)) => Some(Modal::Munition {
                        loc,
                        crit_sel: sel,
                        bin,
                        sel: msel,
                    }),
                    None => {
                        self.status = "No munition options".into();
                        Some(Modal::Crit { loc, sel })
                    }
                };
            }
            _ => self.modal = Some(Modal::Crit { loc, sel }),
        }
    }

    /// Drive the munition picker: scroll the bin's munition list and load the chosen one.
    /// Closing (Esc or after a load) returns to the crit popup at the same slot.
    fn munition_modal_key(
        &mut self,
        loc: Location,
        crit_sel: usize,
        bin: u32,
        sel: usize,
        key: KeyEvent,
    ) {
        let list: Vec<String> = self
            .session
            .active_mech()
            .and_then(|tm| tm.spec.ammo.iter().find(|b| b.id == bin))
            .map(|b| self.bundle.munitions_for(b.base_ammo.as_deref()).to_vec())
            .unwrap_or_default();
        let max = list.len().saturating_sub(1);
        match key.code {
            KeyCode::Esc | KeyCode::Char('t') => {
                self.modal = Some(Modal::Crit { loc, sel: crit_sel });
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.modal = Some(Modal::Munition {
                    loc,
                    crit_sel,
                    bin,
                    sel: sel.saturating_sub(1),
                });
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.modal = Some(Modal::Munition {
                    loc,
                    crit_sel,
                    bin,
                    sel: (sel + 1).min(max),
                });
            }
            KeyCode::Char(' ') | KeyCode::Enter => {
                if let Some(m) = list.get(sel).cloned() {
                    let before = self.session.clone();
                    if let Some(tm) = self.session.active_mech_mut() {
                        tm.set_bin_munition(bin, &m);
                    }
                    if self.session != before {
                        self.push_undo(before);
                        self.dirty = true;
                    }
                    self.status = format!("Loaded {m}");
                }
                self.modal = Some(Modal::Crit { loc, sel: crit_sel });
            }
            _ => {
                self.modal = Some(Modal::Munition {
                    loc,
                    crit_sel,
                    bin,
                    sel,
                })
            }
        }
    }

    /// Drive the Alpha Strike crit popup: select one of the unit's crit types and adjust its count.
    /// The type set varies by unit (aerospace has no MP), so index into the active unit's list.
    fn as_crit_modal_key(&mut self, sel: usize, key: KeyEvent) {
        let kinds: &'static [AsCritKind] = self
            .session
            .active_mech()
            .map_or(&AsCritKind::ALL, |tm| tm.as_crit_kinds());
        let max = kinds.len().saturating_sub(1);
        let kind = kinds.get(sel).copied();
        let adjust = |app: &mut App, delta: i32| {
            let Some(kind) = kind else { return };
            let before = app.session.clone();
            if let Some(tm) = app.session.active_mech_mut() {
                if delta > 0 {
                    tm.as_crit_inc(kind);
                } else {
                    tm.as_crit_dec(kind);
                }
            }
            if app.session != before {
                app.push_undo(before);
                app.dirty = true;
            }
        };
        match key.code {
            KeyCode::Esc | KeyCode::Char('c') => {} // close (modal already taken)
            KeyCode::Up | KeyCode::Char('k') => {
                self.modal = Some(Modal::AsCrit {
                    sel: sel.saturating_sub(1),
                });
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.modal = Some(Modal::AsCrit {
                    sel: (sel + 1).min(max),
                });
            }
            KeyCode::Char(' ') | KeyCode::Right | KeyCode::Enter => {
                adjust(self, 1);
                self.modal = Some(Modal::AsCrit { sel });
            }
            KeyCode::Left => {
                adjust(self, -1);
                self.modal = Some(Modal::AsCrit { sel });
            }
            _ => self.modal = Some(Modal::AsCrit { sel }),
        }
    }

    /// Drive the pilot-skills editor: pick Gunnery/Piloting and adjust it (lower is better).
    fn skills_modal_key(&mut self, sel: usize, key: KeyEvent) {
        let adjust = |app: &mut App, delta: i32| {
            let before = app.session.clone();
            if let Some(tm) = app.session.active_mech_mut() {
                if sel == 0 {
                    tm.adjust_gunnery(delta);
                } else {
                    tm.adjust_piloting(delta);
                }
            }
            if app.session != before {
                app.push_undo(before);
                app.dirty = true;
            }
        };
        match key.code {
            // `g` opens (and closes) the modal in AS/Classic; `s` is the BF binding (spec §3.3).
            KeyCode::Esc | KeyCode::Char('g') | KeyCode::Char('s') | KeyCode::Enter => {} // close
            KeyCode::Up | KeyCode::Char('k') => {
                self.modal = Some(Modal::Skills {
                    sel: sel.saturating_sub(1),
                });
            }
            KeyCode::Down | KeyCode::Char('j') => {
                // AS and BF have a single Skill (row 0 only); Classic has Gunnery + Piloting.
                let max_sel = if matches!(
                    self.session.mode,
                    GameMode::AlphaStrike | GameMode::BattleForce
                ) {
                    0
                } else {
                    1
                };
                self.modal = Some(Modal::Skills {
                    sel: (sel + 1).min(max_sel),
                });
            }
            // Lower is better, so right *improves* the skill (decrements the number).
            KeyCode::Right => {
                adjust(self, -1);
                self.modal = Some(Modal::Skills { sel });
            }
            KeyCode::Left => {
                adjust(self, 1);
                self.modal = Some(Modal::Skills { sel });
            }
            _ => self.modal = Some(Modal::Skills { sel }),
        }
    }

    /// Drive the Ctrl-T display picker (theme + layout profile + icon set). Rows `0..THEMES.len()` are
    /// themes — ↑↓ moves the selection and **live-previews** the highlighted theme; the next row is the
    /// layout profile and the last is the icon set, each toggled live with ←→/Space. Enter keeps the
    /// choice (and persists it via §20 config), Esc restores the theme + profile + icons in effect when
    /// the picker opened.
    fn theme_picker_key(
        &mut self,
        sel: usize,
        original: Theme,
        original_profile: DisplayProfile,
        original_icons: IconSet,
        key: KeyEvent,
    ) {
        let n_theme = THEMES.len();
        let n_rows = n_theme + 2; // + the profile row + the icon-set row
        let profile_row = sel == n_theme;
        let icon_row = sel == n_theme + 1;
        let reopen = |app: &mut App, sel: usize| {
            app.modal = Some(Modal::ThemePicker {
                sel,
                original,
                original_profile,
                original_icons,
            });
        };
        // Selecting a theme row previews it; the toggle rows leave the theme as-is.
        let preview = |sel: usize| {
            if sel < n_theme {
                set_theme(THEMES[sel].2);
            }
        };
        let toggle_profile = |app: &mut App| {
            let next = match profile() {
                DisplayProfile::Pi => DisplayProfile::Modern,
                DisplayProfile::Modern => DisplayProfile::Pi,
            };
            set_profile(next);
            app.status = format!("Layout: {next:?}");
        };
        let toggle_icons = |app: &mut App| {
            let next = match icons() {
                IconSet::Ascii => IconSet::Nerd,
                IconSet::Nerd => IconSet::Ascii,
            };
            set_icons(next);
            app.status = format!("Icons: {}", next.label());
        };
        match key.code {
            KeyCode::Esc => {
                set_theme(original);
                set_profile(original_profile);
                set_icons(original_icons);
                self.status = "Display: unchanged".into();
            }
            KeyCode::Enter => {
                let what = if profile_row {
                    format!("Layout: {:?}", profile())
                } else if icon_row {
                    format!("Icons: {}", icons().label())
                } else {
                    format!("Theme: {}", THEMES[sel].1)
                };
                // Persist the chosen theme + profile + icons so it survives a restart (§20 config).
                self.status = match super::config::save_current() {
                    Ok(()) => format!("{what} (saved)"),
                    Err(e) => format!("{what} (not saved: {e})"),
                };
            }
            KeyCode::Up | KeyCode::Char('k') => {
                let s = if sel == 0 { n_rows - 1 } else { sel - 1 };
                preview(s);
                reopen(self, s);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let s = (sel + 1) % n_rows;
                preview(s);
                reopen(self, s);
            }
            KeyCode::Left | KeyCode::Right | KeyCode::Char(' ') if profile_row => {
                toggle_profile(self);
                reopen(self, sel);
            }
            KeyCode::Left | KeyCode::Right | KeyCode::Char(' ') if icon_row => {
                toggle_icons(self);
                reopen(self, sel);
            }
            _ => reopen(self, sel),
        }
    }

    /// Drive the pre-add skill + cost preview popup: pick a skill row, adjust it (lower is
    /// better), then add the unit at those skills (Enter) or cancel (Esc). The unit isn't in the
    /// session yet, so we carry the chosen skills in the modal rather than mutating a tracked mech.
    fn add_unit_modal_key(
        &mut self,
        idx: usize,
        mut gunnery: u8,
        mut piloting: u8,
        sel: usize,
        key: KeyEvent,
    ) {
        // AS, SBF and BF use a single Skill (the gunnery field); Classic has Gunnery + Piloting.
        // Must match the rendered rows (add_unit_modal_lines) or the selection can vanish.
        let rows = match self.session.mode {
            GameMode::AlphaStrike
            | GameMode::StrategicBattleForce
            | GameMode::BattleForce
            | GameMode::AbstractCombatSystem => 1,
            GameMode::Classic | GameMode::Override => 2,
        };
        match key.code {
            KeyCode::Esc => self.status = "Cancelled".into(), // close (modal already taken)
            KeyCode::Up | KeyCode::Char('k') => {
                self.modal = Some(Modal::AddUnit {
                    idx,
                    gunnery,
                    piloting,
                    sel: sel.saturating_sub(1),
                });
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.modal = Some(Modal::AddUnit {
                    idx,
                    gunnery,
                    piloting,
                    sel: (sel + 1).min(rows - 1),
                });
            }
            // Lower is better, so right *improves* the selected skill (decrements the number).
            KeyCode::Right => {
                let target = if sel == 0 {
                    &mut gunnery
                } else {
                    &mut piloting
                };
                *target = (*target as i32 - 1).clamp(0, SKILL_MAX as i32) as u8;
                self.modal = Some(Modal::AddUnit {
                    idx,
                    gunnery,
                    piloting,
                    sel,
                });
            }
            KeyCode::Left => {
                let target = if sel == 0 {
                    &mut gunnery
                } else {
                    &mut piloting
                };
                *target = (*target as i32 + 1).clamp(0, SKILL_MAX as i32) as u8;
                self.modal = Some(Modal::AddUnit {
                    idx,
                    gunnery,
                    piloting,
                    sel,
                });
            }
            KeyCode::Enter => {
                if let Some(mech) = self.bundle.get(idx).cloned() {
                    if self.add_mech_with_skills(mech, gunnery, piloting) {
                        self.picker.reset();
                        self.show_preview = false;
                        self.screen = self.tracker_screen();
                        // The new unit is active — snap the cursor onto its location set
                        // (a vehicle/BA/platoon has no Center Torso).
                        self.clamp_selection();
                    }
                }
            }
            _ => {
                self.modal = Some(Modal::AddUnit {
                    idx,
                    gunnery,
                    piloting,
                    sel,
                })
            }
        }
    }

    /// Drive the movement editor: row 0 cycles the move mode, row 1 adjusts hexes moved.
    fn move_modal_key(&mut self, sel: usize, key: KeyEvent) {
        let adjust = |app: &mut App, delta: i32| {
            let before = app.session.clone();
            if let Some(tm) = app.session.active_mech_mut() {
                // Aerospace edits velocity (row 0) + altitude (row 1); ground units edit the move
                // mode + hexes moved.
                if tm.spec.is_aerospace() {
                    if sel == 0 {
                        tm.adjust_velocity(delta);
                    } else {
                        tm.adjust_altitude(delta);
                    }
                } else if sel == 0 {
                    tm.cycle_move_mode(delta);
                } else {
                    tm.adjust_hexes_moved(delta);
                }
            }
            if app.session != before {
                app.push_undo(before);
                app.dirty = true;
            }
        };
        match key.code {
            KeyCode::Esc | KeyCode::Char('v') | KeyCode::Enter => {} // close (modal already taken)
            KeyCode::Up | KeyCode::Char('k') => {
                self.modal = Some(Modal::Move {
                    sel: sel.saturating_sub(1),
                });
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.modal = Some(Modal::Move {
                    sel: (sel + 1).min(1),
                });
            }
            KeyCode::Right | KeyCode::Char(' ') => {
                adjust(self, 1);
                self.modal = Some(Modal::Move { sel });
            }
            KeyCode::Left => {
                adjust(self, -1);
                self.modal = Some(Modal::Move { sel });
            }
            _ => self.modal = Some(Modal::Move { sel }),
        }
    }

    /// Drive the AS to-hit shot editor (§33 Phase 2): row 0 toggles attacker-jumped; row 1 adjusts
    /// the target TMM (down past 0 clears the target); rows 2–3 toggle target jumped / immobile.
    fn shot_modal_key(&mut self, sel: usize, key: KeyEvent) {
        let adjust = |app: &mut App, delta: i32| {
            let before = app.session.clone();
            if let Some(tm) = app.session.active_mech_mut() {
                match sel {
                    0 => tm.as_toggle_attacker_jumped(),
                    1 => tm.as_adjust_target_tmm(delta),
                    2 => tm.as_toggle_target_jumped(),
                    _ => tm.as_toggle_target_immobile(),
                }
            }
            if app.session != before {
                app.push_undo(before);
                app.dirty = true;
            }
        };
        match key.code {
            KeyCode::Esc | KeyCode::Char('t') | KeyCode::Enter => {} // close (modal already taken)
            KeyCode::Up | KeyCode::Char('k') => {
                self.modal = Some(Modal::Shot {
                    sel: sel.saturating_sub(1),
                });
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.modal = Some(Modal::Shot {
                    sel: (sel + 1).min(3),
                });
            }
            KeyCode::Right | KeyCode::Char(' ') => {
                adjust(self, 1);
                self.modal = Some(Modal::Shot { sel });
            }
            KeyCode::Left => {
                adjust(self, -1);
                self.modal = Some(Modal::Shot { sel });
            }
            _ => self.modal = Some(Modal::Shot { sel }),
        }
    }

    /// Drive the Classic GATOR to-hit target editor (§24): row 0 adjusts the distance (down past 1
    /// clears the target); row 1 adjusts the target's hexes-moved; rows 2–3 toggle target jumped /
    /// immobile. The attacker's own movement comes from the `v` Move editor, not here.
    fn gator_modal_key(&mut self, sel: usize, key: KeyEvent) {
        let adjust = |app: &mut App, delta: i32| {
            let before = app.session.clone();
            if let Some(tm) = app.session.active_mech_mut() {
                match sel {
                    0 => tm.ct_adjust_distance(delta),
                    1 => tm.ct_adjust_hexes(delta),
                    2 => tm.ct_toggle_target_jumped(),
                    _ => tm.ct_toggle_target_immobile(),
                }
            }
            if app.session != before {
                app.push_undo(before);
                app.dirty = true;
            }
        };
        match key.code {
            KeyCode::Esc | KeyCode::Char('t') | KeyCode::Enter => {} // close (modal already taken)
            KeyCode::Up | KeyCode::Char('k') => {
                self.modal = Some(Modal::Gator {
                    sel: sel.saturating_sub(1),
                });
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.modal = Some(Modal::Gator {
                    sel: (sel + 1).min(3),
                });
            }
            KeyCode::Right | KeyCode::Char(' ') => {
                adjust(self, 1);
                self.modal = Some(Modal::Gator { sel });
            }
            KeyCode::Left => {
                adjust(self, -1);
                self.modal = Some(Modal::Gator { sel });
            }
            _ => self.modal = Some(Modal::Gator { sel }),
        }
    }

    /// Drive the vehicle/aerospace crit popup: navigate the crit-result list (plus per-weapon rows
    /// for aero) and toggle hits.
    fn vehicle_crit_modal_key(&mut self, sel: usize, key: KeyEvent) {
        // Rows are the active unit's system crits, plus aero weapon rows.
        let rows = self
            .session
            .active_mech()
            .map(|tm| tm.crit_rows())
            .unwrap_or_default();
        let max = rows.len().saturating_sub(1);
        match key.code {
            KeyCode::Esc | KeyCode::Char('c') => {} // close (modal already taken)
            KeyCode::Up | KeyCode::Char('k') => {
                self.modal = Some(Modal::VehicleCrit {
                    sel: sel.saturating_sub(1),
                });
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.modal = Some(Modal::VehicleCrit {
                    sel: (sel + 1).min(max),
                });
            }
            KeyCode::Char(' ') | KeyCode::Enter => {
                let before = self.session.clone();
                let msg = rows.get(sel).and_then(|row| {
                    let row = row.clone();
                    self.session.active_mech_mut().map(|tm| match row {
                        CritRow::System {
                            idx, label, max, ..
                        } => {
                            let hits = tm.bump_crit(idx);
                            if hits == 0 {
                                format!("{label} cleared")
                            } else if max > 1 {
                                format!("{label} {hits} hit{}", if hits == 1 { "" } else { "s" })
                            } else {
                                format!("{label} hit")
                            }
                        }
                        CritRow::Weapon { id, label, .. } => {
                            let on = tm.toggle_weapon_crit(id);
                            format!("{label} {}", if on { "destroyed" } else { "cleared" })
                        }
                    })
                });
                if let Some(m) = msg {
                    self.status = m;
                }
                if self.session != before {
                    self.push_undo(before);
                    self.dirty = true;
                }
                self.modal = Some(Modal::VehicleCrit { sel });
            }
            _ => self.modal = Some(Modal::VehicleCrit { sel }),
        }
    }

    /// Drive the Override per-region crit popup: ↑↓ pick a table row, Space/Enter records one more
    /// hit of that result (stacking), Backspace/`-` removes one, Esc/`c` close.
    fn ov_crit_modal_key(&mut self, loc: Location, sel: usize, key: KeyEvent) {
        let rows = self
            .session
            .active_mech()
            .and_then(|tm| override_conv::crit_table(&tm.spec, loc))
            .map_or(0, <[_]>::len);
        let max = rows.saturating_sub(1);
        match key.code {
            KeyCode::Esc | KeyCode::Char('c') => {} // close (modal already taken)
            KeyCode::Up | KeyCode::Char('k') => {
                self.modal = Some(Modal::OvCrit {
                    loc,
                    sel: sel.saturating_sub(1),
                });
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.modal = Some(Modal::OvCrit {
                    loc,
                    sel: (sel + 1).min(max),
                });
            }
            KeyCode::Char(' ') | KeyCode::Enter => {
                if let Some(tm) = self.session.active_mech_mut() {
                    tm.ov_add_crit(loc, sel as u8);
                    self.status = format!("Crit ×{}", tm.ov_crit_count(loc, sel as u8));
                    self.dirty = true;
                }
                self.modal = Some(Modal::OvCrit { loc, sel });
            }
            KeyCode::Backspace | KeyCode::Char('-') => {
                if let Some(tm) = self.session.active_mech_mut() {
                    tm.ov_remove_crit(loc, sel as u8);
                    self.status = "Crit cleared".into();
                    self.dirty = true;
                }
                self.modal = Some(Modal::OvCrit { loc, sel });
            }
            // Toggle this region's ammo spent/live (Override's optional ammo handling): a spent bin
            // makes its ammo crit a dud.
            KeyCode::Char('a') => {
                if let Some(tm) = self.session.active_mech_mut() {
                    if tm.ov_region_has_ammo(loc) {
                        let spent = tm.ov_toggle_ammo_spent(loc);
                        self.status = format!("Ammo {}", if spent { "spent" } else { "live" });
                        self.dirty = true;
                    } else {
                        self.status = "No ammo in this location".into();
                    }
                }
                self.modal = Some(Modal::OvCrit { loc, sel });
            }
            _ => self.modal = Some(Modal::OvCrit { loc, sel }),
        }
    }

    /// Drive the Override shot editor: ↑↓ pick a row, ←/→ (or Space) adjust it, Esc/`t` close. Rows
    /// are attacker move, target TMM, target jumped, target immobile, secondary, rear.
    fn ov_shot_modal_key(&mut self, sel: usize, key: KeyEvent) {
        let adjust = |app: &mut App, delta: i32| {
            if let Some(tm) = app.session.active_mech_mut() {
                let s = &mut tm.ov_shot;
                match sel {
                    0 => {
                        s.target_tmm =
                            (s.target_tmm as i32 + delta).clamp(0, OV_TARGET_TMM_MAX as i32) as u8;
                    }
                    1 => s.target_jumped = !s.target_jumped,
                    2 => s.target_immobile = !s.target_immobile,
                    3 => s.secondary = !s.secondary,
                    _ => s.rear = !s.rear,
                }
                app.dirty = true;
            }
        };
        match key.code {
            KeyCode::Esc | KeyCode::Char('t') | KeyCode::Enter => {} // close (modal already taken)
            KeyCode::Up | KeyCode::Char('k') => {
                self.modal = Some(Modal::OvShot {
                    sel: sel.saturating_sub(1),
                });
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.modal = Some(Modal::OvShot {
                    sel: (sel + 1).min(4),
                });
            }
            KeyCode::Right | KeyCode::Char(' ') => {
                adjust(self, 1);
                self.modal = Some(Modal::OvShot { sel });
            }
            KeyCode::Left => {
                adjust(self, -1);
                self.modal = Some(Modal::OvShot { sel });
            }
            _ => self.modal = Some(Modal::OvShot { sel }),
        }
    }

    /// Drive the Motive System Damage popup: pick a table result to apply, or repair the last.
    fn motive_modal_key(&mut self, sel: usize, key: KeyEvent) {
        let max = MotiveLevel::ALL.len() - 1;
        match key.code {
            KeyCode::Esc | KeyCode::Char('m') => {} // close (modal already taken)
            KeyCode::Up | KeyCode::Char('k') => {
                self.modal = Some(Modal::Motive {
                    sel: sel.saturating_sub(1),
                });
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.modal = Some(Modal::Motive {
                    sel: (sel + 1).min(max),
                });
            }
            KeyCode::Char(' ') | KeyCode::Enter => {
                let before = self.session.clone();
                let level = MotiveLevel::ALL[sel];
                if let Some(tm) = self
                    .session
                    .active_mech_mut()
                    .filter(|t| t.spec.is_vehicle())
                {
                    tm.add_motive(level);
                    self.status = format!(
                        "Motive: {} ({:+} MP)",
                        level.label(),
                        -(tm.motive_mp_lost() as i32)
                    );
                }
                if self.session != before {
                    self.push_undo(before);
                    self.dirty = true;
                }
                self.modal = Some(Modal::Motive { sel });
            }
            // r / Backspace: repair the most recent result without leaving the popup.
            KeyCode::Char('r') | KeyCode::Backspace => {
                let before = self.session.clone();
                if let Some(tm) = self
                    .session
                    .active_mech_mut()
                    .filter(|t| t.spec.is_vehicle())
                {
                    self.status = match tm.repair_motive() {
                        Some(lvl) => format!("Motive repaired ({})", lvl.label()),
                        None => "No motive damage".into(),
                    };
                }
                if self.session != before {
                    self.push_undo(before);
                    self.dirty = true;
                }
                self.modal = Some(Modal::Motive { sel });
            }
            _ => self.modal = Some(Modal::Motive { sel }),
        }
    }

    /// Run a confirmed (yes/no) action.
    fn execute(&mut self, action: PendingAction) {
        match action {
            PendingAction::DeleteActiveMech => {
                let idx = self.session.active;
                if idx < self.session.mechs.len() {
                    self.snapshot_undo();
                    let name = self.session.mechs[idx].spec.display_name();
                    self.session.remove_mech(idx);
                    self.dirty = true;
                    self.status = format!("Removed {name}");
                    if self.session.mechs.is_empty() {
                        self.screen = Screen::Picker;
                        self.picker.reset();
                        self.picker
                            .refilter(&self.names, &self.bundle, &self.filters);
                    }
                }
            }
            PendingAction::DeleteActiveFormation => {
                let fi = self.session.sbf.active_formation;
                if let Some((name, mut idxs)) = self.session.sbf.formations.get(fi).map(|f| {
                    let idxs: Vec<usize> = f
                        .units
                        .iter()
                        .flat_map(|u| u.elements.iter().copied())
                        .collect();
                    (f.name.clone(), idxs)
                }) {
                    self.snapshot_undo();
                    // Remove the formation's pool elements highest-first so each removal's
                    // index remap can't shift a later target.
                    idxs.sort_unstable();
                    for &i in idxs.iter().rev() {
                        self.session.remove_mech(i);
                    }
                    // remove_mech keeps emptied formations (first-class workspaces) — deleting
                    // the formation itself is this action's explicit job.
                    if fi < self.session.sbf.formations.len() {
                        self.session.sbf.formations.remove(fi);
                    }
                    self.session.sbf_prune_empty_units(); // reclamp cursors
                    self.dirty = true;
                    self.status = format!("Removed formation {name}");
                    if self.session.mechs.is_empty() && self.session.sbf.formations.is_empty() {
                        self.screen = Screen::Picker;
                        self.picker.reset();
                        self.picker
                            .refilter(&self.names, &self.bundle, &self.filters);
                    }
                }
            }
            PendingAction::ApplyDoctrine(doctrine) => self.sbf_apply_doctrine(doctrine),
            PendingAction::ApplyBfDoctrine(doctrine) => self.bf_apply_doctrine(doctrine),
            PendingAction::AcsAutoGroup => self.acs_auto_group(),
            PendingAction::BfAmmoDetonate => {
                let before = self.session.clone();
                if let Some(tm) = self.session.active_mech_mut() {
                    tm.bf.killed = Some(BfKill::Ammo);
                    self.status = "CASEP failed — ammo detonation, element destroyed".into();
                }
                if self.session != before {
                    self.push_undo(before);
                    self.dirty = true;
                }
            }
            PendingAction::DeleteSession(name) => {
                let _ = session::delete_named(&name);
                self.status = format!("Deleted session '{name}'");
                self.refresh_sessions();
            }
            PendingAction::Quit => self.should_quit = true,
            // Input-driven actions never arrive here.
            PendingAction::NewSession(_)
            | PendingAction::RenameSession(_)
            | PendingAction::RenameFormation
            | PendingAction::RenameAcsFormation
            | PendingAction::AcsDamage
            | PendingAction::RenameUnit
            | PendingAction::RenameBfUnit
            | PendingAction::SetSessionLimit => {}
        }
    }

    /// Ask before quitting (bound to `q`); Ctrl+C still exits immediately without this prompt.
    fn confirm_quit(&mut self) {
        self.modal = Some(Modal::Confirm {
            prompt: "Quit Neurohelmet? (y/n)".into(),
            action: PendingAction::Quit,
        });
    }

    /// Run an action that needed a typed name.
    fn execute_input(&mut self, action: PendingAction, name: String) {
        match action {
            PendingAction::NewSession(mode) => {
                let name = session::sanitize_name(&name);
                let _ = session::save_named(&self.current_name, &self.session); // persist old
                self.undo_stack.clear();
                self.current_name = name.clone();
                self.session = Session::new_with_mode(mode);
                let _ = session::write_current(&name);
                self.dirty = true;
                self.screen = Screen::Picker;
                self.picker.reset();
                self.picker
                    .refilter(&self.names, &self.bundle, &self.filters);
                let kind = match mode {
                    GameMode::AlphaStrike => "Alpha Strike ",
                    GameMode::Override => "Override ",
                    GameMode::StrategicBattleForce => "Strategic BattleForce ",
                    GameMode::BattleForce => "BattleForce ",
                    GameMode::AbstractCombatSystem => "Abstract Combat System ",
                    GameMode::Classic => "",
                };
                self.status = format!("New {kind}session '{name}'");
                // Chain straight into the optional point-limit prompt (blank = no limit).
                self.open_budget_input();
            }
            PendingAction::SetSessionLimit => {
                let trimmed = name.trim();
                if trimmed.is_empty() {
                    self.session.limit = None;
                    self.status = "No point limit".into();
                } else if let Ok(n) = trimmed.parse::<u64>() {
                    self.session.limit = Some(n);
                    self.status = format!("{} limit {n}", self.limit_unit());
                } else {
                    self.status = "Limit must be a number".into();
                }
                self.dirty = true;
            }
            PendingAction::RenameFormation => {
                let fi = self.session.sbf.active_formation;
                let new = name.trim();
                if new.is_empty() {
                    self.status = "Name unchanged".into();
                } else if self.session.sbf.formations.get(fi).is_some() {
                    // Input actions bypass the handle_key snapshot — push undo explicitly so a
                    // single `z` doesn't also swallow the previous unrelated action.
                    self.snapshot_undo();
                    self.session.sbf.formations[fi].name = new.to_string();
                    self.dirty = true;
                    self.status = format!("Renamed to '{new}'");
                }
            }
            PendingAction::RenameAcsFormation => {
                let fi = self.session.acs.active_formation;
                let new = name.trim();
                if new.is_empty() {
                    self.status = "Name unchanged".into();
                } else if self.session.acs.formations.get(fi).is_some() {
                    self.snapshot_undo();
                    self.session.acs.formations[fi].name = new.to_string();
                    self.dirty = true;
                    self.status = format!("Renamed to '{new}'");
                }
            }
            PendingAction::AcsDamage => match name.trim().parse::<i64>() {
                Ok(dmg) if dmg > 0 => {
                    self.snapshot_undo();
                    self.acs_apply_damage(dmg);
                }
                _ => self.status = "Damage must be a positive number".into(),
            },
            PendingAction::RenameUnit => {
                let (fi, ui) = (
                    self.session.sbf.active_formation,
                    self.session.sbf.active_unit,
                );
                let new = name.trim();
                if new.is_empty() {
                    self.status = "Name unchanged".into();
                } else if self
                    .session
                    .sbf
                    .formations
                    .get(fi)
                    .and_then(|f| f.units.get(ui))
                    .is_some()
                {
                    self.snapshot_undo(); // input actions bypass the handle_key snapshot
                    self.session.sbf.formations[fi].units[ui].name = new.to_string();
                    self.dirty = true;
                    self.status = format!("Renamed to '{new}'");
                }
            }
            PendingAction::RenameBfUnit => {
                let ui = self.bf_active_unit();
                let new = name.trim();
                if new.is_empty() {
                    self.status = "Name unchanged".into();
                } else if let Some(ui) = ui.filter(|&ui| ui < self.session.bf.units.len()) {
                    self.snapshot_undo(); // input actions bypass the handle_key snapshot
                    self.session.bf_rename_unit(ui, new);
                    self.dirty = true;
                    self.status = format!("Renamed to '{new}'");
                }
            }
            PendingAction::RenameSession(old) => {
                let new = session::sanitize_name(&name);
                let _ = session::rename_session(&old, &new);
                if old == self.current_name {
                    self.current_name = new.clone();
                    let _ = session::write_current(&new);
                }
                self.status = format!("Renamed to '{new}'");
                self.refresh_sessions();
            }
            PendingAction::DeleteActiveMech
            | PendingAction::DeleteActiveFormation
            | PendingAction::ApplyDoctrine(_)
            | PendingAction::ApplyBfDoctrine(_)
            | PendingAction::AcsAutoGroup
            | PendingAction::BfAmmoDetonate
            | PendingAction::DeleteSession(_)
            | PendingAction::Quit => {}
        }
    }

    /// The point-system label for the current session ("BV" for Classic, "PV" for Alpha Strike).
    fn limit_unit(&self) -> &'static str {
        match self.session.mode {
            GameMode::AlphaStrike
            | GameMode::StrategicBattleForce
            | GameMode::BattleForce
            | GameMode::AbstractCombatSystem => "PV",
            GameMode::Classic | GameMode::Override => "BV",
        }
    }

    /// Open the force point-limit editor (typed). Pre-fills the current limit; blank clears it.
    fn open_budget_input(&mut self) {
        let unit = self.limit_unit();
        self.modal = Some(Modal::Input {
            prompt: format!("Force {unit} limit (blank = none):"),
            buffer: self
                .session
                .limit
                .map(|l| l.to_string())
                .unwrap_or_default(),
            action: PendingAction::SetSessionLimit,
        });
    }

    fn snapshot_undo(&mut self) {
        let snap = self.session.clone();
        self.push_undo(snap);
    }

    // ----- Sessions screen -----

    fn open_sessions(&mut self) {
        self.refresh_sessions();
        self.screen = Screen::Sessions;
    }

    fn refresh_sessions(&mut self) {
        self.sessions = session::list_sessions();
        if self.sessions.is_empty() {
            self.sessions_sel = 0;
        } else {
            self.sessions_sel = self
                .sessions
                .iter()
                .position(|m| m.name == self.current_name)
                .unwrap_or(0)
                .min(self.sessions.len() - 1);
        }
    }

    fn sessions_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                self.screen = self.loaded_screen();
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.sessions_sel = self.sessions_sel.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.sessions_sel + 1 < self.sessions.len() {
                    self.sessions_sel += 1;
                }
            }
            KeyCode::Enter => self.load_selected_session(),
            KeyCode::Char('n') => {
                self.modal = Some(Modal::Input {
                    prompt: "New session name:".into(),
                    buffer: String::new(),
                    action: PendingAction::NewSession(GameMode::Classic),
                });
            }
            KeyCode::Char('A') => {
                self.modal = Some(Modal::Input {
                    prompt: "New Alpha Strike session name:".into(),
                    buffer: String::new(),
                    action: PendingAction::NewSession(GameMode::AlphaStrike),
                });
            }
            KeyCode::Char('O') => {
                self.modal = Some(Modal::Input {
                    prompt: "New Override session name:".into(),
                    buffer: String::new(),
                    action: PendingAction::NewSession(GameMode::Override),
                });
            }
            KeyCode::Char('B') => {
                self.modal = Some(Modal::Input {
                    prompt: "New Strategic BattleForce session name:".into(),
                    buffer: String::new(),
                    action: PendingAction::NewSession(GameMode::StrategicBattleForce),
                });
            }
            KeyCode::Char('F') => {
                self.modal = Some(Modal::Input {
                    prompt: "New BattleForce session name:".into(),
                    buffer: String::new(),
                    action: PendingAction::NewSession(GameMode::BattleForce),
                });
            }
            KeyCode::Char('C') => {
                self.modal = Some(Modal::Input {
                    prompt: "New Abstract Combat System session name:".into(),
                    buffer: String::new(),
                    action: PendingAction::NewSession(GameMode::AbstractCombatSystem),
                });
            }
            KeyCode::Char('r') => {
                if let Some(m) = self.sessions.get(self.sessions_sel) {
                    self.modal = Some(Modal::Input {
                        prompt: format!("Rename '{}' to:", m.name),
                        buffer: m.name.clone(),
                        action: PendingAction::RenameSession(m.name.clone()),
                    });
                }
            }
            KeyCode::Char('D') => {
                if let Some(m) = self.sessions.get(self.sessions_sel) {
                    if m.name == self.current_name {
                        self.status = "Can't delete the active session".into();
                    } else {
                        self.modal = Some(Modal::Confirm {
                            prompt: format!("Delete session '{}'? (y/n)", m.name),
                            action: PendingAction::DeleteSession(m.name.clone()),
                        });
                    }
                }
            }
            _ => {}
        }
    }

    fn load_selected_session(&mut self) {
        let Some(meta) = self.sessions.get(self.sessions_sel).cloned() else {
            return;
        };
        if meta.name == self.current_name {
            self.status = "Already active".into();
            self.screen = self.loaded_screen();
            return;
        }
        match session::load_named(&meta.name) {
            Ok(Some(mut loaded)) => {
                let _ = session::save_named(&self.current_name, &self.session); // persist old
                self.undo_stack.clear();
                self.current_name = meta.name.clone();
                // Migrate the loaded session's specs to the current data format.
                let migrated = loaded.relink_specs(&self.bundle);
                self.session = loaded;
                let _ = session::write_current(&meta.name);
                self.dirty = true;
                self.clamp_selection();
                self.screen = self.loaded_screen();
                self.status = if migrated > 0 {
                    format!("Loaded '{}' (updated {migrated} spec(s))", meta.name)
                } else {
                    format!("Loaded '{}'", meta.name)
                };
            }
            _ => self.status = format!("Couldn't load '{}'", meta.name),
        }
    }

    // ----- Picker -----

    fn picker_key(&mut self, key: KeyEvent) {
        // Ctrl+F opens the filter editor. Checked before the `Char(c)` arm, which would otherwise
        // type 'f' into the search query (the search box consumes every plain letter).
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('f') {
            self.modal = Some(Modal::Filters { sel: 0 });
            return;
        }
        // Ctrl+B sets the force point limit (a plain letter would type into the search box).
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('b') {
            self.open_budget_input();
            return;
        }
        // Ctrl+G opens the §35 weighted force generator.
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('g') {
            self.open_force_gen();
            return;
        }
        match key.code {
            KeyCode::Esc => {
                if self.show_preview {
                    self.show_preview = false;
                } else if self.session.mechs.is_empty() {
                    // Nothing to go back to — offer the session browser (load/new) instead.
                    self.open_sessions();
                } else {
                    self.picker.reset();
                    self.screen = self.tracker_screen();
                }
            }
            // Toggle the unit-preview popup for the highlighted 'Mech.
            KeyCode::Tab => self.show_preview = !self.show_preview,
            // Enter opens the pre-add skill + cost preview (commit from there) rather than adding
            // immediately, so you can set crew skill and see the point cost before committing.
            KeyCode::Enter => {
                if let Some(idx) = self.picker.current() {
                    // AS-only units (hand-entered emplacements / Battlefield Support) have no
                    // Classic record sheet (and so no Override card), so they can only join an
                    // Alpha Strike session.
                    if matches!(self.session.mode, GameMode::Classic | GameMode::Override)
                        && self.bundle.get(idx).is_some_and(Mech::is_as_only)
                    {
                        self.status = "Alpha Strike-only unit — add it to an AS session".into();
                        return;
                    }
                    self.modal = Some(Modal::AddUnit {
                        idx,
                        gunnery: DEFAULT_GUNNERY,
                        piloting: DEFAULT_PILOTING,
                        sel: 0,
                    });
                }
            }
            KeyCode::Up => self.picker.move_selection(-1),
            KeyCode::Down => self.picker.move_selection(1),
            KeyCode::PageUp => self.picker.page_jump(-1),
            KeyCode::PageDown => self.picker.page_jump(1),
            KeyCode::Backspace => {
                self.picker.query.pop();
                self.picker
                    .refilter(&self.names, &self.bundle, &self.filters);
            }
            KeyCode::Char(c) => {
                self.picker.query.push(c);
                self.picker
                    .refilter(&self.names, &self.bundle, &self.filters);
            }
            _ => {}
        }
    }

    /// Add a unit to the session at the given crew skills, then report the running force total
    /// against the session's budget (if any).
    fn add_mech_with_skills(&mut self, mech: Mech, gunnery: u8, piloting: u8) -> bool {
        let name = mech.display_name();
        if self.session.add_mech(mech) {
            if let Some(tm) = self.session.active_mech_mut() {
                tm.gunnery = gunnery;
                tm.piloting = piloting;
            }
            self.dirty = true;
            let total = self.session.force_total();
            self.status = match self.session.limit {
                Some(l) if total > l => format!("Added {name} — OVER budget ({total}/{l})"),
                Some(l) => format!("Added {name}  ({total}/{l})"),
                None => format!("Added {name}"),
            };
            true
        } else {
            // add_mech only fails on a capped mode (Alpha Strike is uncapped).
            self.status = format!("Roster full (max {})", session::MAX_MECHS);
            false
        }
    }

    // ----- §35 weighted force generator -----

    /// Open the force-generator modal, pre-filled from the current availability lens.
    fn open_force_gen(&mut self) {
        let fg = ForceGen {
            faction: self.filters.faction.clone(),
            era: self.filters.avail_era.clone(),
            count: 4,
            use_budget: self.session.limit.is_some(),
            allow_rare: false,
            class_bias: None,
            seed: fresh_seed(),
            field: 0,
            rolled: false,
            preview: Vec::new(),
            note: String::new(),
        };
        self.modal = Some(Modal::GenerateForce(fg));
    }

    /// Run the weighted draw for the current config, updating `preview`/`note`/`rolled`.
    fn roll_force(&mut self, fg: &mut ForceGen) {
        // One roll yields at most a batch (so the preview fits the modal); an uncapped Alpha Strike
        // roster grows past it by rolling/accepting repeatedly. Capped modes stop at their free slots.
        let batch = session::MAX_MECHS;
        let free = match session::Session::mech_cap(self.session.mode) {
            Some(cap) => cap.saturating_sub(self.session.mechs.len()),
            None => batch,
        };
        let max_units = free.min(batch);
        // Hard gates: Phase 1 is 'Mech-only; the lens fields don't hide (cleared so matches ignores
        // them), but every other set facet (tonnage/role/class/family/year) constrains the pool.
        let mut hard = self.filters.clone();
        hard.unit_type = Some(filters::TypeFilter::Unit(UnitType::Mech));
        hard.faction = None;
        hard.avail_era = None;
        let era_to = fg
            .era
            .as_ref()
            .and_then(|(id, _)| self.bundle.eras.iter().find(|e| e.id == *id).map(|e| e.to));
        let cfg = forcegen::GenConfig {
            faction: fg.faction.as_ref().map(|(id, _)| *id),
            era_id: fg.era.as_ref().map(|(id, _)| *id),
            era_to,
            count: fg.count,
            budget: if fg.use_budget {
                self.session.limit
            } else {
                None
            },
            allow_rare: fg.allow_rare,
            class_bias: fg.class_bias.as_deref(),
            mode: self.session.mode,
            max_units,
        };
        fg.preview = forcegen::generate(&self.bundle, &hard, &cfg, fg.seed);
        fg.rolled = true;
        let pt = match self.session.mode {
            GameMode::AlphaStrike
            | GameMode::StrategicBattleForce
            | GameMode::BattleForce
            | GameMode::AbstractCombatSystem => "PV",
            GameMode::Classic | GameMode::Override => "BV",
        };
        // Distinguish "no candidates" from "candidates exist but the budget is too tight".
        let budget_capped = cfg.budget.is_some_and(|b| {
            forcegen::eligible_count(&self.bundle, &hard, &cfg) > 0
                && fg.preview.len() < fg.count.min(max_units)
                && self.session.limit == Some(b)
        });
        fg.note = if max_units == 0 {
            format!("Roster is full (max {}).", session::MAX_MECHS)
        } else if fg.preview.is_empty() && budget_capped {
            let l = cfg.budget.unwrap_or(0);
            format!("No 'Mech fits the {l} {pt} budget — raise the limit (^b) or widen filters.")
        } else if fg.preview.is_empty() {
            "No canon 'Mechs for this faction/era. Try “Allow rare”, or widen the era.".into()
        } else if budget_capped {
            let l = cfg.budget.unwrap_or(0);
            format!(
                "Budget {l} {pt} fit {} of {} units.",
                fg.preview.len(),
                fg.count
            )
        } else {
            String::new()
        };
    }

    /// Append the rolled preview to the roster at default skills (undoable).
    fn accept_force(&mut self, fg: &ForceGen) {
        if fg.preview.is_empty() {
            self.status = "Nothing to add".into();
            return;
        }
        self.push_undo(self.session.clone());
        let mut added = 0usize;
        for &idx in &fg.preview {
            let Some(spec) = self.bundle.get(idx).cloned() else {
                continue;
            };
            if !self.session.add_mech(spec) {
                break; // roster full
            }
            if let Some(tm) = self.session.active_mech_mut() {
                tm.gunnery = DEFAULT_GUNNERY;
                tm.piloting = DEFAULT_PILOTING;
            }
            added += 1;
        }
        self.dirty = true;
        let total = self.session.force_total();
        self.status = match self.session.limit {
            Some(l) => format!("Generated {added} unit(s) — force {total}/{l}"),
            None => format!("Generated {added} unit(s) — force BV/PV {total}"),
        };
    }

    fn force_gen_modal_key(&mut self, mut fg: ForceGen, key: KeyEvent) {
        if !fg.rolled {
            // Config stage: edit the parameters.
            match key.code {
                KeyCode::Esc => return, // cancel (modal already taken)
                KeyCode::Up | KeyCode::Char('k') => {
                    fg.field = fg.field.checked_sub(1).unwrap_or(ForceGen::ROWS.len() - 1);
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    fg.field = (fg.field + 1) % ForceGen::ROWS.len();
                }
                KeyCode::Left => self.force_gen_adjust(&mut fg, -1),
                KeyCode::Right | KeyCode::Char(' ') => self.force_gen_adjust(&mut fg, 1),
                KeyCode::Enter | KeyCode::Char('r') => self.roll_force(&mut fg),
                _ => {}
            }
        } else {
            // Result stage: Accept / Reroll / back / cancel.
            match key.code {
                KeyCode::Enter => {
                    self.accept_force(&fg);
                    return; // close
                }
                KeyCode::Char('r') => {
                    fg.seed = next_seed(fg.seed);
                    self.roll_force(&mut fg);
                }
                KeyCode::Backspace => {
                    fg.rolled = false; // back to config
                    fg.preview.clear();
                    fg.note.clear();
                }
                KeyCode::Esc => return, // cancel
                _ => {}
            }
        }
        self.modal = Some(Modal::GenerateForce(fg));
    }

    /// Change the selected config row's value by `dir`.
    fn force_gen_adjust(&mut self, fg: &mut ForceGen, dir: i32) {
        match fg.field {
            0 => fg.faction = filters::cycle_opt(&fg.faction, &self.facet_values.factions, dir),
            1 => fg.era = filters::cycle_opt(&fg.era, &self.facet_values.eras, dir),
            2 => fg.count = (fg.count as i32 + dir).clamp(1, session::MAX_MECHS as i32) as usize,
            3 => fg.use_budget = !fg.use_budget,
            4 => fg.allow_rare = !fg.allow_rare,
            5 => {
                let bias: Vec<String> = forcegen::CLASS_BIAS
                    .iter()
                    .map(|s| (*s).to_string())
                    .collect();
                fg.class_bias = filters::cycle_opt(&fg.class_bias, &bias, dir);
            }
            _ => {}
        }
    }

    // ----- Alpha Strike -----

    fn alpha_strike_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('q') => self.confirm_quit(),
            KeyCode::Char('a') => {
                self.screen = Screen::Picker;
                self.picker.reset();
                self.picker
                    .refilter(&self.names, &self.bundle, &self.filters);
            }
            KeyCode::Char('S') => self.open_sessions(),
            KeyCode::Char('D') => {
                if let Some(tm) = self.session.active_mech() {
                    self.modal = Some(Modal::Confirm {
                        prompt: format!(
                            "Remove {} from this session? (y/n)",
                            tm.spec.display_name()
                        ),
                        action: PendingAction::DeleteActiveMech,
                    });
                }
            }
            KeyCode::Char('?') => self.modal = Some(Modal::Help),
            // Edit the single Alpha Strike Skill (the gunnery field) and the force budget.
            KeyCode::Char('g') => self.modal = Some(Modal::Skills { sel: 0 }),
            KeyCode::Char('b') => self.open_budget_input(),
            // Toggle 1:1 ground (hex) scale — movement + ranges shown in hexes instead of inches.
            KeyCode::Char('1') => {
                self.session.as_ground_scale = !self.session.as_ground_scale;
                self.dirty = true;
                self.status = if self.session.as_ground_scale {
                    "1:1 ground scale (hexes)".into()
                } else {
                    "Standard scale (inches)".into()
                };
            }
            KeyCode::Char('[') | KeyCode::Char(',') => {
                self.session.switch(-1);
                self.clamp_selection();
            }
            KeyCode::Char(']') | KeyCode::Char('.') => {
                self.session.switch(1);
                self.clamp_selection();
            }
            // `<` / `>` jump a full screen-page (4 units, the Alpha Strike card grid width).
            KeyCode::Char('<') => {
                self.session.switch(-4);
                self.clamp_selection();
            }
            KeyCode::Char('>') => {
                self.session.switch(4);
                self.clamp_selection();
            }
            KeyCode::Char(' ') | KeyCode::Enter => {
                if let Some(tm) = self.session.active_mech_mut() {
                    tm.as_damage();
                    self.status = format!(
                        "Armor {} / Struct {}",
                        tm.as_armor_remaining(),
                        tm.as_struct_remaining()
                    );
                    self.dirty = true;
                }
            }
            KeyCode::Char('u') => {
                if let Some(tm) = self.session.active_mech_mut() {
                    tm.as_repair();
                    self.dirty = true;
                }
            }
            KeyCode::Char('o') => {
                if let Some(tm) = self.session.active_mech_mut() {
                    tm.as_adjust_heat(1);
                    self.dirty = true;
                }
            }
            KeyCode::Char('i') => {
                if let Some(tm) = self.session.active_mech_mut() {
                    tm.as_adjust_heat(-1);
                    self.dirty = true;
                }
            }
            KeyCode::Char('c') => {
                if self.session.active_mech().is_some() {
                    self.modal = Some(Modal::AsCrit { sel: 0 });
                }
            }
            KeyCode::Char('t') => {
                if self.session.active_mech().is_some() {
                    self.modal = Some(Modal::Shot { sel: 0 });
                }
            }
            KeyCode::Char('L') => self.log_snapshot(),
            _ => {}
        }
    }

    // ----- Tracker -----

    fn tracker_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('q') => self.confirm_quit(),
            KeyCode::Char('a') => {
                self.screen = Screen::Picker;
                self.picker.reset();
                self.picker
                    .refilter(&self.names, &self.bundle, &self.filters);
            }
            KeyCode::Char('S') => self.open_sessions(),
            KeyCode::Char('D') => {
                if let Some(tm) = self.session.active_mech() {
                    self.modal = Some(Modal::Confirm {
                        prompt: format!(
                            "Remove {} from this session? (y/n)",
                            tm.spec.display_name()
                        ),
                        action: PendingAction::DeleteActiveMech,
                    });
                }
            }
            // Shift+Tab arrives as BackTab (we don't enable crossterm's keyboard-enhancement
            // flags, so it isn't Tab+SHIFT) — cycle to the previous mech.
            KeyCode::BackTab => {
                self.session.switch(-1);
                self.clamp_selection();
            }
            KeyCode::Tab => self.toggle_focus(),
            // `,` / `.` are the Planck-friendly aliases for previous / next mech.
            KeyCode::Char('[') | KeyCode::Char(',') => {
                self.session.switch(-1);
                self.clamp_selection();
            }
            KeyCode::Char(']') | KeyCode::Char('.') => {
                self.session.switch(1);
                self.clamp_selection();
            }
            KeyCode::Char('f') => {
                self.facing = match self.facing {
                    Facing::Front => Facing::Rear,
                    Facing::Rear => Facing::Front,
                };
            }
            // Heat up / down (matches the Alpha Strike card, which is `o`/`i` only).
            KeyCode::Char('o') => self.adjust_heat(1),
            KeyCode::Char('i') => self.adjust_heat(-1),
            KeyCode::Char('e') => self.end_turn(),
            KeyCode::Char('L') => self.log_snapshot(),
            KeyCode::Char('d') => self.toggle_prone(),
            KeyCode::Char('g') => self.modal = Some(Modal::Skills { sel: 0 }),
            KeyCode::Char('b') => self.open_budget_input(),
            KeyCode::Char('v') => self.modal = Some(Modal::Move { sel: 0 }),
            KeyCode::Char('t') => {
                if self.session.active_mech().is_some() {
                    self.modal = Some(Modal::Gator { sel: 0 });
                }
            }
            KeyCode::Char('x') => {
                if let Some(tm) = self.session.active_mech_mut() {
                    tm.toggle_shutdown();
                    self.status = if tm.shutdown {
                        "Shutdown".into()
                    } else {
                        "Restarted".into()
                    };
                    self.dirty = true;
                }
            }
            KeyCode::Char('X') => {
                if let Some(tm) = self.session.active_mech_mut() {
                    tm.toggle_unconscious();
                    self.status = if tm.pilot_dead() {
                        "Pilot is dead".into()
                    } else if tm.pilot_unconscious {
                        "Pilot knocked out".into()
                    } else {
                        "Pilot conscious".into()
                    };
                    self.dirty = true;
                }
            }
            KeyCode::Char(' ') | KeyCode::Enter => self.primary_action(),
            KeyCode::Char('u') => self.secondary_action(),
            KeyCode::Char('J') => self.toggle_equip_state(),
            KeyCode::Char('c') => self.open_crit(),
            KeyCode::Char('r') => self.open_dice(),
            // p/P: pilot hits for 'Mechs, crew hits for vehicles. m/M: vehicle motive hits.
            KeyCode::Char('p') => {
                if let Some(tm) = self.session.active_mech_mut() {
                    if tm.spec.is_vehicle() {
                        tm.hit_crew();
                        self.status = format!("Crew hit ({}/{CREW_MAX})", tm.crew_hits);
                    } else {
                        tm.hit_pilot();
                        self.status = format!("Pilot hit ({}/{PILOT_MAX})", tm.pilot_hits);
                    }
                    self.dirty = true;
                }
            }
            KeyCode::Char('P') => {
                if let Some(tm) = self.session.active_mech_mut() {
                    if tm.spec.is_vehicle() {
                        tm.heal_crew();
                        self.status = format!("Crew healed ({}/{CREW_MAX})", tm.crew_hits);
                    } else {
                        tm.heal_pilot();
                        self.status = format!("Pilot healed ({}/{PILOT_MAX})", tm.pilot_hits);
                    }
                    self.dirty = true;
                }
            }
            // m: open the Motive System Damage table (roll a result, pick its severity).
            KeyCode::Char('m') => {
                if self
                    .session
                    .active_mech()
                    .is_some_and(|t| t.spec.is_vehicle())
                {
                    self.modal = Some(Modal::Motive { sel: 0 });
                }
            }
            // M: quick-repair the most recent motive result.
            KeyCode::Char('M') => {
                let before = self.session.clone();
                if let Some(tm) = self
                    .session
                    .active_mech_mut()
                    .filter(|t| t.spec.is_vehicle())
                {
                    self.status = match tm.repair_motive() {
                        Some(lvl) => format!("Motive repaired ({})", lvl.label()),
                        None => "No motive damage".into(),
                    };
                }
                if self.session != before {
                    self.push_undo(before);
                    self.dirty = true;
                }
            }
            KeyCode::Char('?') => self.modal = Some(Modal::Help),
            KeyCode::Up | KeyCode::Char('k') => self.move_selection(Dir::Up),
            KeyCode::Down | KeyCode::Char('j') => self.move_selection(Dir::Down),
            KeyCode::Left | KeyCode::Char('h') => self.move_selection(Dir::Left),
            KeyCode::Right | KeyCode::Char('l') => self.move_selection(Dir::Right),
            _ => {}
        }
    }

    /// Drive the live Override card (Override-mode tracker). Mirrors the Classic record sheet: `Tab`
    /// toggles between the weapons (TIC) panel and the armor panel; `Space`/`u` damage/repair the
    /// selected region or fire/un-fire the selected TIC; `o`/`i` adjust heat on the 0–5 ladder; `c`
    /// opens the per-region crit popup; pilot/crew, skills, end-turn and roster keys match Classic.
    fn override_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('q') => self.confirm_quit(),
            KeyCode::Char('a') => {
                self.screen = Screen::Picker;
                self.picker.reset();
                self.picker
                    .refilter(&self.names, &self.bundle, &self.filters);
            }
            KeyCode::Char('S') => self.open_sessions(),
            KeyCode::Char('D') => {
                if let Some(tm) = self.session.active_mech() {
                    self.modal = Some(Modal::Confirm {
                        prompt: format!(
                            "Remove {} from this session? (y/n)",
                            tm.spec.display_name()
                        ),
                        action: PendingAction::DeleteActiveMech,
                    });
                }
            }
            KeyCode::BackTab => {
                self.session.switch(-1);
                self.clamp_selection();
            }
            KeyCode::Tab => self.toggle_focus(),
            KeyCode::Char('[') | KeyCode::Char(',') => {
                self.session.switch(-1);
                self.clamp_selection();
            }
            KeyCode::Char(']') | KeyCode::Char('.') => {
                self.session.switch(1);
                self.clamp_selection();
            }
            KeyCode::Char('f') => {
                self.facing = match self.facing {
                    Facing::Front => Facing::Rear,
                    Facing::Rear => Facing::Front,
                };
            }
            KeyCode::Char('o') => self.ov_adjust_heat(1),
            KeyCode::Char('i') => self.ov_adjust_heat(-1),
            KeyCode::Char('e') => {
                if let Some(tm) = self.session.active_mech_mut() {
                    tm.ov_end_turn();
                    self.status = "Turn ended (heat dissipated)".into();
                    self.dirty = true;
                }
            }
            KeyCode::Char('x') => {
                if let Some(tm) = self.session.active_mech_mut() {
                    tm.toggle_shutdown();
                    self.status = if tm.shutdown {
                        "Shutdown".into()
                    } else {
                        "Restarted".into()
                    };
                    self.dirty = true;
                }
            }
            KeyCode::Char(' ') | KeyCode::Enter => self.ov_primary_action(),
            KeyCode::Char('u') => self.ov_secondary_action(),
            KeyCode::Char('c') => self.ov_open_crit(),
            KeyCode::Char('t') => {
                if self.session.active_mech().is_some() {
                    self.modal = Some(Modal::OvShot { sel: 0 });
                }
            }
            KeyCode::Char('v') => {
                if self.session.active_mech().is_some() {
                    self.modal = Some(Modal::Move { sel: 0 });
                }
            }
            KeyCode::Char('g') => self.modal = Some(Modal::Skills { sel: 0 }),
            KeyCode::Char('L') => self.log_snapshot(),
            KeyCode::Char('p') => {
                if let Some(tm) = self.session.active_mech_mut() {
                    if tm.spec.is_vehicle() {
                        tm.hit_crew();
                        self.status = format!("Crew hit ({}/{CREW_MAX})", tm.crew_hits);
                    } else {
                        tm.hit_pilot();
                        self.status = format!("Pilot hit ({}/{PILOT_MAX})", tm.pilot_hits);
                    }
                    self.dirty = true;
                }
            }
            KeyCode::Char('P') => {
                if let Some(tm) = self.session.active_mech_mut() {
                    if tm.spec.is_vehicle() {
                        tm.heal_crew();
                        self.status = format!("Crew healed ({}/{CREW_MAX})", tm.crew_hits);
                    } else {
                        tm.heal_pilot();
                        self.status = format!("Pilot healed ({}/{PILOT_MAX})", tm.pilot_hits);
                    }
                    self.dirty = true;
                }
            }
            KeyCode::Char('?') => self.modal = Some(Modal::Help),
            KeyCode::Up | KeyCode::Char('k') => self.ov_navigate(Dir::Up),
            KeyCode::Down | KeyCode::Char('j') => self.ov_navigate(Dir::Down),
            KeyCode::Left | KeyCode::Char('h') => self.ov_navigate(Dir::Left),
            KeyCode::Right | KeyCode::Char('l') => self.ov_navigate(Dir::Right),
            _ => {}
        }
    }

    /// Move the Override selection: the armor doll cursor (spatially, like the Classic doll) when
    /// the doll is focused, or the TIC list when the weapons panel is focused.
    fn ov_navigate(&mut self, dir: Dir) {
        match self.focus {
            Focus::Doll => {
                let locs = self
                    .session
                    .active_mech()
                    .map(|tm| tm.ov_region_locs())
                    .unwrap_or_default();
                if let Some(next) = move_cursor(self.cursor, dir, &locs) {
                    self.cursor = next;
                    // A region without rear armor can't show the rear face.
                    let has_rear = self
                        .session
                        .active_mech()
                        .and_then(|tm| tm.ov_regions().into_iter().find(|r| r.loc == next))
                        .is_some_and(|r| r.rear.is_some());
                    if !has_rear {
                        self.facing = Facing::Front;
                    }
                }
            }
            Focus::Equipment => {
                let n = self
                    .session
                    .active_mech()
                    .map_or(0, |tm| tm.ov_card().tics.len());
                if n == 0 {
                    return;
                }
                let step = match dir {
                    Dir::Up | Dir::Left => -1,
                    Dir::Down | Dir::Right => 1,
                };
                self.ov_tic = (self.ov_tic as i32 + step).rem_euclid(n as i32) as usize;
            }
        }
    }

    /// The Override region at the doll cursor (falling back to the first region if the cursor has
    /// drifted off a valid region loc).
    fn ov_cursor_loc(&self) -> Option<Location> {
        let tm = self.session.active_mech()?;
        let locs = tm.ov_region_locs();
        if locs.contains(&self.cursor) {
            Some(self.cursor)
        } else {
            locs.first().copied()
        }
    }

    /// Adjust the active unit's Override heat (no-op for vehicles, which have no heat track).
    fn ov_adjust_heat(&mut self, delta: i32) {
        if let Some(tm) = self.session.active_mech_mut() {
            if !tm.ov_has_heat() {
                self.status = "No heat track on this unit".into();
                return;
            }
            tm.ov_adjust_heat(delta);
            self.status = format!("Heat {}", tm.ov_heat);
            self.dirty = true;
        }
    }

    /// `Space` in Override: damage the selected armor region (front/rear per facing) when the doll
    /// is focused, or fire the selected TIC (banking its heat) when the weapons panel is focused.
    fn ov_primary_action(&mut self) {
        let facing = self.facing;
        let loc = self.ov_cursor_loc();
        let tic = self.ov_tic;
        let Some(tm) = self.session.active_mech_mut() else {
            return;
        };
        match self.focus {
            Focus::Doll => {
                let Some(loc) = loc else { return };
                let rear = facing == Facing::Rear
                    && tm
                        .ov_regions()
                        .iter()
                        .find(|r| r.loc == loc)
                        .is_some_and(|r| r.rear.is_some());
                tm.ov_damage(loc, rear);
                let layer = if rear { "rear" } else { "front" };
                self.status = format!(
                    "{} {layer}: A {} / S {}",
                    loc.code(),
                    tm.ov_armor_remaining(loc),
                    tm.ov_struct_remaining(loc)
                );
            }
            Focus::Equipment => {
                let card = tm.ov_card();
                let Some(row) = card.tics.get(tic) else {
                    return;
                };
                let heat = if tm.ov_has_heat() { row.heat } else { 0 };
                let name = row.name.clone();
                tm.ov_fire_tic(tic, heat);
                self.status = format!("Fired {name} (heat {})", tm.ov_heat);
            }
        }
        self.dirty = true;
    }

    /// `u` in Override: repair the cursored region, or un-fire the selected TIC.
    fn ov_secondary_action(&mut self) {
        let facing = self.facing;
        let loc = self.ov_cursor_loc();
        let tic = self.ov_tic;
        let Some(tm) = self.session.active_mech_mut() else {
            return;
        };
        match self.focus {
            Focus::Doll => {
                let Some(loc) = loc else { return };
                let rear = facing == Facing::Rear
                    && tm
                        .ov_regions()
                        .iter()
                        .find(|r| r.loc == loc)
                        .is_some_and(|r| r.rear.is_some());
                tm.ov_repair(loc, rear);
                self.status = format!("Repaired {}", loc.code());
            }
            Focus::Equipment => {
                let card = tm.ov_card();
                let Some(row) = card.tics.get(tic) else {
                    return;
                };
                let heat = if tm.ov_has_heat() { row.heat } else { 0 };
                tm.ov_unfire_tic(tic, heat);
                self.status = "Un-fired".into();
            }
        }
        self.dirty = true;
    }

    /// Open the Override crit popup for the cursored armor region (if it has a crit table).
    fn ov_open_crit(&mut self) {
        let Some(loc) = self.ov_cursor_loc() else {
            return;
        };
        let Some(tm) = self.session.active_mech() else {
            return;
        };
        if override_conv::crit_table(&tm.spec, loc).is_some() {
            self.modal = Some(Modal::OvCrit { loc, sel: 0 });
        } else {
            self.status = format!("{} has no crit table", loc.code());
        }
    }

    // ================= Strategic BattleForce (Screen::Sbf — spec Phase 5) =================

    /// SBF screen keys. Reuses the AS/Override verbs; the formation/unit selection lives on
    /// `session.sbf` (persisted, so undo restores it), the hand-entered shot target on the
    /// ephemeral `App::sbf_*` fields.
    /// Export the current SBF session to a PDF record sheet (the `P` key; sibling of `--pdf`).
    /// Renders the live in-memory state, so it reflects exactly what's on screen.
    fn export_pdf(&mut self) {
        if !matches!(
            self.session.mode,
            GameMode::StrategicBattleForce | GameMode::BattleForce | GameMode::AbstractCombatSystem
        ) {
            self.status = "PDF export supports BF, SBF, and ACS sessions only".into();
            return;
        }
        match crate::pdf::export_session(&self.session, &self.current_name, None) {
            Ok(path) => self.status = format!("Wrote PDF record sheet → {}", path.display()),
            Err(e) => self.status = format!("PDF export failed: {e}"),
        }
    }

    fn sbf_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('q') => self.confirm_quit(),
            KeyCode::Char('a') => {
                self.screen = Screen::Picker;
                self.picker.reset();
                self.picker
                    .refilter(&self.names, &self.bundle, &self.filters);
            }
            KeyCode::Char('S') => self.open_sessions(),
            KeyCode::Char('P') => self.export_pdf(),
            KeyCode::Char('D') => {
                if let Some(f) = self
                    .session
                    .sbf
                    .formations
                    .get(self.session.sbf.active_formation)
                {
                    self.modal = Some(Modal::Confirm {
                        prompt: format!("Remove formation {} and its elements? (y/n)", f.name),
                        action: PendingAction::DeleteActiveFormation,
                    });
                }
            }
            KeyCode::Char('[') | KeyCode::Char(',') | KeyCode::BackTab => {
                self.sbf_cycle_formation(-1);
            }
            KeyCode::Char(']') | KeyCode::Char('.') | KeyCode::Tab => {
                self.sbf_cycle_formation(1);
            }
            KeyCode::Up | KeyCode::Char('k') => self.sbf_cycle_unit(-1),
            KeyCode::Down | KeyCode::Char('j') => self.sbf_cycle_unit(1),
            KeyCode::Char(' ') | KeyCode::Enter => self.sbf_damage(),
            KeyCode::Char('u') => self.sbf_repair(),
            KeyCode::Char('c') => {
                if self.sbf_active_unit().is_some() {
                    self.modal = Some(Modal::SbfCrit { sel: 0 });
                }
            }
            KeyCode::Char('t') => {
                if self.sbf_active_unit().is_some() {
                    self.modal = Some(Modal::SbfShot { sel: 0 });
                }
            }
            KeyCode::Char('m') => self.sbf_cycle_morale(),
            KeyCode::Char('n') => {
                if !self.session.sbf.formations.is_empty() {
                    self.session.sbf.begin_round();
                    self.dirty = true;
                    self.status = format!("Round {} begun", self.session.sbf.round);
                }
            }
            KeyCode::Char('e') => {
                let fi = self.session.sbf.active_formation;
                if let Some(f) = self.session.sbf.formations.get(fi) {
                    let name = f.name.clone();
                    self.session.sbf.end_turn(fi);
                    self.dirty = true;
                    self.status = format!("{name} done this turn");
                }
            }
            KeyCode::Char('g') => {
                if self.session.mechs.is_empty() {
                    self.status = "No elements to group — [a] adds some".into();
                } else {
                    // Start on the element the sidebar highlights (usually the one just added).
                    let sel = self.session.active.min(self.session.mechs.len() - 1);
                    self.modal = Some(Modal::SbfGroup { sel });
                }
            }
            KeyCode::Char('r') => {
                if let Some(f) = self
                    .session
                    .sbf
                    .formations
                    .get(self.session.sbf.active_formation)
                {
                    self.modal = Some(Modal::Input {
                        prompt: format!("Rename formation '{}':", f.name),
                        buffer: f.name.clone(),
                        action: PendingAction::RenameFormation,
                    });
                }
            }
            KeyCode::Char('R') => {
                if let Some((fi, ui)) = self.sbf_active_unit() {
                    let name = self.session.sbf.formations[fi].units[ui].name.clone();
                    self.modal = Some(Modal::Input {
                        prompt: format!("Rename unit '{name}':"),
                        buffer: name,
                        action: PendingAction::RenameUnit,
                    });
                }
            }
            KeyCode::Char('C') => {
                if let Some((fi, ui)) = self.sbf_active_unit() {
                    self.session.sbf_set_commander(fi, ui);
                    self.dirty = true;
                    let u = &self.session.sbf.formations[fi].units[ui];
                    self.status = if u.is_commander {
                        format!("{}: Force Commander (COM)", u.name)
                    } else {
                        "Force Commander cleared".into()
                    };
                }
            }
            KeyCode::Char('l') => {
                if let Some((fi, ui)) = self.sbf_active_unit() {
                    self.session.sbf_set_leader(fi, ui);
                    self.dirty = true;
                    let u = &self.session.sbf.formations[fi].units[ui];
                    self.status = if u.is_leader {
                        format!("{}: Formation Leader (LEAD)", u.name)
                    } else {
                        "Formation Leader cleared".into()
                    };
                }
            }
            KeyCode::Char('b') => self.open_budget_input(),
            // Game log: LogEntry carries `sbf` (grouping + live formation state) alongside the
            // element pool, so an SBF snapshot re-renders its formation sheet on export.
            KeyCode::Char('L') => self.log_snapshot(),
            KeyCode::Char('?') => self.modal = Some(Modal::Help),
            _ => {}
        }
    }

    /// The active formation/unit indices, validated against the current grouping.
    fn sbf_active_unit(&self) -> Option<(usize, usize)> {
        let fi = self.session.sbf.active_formation;
        let ui = self.session.sbf.active_unit;
        self.session
            .sbf
            .formations
            .get(fi)
            .and_then(|f| f.units.get(ui))
            .map(|_| (fi, ui))
    }

    fn sbf_cycle_formation(&mut self, delta: i32) {
        let n = self.session.sbf.formations.len();
        if n == 0 {
            return;
        }
        let cur = self.session.sbf.active_formation as i32;
        let next = (cur + delta).rem_euclid(n as i32) as usize;
        self.session.sbf.active_formation = next;
        let units = self.session.sbf.formations[next].units.len();
        self.session.sbf.active_unit = self.session.sbf.active_unit.min(units.saturating_sub(1));
    }

    fn sbf_cycle_unit(&mut self, delta: i32) {
        let Some((fi, _)) = self.sbf_active_unit() else {
            return;
        };
        let n = self.session.sbf.formations[fi].units.len();
        let cur = self.session.sbf.active_unit as i32;
        self.session.sbf.active_unit = (cur + delta).rem_euclid(n as i32) as usize;
    }

    /// Mark one point of SBF damage on the active unit. Overflow past its armor is reported for
    /// the player to spill onto another unit (§4.2 — damage carries over, never discarded).
    fn sbf_damage(&mut self) {
        let Some((fi, ui)) = self.sbf_active_unit() else {
            return;
        };
        if self.session.sbf.formations[fi].units[ui]
            .elements
            .is_empty()
        {
            self.status = "Empty unit — [g] assigns elements".into();
            return;
        }
        let derived = self
            .session
            .sbf_unit(&self.session.sbf.formations[fi].units[ui]);
        let overflow = self.session.sbf.formations[fi].units[ui].apply_damage(&derived, 1);
        let u = &self.session.sbf.formations[fi].units[ui];
        self.dirty = true;
        self.status = if overflow > 0 {
            "Destroyed — spill remaining damage onto another unit (j/k to select)".into()
        } else if u.is_destroyed(&derived) {
            if self
                .session
                .sbf_formation_eliminated(&self.session.sbf.formations[fi])
            {
                format!("{} destroyed — formation eliminated", derived.name)
            } else {
                format!(
                    "{} destroyed — spillover goes to another unit",
                    derived.name
                )
            }
        } else {
            let due = if u.crit_check_due(&derived) {
                "  ⚠ crit check (2d6, c)"
            } else {
                ""
            };
            format!(
                "Armor {}/{}{}",
                u.armor_remaining(&derived),
                derived.armor,
                due
            )
        };
    }

    fn sbf_repair(&mut self) {
        let Some((fi, ui)) = self.sbf_active_unit() else {
            return;
        };
        if self.session.sbf.formations[fi].units[ui]
            .elements
            .is_empty()
        {
            return;
        }
        let derived = self
            .session
            .sbf_unit(&self.session.sbf.formations[fi].units[ui]);
        let u = &mut self.session.sbf.formations[fi].units[ui];
        u.repair(1);
        self.dirty = true;
        self.status = format!("Armor {}/{}", u.armor_remaining(&derived), derived.armor);
    }

    /// Cycle the formation's morale rung (manual morale, §4.3 — a label, no roll):
    /// Normal → Shaken → Broken → Routed → Normal.
    fn sbf_cycle_morale(&mut self) {
        use neurohelmet_core::session::MoraleStatus;
        let fi = self.session.sbf.active_formation;
        if let Some(f) = self.session.sbf.formations.get_mut(fi) {
            f.morale = if f.morale == MoraleStatus::Routed {
                MoraleStatus::Normal
            } else {
                f.morale.worsened()
            };
            self.dirty = true;
            self.status = format!("{}: morale {}", f.name, f.morale.label());
        }
    }

    /// Drive the grouping editor (the manual-first flow): ↑↓ pick a pool element, ←→ move it
    /// between existing units, `n` split it into a new unit of its formation, `f` start a new
    /// formation with it, `u` unassign, `a` open the doctrine auto-group. Closing prunes any
    /// units/formations the edit emptied.
    fn sbf_group_modal_key(&mut self, sel: usize, key: KeyEvent) {
        use neurohelmet_core::session::SbfAssign;
        let n = self.session.mechs.len();
        if n == 0 {
            return; // modal already taken → closes
        }
        let assign = |app: &mut App, target: SbfAssign| {
            let before = app.session.clone();
            app.session.sbf_assign_element(sel, target);
            // No pruning mid-edit (decided 2026-07-04): a unit/formation you just emptied stays
            // available as a move target — vanishing surprised at the table. The panes render
            // empties as "(empty)"/"(no units)", never as destroyed. Units prune on close.
            if app.session != before {
                app.push_undo(before);
                app.dirty = true;
            }
        };
        match key.code {
            KeyCode::Esc | KeyCode::Enter | KeyCode::Char('g') => {
                // Close: element-less units come off the sheet; empty FORMATIONS stay (they're
                // first-class workspaces, deleted only via D).
                let before = self.session.clone();
                self.session.sbf_prune_empty_units();
                if self.session != before {
                    self.push_undo(before);
                    self.dirty = true;
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.modal = Some(Modal::SbfGroup {
                    sel: sel.saturating_sub(1),
                });
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.modal = Some(Modal::SbfGroup {
                    sel: (sel + 1).min(n - 1),
                });
            }
            KeyCode::Left | KeyCode::Right | KeyCode::Char(' ') => {
                // Cycle the element through every grouping stop, in formation/unit order.
                // A formation with no units contributes one virtual "new unit here" stop, so an
                // empty formation (the seeded starter, or one you just vacated) is reachable.
                let stops: Vec<SbfAssign> = self
                    .session
                    .sbf
                    .formations
                    .iter()
                    .enumerate()
                    .flat_map(|(fi, f)| {
                        if f.units.is_empty() {
                            vec![SbfAssign::NewUnit(fi)]
                        } else {
                            (0..f.units.len())
                                .map(|ui| SbfAssign::Unit(fi, ui))
                                .collect()
                        }
                    })
                    .collect();
                if stops.is_empty() {
                    self.status = "No formations yet — [f] starts one".into();
                } else {
                    let step: i32 = if key.code == KeyCode::Left { -1 } else { 1 };
                    let cur = self.session.sbf_element_assignment(sel);
                    let cur_pos = cur.and_then(|(fi, ui)| {
                        stops.iter().position(|&s| s == SbfAssign::Unit(fi, ui))
                    });
                    let target = match cur_pos {
                        Some(pos) => {
                            stops[(pos as i32 + step).rem_euclid(stops.len() as i32) as usize]
                        }
                        // Unassigned: → enters the first stop, ← the last.
                        None if step > 0 => stops[0],
                        None => stops[stops.len() - 1],
                    };
                    // Wrapping onto the element's own unit (it is the only stop) would silently
                    // rotate its element order and spam undo steps — skip instead.
                    if cur.is_some_and(|(fi, ui)| target == SbfAssign::Unit(fi, ui)) {
                        self.status = "Only one unit — [n] splits, [f] starts a formation".into();
                    } else {
                        assign(self, target);
                    }
                }
                self.modal = Some(Modal::SbfGroup { sel });
            }
            KeyCode::Char('n') => {
                // Split: new unit in the element's own formation (or the active one; a fresh
                // formation when none exists).
                let target = match self.session.sbf_element_assignment(sel) {
                    Some((fi, _)) => SbfAssign::NewUnit(fi),
                    None if !self.session.sbf.formations.is_empty() => {
                        SbfAssign::NewUnit(self.session.sbf.active_formation)
                    }
                    None => SbfAssign::NewFormation,
                };
                assign(self, target);
                self.modal = Some(Modal::SbfGroup { sel });
            }
            KeyCode::Char('f') => {
                assign(self, SbfAssign::NewFormation);
                self.modal = Some(Modal::SbfGroup { sel });
            }
            KeyCode::Char('u') => {
                assign(self, SbfAssign::Unassign);
                self.modal = Some(Modal::SbfGroup { sel });
            }
            KeyCode::Char('s') | KeyCode::Char('S') => {
                // Adjust the element's Skill (gunnery drives unit/formation skill and PV).
                let delta: i32 = if key.code == KeyCode::Char('s') {
                    1
                } else {
                    -1
                };
                let before = self.session.clone();
                if let Some(tm) = self.session.mechs.get_mut(sel) {
                    tm.gunnery = (tm.gunnery as i32 + delta).clamp(0, SKILL_MAX as i32) as u8;
                }
                if self.session != before {
                    self.push_undo(before);
                    self.dirty = true;
                }
                self.modal = Some(Modal::SbfGroup { sel });
            }
            KeyCode::Char('x') => {
                // Remove the element from the force entirely (indices remap; emptied groups go).
                let name = self.session.mechs.get(sel).map(|m| m.spec.display_name());
                let before = self.session.clone();
                self.session.remove_mech(sel);
                if self.session != before {
                    self.push_undo(before);
                    self.dirty = true;
                    if let Some(n) = name {
                        self.status = format!("Removed {n} from the force");
                    }
                }
                let n = self.session.mechs.len();
                if n > 0 {
                    self.modal = Some(Modal::SbfGroup {
                        sel: sel.min(n - 1),
                    });
                } // pool empty → editor closes (modal already taken)
            }
            KeyCode::Char('a') => self.modal = Some(Modal::SbfDoctrine { sel: 0 }),
            _ => self.modal = Some(Modal::SbfGroup { sel }),
        }
    }

    /// Drive the doctrine picker: Enter rebuilds all formations under the chosen IO:BF p.165
    /// scheme (the opt-in auto-group). Esc returns to the grouping editor.
    fn sbf_doctrine_modal_key(&mut self, sel: usize, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                let el = self
                    .session
                    .active
                    .min(self.session.mechs.len().saturating_sub(1));
                self.modal = Some(Modal::SbfGroup { sel: el });
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.modal = Some(Modal::SbfDoctrine {
                    sel: sel.saturating_sub(1),
                });
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.modal = Some(Modal::SbfDoctrine {
                    sel: (sel + 1).min(2),
                });
            }
            KeyCode::Enter | KeyCode::Char(' ') => {
                let doctrine = match sel {
                    0 => SbfDoctrine::InnerSphere,
                    1 => SbfDoctrine::Clan,
                    _ => SbfDoctrine::ComStar,
                };
                // Group-first stays frictionless: a pristine grouping applies immediately.
                // Anything hand-entered gets an itemized bill first (builds the habit).
                let losses = self.sbf_doctrine_losses();
                if losses.is_empty() {
                    self.sbf_apply_doctrine(doctrine);
                } else {
                    self.modal = Some(Modal::Confirm {
                        prompt: format!(
                            "Rebuild all formations?\nDiscards {} — z undoes.",
                            losses.join(", ")
                        ),
                        action: PendingAction::ApplyDoctrine(doctrine),
                    });
                }
            }
            _ => self.modal = Some(Modal::SbfDoctrine { sel }),
        }
    }

    /// What a doctrine rebuild would discard, itemized for the confirmation prompt. Empty =
    /// nothing hand-entered is at stake (fresh grouping) and no confirmation is needed.
    fn sbf_doctrine_losses(&self) -> Vec<String> {
        let fs = &self.session.sbf.formations;
        let mut out = Vec::new();
        let custom_names = fs
            .iter()
            .flat_map(|f| {
                std::iter::once(f.name.as_str()).chain(f.units.iter().map(|u| u.name.as_str()))
            })
            .filter(|n| !sbf_default_name(n))
            .count();
        if custom_names > 0 {
            out.push(format!("{custom_names} custom name(s)"));
        }
        let armor: u32 = fs
            .iter()
            .flat_map(|f| f.units.iter())
            .map(|u| u.armor_hits as u32)
            .sum();
        if armor > 0 {
            out.push(format!("{armor} armor hit(s)"));
        }
        let crits: u32 = fs
            .iter()
            .flat_map(|f| f.units.iter())
            .map(|u| u.damage_crits as u32 + u.targeting_crits as u32 + u.mp_crits as u32)
            .sum();
        if crits > 0 {
            out.push(format!("{crits} crit(s)"));
        }
        let morale = fs
            .iter()
            .filter(|f| f.morale != neurohelmet_core::session::MoraleStatus::Normal)
            .count();
        if morale > 0 {
            out.push(format!("{morale} morale rung(s)"));
        }
        if fs
            .iter()
            .flat_map(|f| f.units.iter())
            .any(|u| u.is_commander)
        {
            out.push("the COM mark".into());
        }
        let leads = fs
            .iter()
            .flat_map(|f| f.units.iter())
            .filter(|u| u.is_leader)
            .count();
        if leads > 0 {
            out.push(format!("{leads} LEAD mark(s)"));
        }
        out
    }

    /// Rebuild all formations under a doctrine (one undo step; losses were confirmed or nil).
    fn sbf_apply_doctrine(&mut self, doctrine: SbfDoctrine) {
        let before = self.session.clone();
        self.session.sbf_group_doctrine(doctrine);
        if self.session != before {
            self.push_undo(before);
            self.dirty = true;
        }
        let count = self.session.sbf.formations.len();
        self.status = format!("Auto-grouped into {count} formation(s)");
    }

    /// The hand-entered to-hit context for the active unit (spec §4.1, printed p.172 table; the
    /// optional SAS leg is the p.179 table): formation skill plus the firing unit's targeting
    /// crits plus the App-held shot legs; BFC/DRO come from the derived formation specials. Used
    /// by the shot modal and the detail pane.
    pub fn sbf_to_hit_ctx(&self) -> Option<sbf::SbfToHitCtx> {
        let (fi, ui) = self.sbf_active_unit()?;
        let f = &self.session.sbf.formations[fi];
        let derived = self.session.sbf_formation(f);
        // BFC aggregates at UNIT level (if half the elements carry it); DRO never aggregates
        // (golden-locked converter bug) so it derives from the elements. RAW both stack with the
        // Step-1G conversion skill penalty already baked into the base (p.259 vs p.172 — spec §4.1).
        let unit = self.session.sbf_unit(&f.units[ui]);
        let s = self.sbf_shot;
        let aero = s.aero_kind.to_engine(s.cluster).map(|kind| {
            // The SV fire-control ladder (p.179 Misc): AFC/BFC read off the unit SUAs like the
            // ground bfc row; the "neither +2" bites only all-SV compositions — the row prices
            // support vehicles, not every fire-control-less formation.
            let sv_fire_control = if unit.suas.contains_key("AFC") {
                sbf::SbfSvFireControl::Afc
            } else if unit.suas.contains_key("BFC") {
                sbf::SbfSvFireControl::Bfc
            } else if self.session.sbf_unit_is_sv(&f.units[ui]) {
                sbf::SbfSvFireControl::None
            } else {
                sbf::SbfSvFireControl::Afc // +0 — not a support vehicle, the row does not apply
            };
            // A large craft (arc card) firing an aero attack uses the p.191 capital-scale table.
            let capital = unit.arcs.is_some().then_some(sbf::SbfCapital {
                weapon_class: s.weapon_class,
                target_is_large_craft: s.target_large_craft,
                high_speed: s.high_speed,
                atmospheric: s.atmospheric,
                point_defense: s.point_defense,
                screen: s.screen,
                naval_c3: s.naval_c3,
                teleoperated: s.teleoperated,
                crippled: s.crippled,
                grappled: s.grappled,
                acm: s.acm,
            });
            sbf::SbfAeroShot {
                kind,
                target: s.aero_target,
                // Gates the +2 airborne target row (p.179 fn) — the NARROW formation aerospace
                // test; grounded/landed nuance is out of scope (no landing/liftoff in std SAS).
                attacker_airborne_aero: derived.is_aerospace(),
                behind_target: s.behind_target,
                grounded_dropship: false,
                sv_fire_control,
                capital,
            }
        });
        Some(sbf::SbfToHitCtx {
            attacker_skill: derived.skill,
            firing_unit_targeting_crits: f.units[ui].targeting_crits,
            range: s.range,
            indirect_fire: s.indirect,
            attacker_jump: f.jump_used_this_turn,
            withheld_units: s.withheld,
            bfc: unit.suas.contains_key("BFC"),
            drone: self.session.sbf_unit_is_drone(&f.units[ui]),
            spotting: s.spotting,
            secondary: s.secondary,
            target_tmm: s.target_tmm,
            target_jump: s.target_jump,
            target_evaded: s.target_evaded,
            terrain: s.terrain,
            aero,
        })
    }

    /// Drive the SBF crit-counter popup: rows 0–2 = Damage / Targeting / MP crit counters on the
    /// active unit (←→ adjust; the player rolls the 2d6 and reads the §4.2 table shown below).
    fn sbf_crit_modal_key(&mut self, sel: usize, key: KeyEvent) {
        let adjust = |app: &mut App, delta: i32| {
            let Some((fi, ui)) = app.sbf_active_unit() else {
                return;
            };
            let before = app.session.clone();
            let u = &mut app.session.sbf.formations[fi].units[ui];
            let bump = |c: &mut u8| {
                *c = if delta > 0 {
                    c.saturating_add(1)
                } else {
                    c.saturating_sub(1)
                };
            };
            match sel {
                0 => bump(&mut u.damage_crits),
                1 => bump(&mut u.targeting_crits),
                _ => bump(&mut u.mp_crits),
            }
            if app.session != before {
                app.push_undo(before);
                app.dirty = true;
            }
        };
        match key.code {
            KeyCode::Esc | KeyCode::Char('c') | KeyCode::Enter => {} // close (modal taken)
            KeyCode::Up | KeyCode::Char('k') => {
                self.modal = Some(Modal::SbfCrit {
                    sel: sel.saturating_sub(1),
                });
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.modal = Some(Modal::SbfCrit {
                    sel: (sel + 1).min(2),
                });
            }
            KeyCode::Right | KeyCode::Char(' ') => {
                adjust(self, 1);
                self.modal = Some(Modal::SbfCrit { sel });
            }
            KeyCode::Left | KeyCode::Backspace => {
                adjust(self, -1);
                self.modal = Some(Modal::SbfCrit { sel });
            }
            _ => self.modal = Some(Modal::SbfCrit { sel }),
        }
    }

    /// Drive the SBF to-hit editor (the printed p.172 table): row 0 = range bracket, row 1 =
    /// indirect fire, row 2 = formation JUMP points used (persisted on the formation; +1 each),
    /// row 3 = units withholding fire (−1 each, floor −2), rows 4–5 = spotting / secondary,
    /// rows 6–9 = hand-entered target TMM / jump points / evaded / terrain, rows 10–13 = the
    /// Strategic Aerospace leg (p.179): attack kind / target type / behind-target / cluster.
    /// Whether the active SBF firing unit is a large craft (carries an arc card) — gates the
    /// capital-scale rows in the shot modal.
    pub(crate) fn sbf_firing_unit_is_large_craft(&self) -> bool {
        self.sbf_active_unit().is_some_and(|(fi, ui)| {
            self.session
                .sbf
                .formations
                .get(fi)
                .and_then(|f| f.units.get(ui))
                .is_some_and(|u| self.session.sbf_unit(u).arcs.is_some())
        })
    }

    /// SBF shot-editor rows: 14 base p.172/p.179 rows, plus the 12 capital-scale (p.191) rows a
    /// large craft adds (firing arc + weapon class + ten modifier toggles).
    const SBF_SHOT_BASE_ROWS: usize = 14;
    fn sbf_shot_row_count(&self) -> usize {
        if self.sbf_firing_unit_is_large_craft() {
            Self::SBF_SHOT_BASE_ROWS + 12
        } else {
            Self::SBF_SHOT_BASE_ROWS
        }
    }

    fn sbf_shot_modal_key(&mut self, sel: usize, key: KeyEvent) {
        let adjust = |app: &mut App, delta: i32| match sel {
            0 => {
                app.sbf_shot.range = match (app.sbf_shot.range, delta > 0) {
                    (SbfRange::Short, true) => SbfRange::Medium,
                    (SbfRange::Medium, true) => SbfRange::Long,
                    (SbfRange::Long, true) | (SbfRange::Extreme, true) => SbfRange::Extreme,
                    (SbfRange::Extreme, false) => SbfRange::Long,
                    (SbfRange::Long, false) => SbfRange::Medium,
                    (SbfRange::Medium, false) | (SbfRange::Short, false) => SbfRange::Short,
                };
            }
            1 => app.sbf_shot.indirect = !app.sbf_shot.indirect,
            2 => {
                // JUMP points used this turn — persisted formation state; session mutation → undo.
                let fi = app.session.sbf.active_formation;
                let before = app.session.clone();
                if let Some(f) = app.session.sbf.formations.get_mut(fi) {
                    f.jump_used_this_turn =
                        (f.jump_used_this_turn as i32 + delta).clamp(0, 9) as u8;
                }
                if app.session != before {
                    app.push_undo(before);
                    app.dirty = true;
                }
            }
            3 => {
                let cap = self_units_cap(app);
                app.sbf_shot.withheld = (app.sbf_shot.withheld as i32 + delta).clamp(0, cap) as u8;
            }
            4 => app.sbf_shot.spotting = !app.sbf_shot.spotting,
            5 => app.sbf_shot.secondary = !app.sbf_shot.secondary,
            6 => app.sbf_shot.target_tmm = (app.sbf_shot.target_tmm + delta as i64).clamp(0, 6),
            7 => {
                app.sbf_shot.target_jump =
                    (app.sbf_shot.target_jump as i32 + delta).clamp(0, 9) as u8;
            }
            8 => app.sbf_shot.target_evaded = !app.sbf_shot.target_evaded,
            9 => app.sbf_shot.terrain = (app.sbf_shot.terrain + delta as i64).clamp(0, 4),
            10 => app.sbf_shot.aero_kind = app.sbf_shot.aero_kind.cycled(delta > 0),
            11 => {
                use SbfAeroTarget::*;
                const ORDER: [SbfAeroTarget; 6] = [
                    AirborneAero,
                    AirborneDropship,
                    AirborneVtolWige,
                    SmallCraft,
                    GroundedSquadron,
                    GroundFormation,
                ];
                let i = ORDER
                    .iter()
                    .position(|&t| t == app.sbf_shot.aero_target)
                    .unwrap_or(0);
                let j = if delta > 0 {
                    (i + 1).min(ORDER.len() - 1)
                } else {
                    i.saturating_sub(1)
                };
                app.sbf_shot.aero_target = ORDER[j];
            }
            12 => app.sbf_shot.behind_target = !app.sbf_shot.behind_target,
            13 => app.sbf_shot.cluster = !app.sbf_shot.cluster,
            // ---- Large-Aerospace capital-scale rows (IO:BF p.191), only shown for a large craft.
            14 => {
                let all = large_craft::Arc::ALL;
                let i = all
                    .iter()
                    .position(|&a| a == app.sbf_shot.firing_arc)
                    .unwrap_or(0) as i32;
                app.sbf_shot.firing_arc = all[(i + delta).rem_euclid(all.len() as i32) as usize];
            }
            15 => {
                let all = large_craft::WeaponClass::ALL;
                let i = all
                    .iter()
                    .position(|&c| c == app.sbf_shot.weapon_class)
                    .unwrap_or(0) as i32;
                app.sbf_shot.weapon_class = all[(i + delta).rem_euclid(all.len() as i32) as usize];
            }
            16 => app.sbf_shot.target_large_craft = !app.sbf_shot.target_large_craft,
            17 => app.sbf_shot.high_speed = !app.sbf_shot.high_speed,
            18 => app.sbf_shot.atmospheric = !app.sbf_shot.atmospheric,
            19 => {
                app.sbf_shot.point_defense =
                    (app.sbf_shot.point_defense as i32 + delta).clamp(0, 2) as u8;
            }
            20 => app.sbf_shot.screen = (app.sbf_shot.screen as i32 + delta).clamp(0, 4) as u8,
            21 => app.sbf_shot.naval_c3 = !app.sbf_shot.naval_c3,
            22 => app.sbf_shot.teleoperated = !app.sbf_shot.teleoperated,
            23 => app.sbf_shot.crippled = !app.sbf_shot.crippled,
            24 => app.sbf_shot.grappled = !app.sbf_shot.grappled,
            _ => {
                use sbf::SbfAcm::*;
                app.sbf_shot.acm = match (app.sbf_shot.acm, delta > 0) {
                    (Off, true) => SameSector,
                    (SameSector, true) | (AdjacentSector, true) => AdjacentSector,
                    (AdjacentSector, false) => SameSector,
                    (SameSector, false) | (Off, false) => Off,
                };
            }
        };
        /// Withholding is bounded by the formation's unit count (you can't withhold more units
        /// than you have).
        fn self_units_cap(app: &App) -> i32 {
            app.session
                .sbf
                .formations
                .get(app.session.sbf.active_formation)
                .map_or(0, |f| f.units.len() as i32)
        }
        match key.code {
            KeyCode::Esc | KeyCode::Char('t') | KeyCode::Enter => {} // close (modal taken)
            KeyCode::Up | KeyCode::Char('k') => {
                self.modal = Some(Modal::SbfShot {
                    sel: sel.saturating_sub(1),
                });
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let last = self.sbf_shot_row_count().saturating_sub(1);
                self.modal = Some(Modal::SbfShot {
                    sel: (sel + 1).min(last),
                });
            }
            KeyCode::Right | KeyCode::Char(' ') => {
                adjust(self, 1);
                self.modal = Some(Modal::SbfShot { sel });
            }
            KeyCode::Left => {
                adjust(self, -1);
                self.modal = Some(Modal::SbfShot { sel });
            }
            _ => self.modal = Some(Modal::SbfShot { sel }),
        }
    }

    // ================= Abstract Combat System (Screen::Acs — spec Phase 4) =================

    /// ACS screen keys. Navigation mirrors SBF (Formation ,/. · Combat Unit j/k), then the live
    /// verbs: Space damage (typed amount), `u` repair, `m`/`M` cycle Combat-Unit/Formation morale,
    /// `f` accrue fatigue / `F` rest, `C`/`l` COM/LEAD, `n` new round, `e` toggle done, `g` group
    /// the pool, `r` rename, `D` delete Formation. The detail-pane readout cycles with `[`/`]`
    /// (range) and `+`/`-` (target TMM).
    fn acs_key(&mut self, key: KeyEvent) {
        use neurohelmet_core::engine::acs::AcsRange;
        match key.code {
            KeyCode::Char('q') => self.confirm_quit(),
            KeyCode::Char('a') => {
                self.screen = Screen::Picker;
                self.picker.reset();
                self.picker
                    .refilter(&self.names, &self.bundle, &self.filters);
            }
            KeyCode::Char('S') => self.open_sessions(),
            KeyCode::Char('?') => self.modal = Some(Modal::Help),
            KeyCode::Left | KeyCode::Char('h') | KeyCode::Char(',') => self.acs_cycle_formation(-1),
            KeyCode::Right | KeyCode::Char('.') => self.acs_cycle_formation(1),
            KeyCode::Up | KeyCode::Char('k') => self.acs_cycle_unit(-1),
            KeyCode::Down | KeyCode::Char('j') => self.acs_cycle_unit(1),
            KeyCode::Char(' ') | KeyCode::Enter => self.acs_open_damage_input(),
            KeyCode::Char('u') => self.acs_repair(),
            KeyCode::Char('m') => self.acs_cycle_unit_morale(),
            KeyCode::Char('M') => self.acs_cycle_formation_morale(),
            KeyCode::Char('f') => self.acs_accrue_fatigue(),
            KeyCode::Char('F') => self.acs_rest_fatigue(),
            KeyCode::Char('P') => self.export_pdf(),
            KeyCode::Char('C') => {
                if let Some((fi, ui)) = self.acs_active_unit() {
                    self.session.acs_set_commander(fi, ui);
                    self.dirty = true;
                    self.status = "Force Commander (COM) set".into();
                }
            }
            KeyCode::Char('l') => {
                if let Some((fi, ui)) = self.acs_active_unit() {
                    self.session.acs_set_leader(fi, ui);
                    self.dirty = true;
                    self.status = "Formation Leader (LEAD) set".into();
                }
            }
            KeyCode::Char('n') => {
                self.session.acs.begin_round();
                self.dirty = true;
                self.status = format!("Round {}", self.session.acs.round);
            }
            KeyCode::Char('e') => {
                let fi = self.session.acs.active_formation;
                if let Some(f) = self.session.acs.formations.get_mut(fi) {
                    f.is_done = !f.is_done;
                    self.dirty = true;
                }
            }
            KeyCode::Char('g') => {
                if self.session.mechs.is_empty() {
                    self.status = "Pool is empty — [a] to add elements".into();
                } else {
                    self.modal = Some(Modal::AcsGroup { sel: 0 });
                }
            }
            KeyCode::Char('r') => {
                let fi = self.session.acs.active_formation;
                if let Some(f) = self.session.acs.formations.get(fi) {
                    self.modal = Some(Modal::Input {
                        prompt: format!("Rename '{}' to:", f.name),
                        buffer: f.name.clone(),
                        action: PendingAction::RenameAcsFormation,
                    });
                }
            }
            KeyCode::Char('D') => {
                let fi = self.session.acs.active_formation;
                if self.session.acs.formations.get(fi).is_some() {
                    self.session.acs_remove_formation(fi);
                    self.dirty = true;
                    self.status = "Formation deleted".into();
                }
            }
            KeyCode::Char('[') | KeyCode::Char(']') => {
                let fwd = key.code == KeyCode::Char(']');
                if self.acs_active_formation_is_aero() {
                    // Aero range ladder has an Extreme bracket (SbfRange).
                    use neurohelmet_core::engine::sbf::SbfRange::*;
                    const ORDER: [neurohelmet_core::engine::sbf::SbfRange; 4] =
                        [Short, Medium, Long, Extreme];
                    let i = ORDER
                        .iter()
                        .position(|&r| r == self.acs_shot.aero_range)
                        .unwrap_or(1) as i32;
                    let n = ORDER.len() as i32;
                    self.acs_shot.aero_range =
                        ORDER[(i + if fwd { 1 } else { -1 }).rem_euclid(n) as usize];
                } else {
                    self.acs_shot.range = match (self.acs_shot.range, fwd) {
                        (AcsRange::Short, true) => AcsRange::Medium,
                        (AcsRange::Medium, true) => AcsRange::Long,
                        (AcsRange::Long, true) => AcsRange::Short,
                        (AcsRange::Short, false) => AcsRange::Long,
                        (AcsRange::Medium, false) => AcsRange::Short,
                        (AcsRange::Long, false) => AcsRange::Medium,
                    };
                }
            }
            KeyCode::Char('+') | KeyCode::Char('=') => {
                self.acs_shot.target_tmm = (self.acs_shot.target_tmm + 1).min(9);
            }
            KeyCode::Char('-') | KeyCode::Char('_') => {
                self.acs_shot.target_tmm = (self.acs_shot.target_tmm - 1).max(-4);
            }
            KeyCode::Char('s') => self.acs_shot.secondary = !self.acs_shot.secondary,
            // Aero-only cycles: weapon class, cross-type matchup, Ground-Support mission.
            KeyCode::Char('w') if self.acs_active_formation_is_aero() => {
                let all = large_craft::WeaponClass::ALL;
                let i = all
                    .iter()
                    .position(|&c| c == self.acs_shot.weapon_class)
                    .unwrap_or(0);
                self.acs_shot.weapon_class = all[(i + 1) % all.len()];
            }
            KeyCode::Char('v') if self.acs_active_formation_is_aero() => {
                let all = large_craft::Arc::ALL;
                let i = all
                    .iter()
                    .position(|&a| a == self.acs_shot.firing_arc)
                    .unwrap_or(0);
                self.acs_shot.firing_arc = all[(i + 1) % all.len()];
            }
            KeyCode::Char('L') if self.acs_active_formation_is_aero() => {
                self.acs_shot.target_large_craft = !self.acs_shot.target_large_craft;
            }
            KeyCode::Char('x') if self.acs_active_formation_is_aero() => {
                let all = neurohelmet_core::engine::acs::AcsAeroMatchup::ALL;
                let i = all
                    .iter()
                    .position(|&m| m == self.acs_shot.matchup)
                    .unwrap_or(0);
                self.acs_shot.matchup = all[(i + 1) % all.len()];
            }
            KeyCode::Char('y') if self.acs_active_formation_is_aero() => {
                let all = AcsAeroMission::ALL;
                let i = all
                    .iter()
                    .position(|&m| m == self.acs_shot.aero_mission)
                    .unwrap_or(0);
                self.acs_shot.aero_mission = all[(i + 1) % all.len()];
                self.status = format!(
                    "Ground-Support mission: {}",
                    self.acs_shot.aero_mission.label()
                );
            }
            _ => {}
        }
    }

    /// Whether the active ACS Formation is an aerospace type — routes the shot readout/editor to the
    /// aero (p.250) path instead of the ground (p.248) one.
    pub(crate) fn acs_active_formation_is_aero(&self) -> bool {
        self.acs_active_unit().is_some_and(|(fi, _)| {
            self.session
                .acs
                .formations
                .get(fi)
                .is_some_and(|f| self.session.acs_formation_is_aerospace(f))
        })
    }

    /// The active `(formation, combat unit)` indices, if a real Combat Unit is selected.
    fn acs_active_unit(&self) -> Option<(usize, usize)> {
        let fi = self.session.acs.active_formation;
        let f = self.session.acs.formations.get(fi)?;
        let ui = self.session.acs.active_unit;
        f.units.get(ui).map(|_| (fi, ui))
    }

    fn acs_cycle_formation(&mut self, delta: i32) {
        let n = self.session.acs.formations.len();
        if n == 0 {
            return;
        }
        let cur = self.session.acs.active_formation as i32;
        self.session.acs.active_formation = (cur + delta).rem_euclid(n as i32) as usize;
        self.session.acs.active_unit = 0;
    }

    fn acs_cycle_unit(&mut self, delta: i32) {
        let fi = self.session.acs.active_formation;
        let Some(f) = self.session.acs.formations.get(fi) else {
            return;
        };
        let n = f.units.len();
        if n == 0 {
            return;
        }
        let cur = self.session.acs.active_unit as i32;
        self.session.acs.active_unit = (cur + delta).rem_euclid(n as i32) as usize;
    }

    /// Open a numeric input for the announced damage (ACS armor pools are large — the opponent
    /// states the total, the player types it in, unlike SBF's per-point Space).
    fn acs_open_damage_input(&mut self) {
        if self.acs_active_unit().is_none() {
            self.status = "No Combat Unit — [g] to group the pool".into();
            return;
        }
        self.modal = Some(Modal::Input {
            prompt: "Damage to apply:".into(),
            buffer: String::new(),
            action: PendingAction::AcsDamage,
        });
    }

    /// Apply `dmg` to the active Combat Unit's armor pool; report thresholds crossed / destruction.
    pub fn acs_apply_damage(&mut self, dmg: i64) {
        let Some((fi, ui)) = self.acs_active_unit() else {
            return;
        };
        let derived = self
            .session
            .acs_combat_unit(&self.session.acs.formations[fi].units[ui]);
        let crossed = self.session.acs.formations[fi].units[ui].apply_damage(&derived, dmg);
        let st = &self.session.acs.formations[fi].units[ui];
        self.dirty = true;
        self.status = if st.is_destroyed(&derived) {
            format!("{} destroyed", derived.name)
        } else if crossed > 0 {
            format!(
                "Armor {}/{} — {crossed} threshold(s) crossed → morale check ([M])",
                st.armor_remaining(&derived),
                derived.armor
            )
        } else {
            format!("Armor {}/{}", st.armor_remaining(&derived), derived.armor)
        };
    }

    fn acs_repair(&mut self) {
        let Some((fi, ui)) = self.acs_active_unit() else {
            return;
        };
        let derived = self
            .session
            .acs_combat_unit(&self.session.acs.formations[fi].units[ui]);
        let st = &mut self.session.acs.formations[fi].units[ui];
        st.repair(1);
        self.dirty = true;
        self.status = format!("Armor {}/{}", st.armor_remaining(&derived), derived.armor);
    }

    fn acs_cycle_unit_morale(&mut self) {
        let Some((fi, ui)) = self.acs_active_unit() else {
            return;
        };
        let st = &mut self.session.acs.formations[fi].units[ui];
        st.morale = st.morale.cycled();
        self.dirty = true;
        self.status = format!("{}: morale {}", st.name, st.morale.label());
    }

    fn acs_cycle_formation_morale(&mut self) {
        let fi = self.session.acs.active_formation;
        if let Some(f) = self.session.acs.formations.get_mut(fi) {
            f.morale = f.morale.cycled();
            self.dirty = true;
            self.status = format!("{}: formation morale {}", f.name, f.morale.label());
        }
    }

    fn acs_accrue_fatigue(&mut self) {
        use neurohelmet_core::engine::acs::AcsExperience;
        let Some((fi, ui)) = self.acs_active_unit() else {
            return;
        };
        let derived = self
            .session
            .acs_combat_unit(&self.session.acs.formations[fi].units[ui]);
        let fp = AcsExperience::from_skill(derived.skill).fatigue_earned();
        let st = &mut self.session.acs.formations[fi].units[ui];
        st.add_fatigue(fp);
        self.dirty = true;
        self.status = format!(
            "{}: {:.1} FP (fought this turn)",
            st.name,
            st.fatigue_points()
        );
    }

    fn acs_rest_fatigue(&mut self) {
        let Some((fi, ui)) = self.acs_active_unit() else {
            return;
        };
        let st = &mut self.session.acs.formations[fi].units[ui];
        // A Combat Unit that did not move/attack/get-attacked recovers 1 FP (p.249).
        st.fatigue_points_x2 = st.fatigue_points_x2.saturating_sub(2);
        self.dirty = true;
        self.status = format!("{}: rested → {:.1} FP", st.name, st.fatigue_points());
    }

    /// Auto-group the WHOLE pool into one default-nested Formation, replacing any existing grouping
    /// (the opt-in `a` inside the editor). Manual-first: this is never applied implicitly.
    /// What a rebuild would throw away, itemized for the confirm prompt (the ACS analogue of
    /// [`Self::sbf_doctrine_losses`]). Empty means the grouping is pristine and auto-group can run
    /// without asking — grouping-first stays frictionless; hand-entered state gets a bill first.
    fn acs_auto_group_losses(&self) -> Vec<String> {
        use neurohelmet_core::engine::acs::AcsMorale;
        let fs = &self.session.acs.formations;
        let mut out = Vec::new();
        // Auto-group collapses everything into a single "Formation 1", so a multi-Formation
        // arrangement is itself hand-built structure worth naming.
        let populated = fs.iter().filter(|f| !f.units.is_empty()).count();
        if populated > 1 {
            out.push(format!("{populated} formation(s) of grouping"));
        }
        let custom_names = fs
            .iter()
            .flat_map(|f| {
                std::iter::once(f.name.as_str()).chain(f.units.iter().map(|u| u.name.as_str()))
            })
            .filter(|n| !acs_default_name(n))
            .count();
        if custom_names > 0 {
            out.push(format!("{custom_names} custom name(s)"));
        }
        let armor: u32 = fs
            .iter()
            .flat_map(|f| f.units.iter())
            .map(|u| u.armor_hits)
            .sum();
        if armor > 0 {
            out.push(format!("{armor} armor hit(s)"));
        }
        let fatigue: u16 = fs
            .iter()
            .flat_map(|f| f.units.iter())
            .map(|u| u.fatigue_points_x2)
            .sum();
        if fatigue > 0 {
            out.push("accrued fatigue".into());
        }
        let morale = fs.iter().filter(|f| f.morale != AcsMorale::Normal).count()
            + fs.iter()
                .flat_map(|f| f.units.iter())
                .filter(|u| u.morale != AcsMorale::Normal)
                .count();
        if morale > 0 {
            out.push(format!("{morale} morale rung(s)"));
        }
        if fs
            .iter()
            .flat_map(|f| f.units.iter())
            .any(|u| u.is_commander)
        {
            out.push("the COM mark".into());
        }
        let leads = fs
            .iter()
            .flat_map(|f| f.units.iter())
            .filter(|u| u.is_leader)
            .count();
        if leads > 0 {
            out.push(format!("{leads} LEAD mark(s)"));
        }
        out
    }

    /// Rebuild the whole ACS grouping from the pool (one undo step; losses were confirmed or nil).
    fn acs_auto_group(&mut self) {
        let n = self.session.mechs.len();
        if n == 0 {
            self.status = "Pool is empty — [a] to add elements".into();
            return;
        }
        let before = self.session.clone();
        self.session.acs.formations.clear();
        self.session.acs_new_formation("Formation 1", 0..n);
        self.session.acs.active_formation = 0;
        self.session.acs.active_unit = 0;
        if self.session != before {
            self.push_undo(before);
            self.dirty = true;
        }
        self.status = format!("Auto-grouped {n} element(s) — z undoes");
    }

    /// Drive the ACS grouping editor (four-tier analogue of `sbf_group_modal_key`): ↑↓ pick a pool
    /// element, ←→ cycle it through existing SBF Units, `n`/`t`/`c`/`F` split it into a new SBF Unit
    /// / Team / Combat Unit / Formation, `u` unassign, `a` auto-group the whole pool. Prunes on close.
    fn acs_group_modal_key(&mut self, sel: usize, key: KeyEvent) {
        use neurohelmet_core::session::AcsAssign;
        let n = self.session.mechs.len();
        if n == 0 {
            self.modal = None;
            return;
        }
        let assign = |app: &mut App, target: AcsAssign| {
            let before = app.session.clone();
            app.session.acs_assign_element(sel, target);
            if app.session != before {
                app.push_undo(before);
                app.dirty = true;
            }
            app.modal = Some(Modal::AcsGroup { sel });
        };
        // Resolve context for the "new tier" splits: the element's current path if grouped, else
        // the active Formation's first Combat Unit / Team (so an empty seeded Formation is a target).
        let cur = self.session.acs_element_assignment(sel);
        let afi = self.session.acs.active_formation;
        let ctx_team = cur.map(|(a, b, c, _)| (a, b, c)).or_else(|| {
            let f = self.session.acs.formations.get(afi)?;
            let cu = f.units.first()?;
            (!cu.teams.is_empty()).then_some((afi, 0, 0))
        });
        let ctx_cu = cur.map(|(a, b, _, _)| (a, b)).or_else(|| {
            let f = self.session.acs.formations.get(afi)?;
            (!f.units.is_empty()).then_some((afi, 0))
        });
        let ctx_form = cur
            .map(|(a, _, _, _)| a)
            .or_else(|| self.session.acs.formations.get(afi).map(|_| afi));
        match key.code {
            KeyCode::Esc | KeyCode::Enter | KeyCode::Char('g') => {
                let before = self.session.clone();
                self.session.acs_prune_empty();
                if self.session != before {
                    self.push_undo(before);
                    self.dirty = true;
                }
                self.modal = None;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.modal = Some(Modal::AcsGroup {
                    sel: sel.saturating_sub(1),
                });
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.modal = Some(Modal::AcsGroup {
                    sel: (sel + 1).min(n - 1),
                });
            }
            KeyCode::Left | KeyCode::Right | KeyCode::Char(' ') => {
                let stops = self.session.acs_unit_stops();
                if stops.is_empty() {
                    self.status = "No formations yet — [F] starts one".into();
                    self.modal = Some(Modal::AcsGroup { sel });
                    return;
                }
                let step: i32 = if key.code == KeyCode::Left { -1 } else { 1 };
                let cur_pos = cur.and_then(|(fi, cui, ti, ui)| {
                    stops
                        .iter()
                        .position(|&s| s == AcsAssign::Unit(fi, cui, ti, ui))
                });
                let next = match cur_pos {
                    Some(p) => (p as i32 + step).rem_euclid(stops.len() as i32) as usize,
                    None => {
                        if step < 0 {
                            stops.len() - 1
                        } else {
                            0
                        }
                    }
                };
                assign(self, stops[next]);
            }
            KeyCode::Char('n') => {
                let t = match ctx_team {
                    Some((fi, cui, ti)) => AcsAssign::NewUnit(fi, cui, ti),
                    None => match ctx_cu {
                        Some((fi, cui)) => AcsAssign::NewTeam(fi, cui),
                        None => ctx_form
                            .map(AcsAssign::NewCombatUnit)
                            .unwrap_or(AcsAssign::NewFormation),
                    },
                };
                assign(self, t);
            }
            KeyCode::Char('t') => {
                let t = match ctx_cu {
                    Some((fi, cui)) => AcsAssign::NewTeam(fi, cui),
                    None => ctx_form
                        .map(AcsAssign::NewCombatUnit)
                        .unwrap_or(AcsAssign::NewFormation),
                };
                assign(self, t);
            }
            KeyCode::Char('c') => {
                let t = ctx_form
                    .map(AcsAssign::NewCombatUnit)
                    .unwrap_or(AcsAssign::NewFormation);
                assign(self, t);
            }
            KeyCode::Char('F') => assign(self, AcsAssign::NewFormation),
            KeyCode::Char('u') => assign(self, AcsAssign::Unassign),
            KeyCode::Char('a') => {
                // Grouping-first stays frictionless: a pristine grouping rebuilds immediately.
                // Anything hand-entered gets an itemized bill first (mirrors the SBF doctrine flow).
                let losses = self.acs_auto_group_losses();
                if losses.is_empty() {
                    self.acs_auto_group();
                    self.modal = Some(Modal::AcsGroup { sel });
                } else {
                    self.modal = Some(Modal::Confirm {
                        prompt: format!(
                            "Rebuild the whole grouping?\nDiscards {} — z undoes.",
                            losses.join(", ")
                        ),
                        action: PendingAction::AcsAutoGroup,
                    });
                }
            }
            _ => self.modal = Some(Modal::AcsGroup { sel }),
        }
    }

    /// The active Combat Unit's to-hit context for the detail-pane readout (Phase 3 calculators):
    /// attacker-side terms derive from the live Combat Unit; range/target-TMM are the `acs_shot`.
    pub fn acs_to_hit_ctx(&self) -> Option<neurohelmet_core::engine::acs::AcsToHitCtx> {
        use neurohelmet_core::engine::acs::{acs_fatigue_band, AcsExperience, AcsToHitCtx};
        let (fi, ui) = self.acs_active_unit()?;
        let st = &self.session.acs.formations[fi].units[ui];
        let derived = self.session.acs_combat_unit(st);
        let exp = AcsExperience::from_skill(derived.skill);
        Some(AcsToHitCtx {
            range: self.acs_shot.range,
            attacker: exp,
            target_tmm: self.acs_shot.target_tmm,
            own_morale: st.morale,
            fatigue: acs_fatigue_band(st.fatigue_points(), exp),
            secondary_target: self.acs_shot.secondary,
            ..AcsToHitCtx::default()
        })
    }

    /// The ACS **aerospace** to-hit context (IO:BF p.250 + p.241) for the active aero Combat Unit —
    /// the aero range ladder + the cross-type matchup + the shared capital leg (built only for a
    /// large craft carrying an arc card). Mirror of [`Self::acs_to_hit_ctx`] for aero Formations.
    pub fn acs_aero_to_hit_ctx(&self) -> Option<neurohelmet_core::engine::acs::AcsAeroToHitCtx> {
        use neurohelmet_core::engine::acs::{
            acs_fatigue_band, AcsAeroMatchup, AcsAeroToHitCtx, AcsExperience,
        };
        use neurohelmet_core::engine::sbf::{SbfAcm, SbfCapital};
        let (fi, ui) = self.acs_active_unit()?;
        let st = &self.session.acs.formations[fi].units[ui];
        let derived = self.session.acs_combat_unit(st);
        let exp = AcsExperience::from_skill(derived.skill);
        let s = self.acs_shot;
        // The weapon-class penalty is waived vs ANY large-craft target (p.191 Notes): the four
        // large-craft `matchup` cases imply it, plus the explicit toggle for same-type / station
        // targets the 6-row matrix can't represent.
        let target_large = s.target_large_craft
            || matches!(
                s.matchup,
                AcsAeroMatchup::AeroVsWarship
                    | AcsAeroMatchup::AeroVsDropship
                    | AcsAeroMatchup::DropshipVsWarship
                    | AcsAeroMatchup::WarshipVsDropship
            );
        let capital = derived.arcs.is_some().then_some(SbfCapital {
            weapon_class: s.weapon_class,
            target_is_large_craft: target_large,
            high_speed: false,
            atmospheric: false,
            point_defense: 0,
            screen: 0,
            naval_c3: false,
            teleoperated: false,
            crippled: false,
            grappled: false,
            acm: SbfAcm::Off,
        });
        Some(AcsAeroToHitCtx {
            range: s.aero_range,
            attacker: exp,
            target_tmm: s.target_tmm,
            matchup: s.matchup,
            capital,
            own_morale: st.morale,
            fatigue: acs_fatigue_band(st.fatigue_points(), exp),
            secondary_target: s.secondary,
            ..AcsAeroToHitCtx::default()
        })
    }

    // ================= Standard BattleForce (Screen::BattleForce — spec Phase 3) =================

    /// Standard BF screen keys — AS mode's sibling, not SBF's: the AS verbs unchanged (Space/`u`
    /// damage-repair on the shared AS armor track, `o`/`i` heat, the card-grid navigation), plus
    /// the BF modals. One deliberate deviation from AS (spec §3.3): `g` is the grouping editor
    /// (SBF muscle memory) and the skills modal moves to `s`.
    fn bf_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('q') => self.confirm_quit(),
            KeyCode::Char('a') => {
                self.screen = Screen::Picker;
                self.picker.reset();
                self.picker
                    .refilter(&self.names, &self.bundle, &self.filters);
            }
            KeyCode::Char('S') => self.open_sessions(),
            KeyCode::Char('D') => {
                if let Some(tm) = self.session.active_mech() {
                    self.modal = Some(Modal::Confirm {
                        prompt: format!(
                            "Remove {} from this session? (y/n)",
                            tm.spec.display_name()
                        ),
                        action: PendingAction::DeleteActiveMech,
                    });
                }
            }
            KeyCode::Char('?') => self.modal = Some(Modal::Help),
            KeyCode::Char('P') => self.export_pdf(),
            // Skills on `s` (deviation note above); `b` sets the force PV limit.
            KeyCode::Char('s') => self.modal = Some(Modal::Skills { sel: 0 }),
            KeyCode::Char('b') => self.open_budget_input(),
            KeyCode::Char('[') | KeyCode::Char(',') => {
                self.session.switch(-1);
                self.clamp_selection();
            }
            KeyCode::Char(']') | KeyCode::Char('.') => {
                self.session.switch(1);
                self.clamp_selection();
            }
            // `<` / `>` jump a full page (4 cards, the grid width — the AS pattern).
            KeyCode::Char('<') => {
                self.session.switch(-4);
                self.clamp_selection();
            }
            KeyCode::Char('>') => {
                self.session.switch(4);
                self.clamp_selection();
            }
            KeyCode::Char(' ') | KeyCode::Enter => {
                if let Some(tm) = self.session.active_mech_mut() {
                    let struct_before = tm.as_struct_hits;
                    tm.as_damage();
                    // Any hit that damages structure owes a crit-chance roll (p.42) — unless
                    // the element has no crit column at all (infantry/BA never take crits).
                    let crit_due = tm.as_struct_hits > struct_before
                        && !tm.bf_destroyed()
                        && battleforce::bf_crit_col(&bf_element_of(tm)).is_some();
                    self.status = format!(
                        "Armor {} / Struct {}{}",
                        tm.as_armor_remaining(),
                        tm.as_struct_remaining(),
                        if crit_due {
                            "  ⚠ crit check (2D6, c)"
                        } else {
                            ""
                        }
                    );
                    self.dirty = true;
                }
            }
            KeyCode::Char('u') => {
                if let Some(tm) = self.session.active_mech_mut() {
                    tm.as_repair();
                    self.dirty = true;
                }
            }
            KeyCode::Char('o') => {
                if let Some(tm) = self.session.active_mech_mut() {
                    tm.as_adjust_heat(1);
                    self.dirty = true;
                }
            }
            KeyCode::Char('i') => {
                if let Some(tm) = self.session.active_mech_mut() {
                    tm.as_adjust_heat(-1);
                    self.dirty = true;
                }
            }
            KeyCode::Char('c') => {
                if let Some(tm) = self.session.active_mech() {
                    if battleforce::bf_crit_col(&bf_element_of(tm)).is_some() {
                        self.modal = Some(Modal::BfCrit { sel: 0 });
                    } else {
                        self.status = "Infantry and BA never take critical hits (p.42)".into();
                    }
                }
            }
            KeyCode::Char('t') => {
                if self.session.active_mech().is_some() {
                    self.modal = Some(Modal::BfShot { sel: 0 });
                }
            }
            KeyCode::Char('g') => {
                if self.session.mechs.is_empty() {
                    self.status = "No elements to group — [a] adds some".into();
                } else {
                    // Start on the element the grid highlights (usually the one just added).
                    let sel = self.session.active.min(self.session.mechs.len() - 1);
                    self.modal = Some(Modal::BfGroup { sel });
                }
            }
            KeyCode::Char('m') => self.bf_cycle_morale(),
            KeyCode::Char('n') => {
                self.session.bf_begin_round();
                self.dirty = true;
                self.status = format!(
                    "Round {} begun (crew-stunned cleared)",
                    self.session.bf.round
                );
            }
            KeyCode::Char('r') => {
                if let Some(u) = self
                    .bf_active_unit()
                    .and_then(|ui| self.session.bf.units.get(ui))
                {
                    self.modal = Some(Modal::Input {
                        prompt: format!("Rename unit '{}':", u.name),
                        buffer: u.name.clone(),
                        action: PendingAction::RenameBfUnit,
                    });
                }
            }
            KeyCode::Char('L') => self.log_snapshot(),
            _ => {}
        }
    }

    /// The Unit the BF verbs (`m` morale, `r` rename) act on: the one holding the active element,
    /// else the Unit cursor (so a fresh sheet's seeded empty Unit is still reachable).
    pub(crate) fn bf_active_unit(&self) -> Option<usize> {
        self.session
            .bf_element_assignment(self.session.active)
            .or_else(|| {
                if self.session.bf.units.is_empty() {
                    None
                } else {
                    Some(
                        self.session
                            .bf
                            .active_unit
                            .min(self.session.bf.units.len() - 1),
                    )
                }
            })
    }

    /// Cycle the active Unit's manual morale rung: Normal → Broken → Routed → Normal (spec §3.3).
    fn bf_cycle_morale(&mut self) {
        let Some(ui) = self.bf_active_unit() else {
            self.status = "No unit — [g] groups elements".into();
            return;
        };
        if let Some(u) = self.session.bf.units.get_mut(ui) {
            u.morale = u.morale.cycled();
            self.dirty = true;
            self.status = format!("{}: morale {}", u.name, u.morale.label());
        }
    }

    /// The composed [`BfShot`] for pool element `i`: the App-held hand-entered legs
    /// ([`BfShotUi`]) sanitized against the element — an attack kind the element can't declare
    /// (spec §1.3/§1.5) falls back to Standard, DFA against airborne aerospace falls back to
    /// Standard (p.45; the modal shows the warning), and the grounded-aero toggle only sticks
    /// to aerospace elements (the p.46 rows are aero-only).
    pub fn bf_shot_for(&self, i: usize) -> BfShot {
        let s = self.bf_shot;
        let (kind, grounded) = match self.session.mechs.get(i) {
            Some(tm) => {
                let el = bf_element_of(tm);
                let mut kind = if bf_kind_eligible(&el, s.kind) {
                    s.kind
                } else {
                    BfAttackKind::Standard
                };
                if kind == BfAttackKind::Physical(BfPhysical::Dfa)
                    && matches!(
                        s.target_kind,
                        BfTargetKind::AirborneAero(_) | BfTargetKind::AirborneDropship
                    )
                {
                    // DFA "may not target airborne aerospace" (p.45).
                    kind = BfAttackKind::Standard;
                }
                (kind, s.grounded && battleforce::bf_is_aero(&el))
            }
            None => (BfAttackKind::Standard, false),
        };
        BfShot {
            range: s.range,
            kind,
            attacker_move: s.attacker_move,
            grounded,
            area_effect: s.area_effect,
            secondary: s.secondary,
            also_spotting: s.also_spotting,
            target_tmm: s.target_tmm,
            target_move: s.target_move,
            target_move_adj: s.target_move_adj,
            target_immobile: s.target_immobile,
            target_kind: s.target_kind,
            target_woods: s.target_woods,
            target_partial_cover: s.target_partial_cover,
            target_underwater: s.target_underwater,
            target_stealth: s.target_stealth,
            target_mas: s.target_mas == 1,
            target_lmas: s.target_mas == 2,
            target_carrying_ba: s.target_carrying_ba,
        }
    }

    /// `(current ground/thrust MP, current jump MP, alive)` per member — the [`battleforce::bf_unit_mv`]
    /// legs (spec §1.7), recomputed each frame from the live element state.
    pub fn bf_member_stats(&self, elements: &[usize]) -> Vec<(u32, Option<u32>, bool)> {
        elements
            .iter()
            .filter_map(|&i| {
                let tm = self.session.mechs.get(i)?;
                let el = bf_element_of(tm);
                let ground = self.session.bf_current_mp(i);
                // Live jump MP: heat and MP crits are MP effects (pp.43/49) so they bite the jump
                // leg too; the motive flags, TSM and the vehicle/aero Engine-crit legs are
                // ground-drive/thrust effects and don't (jump-capable elements roll the 'Mech
                // column anyway).
                let jump = (el.jump_move > 0).then(|| {
                    battleforce::bf_current_mp(
                        inches_to_hexes(el.jump_move),
                        tm.as_heat,
                        tm.bf.mp_lost,
                        BfMotive::default(),
                        false,
                        0,
                        None,
                    )
                });
                Some((ground, jump, !tm.bf_destroyed()))
            })
            .collect()
    }

    /// Rows in the BF crit modal: the 11 crit-table rows (2D6 2..=12) for the element's column,
    /// plus the three once-per-game motive rungs for the Vehicle column (p.44) — except BD gun
    /// emplacements, which are immobile with a weapons-only crit vocabulary (spec
    /// §Data-fidelity 8): no motive rows.
    pub(crate) fn bf_crit_row_count(&self) -> usize {
        let Some(tm) = self.session.active_mech() else {
            return 0;
        };
        let el = bf_element_of(tm);
        match battleforce::bf_crit_col(&el) {
            None => 0,
            Some(battleforce::BfCritCol::Vehicle) if el.as_type != "BD" => 14,
            Some(_) => 11,
        }
    }

    /// Apply the crit-modal row `sel` to the active element (spec §1.4 effect semantics), one
    /// undo step. Returns `false` when the flow moved to another modal (the CASEP confirm) —
    /// the caller must not re-open the crit modal over it.
    fn bf_apply_crit_row(&mut self, sel: usize) -> bool {
        let i = self.session.active;
        let Some(tm) = self.session.active_mech() else {
            return true;
        };
        let el = bf_element_of(tm);
        let Some(col) = battleforce::bf_crit_col(&el) else {
            return true;
        };
        use battleforce::{BfCrit, BfCritCol};

        // Vehicle motive rows (11–13): independent once-per-game spent-flags (p.43) — marking
        // an already-marked effect is a no-op; different effects stack.
        if sel >= 11 {
            let effect = match sel {
                11 => BfMotive {
                    minus_one: true,
                    ..Default::default()
                },
                12 => BfMotive {
                    half: true,
                    ..Default::default()
                },
                _ => BfMotive {
                    immobile: true,
                    ..Default::default()
                },
            };
            let before = self.session.clone();
            if let Some(tm) = self.session.active_mech_mut() {
                let m = tm.bf.motive;
                let already = (effect.minus_one && m.minus_one)
                    || (effect.half && m.half)
                    || (effect.immobile && m.immobile);
                tm.bf_mark_motive(effect);
                self.status = if already {
                    "Motive effect already marked — each occurs once per game (p.43)".into()
                } else if effect.minus_one {
                    "Motive: −1 MV".into()
                } else if effect.half {
                    "Motive: ½ MV (round down)".into()
                } else {
                    "Motive: immobilized (to-hit −4)".into()
                };
            }
            if self.session != before {
                self.push_undo(before);
                self.dirty = true;
            }
            return true;
        }

        let roll = sel as i32 + 2;
        let crit = battleforce::bf_crit(roll, col);
        // BD gun emplacements have a weapons-only crit vocabulary (spec §Data-fidelity 8): any
        // other rolled effect "does not apply" and is +1 damage instead, no chained crit roll
        // (p.42).
        if el.as_type == "BD" && !matches!(crit, BfCrit::NoCrit | BfCrit::Weapon) {
            let before = self.session.clone();
            if let Some(tm) = self.session.active_mech_mut() {
                tm.as_damage();
                self.status =
                    "Doesn't apply to a gun emplacement — +1 damage instead (p.42)".into();
            }
            if self.session != before {
                self.push_undo(before);
                self.dirty = true;
            }
            return true;
        }
        // CASEP ammo (p.151) needs the player's 1D6 — hand off to a confirm prompt.
        if crit == BfCrit::Ammo && !el.has_any_sua(&["CASEII", "ENE"]) && el.has_sua("CASEP") {
            self.modal = Some(Modal::Confirm {
                prompt: "CASEP (p.151): roll 1D6 — on 3+ the ammo crit is ignored.\nDid it come up 1-2 (detonation)? (y/n)".into(),
                action: PendingAction::BfAmmoDetonate,
            });
            return false;
        }

        let before = self.session.clone();
        let mut mp_crit = false;
        if let Some(tm) = self.session.active_mech_mut() {
            self.status = match crit {
                BfCrit::NoCrit => "No critical hit".into(),
                BfCrit::Ammo => {
                    if el.has_any_sua(&["CASEII", "ENE"]) {
                        "Ammo crit ignored (CASEII/ENE)".into()
                    } else if el.has_sua("CASE") {
                        tm.as_damage();
                        "Ammo (CASE): 1 damage — crit-check that damage normally (p.42)".into()
                    } else {
                        tm.bf.killed = Some(BfKill::Ammo);
                        "Ammo explosion — element destroyed".into()
                    }
                }
                BfCrit::Engine => {
                    tm.bf.engine += 1;
                    let hits = tm.bf.engine;
                    match col {
                        BfCritCol::Mech => {
                            if hits >= battleforce::BF_ENGINE_HITS_DESTROY {
                                "Engine hit 2 — element destroyed".into()
                            } else if el.has_any_sua(&["EE", "FC"]) {
                                "Engine hit (non-fusion): 2D6 each End Phase it fired — 12 explodes (p.146)".into()
                            } else {
                                "Engine hit: +1 heat every turn it fires weapons".into()
                            }
                        }
                        // Vehicle/Aerospace Engine MV/TP effects derive live from the hit count
                        // in bf_current_mp / bf_current_damage (§1.4, as built 2026-07-05) —
                        // nothing is snapshotted into mp_lost here.
                        BfCritCol::Vehicle => {
                            if hits >= battleforce::BF_ENGINE_HITS_DESTROY {
                                "Engine hit 2 — element destroyed".into()
                            } else {
                                "Engine: MV × 0.5 and all damage × 0.5 (round down)".into()
                            }
                        }
                        BfCritCol::Aerospace => {
                            if hits >= battleforce::BF_ENGINE_HITS_DESTROY {
                                "Engine hit 2: TP 0 + shutdown".into()
                            } else {
                                "Engine: thrust −50% (round down, min 1 lost)".into()
                            }
                        }
                        BfCritCol::ProtoMech => "Engine hit marked".into(),
                        // DropShip/Small Craft engine ladder (p.43): thrust −25% / −50% / shutdown by
                        // hit. MV is table-side for large craft, so the hit is tracked here and the
                        // thrust reduction is applied at the table (spec §10, like the other
                        // large-craft movement effects).
                        BfCritCol::DropShip => format!(
                            "Engine hit {hits}: thrust −25% / −50% / shutdown by hit — apply at table (p.43)"
                        ),
                        // JumpShips are station-keeping (thrust 0) and WarShips carry their own
                        // drive; the movement/jump effect is table-side for large craft (spec §10).
                        BfCritCol::JumpShip => format!(
                            "Engine hit {hits}: drive / thrust damage — apply at table (p.87)"
                        ),
                    }
                }
                BfCrit::FireControl => {
                    tm.bf.fire_control += 1;
                    format!(
                        "Fire Control: +{} to-hit (never on physicals)",
                        2 * tm.bf.fire_control
                    )
                }
                BfCrit::Mp => {
                    mp_crit = true; // applied below via bf_apply_mp_crit (needs &mut Session)
                    String::new()
                }
                BfCrit::Weapon => {
                    // Large craft halve EVERY attack type in ONE randomly-determined firing arc
                    // (×0.5, round down; IO:BF p.85) — the random-arc pick and per-class halving are
                    // table-side (spec §10), NOT the standard-scale −1/damage counter (which the
                    // arc-damage path does not read). Standard elements use that counter.
                    if matches!(col, BfCritCol::DropShip | BfCritCol::JumpShip) {
                        "Weapon hit: halve one randomly-determined firing arc's attacks (×0.5, round down) — resolve at table (p.85)".into()
                    } else {
                        tm.bf.weapon += 1;
                        format!("Weapon hit: −{} to every damage value", tm.bf.weapon)
                    }
                }
                BfCrit::CrewStunned => {
                    tm.bf.crew_stunned = true;
                    "Crew stunned — no attacks next turn (cleared by n)".into()
                }
                BfCrit::CrewKilled => {
                    tm.bf.killed = Some(BfKill::CrewKilled);
                    "Crew killed — element destroyed".into()
                }
                BfCrit::Fuel => {
                    tm.bf.killed = Some(BfKill::Fuel);
                    "Fuel hit — element destroyed".into()
                }
                BfCrit::HeadBlownOff => {
                    tm.bf.killed = Some(BfKill::HeadBlownOff);
                    "Head blown off — element destroyed".into()
                }
                BfCrit::ProtoDestroyed => {
                    tm.bf.killed = Some(BfKill::ProtoDestroyed);
                    "ProtoMech destroyed".into()
                }
                // Large-craft crit results (IO:BF p.85). Stage counters live on `tm.bf`; the
                // transport / jump / maneuver *consequences* are inter-unit or positional and are
                // resolved at the table (spec §10). Crew Hit auto-eliminates on its final stage.
                BfCrit::KfBoom => {
                    tm.bf.kf_boom = true;
                    "KF Boom destroyed: may still dock, but no hyperspace jump (resolve at table, p.85)".into()
                }
                BfCrit::DockingCollar => {
                    tm.bf.docking_collar = true;
                    "Docking Collar hit: may not dock with a station / JumpShip / WarShip (resolve at table, p.85)".into()
                }
                BfCrit::Dock => {
                    tm.bf.dock_hits += 1;
                    let remaining = tm.spec.as_stats.dt_rating.saturating_sub(tm.bf.dock_hits);
                    format!(
                        "Dock hit {}: DropShip-Transport rating −1 → {remaining} capacity remaining (resolve at table, p.85)",
                        tm.bf.dock_hits
                    )
                }
                BfCrit::Thruster => {
                    tm.bf.thruster = true;
                    "Thruster hit: +1 Thrust per facing change (resolve at table, p.43)".into()
                }
                BfCrit::Door => {
                    tm.bf.door_hits += 1;
                    let total = tm.spec.as_stats.door_count;
                    let remaining = total.saturating_sub(u16::from(tm.bf.door_hits));
                    format!(
                        "Door hit {}: a transport-bay door lost → {remaining} of {total} remaining (resolve at table, p.85)",
                        tm.bf.door_hits
                    )
                }
                BfCrit::KfDrive => {
                    // K-F Drive hits have no effect on Space Stations (p.85).
                    if el.as_type == "SS" {
                        "K-F Drive hit: no effect on a Space Station (p.85)".into()
                    } else {
                        tm.bf.kf_drive += 1;
                        format!(
                            "K-F Drive hit {}: −{} drive integrity — no hyperspace jump at 0 (resolve at table, p.85)",
                            tm.bf.kf_drive,
                            2 * tm.bf.kf_drive
                        )
                    }
                }
                BfCrit::CrewHit => {
                    tm.bf.crew_hit += 1;
                    // DropShips/Small Craft: +2 then eliminate (2 stages). JumpShips/WarShips/
                    // Space Stations: +2 / +4 / eliminate (3 stages), p.85.
                    let stages = if col == BfCritCol::JumpShip { 3 } else { 2 };
                    if tm.bf.crew_hit >= stages {
                        tm.bf.killed = Some(BfKill::CrewKilled);
                        format!(
                            "Crew hit {}: crew eliminated — element destroyed (p.85)",
                            tm.bf.crew_hit
                        )
                    } else {
                        format!(
                            "Crew hit {}: +{} to-hit to all this element's shots (p.85)",
                            tm.bf.crew_hit,
                            2 * tm.bf.crew_hit
                        )
                    }
                }
            };
        }
        if mp_crit {
            self.session.bf_apply_mp_crit(i);
            self.status = format!(
                "MP crit: −half current MP — now {}",
                self.session.bf_current_mp(i)
            );
        }
        if self.session != before {
            self.push_undo(before);
            self.dirty = true;
        }
        true
    }

    /// Drive the BF crit modal: ↑↓ pick a 2D6 row (or a vehicle motive rung), Enter/Space applies
    /// its effect (one undo step), `a` marks/unmarks the ARM first-crit-chance checkbox (p.143).
    fn bf_crit_modal_key(&mut self, sel: usize, key: KeyEvent) {
        let rows = self.bf_crit_row_count();
        if rows == 0 {
            return; // no element / infantry — closes (modal already taken)
        }
        match key.code {
            KeyCode::Esc | KeyCode::Char('c') => {} // close (modal already taken)
            KeyCode::Up | KeyCode::Char('k') => {
                self.modal = Some(Modal::BfCrit {
                    sel: sel.saturating_sub(1),
                });
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.modal = Some(Modal::BfCrit {
                    sel: (sel + 1).min(rows - 1),
                });
            }
            KeyCode::Char(' ') | KeyCode::Enter => {
                if self.bf_apply_crit_row(sel) {
                    self.modal = Some(Modal::BfCrit { sel });
                } // else: the CASEP confirm replaced this modal
            }
            KeyCode::Char('a') => {
                let before = self.session.clone();
                let has_arm = self
                    .session
                    .active_mech()
                    .is_some_and(|tm| bf_element_of(tm).has_sua("ARM"));
                if let Some(tm) = self.session.active_mech_mut().filter(|_| has_arm) {
                    tm.bf.arm_spent = !tm.bf.arm_spent;
                    self.status = if tm.bf.arm_spent {
                        "ARM spent — this crit chance is ignored, later ones resolve normally"
                            .into()
                    } else {
                        "ARM restored (first crit chance of the scenario is ignored)".into()
                    };
                } else if !has_arm {
                    self.status = "No ARM on this element".into();
                }
                if self.session != before {
                    self.push_undo(before);
                    self.dirty = true;
                }
                self.modal = Some(Modal::BfCrit { sel });
            }
            _ => self.modal = Some(Modal::BfCrit { sel }),
        }
    }

    /// Number of rows in the BF shot modal (see [`view`]'s `bf_shot_modal_lines` for the order).
    pub(crate) const BF_SHOT_ROWS: usize = 24;

    /// Row count of the BF shot editor for the active unit: large craft add the firing-arc +
    /// weapon-class picker (24 rows); ground units stop before them (22).
    fn bf_shot_row_count(&self) -> usize {
        if self
            .session
            .active_mech()
            .is_some_and(|tm| tm.spec.as_stats.arcs.is_some())
        {
            Self::BF_SHOT_ROWS
        } else {
            Self::BF_SHOT_ROWS - 2
        }
    }

    /// Drive the BF to-hit shot editor (the p.39 table): ↑↓ pick a row, ←→/Space adjust it. All
    /// state is the ephemeral [`BfShotUi`] — nothing session-side mutates, so no undo steps.
    fn bf_shot_modal_key(&mut self, sel: usize, key: KeyEvent) {
        let el = self.session.active_mech().map(bf_element_of);
        let heat = self.session.active_mech().map_or(0, |tm| tm.as_heat);
        let adjust = |app: &mut App, delta: i32| {
            let s = &mut app.bf_shot;
            match sel {
                0 => {
                    // Attacker move 3-way (clamped ladder, the SBF range pattern).
                    s.attacker_move = match (s.attacker_move, delta > 0) {
                        (BfMove::StoodStill, true) => BfMove::Moved,
                        (BfMove::Moved, true) | (BfMove::Jumped, true) => BfMove::Jumped,
                        (BfMove::Jumped, false) => BfMove::Moved,
                        (BfMove::Moved, false) | (BfMove::StoodStill, false) => BfMove::StoodStill,
                    };
                }
                1 => {
                    s.range = match (s.range, delta > 0) {
                        (BfRange::Short, true) => BfRange::Medium,
                        (BfRange::Medium, true) => BfRange::Long,
                        (BfRange::Long, true) | (BfRange::Extreme, true) => BfRange::Extreme,
                        (BfRange::Extreme, false) => BfRange::Long,
                        (BfRange::Long, false) => BfRange::Medium,
                        (BfRange::Medium, false) | (BfRange::Short, false) => BfRange::Short,
                    };
                }
                2 => {
                    // Attack kind, cycling only the stops the element can declare (§1.3/§1.5).
                    let n = BF_KIND_CYCLE.len() as i32;
                    let cur = BF_KIND_CYCLE
                        .iter()
                        .position(|&k| bf_kind_same(k, s.kind))
                        .unwrap_or(0) as i32;
                    let mut p = cur;
                    for _ in 0..n {
                        p = (p + delta.signum()).rem_euclid(n);
                        let k = BF_KIND_CYCLE[p as usize];
                        if el
                            .as_ref()
                            .map_or(k == BfAttackKind::Standard, |e| bf_kind_eligible(e, k))
                        {
                            break;
                        }
                    }
                    s.kind = BF_KIND_CYCLE[p as usize];
                }
                3 => {
                    if let BfAttackKind::Indirect {
                        spotter_also_attacked,
                        ..
                    } = &mut s.kind
                    {
                        *spotter_also_attacked = !*spotter_also_attacked;
                    }
                }
                4 => {
                    if let BfAttackKind::Indirect {
                        spotter_is_remote_sensor,
                        ..
                    } = &mut s.kind
                    {
                        *spotter_is_remote_sensor = !*spotter_is_remote_sensor;
                    }
                }
                5 => {
                    let max = el
                        .as_ref()
                        .map_or(0, |e| battleforce::bf_max_ov_commit(e, heat));
                    s.ov = (s.ov as i32 + delta).clamp(0, i32::from(max)) as u8;
                }
                6 => s.area_effect = !s.area_effect,
                7 => s.secondary = !s.secondary,
                8 => s.also_spotting = !s.also_spotting,
                9 => {
                    if el.as_ref().is_some_and(battleforce::bf_is_aero) {
                        s.grounded = !s.grounded;
                    } else {
                        app.status = "Grounded rows apply to aerospace elements (p.46)".into();
                    }
                }
                10 => {
                    s.target_tmm = (i32::from(s.target_tmm) + delta)
                        .clamp(0, i32::from(session::AS_TARGET_TMM_MAX))
                        as u8;
                }
                11 => {
                    const MOVES: [BfTargetMove; 5] = [
                        BfTargetMove::StoodStill,
                        BfTargetMove::Ground,
                        BfTargetMove::Jumped,
                        BfTargetMove::Submersible,
                        BfTargetMove::Dropped,
                    ];
                    let cur = MOVES.iter().position(|&m| m == s.target_move).unwrap_or(0) as i32;
                    s.target_move =
                        MOVES[(cur + delta.signum()).rem_euclid(MOVES.len() as i32) as usize];
                }
                12 => s.target_move_adj = (s.target_move_adj + delta).clamp(-3, 3),
                13 => s.target_immobile = !s.target_immobile,
                14 => {
                    const KINDS: [BfTargetKind; 9] = [
                        BfTargetKind::None,
                        BfTargetKind::BattleArmor,
                        BfTargetKind::ProtoMech,
                        BfTargetKind::Large,
                        BfTargetKind::AirborneAero(BfAeroAngle::Nose),
                        BfTargetKind::AirborneAero(BfAeroAngle::Side),
                        BfTargetKind::AirborneAero(BfAeroAngle::Aft),
                        BfTargetKind::AirborneDropship,
                        BfTargetKind::AirborneVtolWige,
                    ];
                    let cur = KINDS.iter().position(|&k| k == s.target_kind).unwrap_or(0) as i32;
                    s.target_kind =
                        KINDS[(cur + delta.signum()).rem_euclid(KINDS.len() as i32) as usize];
                }
                15 => s.target_mas = (i32::from(s.target_mas) + delta).rem_euclid(3) as u8,
                16 => s.target_woods = !s.target_woods,
                17 => s.target_partial_cover = !s.target_partial_cover,
                18 => s.target_underwater = !s.target_underwater,
                19 => s.target_stealth = !s.target_stealth,
                20 => s.target_carrying_ba = !s.target_carrying_ba,
                21 => {
                    // Strafing/striking rear +1 damage (p.41); bombing never strikes the rear
                    // (p.48) and nothing else composes with the toggle.
                    if matches!(
                        s.kind,
                        BfAttackKind::AirToGround(BfA2G::Strafing | BfA2G::Striking)
                    ) {
                        s.strike_rear = !s.strike_rear;
                    } else {
                        app.status =
                            "Rear +1 applies to strafing/striking only (bombing never strikes rear, p.48)"
                                .into();
                    }
                }
                22 => {
                    // Large-craft firing arc (Nose/Left/Right/Aft).
                    let order = large_craft::Arc::ALL;
                    let cur = order.iter().position(|&a| a == s.firing_arc).unwrap_or(0) as i32;
                    s.firing_arc =
                        order[(cur + delta.signum()).rem_euclid(order.len() as i32) as usize];
                }
                23 => {
                    // Large-craft weapon class (STD/CAP/SCAP/MSL).
                    let order = large_craft::WeaponClass::ALL;
                    let cur = order.iter().position(|&c| c == s.weapon_class).unwrap_or(0) as i32;
                    s.weapon_class =
                        order[(cur + delta.signum()).rem_euclid(order.len() as i32) as usize];
                }
                _ => {}
            }
        };
        match key.code {
            KeyCode::Esc | KeyCode::Char('t') | KeyCode::Enter => {} // close (modal taken)
            KeyCode::Up | KeyCode::Char('k') => {
                self.modal = Some(Modal::BfShot {
                    sel: sel.saturating_sub(1),
                });
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.modal = Some(Modal::BfShot {
                    sel: (sel + 1).min(self.bf_shot_row_count() - 1),
                });
            }
            KeyCode::Right | KeyCode::Char(' ') => {
                adjust(self, 1);
                self.modal = Some(Modal::BfShot { sel });
            }
            KeyCode::Left => {
                adjust(self, -1);
                self.modal = Some(Modal::BfShot { sel });
            }
            _ => self.modal = Some(Modal::BfShot { sel }),
        }
    }

    /// Drive the BF grouping editor — the SbfGroup clone one tier flatter (spec §3.3): ↑↓ pick a
    /// pool element, ←→ move it between Units, `n` split it into a new Unit, `u` unassign,
    /// `s`/`S` adjust its Skill, `x` remove it from the force, `a` doctrine auto-group. Closing
    /// prunes any Units the edit emptied (the SBF close behavior).
    fn bf_group_modal_key(&mut self, sel: usize, key: KeyEvent) {
        let n = self.session.mechs.len();
        if n == 0 {
            return; // modal already taken → closes
        }
        let assign = |app: &mut App, target: BfAssign| {
            let before = app.session.clone();
            app.session.bf_assign_element(sel, target);
            // No pruning mid-edit (the SBF decision): a Unit you just emptied stays available
            // as a move target; the sheet renders it "(empty)". Units prune on close.
            if app.session != before {
                app.push_undo(before);
                app.dirty = true;
            }
        };
        match key.code {
            KeyCode::Esc | KeyCode::Enter | KeyCode::Char('g') => {
                let before = self.session.clone();
                self.session.bf_prune_empty_units();
                if self.session != before {
                    self.push_undo(before);
                    self.dirty = true;
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.modal = Some(Modal::BfGroup {
                    sel: sel.saturating_sub(1),
                });
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.modal = Some(Modal::BfGroup {
                    sel: (sel + 1).min(n - 1),
                });
            }
            KeyCode::Left | KeyCode::Right | KeyCode::Char(' ') => {
                // Cycle the element through every Unit, in sheet order.
                let stops: Vec<BfAssign> = (0..self.session.bf.units.len())
                    .map(BfAssign::Unit)
                    .collect();
                if stops.is_empty() {
                    self.status = "No units yet — [n] starts one".into();
                } else {
                    let step: i32 = if key.code == KeyCode::Left { -1 } else { 1 };
                    let cur = self.session.bf_element_assignment(sel);
                    let cur_pos =
                        cur.and_then(|ui| stops.iter().position(|&s| s == BfAssign::Unit(ui)));
                    let target = match cur_pos {
                        Some(pos) => {
                            stops[(pos as i32 + step).rem_euclid(stops.len() as i32) as usize]
                        }
                        // Unassigned: → enters the first Unit, ← the last.
                        None if step > 0 => stops[0],
                        None => stops[stops.len() - 1],
                    };
                    // Wrapping onto the element's own Unit (the only stop) would silently rotate
                    // its element order and spam undo steps — skip instead (the SBF guard).
                    if cur.is_some_and(|ui| target == BfAssign::Unit(ui)) {
                        self.status = "Only one unit — [n] splits, [u] unassigns".into();
                    } else {
                        assign(self, target);
                    }
                }
                self.modal = Some(Modal::BfGroup { sel });
            }
            KeyCode::Char('n') => {
                assign(self, BfAssign::NewUnit);
                self.modal = Some(Modal::BfGroup { sel });
            }
            KeyCode::Char('u') => {
                assign(self, BfAssign::Unassign);
                self.modal = Some(Modal::BfGroup { sel });
            }
            KeyCode::Char('s') | KeyCode::Char('S') => {
                // Adjust the element's Skill (gunnery drives BF to-hit and skill-adjusted PV).
                let delta: i32 = if key.code == KeyCode::Char('s') {
                    1
                } else {
                    -1
                };
                let before = self.session.clone();
                if let Some(tm) = self.session.mechs.get_mut(sel) {
                    tm.gunnery = (i32::from(tm.gunnery) + delta).clamp(0, SKILL_MAX as i32) as u8;
                }
                if self.session != before {
                    self.push_undo(before);
                    self.dirty = true;
                }
                self.modal = Some(Modal::BfGroup { sel });
            }
            KeyCode::Char('x') => {
                // Remove the element from the force entirely (indices remap; emptied Units go).
                let name = self.session.mechs.get(sel).map(|m| m.spec.display_name());
                let before = self.session.clone();
                self.session.remove_mech(sel);
                if self.session != before {
                    self.push_undo(before);
                    self.dirty = true;
                    if let Some(n) = name {
                        self.status = format!("Removed {n} from the force");
                    }
                }
                let n = self.session.mechs.len();
                if n > 0 {
                    self.modal = Some(Modal::BfGroup {
                        sel: sel.min(n - 1),
                    });
                } // pool empty → editor closes (modal already taken)
            }
            KeyCode::Char('a') => self.modal = Some(Modal::BfDoctrine { sel: 0 }),
            _ => self.modal = Some(Modal::BfGroup { sel }),
        }
    }

    /// Drive the BF doctrine picker: Enter rebuilds all Units under the chosen force-organization
    /// scheme (ground 4/5/6, aero pairs of 2 — spec §1.7), with the itemized destructive-regroup
    /// confirmation when hand-entered grouping state is at stake. Esc returns to the editor.
    fn bf_doctrine_modal_key(&mut self, sel: usize, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                let el = self
                    .session
                    .active
                    .min(self.session.mechs.len().saturating_sub(1));
                self.modal = Some(Modal::BfGroup { sel: el });
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.modal = Some(Modal::BfDoctrine {
                    sel: sel.saturating_sub(1),
                });
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.modal = Some(Modal::BfDoctrine {
                    sel: (sel + 1).min(2),
                });
            }
            KeyCode::Enter | KeyCode::Char(' ') => {
                let doctrine = match sel {
                    0 => SbfDoctrine::InnerSphere,
                    1 => SbfDoctrine::Clan,
                    _ => SbfDoctrine::ComStar,
                };
                // A pristine grouping applies immediately; anything hand-entered gets the
                // itemized bill first (the SBF precedent). Element damage/heat/crits live on the
                // cards and survive a regroup — only grouping-level state is at stake.
                let losses = self.bf_doctrine_losses();
                if losses.is_empty() {
                    self.bf_apply_doctrine(doctrine);
                } else {
                    self.modal = Some(Modal::Confirm {
                        prompt: format!(
                            "Rebuild all units?\nDiscards {} — z undoes.",
                            losses.join(", ")
                        ),
                        action: PendingAction::ApplyBfDoctrine(doctrine),
                    });
                }
            }
            _ => self.modal = Some(Modal::BfDoctrine { sel }),
        }
    }

    /// What a BF doctrine rebuild would discard, itemized for the confirmation prompt (empty =
    /// nothing hand-entered at stake). Unlike SBF, element live state (armor/heat/crits) lives on
    /// the cards, not the grouping — only Unit names, morale rungs and notes can be lost.
    fn bf_doctrine_losses(&self) -> Vec<String> {
        let us = &self.session.bf.units;
        let mut out = Vec::new();
        let custom_names = us.iter().filter(|u| !sbf_default_name(&u.name)).count();
        if custom_names > 0 {
            out.push(format!("{custom_names} custom name(s)"));
        }
        let morale = us
            .iter()
            .filter(|u| u.morale != neurohelmet_core::session::BfMorale::Normal)
            .count();
        if morale > 0 {
            out.push(format!("{morale} morale rung(s)"));
        }
        let notes = us.iter().filter(|u| !u.notes.is_empty()).count();
        if notes > 0 {
            out.push(format!("{notes} unit note(s)"));
        }
        out
    }

    /// Rebuild all Units under a doctrine (one undo step; losses were confirmed or nil).
    fn bf_apply_doctrine(&mut self, doctrine: SbfDoctrine) {
        let before = self.session.clone();
        self.session.bf_group_doctrine(doctrine);
        if self.session != before {
            self.push_undo(before);
            self.dirty = true;
        }
        let count = self.session.bf.units.len();
        self.status = format!("Auto-grouped into {count} unit(s)");
    }

    /// Open the critical-slot popup for the cursored location (if it has any slots).
    fn open_crit(&mut self) {
        // Vehicles and aerospace use a rolled crit-result list, not per-location slots
        // (the modal reads the active unit's `unit_crits()`).
        if self
            .session
            .active_mech()
            .is_some_and(|tm| tm.spec.is_vehicle() || tm.spec.is_aerospace())
        {
            self.modal = Some(Modal::VehicleCrit { sel: 0 });
            return;
        }
        let loc = self.cursor;
        let has = self
            .session
            .active_mech()
            .and_then(|tm| tm.spec.crit_slots.get(&loc))
            .is_some_and(|s| !s.is_empty());
        if has {
            self.modal = Some(Modal::Crit { loc, sel: 0 });
        } else {
            self.status = format!("{} has no crit slots", loc.code());
        }
    }

    /// `J`: toggle the selected equipment-panel row's manual active state — jam/unjam a Ultra/Rotary
    /// autocannon, engage/disengage a MASC or Supercharger (which boosts Running MP), or flip the
    /// display-only ECM / Stealth marker. A no-op on any other row. Only meaningful with the
    /// equipment panel focused.
    fn toggle_equip_state(&mut self) {
        if self.focus != Focus::Equipment {
            return;
        }
        let Some(row) = self.equip_rows().get(self.equip_sel).copied() else {
            return;
        };
        let Some(tm) = self.session.active_mech_mut() else {
            return;
        };
        match row {
            EquipRow::Weapon(id) => {
                let name = tm
                    .spec
                    .weapons
                    .iter()
                    .find(|w| w.id == id)
                    .map(|w| w.name.clone());
                let can_jam = tm.spec.weapons.iter().any(|w| w.id == id && w.can_jam());
                if !can_jam {
                    self.status = "Only Ultra/Rotary ACs can jam".into();
                    return;
                }
                let name = name.unwrap_or_default();
                self.status = if tm.toggle_jam(id) {
                    format!("{name} JAMMED")
                } else {
                    format!("{name} jam cleared")
                };
                self.dirty = true;
            }
            EquipRow::Equip(idx) => {
                let Some(e) = tm.spec.equipment.get(idx).cloned() else {
                    return;
                };
                // Classify with the same core helper the renderer uses, then flip the right field.
                let Some((label, _)) = tm.equip_toggle(&e) else {
                    self.status = format!("{} — no toggle-able state", e.name);
                    return;
                };
                let now = match label {
                    "MASC" => {
                        tm.masc_engaged = !tm.masc_engaged;
                        tm.masc_engaged
                    }
                    "SC" => {
                        tm.supercharger_engaged = !tm.supercharger_engaged;
                        tm.supercharger_engaged
                    }
                    "ECM" => {
                        tm.ecm_active = !tm.ecm_active;
                        tm.ecm_active
                    }
                    _ => {
                        tm.stealth_active = !tm.stealth_active;
                        tm.stealth_active
                    }
                };
                // MASC/Supercharger "engage"; ECM/Stealth are "on/off" markers.
                let (full, verb) = match label {
                    "MASC" => ("MASC", if now { "engaged" } else { "disengaged" }),
                    "SC" => ("Supercharger", if now { "engaged" } else { "disengaged" }),
                    "ECM" => ("ECM", if now { "on" } else { "off" }),
                    other => (other, if now { "on" } else { "off" }),
                };
                self.status = format!("{full} {verb}");
                self.dirty = true;
            }
            EquipRow::Ammo(_) => self.status = "No toggle-able state".into(),
        }
    }

    /// The weapon id of the currently selected equipment row, if that row is a weapon.
    pub fn selected_weapon_id(&self) -> Option<u32> {
        match self.equip_rows().get(self.equip_sel) {
            Some(EquipRow::Weapon(id)) => Some(*id),
            _ => None,
        }
    }

    /// Open the read-only dice-reference popup (§18). Defaults to the Cluster tab when the selected
    /// weapon actually rolls on the cluster table, else the hit-location tab.
    fn open_dice(&mut self) {
        let rolls_cluster = self
            .selected_weapon_id()
            .and_then(|id| {
                self.session
                    .active_mech()
                    .and_then(|tm| tm.weapon_cluster_profile(id))
            })
            .is_some_and(|p| !matches!(p, ClusterProfile::Single));
        let tab = if rolls_cluster {
            DiceTab::Cluster
        } else {
            DiceTab::HitLoc
        };
        self.modal = Some(Modal::Dice { tab });
    }

    /// Drive the dice-reference popup: `Tab`/→ cycle forward, `BackTab`/← back; it changes no state.
    fn dice_modal_key(&mut self, tab: DiceTab, key: KeyEvent) {
        let all = DiceTab::ALL;
        let i = all.iter().position(|&t| t == tab).unwrap_or(0) as i32;
        let step = |d: i32| all[(i + d).rem_euclid(all.len() as i32) as usize];
        match key.code {
            KeyCode::Esc | KeyCode::Char('r') => {} // close (modal already taken)
            KeyCode::Tab | KeyCode::Right | KeyCode::Char('l') => {
                self.modal = Some(Modal::Dice { tab: step(1) });
            }
            KeyCode::BackTab | KeyCode::Left | KeyCode::Char('h') => {
                self.modal = Some(Modal::Dice { tab: step(-1) });
            }
            _ => self.modal = Some(Modal::Dice { tab }),
        }
    }

    /// Drive the picker filter editor: ↑↓ pick a facet, ←→ cycle its value, `c`/Backspace clears
    /// all, Esc/Enter/Ctrl+F applies (re-filters). Any value change re-runs the picker filter.
    fn filters_modal_key(&mut self, sel: usize, key: KeyEvent) {
        let last = Facet::ALL.len() - 1;
        let mut changed = false;
        let mut new_sel = sel;
        match key.code {
            // The Faction lens has 82 values — too many to cycle. Enter opens a type-to-filter picker.
            KeyCode::Enter if Facet::ALL[sel] == Facet::Faction => {
                self.modal = Some(Modal::FactionPick {
                    query: String::new(),
                    sel: 0,
                });
                return;
            }
            KeyCode::Esc | KeyCode::Enter => return, // close (modal already taken)
            KeyCode::Char('f') if key.modifiers.contains(KeyModifiers::CONTROL) => return,
            KeyCode::Up | KeyCode::Char('k') => new_sel = sel.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => new_sel = (sel + 1).min(last),
            KeyCode::Right | KeyCode::Char('l') => {
                filters::cycle(&mut self.filters, Facet::ALL[sel], &self.facet_values, 1);
                changed = true;
            }
            KeyCode::Left | KeyCode::Char('h') => {
                filters::cycle(&mut self.filters, Facet::ALL[sel], &self.facet_values, -1);
                changed = true;
            }
            KeyCode::Char('c') => {
                self.filters.clear();
                changed = true;
            }
            // Backspace deletes a typed digit on a year-bound facet; elsewhere it clears all.
            KeyCode::Backspace => {
                if Facet::ALL[sel].is_year() {
                    self.filters.year_backspace(Facet::ALL[sel]);
                } else {
                    self.filters.clear();
                }
                changed = true;
            }
            // Digits type a year bound (only meaningful on the Year ≥ / Year ≤ facets).
            KeyCode::Char(d) if d.is_ascii_digit() && Facet::ALL[sel].is_year() => {
                self.filters.year_push_digit(Facet::ALL[sel], d);
                changed = true;
            }
            _ => {}
        }
        if changed {
            self.picker.selected = 0;
            self.picker
                .refilter(&self.names, &self.bundle, &self.filters);
        }
        self.modal = Some(Modal::Filters { sel: new_sel });
    }

    /// The faction catalog filtered by `query` (case-insensitive substring). With an empty query the
    /// list leads with `None` (the "(any)" / clear entry) followed by every faction; once the player
    /// types, only matches are shown (so the top row is the best match, not "(any)").
    pub(crate) fn faction_pick_list(&self, query: &str) -> Vec<Option<(u16, String)>> {
        let q = query.trim().to_ascii_lowercase();
        if q.is_empty() {
            let mut out: Vec<Option<(u16, String)>> = vec![None];
            out.extend(self.facet_values.factions.iter().cloned().map(Some));
            out
        } else {
            self.facet_values
                .factions
                .iter()
                .filter(|(_, name)| name.to_ascii_lowercase().contains(&q))
                .cloned()
                .map(Some)
                .collect()
        }
    }

    /// Handle a key in the faction combo box. Letters type into the query (so vim-style j/k can't be
    /// used to navigate here — arrows do); Enter commits the highlighted faction and returns to the
    /// Filters modal; Esc returns without changing the lens.
    fn faction_pick_key(&mut self, mut query: String, mut sel: usize, key: KeyEvent) {
        let faction_facet = Facet::ALL
            .iter()
            .position(|f| *f == Facet::Faction)
            .unwrap_or(0);
        match key.code {
            KeyCode::Esc => {
                self.modal = Some(Modal::Filters { sel: faction_facet });
                return;
            }
            KeyCode::Enter => {
                let list = self.faction_pick_list(&query);
                if let Some(choice) = list.get(sel) {
                    self.filters.faction = choice.clone();
                    self.picker.selected = 0;
                    self.picker
                        .refilter(&self.names, &self.bundle, &self.filters);
                }
                self.modal = Some(Modal::Filters { sel: faction_facet });
                return;
            }
            KeyCode::Up => sel = sel.saturating_sub(1),
            KeyCode::Down => {
                let n = self.faction_pick_list(&query).len();
                if sel + 1 < n {
                    sel += 1;
                }
            }
            KeyCode::Backspace => {
                query.pop();
                sel = 0;
            }
            KeyCode::Char(c) => {
                query.push(c);
                sel = 0; // re-anchor on the new top match
            }
            _ => {}
        }
        self.modal = Some(Modal::FactionPick { query, sel });
    }

    fn toggle_focus(&mut self) {
        self.focus = match self.focus {
            Focus::Doll => Focus::Equipment,
            Focus::Equipment => Focus::Doll,
        };
    }

    fn move_selection(&mut self, dir: Dir) {
        match self.focus {
            Focus::Doll => {
                // The unit's real location set — vehicles, BA troopers, and the platoon are
                // not in the mech-config set.
                let locs = self
                    .session
                    .active_mech()
                    .map(|tm| tm.spec.locations())
                    .unwrap_or_default();
                if let Some(next) = move_cursor(self.cursor, dir, &locs) {
                    self.cursor = next;
                    if !self.cursor.has_rear() {
                        self.facing = Facing::Front;
                    }
                    // For Battle Armor the cursor trooper is also the suit you fire from.
                    self.sync_firing_suit();
                }
            }
            Focus::Equipment => {
                let n = self.equip_rows().len();
                if n == 0 {
                    return;
                }
                let step = match dir {
                    Dir::Up | Dir::Left => -1,
                    Dir::Down | Dir::Right => 1,
                };
                let idx = (self.equip_sel as i32 + step).rem_euclid(n as i32) as usize;
                self.equip_sel = idx;
            }
        }
    }

    fn adjust_heat(&mut self, delta: i32) {
        if let Some(tm) = self.session.active_mech_mut() {
            tm.adjust_heat(delta);
            self.dirty = true;
        }
    }

    fn end_turn(&mut self) {
        if let Some(tm) = self.session.active_mech_mut() {
            tm.end_turn();
            self.dirty = true;
            self.status = "End of turn: heat dissipated".into();
        }
    }

    /// Snapshot every mech's current state into the session's game log (`L`). Opt-in and
    /// non-destructive — it only reads state and appends one JSONL line; "Turn N" is just the
    /// snapshot count. Does nothing useful with an empty roster.
    fn log_snapshot(&mut self) {
        if self.session.mechs.is_empty() {
            self.status = "Nothing to log (empty roster)".into();
            return;
        }
        self.session.turn += 1;
        let turn = self.session.turn;
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()
            .map(|d| d.as_secs().to_string());
        let entry = neurohelmet_core::log::LogEntry {
            turn,
            label: format!("Turn {turn}"),
            ts,
            mode: self.session.mode,
            mechs: self.session.mechs.clone(),
            // Formation grouping + live state so an SBF entry re-renders its formation sheet
            // (empty and ignored for the other modes).
            sbf: self.session.sbf.clone(),
            // BF Unit grouping + round state, same self-containment obligation (Phase 3 renders it).
            bf: self.session.bf.clone(),
        };
        match neurohelmet_core::log::append_log(&self.current_name, &entry) {
            Ok(()) => {
                self.dirty = true; // persist the bumped turn counter
                self.status = format!("Logged Turn {turn} ({} mechs)", self.session.mechs.len());
            }
            Err(e) => {
                self.session.turn -= 1; // roll back the counter if the write failed
                self.status = format!("Log failed: {e}");
            }
        }
    }

    /// Toggle the active 'Mech's prone (knocked-down) state.
    fn toggle_prone(&mut self) {
        if let Some(tm) = self.session.active_mech_mut() {
            tm.prone = !tm.prone;
            let now = tm.prone;
            self.dirty = true;
            self.status = if now {
                "Prone (knocked down)".into()
            } else {
                "Stood up".into()
            };
        }
    }

    /// Space/Enter: damage in the doll, fire/spend in the equipment panel.
    fn primary_action(&mut self) {
        match self.focus {
            Focus::Doll => {
                let (loc, facing) = (self.cursor, self.facing);
                if let Some(tm) = self.session.active_mech_mut() {
                    tm.damage(loc, facing, 1);
                    self.dirty = true;
                }
            }
            Focus::Equipment => {
                let rows = self.equip_rows();
                let Some(row) = rows.get(self.equip_sel).copied() else {
                    return;
                };
                // Battle Armor fires one suit at a time (the cursor trooper); a dead suit can't.
                if let Some(tm) = self.session.active_mech() {
                    if tm.spec.unit_type == UnitType::BattleArmor && !tm.suit_alive(tm.active_suit)
                    {
                        self.status =
                            format!("{} destroyed — pick a living suit", self.cursor.code());
                        return;
                    }
                }
                let suit_tag = self
                    .session
                    .active_mech()
                    .filter(|tm| tm.spec.unit_type == UnitType::BattleArmor)
                    .map(|_| format!("{} ", self.cursor.code()))
                    .unwrap_or_default();
                if let Some(tm) = self.session.active_mech_mut() {
                    match row {
                        EquipRow::Weapon(id) => {
                            // Battle Armor fires each weapon once per suit (the active/cursor
                            // suit); everything else fires once a turn (Ultra ACs twice, Rotary up
                            // to six).
                            let ba = tm.spec.unit_type == UnitType::BattleArmor;
                            let max = tm
                                .spec
                                .weapons
                                .iter()
                                .find(|w| w.id == id)
                                .map_or(1, |w| w.max_shots());
                            let blocked = if ba {
                                tm.active_suit_fired(id)
                            } else {
                                tm.shots_fired(id) >= max
                            };
                            if tm.is_jammed(id) {
                                self.status = "JAMMED — press J to clear".into();
                            } else if blocked {
                                self.status = if ba {
                                    format!("{suit_tag}already fired this weapon — u to un-fire")
                                } else if max > 1 {
                                    format!("Max shots this turn ({max}) — u to un-fire")
                                } else {
                                    "Already fired this turn — u to un-fire".into()
                                };
                            } else if let Some(r) = tm.fire_weapon(id) {
                                let mut parts = Vec::new();
                                // Vehicles/infantry don't track heat — don't report it on firing.
                                if r.heat > 0 && !tm.spec.is_vehicle() && !tm.spec.is_infantry() {
                                    parts.push(format!("+{} heat", r.heat));
                                }
                                if r.ammo_spent {
                                    parts.push("-1 ammo".into());
                                }
                                if r.out_of_ammo {
                                    parts.push("OUT OF AMMO".into());
                                }
                                self.status = if parts.is_empty() {
                                    format!("{suit_tag}Fired")
                                } else {
                                    format!("{suit_tag}Fired ({})", parts.join(", "))
                                };
                            }
                        }
                        EquipRow::Ammo(id) => {
                            tm.fire_ammo(id, 1);
                            self.status = "Spent 1 shot".into();
                        }
                        EquipRow::Equip(idx) => {
                            let name = tm
                                .spec
                                .equipment
                                .get(idx)
                                .map(|e| e.name.clone())
                                .unwrap_or_default();
                            self.status = format!("{name} — no action");
                        }
                    }
                    self.dirty = true;
                }
            }
        }
    }

    /// 'u': repair in the doll, undo-fire / refill in the equipment panel.
    fn secondary_action(&mut self) {
        match self.focus {
            Focus::Doll => {
                let (loc, facing) = (self.cursor, self.facing);
                if let Some(tm) = self.session.active_mech_mut() {
                    let st = tm.locations.get(&loc).copied().unwrap_or_default();
                    // Repair internal structure first, then armor on top of it.
                    if st.internal_hits > 0 {
                        tm.repair_internal(loc, 1);
                    } else {
                        let armor_hit = match facing {
                            Facing::Front => st.armor_hits,
                            Facing::Rear => st.rear_hits,
                        };
                        if armor_hit > 0 {
                            tm.repair_armor(loc, facing, 1);
                        }
                    }
                    self.dirty = true;
                }
            }
            Focus::Equipment => {
                let rows = self.equip_rows();
                let Some(row) = rows.get(self.equip_sel).copied() else {
                    return;
                };
                if let Some(tm) = self.session.active_mech_mut() {
                    match row {
                        EquipRow::Weapon(id) => {
                            self.status = match tm.unfire_weapon(id) {
                                Some(heat) => format!("Un-fired (-{heat} heat)"),
                                None => "Not fired this turn".into(),
                            };
                        }
                        EquipRow::Ammo(id) => {
                            tm.adjust_ammo(id, 1);
                            self.status = "Refilled 1 shot".into();
                        }
                        EquipRow::Equip(idx) => {
                            let name = tm
                                .spec
                                .equipment
                                .get(idx)
                                .map(|e| e.name.clone())
                                .unwrap_or_default();
                            self.status = format!("{name} — no action");
                        }
                    }
                    self.dirty = true;
                }
            }
        }
    }
}

#[derive(Clone, Copy)]
pub enum Dir {
    Up,
    Down,
    Left,
    Right,
}

/// Grid coordinates (row, col) for the box-grid paper doll.
/// The (row, col) cell of a location in the 3×5 doll grid. Shared by the renderer and the
/// cursor navigation so they always agree. Quad/tripod legs reuse the biped cells.
pub(crate) fn grid_pos(loc: Location) -> (i32, i32) {
    match loc {
        Location::Head => (0, 2),
        Location::LeftArm => (1, 0),
        Location::LeftTorso => (1, 1),
        Location::CenterTorso => (1, 2),
        Location::RightTorso => (1, 3),
        Location::RightArm => (1, 4),
        Location::LeftLeg => (2, 1),
        Location::RightLeg => (2, 3),
        // Quad: front legs flank the torsos (arm row), rear legs at the outer bottom corners.
        Location::FrontLeftLeg => (1, 0),
        Location::FrontRightLeg => (1, 4),
        Location::RearLeftLeg => (2, 0),
        Location::RearRightLeg => (2, 4),
        // Tripod center leg sits between the two outer legs.
        Location::CenterLeg => (2, 2),
        // Combat-vehicle doll: Front across the top, Turret/Sides in the middle, Body, then Rear.
        Location::Front => (0, 2),
        Location::FrontLeftSide => (0, 1),
        Location::FrontRightSide => (0, 3),
        Location::FrontTurret => (1, 0),
        Location::LeftSide => (1, 1),
        Location::Turret => (1, 2),
        Location::RightSide => (1, 3),
        Location::Rotor => (1, 4),
        Location::Body => (2, 2),
        Location::RearLeftSide => (3, 1),
        Location::Rear => (3, 2),
        Location::RearRightSide => (3, 3),
        // Battle Armor squad: troopers 1-3 across the top, 4-6 across the middle.
        Location::Trooper1 => (0, 1),
        Location::Trooper2 => (0, 2),
        Location::Trooper3 => (0, 3),
        Location::Trooper4 => (1, 1),
        Location::Trooper5 => (1, 2),
        Location::Trooper6 => (1, 3),
        // Conventional infantry: one strength track, centre stage.
        Location::Platoon => (1, 2),
        // Aerospace fighter: Nose up top, wings flanking the central SI pool, Aft at the bottom.
        Location::Nose => (0, 2),
        Location::LeftWing => (1, 1),
        Location::AeroSI => (1, 2),
        Location::RightWing => (1, 3),
        Location::Aft => (2, 2),
    }
}

/// Move the doll cursor in a direction, picking the nearest location that way.
fn move_cursor(from: Location, dir: Dir, locs: &[Location]) -> Option<Location> {
    let (r0, c0) = grid_pos(from);
    locs.iter()
        .copied()
        .filter(|&loc| loc != from)
        .filter_map(|loc| {
            let (r, c) = grid_pos(loc);
            let ok = match dir {
                Dir::Up => r < r0,
                Dir::Down => r > r0,
                Dir::Left => c < c0,
                Dir::Right => c > c0,
            };
            if !ok {
                return None;
            }
            // Cost: primary axis distance dominates, secondary axis breaks ties.
            let cost = match dir {
                Dir::Up | Dir::Down => (r - r0).abs() * 10 + (c - c0).abs(),
                Dir::Left | Dir::Right => (c - c0).abs() * 10 + (r - r0).abs(),
            };
            Some((loc, cost))
        })
        .min_by_key(|(_, cost)| *cost)
        .map(|(loc, _)| loc)
}
