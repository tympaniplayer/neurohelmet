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

//! Phase 1 of Standard BattleForce support (see `docs/standard-bf-implementation-spec.md`): the
//! pure rules module — hex-native range tables, live MV/TMM arithmetic, the BF to-hit calculator
//! (p.39), the type-columned critical-hit table (p.42), motive damage (p.44), physical and
//! air-to-ground damage readouts (pp.43–48), overheat commitment (pp.48–49), and Unit (lance)
//! derived stats (pp.52–53).
//!
//! There is no converter in this mode: the element data IS the baked Alpha Strike card ("Alpha
//! Strike and BattleForce share the same unit conversions from Total Warfare", p.50), read through
//! [`AsElement`]; only MV converts (inches ÷ 2 = hexes — [`super::alpha_strike::movement_hexes`],
//! reused, not duplicated). IO:BF is the only rules authority; all `p.NN` cites are btrules
//! extraction markers (spec §"Page-citation convention"). Where BF and Alpha Strike:CE disagree
//! (attacker standstill −1, no to-hit floor, ground E = L−1), BF wins inside this mode — AS mode
//! is untouched.
//!
//! Pure functions; the engine rolls nothing — the player supplies every 2D6 total (the
//! `dice::cluster_hits` doctrine). Crit trigger predicates (structure damage, BAR, aero TH, hull
//! breach) and effect application live on the Phase-2 session state, never here. All arithmetic
//! uses the book's named rounding (p.25): round up / round down / round normally (= half up,
//! [`jround`]); each function names its mode.

use super::as_element::{jround, AsElement, SbfElementType};
use serde::{Deserialize, Serialize};

/// Firing-range bracket. The four-band semantics are identical to SBF's, so the enum is reused
/// rename-free (spec §Phase 1 header).
pub use super::sbf::SbfRange as BfRange;

// ============================ §1.1 Range table & damage ============================

/// Range-bracket reference labels (p.38, second printing p.344). Ground: S 0–1 / M 2–4 / L 5–8,
/// with E 9–10 starred — the standard ground table has no E bracket ("Ground Elements have 3
/// ranges", p.39; "only aerospace Elements use extreme range", p.41); ground Extreme is the
/// Advanced rule (p.84). Air-to-air: S 0–32 / M 33–64 / L 65–107 / E 108–133. Underwater
/// brackets (S 0 / M 1–2 / L 3–4; all underwater ranges halved, p.39 fn2) stay a table concern.
/// BF hexes are 90 m (p.27); these brackets are hex-native, NOT AS-inches÷2.
pub fn bf_range_label(aero: bool) -> &'static str {
    if aero {
        "S 0-32  M 33-64  L 65-107  E 108-133"
    } else {
        "S 0-1  M 2-4  L 5-8  E 9-10*"
    }
}

/// Whether an element fights at aerospace ranges: fighters and small craft (`AF`/`CF`/`SC`),
/// plus aerodyne / spheroid / station-keeping movers — the movement mode is decisive for the
/// fixed-wing-SV routing that `sbf_type_from_tp` cannot make (`isAerospaceSV` is not baked).
/// Crit-column routing is [`bf_crit_col`]'s (SC rolls DropShip, not Aerospace). LAM/BIM 'Mechs
/// count as ground until converted (table concern).
pub fn bf_is_aero(el: &AsElement) -> bool {
    matches!(el.as_type.as_str(), "AF" | "CF" | "SC")
        || matches!(el.primary_mode.as_str(), "a" | "p" | "k")
}

/// Damage at a range bracket (p.39, p.41): the card's S/M/L values, −1 per Weapon crit, floored
/// at 0 (p.43). Ground Extreme (the Advanced 9–10-hex bracket) is **computed at attack time as
/// Long − 1, min 0** (p.84) — never the baked `dmg_e`, which is aerospace-only data; aerospace E
/// is the baked value. `None` = no attack at this bracket (a printed dash/0, or a value reduced
/// to nothing); `Some(0.5)` = minimal damage, rendered `0*` (on a hit, 1D6: 3+ → 1 damage, else
/// 0 — p.41; the table rolls it, like AS mode).
pub fn bf_damage(el: &AsElement, range: BfRange, weapon_crits: u8) -> Option<f32> {
    let d = &el.std_damage;
    let raw = match range {
        BfRange::Short => d.s,
        BfRange::Medium => d.m,
        BfRange::Long => d.l.unwrap_or(0.0),
        BfRange::Extreme if bf_is_aero(el) => d.e.unwrap_or(0.0),
        BfRange::Extreme => (d.l.unwrap_or(0.0) - 1.0).max(0.0),
    };
    let v = (raw - f32::from(weapon_crits)).max(0.0);
    if v > 0.0 {
        Some(v)
    } else {
        None
    }
}

// ============================ §1.2 Live MV, TMM & crit/heat arithmetic ============================

/// Vehicle motive-damage effects (p.44) as independent once-per-game spent-flags. "A vehicle may
/// only suffer each effect once per game" (p.43) limits repeats of the SAME effect, not
/// combinations — a vehicle that rolls 8–9 then 10–11 has **both** −1 MV and ½ MV (as built
/// 2026-07-05; the first cut modeled a monotone rung, which could never stack them). Flags only
/// ever set ([`crate::session::TrackedMech::bf_mark_motive`]); marking a marked effect is a
/// no-op. The 1D6 chance roll (1–4 no effect, 5–6 effect) and the modified 2D6 effect roll stay
/// at the table; the app records the flags. Crash rider (p.43): a VTOL/WiGE reduced to 0 MV
/// while ≥1 elevation up crashes — 1 damage (crit-check if it damages structure) + `immobile`;
/// the crit modal surfaces both.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BfMotive {
    /// −1 MV.
    #[serde(default)]
    pub minus_one: bool,
    /// MV × 0.5, round down.
    #[serde(default)]
    pub half: bool,
    /// Immobilized (to-hit −4 basis, §1.3).
    #[serde(default)]
    pub immobile: bool,
}

impl BfMotive {
    /// Whether any motive effect is marked.
    pub fn any(self) -> bool {
        self.minus_one || self.half || self.immobile
    }
}

/// Read the motive-damage effect table (p.44) for a MODIFIED 2D6 total: 2–7 no effect, 8–9
/// −1 MV, 10–11 ½ MV, 12 immobilized. Roll modifiers (Tracked/Naval +0, Wheeled +2,
/// Hover/Hydrofoil +3, VTOL/WiGE +4, rear hit +1) can push the total past 12 — still immobilized.
/// Returns the single rolled effect as a one-flag [`BfMotive`] (all-false = no effect). This is
/// the crit modal's dim reference; the player rolls at the table.
pub fn bf_motive_effect(roll_2d6: i32) -> BfMotive {
    match roll_2d6 {
        i32::MIN..=7 => BfMotive::default(),
        8 | 9 => BfMotive { minus_one: true, ..Default::default() },
        10 | 11 => BfMotive { half: true, ..Default::default() },
        _ => BfMotive { immobile: true, ..Default::default() },
    }
}

/// MP-crit loss (p.43): each hit removes half of CURRENT MP/TP, rounded normally ([`jround`]),
/// minimum 1 lost. The loss is multiplicative at apply time — the state layer computes it when
/// the crit lands and accumulates it into `mp_lost`; it is never `count × k` (spec §1.2).
pub fn bf_mp_crit_loss(current_mp: u32) -> u32 {
    (jround(f64::from(current_mp) / 2.0) as u32).max(1)
}

/// Current available ground MP (aerospace: TP): base hexes, TSM-adjusted, minus the heat level
/// (heat subtracts from MP directly, p.49), minus the accumulated MP-crit loss
/// ([`bf_mp_crit_loss`]), then the once-per-game motive flags (p.44) and the live Engine-crit
/// effect, floored at 0. At 0 MP the element cannot move (aero at 0 TP: velocity frozen — table
/// concern beyond the badge).
///
/// Order (spec §1.2, as built 2026-07-05): additive losses first (heat, `mp_lost`, motive −1),
/// then the halvings (motive ½ round down; vehicle Engine crit ×0.5 round down — order between
/// the two is immaterial, ⌊⌊x/2⌋/2⌋ = ⌊x/4⌋), then the zeroes (motive immobile; aero 2nd Engine
/// hit). Engine-crit MV/TP effects are **derived live from the persisted hit count**, never
/// snapshotted into `mp_lost` (the `ov_card()` doctrine) — `mp_lost` holds MP crits only, so no
/// fixed evaluation order can disagree with chronological table play:
/// - Vehicle column, 1 Engine hit: MV × 0.5, round down (p.43).
/// - Aerospace column, 1 Engine hit: thrust −50% of current, round down, min 1 lost
///   ([`bf_aero_engine_tp_loss`]); 2+ hits: TP 0 (+ shutdown badge — §1.4, permanent p.42).
///
/// TSM (p.154): at heat ≥ 1 the element gains +1 MP, and at heat exactly 1 it ignores the 1-MP
/// heat loss entirely (heat 2+ subtracts normally). The movement effect is NOT pre-baked into AS
/// stats, so `tsm` = the element has the plain `TSM` special; I-TSM/TSMX have no movement effect
/// (already in stats). (The spec's §1.2 signature carries this as the "takes the element (or a
/// `tsm: bool`)" option — the bool keeps the function scalar.)
pub fn bf_current_mp(
    base_hexes: u32,
    heat: u8,
    mp_lost: u32,
    motive: BfMotive,
    tsm: bool,
    engine_hits: u8,
    col: Option<BfCritCol>,
) -> u32 {
    let mut mp = base_hexes;
    if tsm && heat >= 1 {
        mp += 1;
    }
    let heat_loss = if tsm && heat == 1 { 0 } else { u32::from(heat) };
    mp = mp.saturating_sub(heat_loss);
    mp = mp.saturating_sub(mp_lost);
    if motive.minus_one {
        mp = mp.saturating_sub(1);
    }
    if motive.half {
        mp /= 2; // MV × 0.5, round down (p.44)
    }
    if engine_hits >= 1 && col == Some(BfCritCol::Vehicle) {
        mp /= 2; // Engine crit: MV × 0.5, round down (p.43)
    }
    if col == Some(BfCritCol::Aerospace) {
        if engine_hits >= BF_ENGINE_HITS_DESTROY {
            mp = 0; // 2nd Engine hit: TP 0 + shutdown (§1.4)
        } else if engine_hits >= 1 {
            mp = mp.saturating_sub(bf_aero_engine_tp_loss(mp));
        }
    }
    if motive.immobile {
        mp = 0;
    }
    mp
}

/// TMM from available MP (numeric brackets survive extraction only in the p.347 quick-ref; basis
/// p.86 fn1: "based on available MP modified by heat level and critical hits … MP expended are
/// irrelevant. Does not apply to aerospace Elements."). The BF ground sheet prints no TMM box —
/// this bracket table is the live source, numerically identical to the AS TMM table at 2″ = 1
/// hex (the OQ-5 catalog sweep pins the divergences; the rule wins live). Immobile is not a TMM:
/// it is the flat −4 to-hit row (§1.3).
pub fn bf_tmm(available_mp: u32) -> i32 {
    match available_mp {
        0..=2 => 0,
        3..=4 => 1,
        5..=6 => 2,
        7..=9 => 3,
        10..=17 => 4,
        _ => 5,
    }
}

/// Engine hits that destroy a 'Mech or Vehicle (pp.42–43; matches `as_destroyed`'s 2-engine
/// rule). Aerospace differs: its 2nd Engine hit is TP 0 + shutdown, not destruction (§1.4).
pub const BF_ENGINE_HITS_DESTROY: u8 = 2;

/// Aerospace Engine crit, first hit (p.43 per §1.4): thrust −50%, round down, minimum 1 TP lost.
/// Returns the TP LOST from the current TP; derived live inside [`bf_current_mp`] from the
/// persisted hit count (as built 2026-07-05 — never snapshotted, so cooling heat afterwards
/// cannot resurrect thrust past the crit). The second Engine hit is TP 0 + shutdown. The vehicle
/// Engine-crit counterpart (MV ×0.5 and all damage ×0.5, round down, p.43) is likewise derived
/// live: MV in [`bf_current_mp`], damage in [`bf_shot_damage`]/[`bf_indirect_damage`]/
/// [`bf_rear_damage`] via the shared halving leg.
pub fn bf_aero_engine_tp_loss(current_tp: u32) -> u32 {
    (current_tp / 2).max(1)
}

// ============================ §1.3 The to-hit calculator ============================

/// Attacker movement this turn — the shot modal's 3-way (p.39 attacker-move rows). Standstill −1
/// and Jumping +2 exempt infantry/BA (fn1); Ground/minimum is +0.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BfMove {
    StoodStill,
    #[default]
    Moved,
    Jumped,
}

/// Target movement — the hand-entered target-move rows (p.39). `Jumped`/`Submersible` add
/// TMM + 1, adjusted ±JMPS/JMPW / ±SUBS/SUBW via [`BfShot::target_move_adj`] (the standard
/// printing's "JMPS#" labels on the submersible row are typos; the p.86 copy corrects them).
/// `Dropped` (by an airborne Unit) is a flat +3. Immobile overrides the whole group via
/// [`BfShot::target_immobile`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BfTargetMove {
    StoodStill,
    #[default]
    Ground,
    Jumped,
    Submersible,
    Dropped,
}

/// Physical-attack subtype (pp.43–45). Type eligibility: [`bf_physical_eligible`]; TN rows in
/// [`bf_to_hit`]; damage in [`bf_physical_damage`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BfPhysical {
    Standard,
    Melee,
    Charge,
    Dfa,
    AntiMech,
}

/// Air-to-ground attack flavor (pp.47–48; TN rows p.85): Altitude Bombing +3, Dive Bombing +2,
/// Strafing +2, Striking +2. Bombing excludes the target-immobile and target-hex terrain rows
/// (p.47); all A2G attacks resolve at Short range (p.47) — the caller sets the bracket.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BfA2G {
    AltitudeBombing,
    DiveBombing,
    Strafing,
    Striking,
}

/// Angle of attack against an airborne aerospace target: nose +1 / sides +2 / aft +0 (p.86 fn10;
/// the standard chapter carries these only in a diagram the extraction dropped — OQ 11). The
/// printed +2 target-type row is the side default.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BfAeroAngle {
    Nose,
    #[default]
    Side,
    Aft,
}

/// Hand-entered target profile — the p.39 target-type rows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BfTargetKind {
    #[default]
    None,
    /// +1 (and the harder STL profile, fn11).
    BattleArmor,
    /// +1.
    ProtoMech,
    /// LG/SLG/VLG: −1.
    Large,
    /// +1/+2/+0 by angle of attack.
    AirborneAero(BfAeroAngle),
    /// −2.
    AirborneDropship,
    /// +1.
    AirborneVtolWige,
}

/// Attack kind (p.39 attack rows; physical pp.43–45; A2G pp.47–48, p.85).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BfAttackKind {
    #[default]
    Standard,
    /// Indirect fire: +1; +2 instead if the spotter also attacked this turn; a remote-sensor
    /// spotter adds a further +3 (fn4).
    Indirect { spotter_also_attacked: bool, spotter_is_remote_sensor: bool },
    /// REAR-weapons attack (§1.6): +1 TN, fires the REAR damage values ([`bf_rear_damage`]) —
    /// distinct from *being hit* in the rear, which is +1 damage and no TN row (p.41).
    RearWeapons,
    Physical(BfPhysical),
    AirToGround(BfA2G),
}

/// One declared attack — everything the p.39 To-Hit Modifiers Table needs that is not derived
/// from the attacking element. Target-side fields are hand-entered (single-force scope: the only
/// cross-side data is the target's numbers), mirroring `AsTarget`/`SbfToHitCtx`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BfShot {
    pub range: BfRange,
    pub kind: BfAttackKind,
    pub attacker_move: BfMove,
    /// Attacker is a grounded aerospace element making a ground-to-ground weapon attack (p.46;
    /// hand-toggle): fighters/CF +2; grounded DropShips −2 (OQ 1, DECIDED: prose p.46 + advanced
    /// table p.85 over the standard table's −1 — unreachable in the baked catalog, the constant
    /// is documented either way).
    pub grounded: bool,
    /// Area-effect attack: +1.
    pub area_effect: bool,
    /// Secondary target: +1.
    pub secondary: bool,
    /// Attacker is also spotting for indirect fire this turn: +1.
    pub also_spotting: bool,
    // ---- target side (hand-entered) ----
    pub target_tmm: u8,
    pub target_move: BfTargetMove,
    /// ±JMPS/JMPW (jumped) or ±SUBS/SUBW (submersible) adjustment to the target-move row.
    pub target_move_adj: i32,
    /// Immobile (shutdown counts, p.49): flat −4 overriding TMM and the move row (p.39 table,
    /// p.86 fn13). Bombing ignores this row (p.47).
    pub target_immobile: bool,
    pub target_kind: BfTargetKind,
    /// Woods: +1.
    pub target_woods: bool,
    /// Partial cover: +1.
    pub target_partial_cover: bool,
    /// Underwater: +1 — legal only when the attacker is also submerged (fn2); the modal sets the
    /// toggle only in that case.
    pub target_underwater: bool,
    /// Target's STL is active: +0/+1/+2 by S/M/L bracket (BA targets +1/+1/+2), fn11.
    pub target_stealth: bool,
    /// Target has MAS: +3 if it stood still (spec §Data-fidelity 5).
    pub target_mas: bool,
    /// Target has LMAS: +2 if it stood still (spec §Data-fidelity 5).
    pub target_lmas: bool,
    /// Physical attack against a target carrying Battle Armor: +3 (p.39 physical rows).
    pub target_carrying_ba: bool,
}

/// The BF to-hit target number (p.39; second printings p.345 and p.86): "The Base To-Hit number
/// for all attacks is the attacking Element's Skill Rating"; hit on 2D6 ≥ TN. LOS and range are
/// Unit-to-Unit at the table, but "the to-hit number is calculated for each Element
/// individually". **No floor** — BF states none (OQ 4); AS mode's floor-2 is an AS:CE rule and
/// the divergence is deliberate (OQ 3 records the same for attacker standstill −1).
///
/// Physical attacks exclude heat, FC crits and SHLD (fns 6–8); infantry/BA are exempt from the
/// attacker standstill/jump rows (fn1); bombing excludes the target-immobile and terrain rows
/// (p.47). Physical rows use the standard chapter's Charge +1 / DFA +1 (OQ 2, DECIDED; the
/// advanced table's +2/+3 is unexplained). `heat` and `fc_crits` are the live attacker state;
/// `skill` is the tracked gunnery.
pub fn bf_to_hit(el: &AsElement, skill: u8, heat: u8, fc_crits: u8, shot: &BfShot) -> i32 {
    let physical = matches!(shot.kind, BfAttackKind::Physical(_));
    let weapon = !physical; // fns 6–8: heat/FC/SHLD bite weapon attacks only
    let bombing = matches!(
        shot.kind,
        BfAttackKind::AirToGround(BfA2G::AltitudeBombing | BfA2G::DiveBombing)
    );
    let infantry = matches!(el.sbf_type, SbfElementType::Ba | SbfElementType::Ci);
    let target_airborne = matches!(
        shot.target_kind,
        BfTargetKind::AirborneAero(_)
            | BfTargetKind::AirborneDropship
            | BfTargetKind::AirborneVtolWige
    );

    let mut tn = i32::from(skill);

    // Attacker movement (fn1: infantry/BA exempt from standstill and jumping).
    tn += match shot.attacker_move {
        BfMove::StoodStill if !infantry => -1,
        BfMove::Jumped if !infantry => 2,
        _ => 0,
    };

    // Attacker state — derived from the element (the table's ✻ rows).
    if weapon {
        tn += i32::from(heat); // fn8: heat level, weapon attacks only
        tn += 2 * i32::from(fc_crits); // fn7: +2 per Fire Control crit
        if el.has_sua("SHLD") {
            tn += 1; // fn6: SHLD penalises the bearer's own weapon attacks
        }
    }
    if el.as_type == "SV" {
        // Support-vehicle fire control: AFC +0, BFC +1, neither +2.
        if el.has_sua("BFC") {
            tn += 1;
        } else if !el.has_sua("AFC") {
            tn += 2;
        }
    }
    if el.as_type == "IM" && !el.has_sua("AFC") {
        tn += 1; // IndustrialMech without advanced fire control
    }
    if shot.grounded && weapon && !target_airborne {
        // Grounded aerospace, ground-to-ground weapon attack (p.46): fighters/CF +2; grounded
        // DropShips −2 (OQ 1, DECIDED).
        tn += if matches!(el.as_type.as_str(), "DS" | "DA") { -2 } else { 2 };
    }

    // Attack rows.
    match shot.kind {
        BfAttackKind::Standard => {}
        BfAttackKind::Indirect { spotter_also_attacked, spotter_is_remote_sensor } => {
            tn += if spotter_also_attacked { 2 } else { 1 };
            if spotter_is_remote_sensor {
                tn += 3; // fn4: an additional +3
            }
        }
        BfAttackKind::RearWeapons => tn += 1,
        BfAttackKind::Physical(p) => {
            // Standard-chapter rows (OQ 2, DECIDED): Standard/Melee +0, Charge/DFA/Anti-'Mech +1.
            tn += match p {
                BfPhysical::Standard | BfPhysical::Melee => 0,
                BfPhysical::Charge | BfPhysical::Dfa | BfPhysical::AntiMech => 1,
            };
            if el.sbf_type == SbfElementType::Ci {
                // Conventional-infantry attacker +3; with the anti-'Mech +1 this is the advanced
                // table's flat +4 (p.85) and the specialty-infantry text (p.124).
                tn += 3;
            }
            if shot.target_carrying_ba {
                tn += 3;
            }
            if el.has_sua("I-TSM") {
                tn += 2; // I-TSM: +2 TN on physicals (spec §1.5, pp.149/151/154)
            }
        }
        BfAttackKind::AirToGround(a) => {
            tn += match a {
                BfA2G::AltitudeBombing => 3,
                BfA2G::DiveBombing | BfA2G::Strafing | BfA2G::Striking => 2,
            };
        }
    }
    tn += i32::from(shot.area_effect);
    tn += i32::from(shot.secondary);
    tn += i32::from(shot.also_spotting);

    // Range: S +0 / M +2 / L +4 / E +6.
    tn += match shot.range {
        BfRange::Short => 0,
        BfRange::Medium => 2,
        BfRange::Long => 4,
        BfRange::Extreme => 6,
    };

    // Target movement. Immobile (incl. shutdown) is a flat −4 overriding TMM (p.39, p.49, p.86
    // fn13) — except bombing, which ignores the immobile row (p.47).
    if shot.target_immobile {
        if !bombing {
            tn -= 4;
        }
    } else {
        tn += match shot.target_move {
            BfTargetMove::StoodStill => 0,
            BfTargetMove::Ground => i32::from(shot.target_tmm),
            BfTargetMove::Jumped | BfTargetMove::Submersible => {
                i32::from(shot.target_tmm) + 1 + shot.target_move_adj
            }
            BfTargetMove::Dropped => 3,
        };
    }
    // MAS +3 / LMAS +2 against a target that is "immobile or remained at a standstill" — weapon
    // attacks only ("but not physical attacks", p.148; spec §Data-fidelity 5).
    if weapon && (shot.target_immobile || shot.target_move == BfTargetMove::StoodStill) {
        if shot.target_mas {
            tn += 3;
        } else if shot.target_lmas {
            tn += 2;
        }
    }

    // Target type.
    tn += match shot.target_kind {
        BfTargetKind::None => 0,
        BfTargetKind::BattleArmor | BfTargetKind::ProtoMech | BfTargetKind::AirborneVtolWige => 1,
        BfTargetKind::Large => -1,
        BfTargetKind::AirborneAero(angle) => match angle {
            BfAeroAngle::Nose => 1,
            BfAeroAngle::Side => 2,
            BfAeroAngle::Aft => 0,
        },
        BfTargetKind::AirborneDropship => -2,
    };
    // STL active (fn11): +0/+1/+2 by S/M/L; BA targets +1/+1/+2 — weapon attacks only ("make a
    // target more difficult to hit with weapon attacks (but not physical attacks)", p.151).
    // Extreme reads as Long (the standard bracket set has no E row; ground E is the Advanced
    // bracket).
    if weapon && shot.target_stealth {
        tn += match shot.range {
            BfRange::Short => i32::from(matches!(shot.target_kind, BfTargetKind::BattleArmor)),
            BfRange::Medium => 1,
            BfRange::Long | BfRange::Extreme => 2,
        };
    }
    // Ground-to-air Flak: "Applies for ground-to-air attacks against airborne aerospace, VTOL
    // and WiGE targets only" (p.85, fn6 on p.86): FLK attacker, Standard weapon attack vs an
    // airborne target: −2. Ground-to-air is DERIVED, not hand-entered: a non-aero attacker is
    // always ground-based; an aero attacker is ground-based only when its grounded toggle is on
    // (p.46). Standard attacks only — "REAR attacks cannot make use of other special attack
    // abilities, such as heat, indirect fire, flak, or artillery" (p.152), and IF/physical/A2G
    // kinds are not ground-to-air gunnery. The miss-by-≤2 FLK consolation damage (p.148) is the
    // modal's note, not a TN term.
    if weapon
        && matches!(shot.kind, BfAttackKind::Standard)
        && target_airborne
        && el.has_sua("FLK")
        && (!bf_is_aero(el) || shot.grounded)
    {
        tn -= 2;
    }

    // Terrain (bombing ignores target-hex terrain, p.47).
    if !bombing {
        tn += i32::from(shot.target_woods);
        tn += i32::from(shot.target_partial_cover);
        tn += i32::from(shot.target_underwater); // fn2: attacker also submerged
    }

    tn // no floor (OQ 4)
}

// ============================ §1.4 The crit table ============================

/// Crit-table column (p.42 = the p.87 "Expanded" table — the same table for every column
/// neurohelmet can field; p.87 adds only a JumpShip column, omitted as unreachable and unused in
/// standard play — spec §Data-fidelity 7).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BfCritCol {
    Mech,
    ProtoMech,
    Vehicle,
    Aerospace,
    DropShip,
}

/// One result on the type-columned crit table (p.42). Effect semantics (§1.4) land in the
/// Phase-2 `apply_crit`; "The effects of Critical Hits are permanent" (p.42). A result that does
/// not apply to the element, or a once-per-element crit rolled again, is **+1 damage instead, no
/// chained crit roll** (p.42).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BfCrit {
    NoCrit,
    Ammo,
    Engine,
    FireControl,
    Mp,
    Weapon,
    CrewStunned,
    CrewKilled,
    Fuel,
    HeadBlownOff,
    ProtoDestroyed,
    KfBoom,
    DockingCollar,
    Thruster,
    Door,
    CrewHit,
}

/// The column an element rolls on (p.42): BM/IM → 'Mech (IndustrialMechs roll TWICE and apply
/// both, p.42 — [`bf_crit_rolls`]); PM → ProtoMech; CV, ground SV, BD (neurohelmet-local gun
/// emplacement, weapons-only crit vocabulary — spec §Data-fidelity 8) and MS → Vehicle; AF/CF
/// and fixed-wing SV → Aerospace; Small Craft and the DS family → DropShip (both printings
/// footnote the DropShips column "Includes Small Craft" — p.42 ‡, p.87 ‡ — and the p.43 Engine
/// text gives DropShip/Small Craft the 3-stage −25%/50%/shutdown ladder, not the fighter one;
/// mark-only until small craft are baked, spec §Data-fidelity 7). `None` = infantry and BA,
/// which never take crits (p.42).
pub fn bf_crit_col(el: &AsElement) -> Option<BfCritCol> {
    match el.as_type.as_str() {
        "BM" | "IM" => Some(BfCritCol::Mech),
        "PM" => Some(BfCritCol::ProtoMech),
        "BA" | "CI" => None,
        "AF" | "CF" => Some(BfCritCol::Aerospace),
        "SC" | "DS" | "DA" | "JS" | "WS" | "SS" => Some(BfCritCol::DropShip),
        _ if bf_is_aero(el) => Some(BfCritCol::Aerospace), // fixed-wing SV
        _ => Some(BfCritCol::Vehicle),                     // CV, ground SV, BD, MS
    }
}

/// Crit rolls owed per crit chance (p.42): IndustrialMechs roll TWICE and apply both results;
/// everything else rolls once. Takes the element, not the column — IM-ness collapses into
/// [`BfCritCol::Mech`].
pub fn bf_crit_rolls(el: &AsElement) -> u8 {
    if el.as_type == "IM" {
        2
    } else {
        1
    }
}

/// The defender's own crit-roll modifier — a dim reference in the crit modal, since the roll is
/// against YOUR element: CR −2 (p.145), IRA +1 (p.148), RFA +2 (p.152). ARM is not a roll
/// modifier: it ignores the FIRST crit chance of the scenario outright — a spent-checkbox
/// (`arm_spent`, p.143).
pub fn bf_crit_roll_mod(el: &AsElement) -> i32 {
    let mut m = 0;
    if el.has_sua("CR") {
        m -= 2;
    }
    if el.has_sua("IRA") {
        m += 1;
    }
    if el.has_sua("RFA") {
        m += 2;
    }
    m
}

/// Read the crit table (p.42 = p.87; second printings pp.345/350) for a MODIFIED 2D6 total.
/// Defender modifiers ([`bf_crit_roll_mod`]) can push the total off the printed 2–12 rows: a
/// modified total ≤ 1 is No Critical Hit (CR, p.145) and ≥ 13 is an Engine Hit (IRA "modified
/// >12", p.148; RFA "13+", p.152) — encoded here so the caller passes the modified total as-is.
pub fn bf_crit(roll_2d6: i32, col: BfCritCol) -> BfCrit {
    use BfCrit::*;
    if roll_2d6 <= 1 {
        return NoCrit;
    }
    if roll_2d6 >= 13 {
        return Engine;
    }
    match col {
        BfCritCol::Mech => match roll_2d6 {
            2 => Ammo,
            3 | 11 => Engine,
            4 | 10 => FireControl,
            5 | 9 => NoCrit,
            6 | 8 => Weapon,
            7 => Mp,
            _ => HeadBlownOff, // 12
        },
        BfCritCol::ProtoMech => match roll_2d6 {
            2 | 3 | 11 | 12 => Weapon,
            4 => FireControl,
            5 | 7 | 9 => Mp,
            10 => ProtoDestroyed,
            _ => NoCrit, // 6, 8
        },
        BfCritCol::Vehicle => match roll_2d6 {
            2 => Ammo,
            3 => CrewStunned,
            4 | 5 => FireControl,
            9 | 10 => Weapon,
            11 => CrewKilled,
            12 => Engine,
            _ => NoCrit, // 6–8
        },
        BfCritCol::Aerospace => match roll_2d6 {
            2 => Fuel,
            3 | 11 => FireControl,
            4 | 10 => Engine,
            5 | 9 => Weapon,
            12 => CrewKilled,
            _ => NoCrit, // 6–8
        },
        BfCritCol::DropShip => match roll_2d6 {
            2 => KfBoom,
            3 => DockingCollar,
            5 => FireControl,
            6 | 8 => Weapon,
            7 => Thruster,
            9 => Door,
            11 => Engine,
            12 => CrewHit,
            _ => NoCrit, // 4, 10
        },
    }
}

// ============================ §1.5 Physical attacks & air-to-ground readouts ============================

/// Physical-attack eligibility by element type (pp.43–45; the modal greys ineligible picks):
/// 'Mechs (BM/IM) may use Standard, Melee and the Specials (Charge/DFA) — but MEL elements may
/// not choose Standard instead of their melee weapon (p.44), non-MEL 'Mechs have no melee
/// weapon, and DFA requires jump capability (p.45). ProtoMechs: Standard only. Vehicles (incl.
/// BD/MS): Charge only. Anti-'Mech requires an infantry element with the Anti-'Mech special —
/// "Infantry Elements with the Anti-'Mech (AM) special ability can make a special attack"
/// (p.143): BA carry the capability innately, conventional infantry must bake `AM` — and it is
/// infantry's only physical. Aerospace elements make none. (DFA also may not target airborne
/// aerospace — sanitized by `bf_shot_for` and warned in the modal, p.45.)
pub fn bf_physical_eligible(kind: BfPhysical, el: &AsElement) -> bool {
    if bf_is_aero(el) {
        return false;
    }
    match el.as_type.as_str() {
        "BM" | "IM" => match kind {
            BfPhysical::Standard => !el.has_sua("MEL"),
            BfPhysical::Melee => el.has_sua("MEL"),
            BfPhysical::Charge => true,
            BfPhysical::Dfa => el.jump_move > 0,
            BfPhysical::AntiMech => false,
        },
        "PM" => matches!(kind, BfPhysical::Standard),
        "CV" | "SV" | "BD" | "MS" => matches!(kind, BfPhysical::Charge),
        "BA" => matches!(kind, BfPhysical::AntiMech),
        "CI" => matches!(kind, BfPhysical::AntiMech) && el.has_sua("AM"),
        _ => false,
    }
}

/// Physical-attack damage readout (pp.43–45). Physical damage is never overheat-boosted (p.49)
/// and Weapon crits do not reduce it (p.43).
///
/// - Standard / Melee: attacker Size; MEL +1 (p.44); TSM (at heat ≥ 1) / TSMX / I-TSM each +1
///   (pp.149/151/154; I-TSM's +2 TN lives in [`bf_to_hit`]). `heat` feeds the TSM condition —
///   the spec's §1.5 signature omitted it; added because the TSM bonus is heat-conditional.
/// - Charge: available MV × size multiplier, round normally; ENG/SAW round up ([`charge` helper]);
///   the attacker-side recoil (target-Size damage to self, vehicle motive roll) is a modal note.
/// - DFA: charge damage + 1 (p.45); `available_mp` is the MP spent — the modal passes jump MP.
///   Attacker takes own Size (Size + 1 on a miss); one crit roll vs the target regardless of
///   structure damage, plus one more if structure was damaged — modal notes.
/// - Anti-'Mech: the element's normal (Short) damage, + one crit roll on success (p.124).
pub fn bf_physical_damage(kind: BfPhysical, el: &AsElement, available_mp: u32, heat: u8) -> f32 {
    match kind {
        BfPhysical::Standard | BfPhysical::Melee => {
            let mut dmg = f32::from(el.size);
            if el.has_sua("MEL") {
                dmg += 1.0;
            }
            if el.has_sua("TSM") && heat >= 1 {
                dmg += 1.0;
            }
            if el.has_sua("TSMX") {
                dmg += 1.0;
            }
            if el.has_sua("I-TSM") {
                dmg += 1.0;
            }
            dmg
        }
        BfPhysical::Charge => charge_damage(el, available_mp),
        BfPhysical::Dfa => charge_damage(el, available_mp) + 1.0,
        BfPhysical::AntiMech => el.std_damage.s,
    }
}

/// Charge damage (p.44): available MV × the size multiplier (Size 1/2/3/4 → ×0.25/0.50/0.75/1.0;
/// larger clamps to ×1.0), round normally ([`jround`]); ENG/SAW vehicles round up instead.
fn charge_damage(el: &AsElement, available_mp: u32) -> f32 {
    let mult = f32::from(el.size.min(4)) * 0.25;
    let raw = available_mp as f32 * mult;
    if el.has_any_sua(&["ENG", "SAW"]) {
        raw.ceil()
    } else {
        jround(f64::from(raw)) as f32
    }
}

/// Strafing damage (p.47): half the S value, round normally ([`jround`]), minimum 1 — the
/// overheat commit and the rear +1 (p.41) are added BEFORE halving. Hits every element in the
/// strafed hexes; resolves at Short range (p.47). Weapon crits reduce the S value first (p.43:
/// −1 to all damage values).
pub fn bf_strafe_damage(el: &AsElement, weapon_crits: u8, ov_commit: u8, rear: bool) -> f32 {
    let s = (el.std_damage.s - f32::from(weapon_crits)).max(0.0);
    let full = s + f32::from(ov_commit) + f32::from(rear);
    (jround(f64::from(full) / 2.0) as f32).max(1.0)
}

/// Striking damage (pp.47–48): the S value + the overheat commit, +1 if the attack strikes the
/// rear (p.41); resolves at Short range (p.47). Weapon crits reduce the S value first (p.43).
pub fn bf_strike_damage(el: &AsElement, weapon_crits: u8, ov_commit: u8, rear: bool) -> f32 {
    (el.std_damage.s - f32::from(weapon_crits)).max(0.0)
        + f32::from(ov_commit)
        + f32::from(rear)
}

/// HE/Cluster bomb damage to every element in the bombed hex, per bomb (p.48).
pub const BF_BOMB_DAMAGE_PER_BOMB: f32 = 2.0;
/// Inferno bomb: heat added to 'Mechs and landed fighters (non-stacking, p.48).
pub const BF_INFERNO_BOMB_HEAT: u8 = 2;
/// Inferno bomb: damage to ProtoMechs/BA (p.48). Non-BA infantry is destroyed outright;
/// DropShips are unaffected.
pub const BF_INFERNO_BOMB_DAMAGE_PM_BA: f32 = 2.0;

/// Total HE/Cluster bombing damage to each element in the hex: 2 × bombs dropped (p.48).
/// "Bombing attacks never strike a Unit from the rear" (p.48); bombing resolves at Short range
/// (p.47). Each carried bomb is −1 TP while loaded (p.30) — the BOMB-carrier badge.
pub fn bf_bomb_damage(bombs: u32) -> f32 {
    bombs as f32 * BF_BOMB_DAMAGE_PER_BOMB
}

// ============================ §1.6 Overheat commitment ============================

/// Maximum overheat a shot can commit at declaration (pp.48–49): 1..=min(OV, heat room). Heat
/// gained = the amount used (−1 if in water, p.49 — table concern), and the heat scale tops out
/// at 4 ("1 2 3 S", p.26), so the room is `4 − heat`. Voluntary +1 heat beyond OV is legal
/// (p.49) but is the manual heat key's business, not the commit bound; HT-inflicted heat caps at
/// +2/turn (p.49).
pub fn bf_max_ov_commit(el: &AsElement, heat: u8) -> u8 {
    el.overheat.min(4u8.saturating_sub(heat))
}

/// Whether committed overheat applies at this bracket: "at all range brackets for which it has a
/// damage value" (p.49), but elements without OVL apply overheat at Short/Medium ONLY (p.151);
/// OVL extends it to Long (and the aerospace Extreme band, by the same p.49 reading).
pub fn bf_ov_applies(el: &AsElement, range: BfRange) -> bool {
    matches!(range, BfRange::Short | BfRange::Medium) || el.has_sua("OVL")
}

/// The vehicle Engine-crit damage halving (p.43: 1st hit — all damage values × 0.5, round down,
/// min 0), applied to an already-weapon-crit-reduced value, BEFORE any overheat add. Derived
/// live from the persisted hit count on the Vehicle crit column only; every other column passes
/// through. `None` = halved to nothing (no attack).
fn bf_engine_halved(el: &AsElement, engine_hits: u8, v: f32) -> Option<f32> {
    if engine_hits >= 1 && bf_crit_col(el) == Some(BfCritCol::Vehicle) {
        let h = (v / 2.0).floor();
        return if h > 0.0 { Some(h) } else { None };
    }
    Some(v)
}

/// Weapon-attack damage readout with overheat committed (§1.6): the [`bf_damage`] value, through
/// the vehicle Engine-crit halving (p.43 — after the Weapon-crit subtraction, before the OV
/// add), plus the commit where it applies ([`bf_ov_applies`]) — never on REAR (p.152), IF, or
/// physical attacks (p.49), which have their own readouts. `None` = no damage value at this
/// bracket; overheat cannot create an attack where the card has none (p.49). Same-turn REAR +
/// forward fire reduces the forward damage 1-for-1 per point of REAR damage dealt, BEFORE
/// overheat (p.152) — the shot modal composes that reduced forward line.
pub fn bf_shot_damage(
    el: &AsElement,
    range: BfRange,
    weapon_crits: u8,
    ov_commit: u8,
    engine_hits: u8,
) -> Option<f32> {
    let base = bf_engine_halved(el, engine_hits, bf_damage(el, range, weapon_crits)?)?;
    if ov_commit > 0 && bf_ov_applies(el, range) {
        Some(base + f32::from(ov_commit))
    } else {
        Some(base)
    }
}

/// REAR-weapons attack damage (pp.151–152): the element's `REAR#/#/#(/#)` values — not the card
/// S/M/L — at +1 TN (§1.3), reduced by Weapon crits like every damage value (p.43), through the
/// vehicle Engine-crit halving (p.43 halves "all Damage Values"), never overheat-boosted
/// (p.152). `None` = no REAR ability or no value at this bracket. Heat gained = the REAR fire
/// still generates weapon heat normally (table concern).
pub fn bf_rear_damage(
    el: &AsElement,
    range: BfRange,
    weapon_crits: u8,
    engine_hits: u8,
) -> Option<f32> {
    let d = el.sua_dmg("REAR")?;
    let raw = match range {
        BfRange::Short => d.s,
        BfRange::Medium => d.m,
        BfRange::Long => d.l.unwrap_or(0.0),
        BfRange::Extreme => d.e.unwrap_or(0.0),
    };
    let v = (raw - f32::from(weapon_crits)).max(0.0);
    if v > 0.0 {
        bf_engine_halved(el, engine_hits, v)
    } else {
        None
    }
}

/// Indirect-fire damage: the IF# value regardless of bracket (spec §Data-fidelity 5), reduced by
/// Weapon crits (p.43 lists IF among the damage-bearing specials), through the vehicle
/// Engine-crit halving (p.43), never overheat-boosted (p.49). `None` = no IF ability or reduced
/// to nothing; `Some(0.5)` = the `IF0*` minimal value.
pub fn bf_indirect_damage(el: &AsElement, weapon_crits: u8, engine_hits: u8) -> Option<f32> {
    if !el.has_sua("IF") {
        return None;
    }
    let v = (el.sua_num("IF") - f32::from(weapon_crits)).max(0.0);
    if v > 0.0 {
        bf_engine_halved(el, engine_hits, v)
    } else {
        None
    }
}

// ============================ §1.7 Unit (lance) derived stats ============================

/// Live Unit movement (p.52): "A Unit's MP always equals the lowest MP of any of its dismounted
/// surviving Elements" (mounted-passenger exclusion is untracked transport state, spec §Scope
/// 2). Ground and jump minima are computed separately (the p.52 5-MP-ground / 3-MP-jump
/// example), and the Unit "is considered jump-capable (j) only if all surviving Elements … have
/// Jumping MP". Members are `(current ground MP, current jump MP, alive)`; recompute on every
/// heat change, MP/engine/motive crit, destruction, or membership change ("Players must
/// recalculate a Unit's MP during play", p.52 — the recalculation this mode automates). An
/// empty or fully-destroyed Unit is `(0, None)`. A shutdown member pins the Unit instead — a
/// badge, not an MP change (p.49).
pub fn bf_unit_mv(members: &[(u32, Option<u32>, bool)]) -> (u32, Option<u32>) {
    let mut ground_min: Option<u32> = None;
    let mut jump_min: Option<u32> = None;
    let mut all_jump = true;
    for (g, j, _) in members.iter().filter(|(_, _, alive)| *alive) {
        ground_min = Some(ground_min.map_or(*g, |m| m.min(*g)));
        match j {
            Some(jv) => jump_min = Some(jump_min.map_or(*jv, |m| m.min(*jv))),
            None => all_jump = false,
        }
    }
    match ground_min {
        Some(g) => (g, if all_jump { jump_min } else { None }),
        None => (0, None),
    }
}

/// Static Unit Size (p.53): sum ÷ count, round normally ([`jround`]) — "determined at the start
/// of play, and is not adjusted for destroyed Elements". Stamped at grouping time, restamped
/// only on membership edits. Empty → 0.
pub fn bf_unit_size(sizes: &[u8]) -> i64 {
    if sizes.is_empty() {
        return 0;
    }
    jround(sizes.iter().map(|&s| f64::from(s)).sum::<f64>() / sizes.len() as f64)
}

// ============================ Tests ============================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::as_element::{sbf_type_from_tp, DamageVector, SuaVal};
    use std::collections::BTreeMap;

    /// A base element of the given AS type; tests tweak fields via struct-update.
    fn el(tp: &str) -> AsElement {
        AsElement {
            name: "Test".into(),
            as_type: tp.into(),
            sbf_type: sbf_type_from_tp(tp),
            size: 2,
            primary_move: 8, // inches
            primary_mode: String::new(),
            jump_move: 0,
            skill: 4,
            full_armor: 6,
            full_structure: 4,
            std_damage: DamageVector { s: 3.0, m: 3.0, l: Some(2.0), e: Some(0.0) },
            overheat: 0,
            threshold: 0,
            suas: BTreeMap::new(),
            turret_suas: BTreeMap::new(),
            base_pv: 30,
            arcs: None,
        }
    }

    fn with_sua(mut e: AsElement, code: &str) -> AsElement {
        e.suas.insert(code.into(), SuaVal::Flag);
        e
    }

    /// Baseline hand-entered shot at Short range: every other field is the +0 default.
    fn shot() -> BfShot {
        BfShot { range: BfRange::Short, ..Default::default() }
    }

    // Motive-flag shorthands (§1.2): none / −1 MV / ½ MV / immobile.
    const M_NONE: BfMotive = BfMotive { minus_one: false, half: false, immobile: false };
    const M_MINUS1: BfMotive = BfMotive { minus_one: true, half: false, immobile: false };
    const M_HALF: BfMotive = BfMotive { minus_one: false, half: true, immobile: false };
    const M_IMM: BfMotive = BfMotive { minus_one: false, half: false, immobile: true };

    /// [`bf_current_mp`] with the ground-'Mech default engine legs (no engine crits, no column).
    fn cur_mp(base: u32, heat: u8, mp_lost: u32, motive: BfMotive, tsm: bool) -> u32 {
        bf_current_mp(base, heat, mp_lost, motive, tsm, 0, None)
    }

    fn th(e: &AsElement, skill: u8, s: &BfShot) -> i32 {
        bf_to_hit(e, skill, 0, 0, s)
    }

    // ---- §1.3: the p.40 worked examples ----

    #[test]
    fn to_hit_worked_examples_p40() {
        let bm = el("BM");
        // Skill 3, medium range: 3 + 2 = 5.
        assert_eq!(th(&bm, 3, &BfShot { range: BfRange::Medium, ..shot() }), 5);
        // Skill 3, long range, TMM 2, Large target: 3 + 4 + 2 − 1 = 8.
        assert_eq!(
            th(
                &bm,
                3,
                &BfShot {
                    range: BfRange::Long,
                    target_tmm: 2,
                    target_kind: BfTargetKind::Large,
                    ..shot()
                }
            ),
            8
        );
        // Skill 3, short, TMM 2 + jumped (+3), target in water (+1): 3 + 3 + 1 = 7.
        assert_eq!(
            th(
                &bm,
                3,
                &BfShot {
                    target_tmm: 2,
                    target_move: BfTargetMove::Jumped,
                    target_underwater: true,
                    ..shot()
                }
            ),
            7
        );
    }

    #[test]
    fn to_hit_attacker_movement_and_infantry_exemption() {
        let bm = el("BM");
        assert_eq!(th(&bm, 4, &BfShot { attacker_move: BfMove::StoodStill, ..shot() }), 3);
        assert_eq!(th(&bm, 4, &BfShot { attacker_move: BfMove::Moved, ..shot() }), 4);
        assert_eq!(th(&bm, 4, &BfShot { attacker_move: BfMove::Jumped, ..shot() }), 6);
        // fn1: infantry/BA exempt from standstill and jumping.
        for tp in ["CI", "BA"] {
            let inf = el(tp);
            assert_eq!(th(&inf, 4, &BfShot { attacker_move: BfMove::StoodStill, ..shot() }), 4);
            assert_eq!(th(&inf, 4, &BfShot { attacker_move: BfMove::Jumped, ..shot() }), 4);
        }
    }

    #[test]
    fn to_hit_attacker_state_rows() {
        // Heat is linear, FC crits are +2 each, SHLD +1 — weapon attacks only (tested here on a
        // standard weapon attack; the physical exclusion is its own test).
        let bm = el("BM");
        assert_eq!(bf_to_hit(&bm, 4, 3, 0, &shot()), 7);
        assert_eq!(bf_to_hit(&bm, 4, 0, 2, &shot()), 8);
        assert_eq!(th(&with_sua(el("BM"), "SHLD"), 4, &shot()), 5);
        // Support-vehicle fire control: neither +2, BFC +1, AFC +0.
        assert_eq!(th(&el("SV"), 4, &shot()), 6);
        assert_eq!(th(&with_sua(el("SV"), "BFC"), 4, &shot()), 5);
        assert_eq!(th(&with_sua(el("SV"), "AFC"), 4, &shot()), 4);
        // IndustrialMech without AFC +1 (with AFC +0).
        assert_eq!(th(&el("IM"), 4, &shot()), 5);
        assert_eq!(th(&with_sua(el("IM"), "AFC"), 4, &shot()), 4);
        // Grounded fighter, ground-to-ground weapon attack: +2. Not applied when the target is
        // airborne (that is ground-to-air), and the DECIDED grounded-DropShip constant is −2 (OQ 1).
        assert_eq!(th(&el("AF"), 4, &BfShot { grounded: true, ..shot() }), 6);
        assert_eq!(
            th(
                &el("AF"),
                4,
                &BfShot {
                    grounded: true,
                    target_kind: BfTargetKind::AirborneAero(BfAeroAngle::Side),
                    ..shot()
                }
            ),
            6 // +2 airborne-side only; no grounded +2
        );
        assert_eq!(th(&el("DS"), 4, &BfShot { grounded: true, ..shot() }), 2);
    }

    #[test]
    fn to_hit_attack_rows() {
        let bm = el("BM");
        let indirect = |spot, remote| BfAttackKind::Indirect {
            spotter_also_attacked: spot,
            spotter_is_remote_sensor: remote,
        };
        assert_eq!(th(&bm, 4, &BfShot { kind: indirect(false, false), ..shot() }), 5);
        assert_eq!(th(&bm, 4, &BfShot { kind: indirect(true, false), ..shot() }), 6);
        assert_eq!(th(&bm, 4, &BfShot { kind: indirect(false, true), ..shot() }), 8); // +1 +3
        assert_eq!(th(&bm, 4, &BfShot { kind: BfAttackKind::RearWeapons, ..shot() }), 5);
        assert_eq!(th(&bm, 4, &BfShot { area_effect: true, ..shot() }), 5);
        assert_eq!(th(&bm, 4, &BfShot { secondary: true, ..shot() }), 5);
        assert_eq!(th(&bm, 4, &BfShot { also_spotting: true, ..shot() }), 5);
        // Range ladder: S +0 / M +2 / L +4 / E +6.
        assert_eq!(th(&bm, 4, &BfShot { range: BfRange::Short, ..shot() }), 4);
        assert_eq!(th(&bm, 4, &BfShot { range: BfRange::Medium, ..shot() }), 6);
        assert_eq!(th(&bm, 4, &BfShot { range: BfRange::Long, ..shot() }), 8);
        assert_eq!(th(&bm, 4, &BfShot { range: BfRange::Extreme, ..shot() }), 10);
    }

    #[test]
    fn to_hit_physical_rows_and_exclusions() {
        // fns 6–8: heat, FC crits and SHLD are ignored on physical attacks.
        let shld = with_sua(el("BM"), "SHLD");
        let phys = BfShot { kind: BfAttackKind::Physical(BfPhysical::Standard), ..shot() };
        assert_eq!(bf_to_hit(&shld, 4, 3, 2, &phys), 4);
        assert_eq!(bf_to_hit(&shld, 4, 3, 2, &shot()), 12); // same state, weapon attack

        // Standard-chapter physical rows (OQ 2): Standard/Melee +0, Charge/DFA/Anti-'Mech +1.
        let bm = el("BM");
        let pk = |p| BfShot { kind: BfAttackKind::Physical(p), ..shot() };
        assert_eq!(th(&bm, 4, &pk(BfPhysical::Standard)), 4);
        assert_eq!(th(&bm, 4, &pk(BfPhysical::Melee)), 4);
        assert_eq!(th(&bm, 4, &pk(BfPhysical::Charge)), 5);
        assert_eq!(th(&bm, 4, &pk(BfPhysical::Dfa)), 5);
        // BA anti-'Mech = +1; conventional infantry = +1 anti-'Mech + 3 CI attacker = +4 (p.85, p.124).
        assert_eq!(th(&el("BA"), 4, &pk(BfPhysical::AntiMech)), 5);
        assert_eq!(th(&el("CI"), 4, &pk(BfPhysical::AntiMech)), 8);
        // Target carrying BA: +3, physicals only.
        assert_eq!(
            th(&bm, 4, &BfShot { target_carrying_ba: true, ..pk(BfPhysical::Charge) }),
            8
        );
        assert_eq!(th(&bm, 4, &BfShot { target_carrying_ba: true, ..shot() }), 4);
        // I-TSM: +2 TN on physicals only (spec §1.5).
        let itsm = with_sua(el("BM"), "I-TSM");
        assert_eq!(th(&itsm, 4, &pk(BfPhysical::Standard)), 6);
        assert_eq!(th(&itsm, 4, &shot()), 4);
    }

    #[test]
    fn to_hit_a2g_rows_and_bombing_exclusions() {
        let af = el("AF");
        let a2g = |a| BfShot { kind: BfAttackKind::AirToGround(a), ..shot() };
        assert_eq!(th(&af, 4, &a2g(BfA2G::AltitudeBombing)), 7);
        assert_eq!(th(&af, 4, &a2g(BfA2G::DiveBombing)), 6);
        assert_eq!(th(&af, 4, &a2g(BfA2G::Strafing)), 6);
        assert_eq!(th(&af, 4, &a2g(BfA2G::Striking)), 6);
        // Bombing excludes the immobile row and target-hex terrain (p.47)…
        let loaded = |a| BfShot {
            target_immobile: true,
            target_woods: true,
            target_partial_cover: true,
            ..a2g(a)
        };
        assert_eq!(th(&af, 4, &loaded(BfA2G::AltitudeBombing)), 7);
        assert_eq!(th(&af, 4, &loaded(BfA2G::DiveBombing)), 6);
        // …while strafing/striking keep both (−4 + 2 terrain).
        assert_eq!(th(&af, 4, &loaded(BfA2G::Strafing)), 4);
        assert_eq!(th(&af, 4, &loaded(BfA2G::Striking)), 4);
    }

    #[test]
    fn to_hit_target_movement_rows() {
        let bm = el("BM");
        let mv = |m, tmm| BfShot { target_move: m, target_tmm: tmm, ..shot() };
        assert_eq!(th(&bm, 4, &mv(BfTargetMove::StoodStill, 2)), 4); // standstill: +0, TMM ignored
        assert_eq!(th(&bm, 4, &mv(BfTargetMove::Ground, 2)), 6);
        assert_eq!(th(&bm, 4, &mv(BfTargetMove::Jumped, 2)), 7); // TMM + 1
        assert_eq!(th(&bm, 4, &mv(BfTargetMove::Submersible, 1)), 6); // TMM + 1
        // ±JMPS/JMPW / ±SUBS/SUBW adjust the jump/submersible rows.
        assert_eq!(
            th(&bm, 4, &BfShot { target_move_adj: -1, ..mv(BfTargetMove::Jumped, 2) }),
            6
        );
        assert_eq!(
            th(&bm, 4, &BfShot { target_move_adj: 1, ..mv(BfTargetMove::Submersible, 1) }),
            7
        );
        // Dropped by an airborne Unit: flat +3, no TMM.
        assert_eq!(th(&bm, 4, &mv(BfTargetMove::Dropped, 2)), 7);
        // Immobile: flat −4, overriding TMM and the move row.
        assert_eq!(
            th(&bm, 4, &BfShot { target_immobile: true, ..mv(BfTargetMove::Jumped, 3) }),
            0
        );
        // MAS +3 / LMAS +2 against a target that stood still OR is immobile — weapon attacks
        // only (p.148; spec §Data-fidelity 5).
        assert_eq!(
            th(&bm, 4, &BfShot { target_mas: true, ..mv(BfTargetMove::StoodStill, 0) }),
            7
        );
        assert_eq!(
            th(&bm, 4, &BfShot { target_lmas: true, ..mv(BfTargetMove::StoodStill, 0) }),
            6
        );
        assert_eq!(th(&bm, 4, &BfShot { target_mas: true, ..mv(BfTargetMove::Ground, 2) }), 6);
        // Immobile MAS target: −4 immobile + 3 MAS = 3 (the book's "immobile or remained at a
        // standstill", p.148).
        assert_eq!(
            th(&bm, 4, &BfShot { target_immobile: true, target_mas: true, ..shot() }),
            3
        );
        assert_eq!(
            th(&bm, 4, &BfShot { target_immobile: true, target_lmas: true, ..shot() }),
            2
        );
        // Physical attacks never take the MAS bonus ("but not physical attacks", p.148).
        let phys_mas = BfShot {
            kind: BfAttackKind::Physical(BfPhysical::Standard),
            target_mas: true,
            ..mv(BfTargetMove::StoodStill, 0)
        };
        assert_eq!(th(&bm, 4, &phys_mas), 4);
        assert_eq!(
            th(&bm, 4, &BfShot { target_immobile: true, ..phys_mas }),
            0 // −4 immobile only, no +3
        );
    }

    #[test]
    fn to_hit_stl_is_weapon_attacks_only() {
        // p.151: STL makes a target harder to hit "with weapon attacks (but not physical
        // attacks)" — a physical entered at any bracket takes no STL row.
        let bm = el("BM");
        let phys = |r| BfShot {
            kind: BfAttackKind::Physical(BfPhysical::Standard),
            range: r,
            target_stealth: true,
            ..shot()
        };
        assert_eq!(th(&bm, 4, &phys(BfRange::Medium)), 6); // +2 range only, no +1 STL
        assert_eq!(th(&bm, 4, &phys(BfRange::Long)), 8); // +4 range only, no +2 STL
        // BA-target stealth at Short is likewise weapon-only.
        let ba_phys = BfShot { target_kind: BfTargetKind::BattleArmor, ..phys(BfRange::Short) };
        assert_eq!(th(&bm, 4, &ba_phys), 5); // +1 BA type only, no +1 STL
    }

    #[test]
    fn to_hit_target_type_terrain_and_flak() {
        let bm = el("BM");
        let kind = |k| BfShot { target_kind: k, ..shot() };
        assert_eq!(th(&bm, 4, &kind(BfTargetKind::BattleArmor)), 5);
        assert_eq!(th(&bm, 4, &kind(BfTargetKind::ProtoMech)), 5);
        assert_eq!(th(&bm, 4, &kind(BfTargetKind::Large)), 3);
        // Airborne aerospace: the 3-way angle of attack (p.86 fn10) — side is the table default.
        assert_eq!(th(&bm, 4, &kind(BfTargetKind::AirborneAero(BfAeroAngle::Nose))), 5);
        assert_eq!(th(&bm, 4, &kind(BfTargetKind::AirborneAero(BfAeroAngle::Side))), 6);
        assert_eq!(th(&bm, 4, &kind(BfTargetKind::AirborneAero(BfAeroAngle::Aft))), 4);
        assert_eq!(th(&bm, 4, &kind(BfTargetKind::AirborneDropship)), 2);
        assert_eq!(th(&bm, 4, &kind(BfTargetKind::AirborneVtolWige)), 5);
        // STL (fn11): +0/+1/+2 by S/M/L (E reads as L); BA targets +1/+1/+2.
        let stl = |r, k| BfShot { range: r, target_stealth: true, target_kind: k, ..shot() };
        assert_eq!(th(&bm, 4, &stl(BfRange::Short, BfTargetKind::None)), 4);
        assert_eq!(th(&bm, 4, &stl(BfRange::Medium, BfTargetKind::None)), 7); // +2 range +1 STL
        assert_eq!(th(&bm, 4, &stl(BfRange::Long, BfTargetKind::None)), 10); // +4 range +2 STL
        assert_eq!(th(&bm, 4, &stl(BfRange::Extreme, BfTargetKind::None)), 12);
        assert_eq!(th(&bm, 4, &stl(BfRange::Short, BfTargetKind::BattleArmor)), 6); // +1 BA +1 STL
        // Ground-to-air Flak: FLK attacker, Standard weapon attack vs an airborne target: −2.
        let mut flk = el("BM");
        flk.suas.insert(
            "FLK".into(),
            SuaVal::Dmg(DamageVector { s: 1.0, m: 1.0, l: Some(1.0), e: None }),
        );
        assert_eq!(th(&flk, 4, &kind(BfTargetKind::AirborneVtolWige)), 3); // +1 − 2
        assert_eq!(th(&flk, 4, &shot()), 4); // no airborne target, no Flak row
        // Never on REAR attacks ("REAR attacks cannot make use of … flak", p.152).
        let mut rear_flk = flk.clone();
        rear_flk.suas.insert(
            "REAR".into(),
            SuaVal::Dmg(DamageVector { s: 1.0, m: 1.0, l: None, e: None }),
        );
        assert_eq!(
            th(
                &rear_flk,
                4,
                &BfShot {
                    kind: BfAttackKind::RearWeapons,
                    target_kind: BfTargetKind::AirborneVtolWige,
                    ..shot()
                }
            ),
            6 // +1 REAR +1 type, no −2
        );
        // Ground-to-air only (p.86 fn6): an airborne FLK fighter shooting air-to-air gets no
        // −2 — but the same fighter grounded (p.46 toggle) is ground-based and does.
        let mut flk_af = el("AF");
        flk_af.suas.insert(
            "FLK".into(),
            SuaVal::Dmg(DamageVector { s: 1.0, m: 1.0, l: Some(1.0), e: None }),
        );
        let vs_air = BfShot {
            target_kind: BfTargetKind::AirborneAero(BfAeroAngle::Aft),
            ..shot()
        };
        assert_eq!(th(&flk_af, 4, &vs_air), 4); // airborne attacker: +0 aft, no −2
        assert_eq!(
            th(&flk_af, 4, &BfShot { grounded: true, ..vs_air }),
            2 // grounded fighter, ground-to-air: −2 (no grounded +2 vs airborne, p.46)
        );
        // Terrain rows.
        assert_eq!(th(&bm, 4, &BfShot { target_woods: true, ..shot() }), 5);
        assert_eq!(th(&bm, 4, &BfShot { target_partial_cover: true, ..shot() }), 5);
        assert_eq!(th(&bm, 4, &BfShot { target_underwater: true, ..shot() }), 5);
    }

    #[test]
    fn to_hit_has_no_floor() {
        // OQ 4: BF states no minimum TN; a heavily-modified shot can land below 2 (AS mode's
        // floor stays in AS mode).
        let bm = el("BM");
        let tn = th(
            &bm,
            0,
            &BfShot {
                attacker_move: BfMove::StoodStill,
                target_immobile: true,
                target_kind: BfTargetKind::Large,
                ..shot()
            },
        );
        assert_eq!(tn, -6); // 0 − 1 − 4 − 1
    }

    // ---- §1.1: range labels & damage ----

    #[test]
    fn range_labels() {
        assert_eq!(bf_range_label(false), "S 0-1  M 2-4  L 5-8  E 9-10*");
        assert_eq!(bf_range_label(true), "S 0-32  M 33-64  L 65-107  E 108-133");
    }

    #[test]
    fn damage_ground_extreme_is_derived_and_aero_is_baked() {
        // Ground E = max(L − 1, 0), computed — never the baked dmg_e.
        let mut g = el("BM");
        g.std_damage = DamageVector { s: 3.0, m: 3.0, l: Some(2.0), e: Some(4.0) };
        assert_eq!(bf_damage(&g, BfRange::Extreme, 0), Some(1.0)); // L 2 → E 1, ignores baked 4
        g.std_damage.l = Some(0.5); // L 0* → E max(−0.5, 0) = 0 → no attack
        assert_eq!(bf_damage(&g, BfRange::Extreme, 0), None);
        // Aerospace E = the baked value.
        let mut a = el("AF");
        a.std_damage = DamageVector { s: 3.0, m: 3.0, l: Some(2.0), e: Some(1.0) };
        assert_eq!(bf_damage(&a, BfRange::Extreme, 0), Some(1.0));
        a.std_damage.e = Some(0.0);
        assert_eq!(bf_damage(&a, BfRange::Extreme, 0), None);
    }

    #[test]
    fn damage_weapon_crits_and_minimal() {
        let mut e = el("BM");
        e.std_damage = DamageVector { s: 3.0, m: 0.5, l: None, e: None };
        assert_eq!(bf_damage(&e, BfRange::Short, 0), Some(3.0));
        assert_eq!(bf_damage(&e, BfRange::Short, 1), Some(2.0)); // −1 per Weapon crit
        assert_eq!(bf_damage(&e, BfRange::Short, 3), None); // reduced to nothing
        assert_eq!(bf_damage(&e, BfRange::Medium, 0), Some(0.5)); // 0* minimal
        assert_eq!(bf_damage(&e, BfRange::Medium, 1), None); // minimal − 1 → 0
        assert_eq!(bf_damage(&e, BfRange::Long, 0), None); // printed dash
    }

    // ---- §1.2: MV, TMM, crits ----

    #[test]
    fn tmm_bracket_edges() {
        for (mp, want) in [
            (0, 0),
            (2, 0),
            (3, 1),
            (4, 1),
            (5, 2),
            (6, 2),
            (7, 3),
            (9, 3),
            (10, 4),
            (17, 4),
            (18, 5),
            (30, 5),
        ] {
            assert_eq!(bf_tmm(mp), want, "mp {mp}");
        }
    }

    #[test]
    fn current_mp_heat_tsm_and_motive() {
        // Heat subtracts from MP directly (p.49), floored 0.
        assert_eq!(cur_mp(4, 0, 0, M_NONE, false), 4);
        assert_eq!(cur_mp(4, 2, 0, M_NONE, false), 2);
        assert_eq!(cur_mp(4, 4, 0, M_NONE, false), 0);
        assert_eq!(cur_mp(2, 4, 0, M_NONE, false), 0); // saturates
        // TSM (p.154): heat 1 → +1 MP and the heat loss is ignored; heat 2+ subtracts normally.
        assert_eq!(cur_mp(4, 1, 0, M_NONE, true), 5);
        assert_eq!(cur_mp(4, 2, 0, M_NONE, true), 3); // 4 + 1 − 2
        assert_eq!(cur_mp(4, 0, 0, M_NONE, true), 4); // no heat, no bonus
        // Motive flags (p.44): −1 / half round down / immobile.
        assert_eq!(cur_mp(7, 0, 0, M_MINUS1, false), 6);
        assert_eq!(cur_mp(7, 0, 0, M_HALF, false), 3);
        assert_eq!(cur_mp(7, 0, 0, M_IMM, false), 0);
    }

    #[test]
    fn motive_flags_stack_and_order() {
        // The effects are independent once-per-game flags that STACK (p.43 limits repeats of
        // the same effect, not combinations): −1 applies before the halving, so base 8 with
        // both marked = (8 − 1) / 2 = 3, not 8/2 = 4.
        let both = BfMotive { minus_one: true, half: true, immobile: false };
        assert_eq!(cur_mp(8, 0, 0, both, false), 3);
        assert_eq!(cur_mp(7, 0, 0, both, false), 3); // (7−1)/2, round down
        // Immobile zeroes regardless of what else is marked.
        let all = BfMotive { minus_one: true, half: true, immobile: true };
        assert_eq!(cur_mp(8, 0, 0, all, false), 0);
        assert!(all.any() && !M_NONE.any());
        // §1.2 live order: mp_lost comes off BEFORE the motive halving — base 6, MP-crit loss
        // 3, then ½ MV → (6 − 3) / 2 = 1 (round down, p.44), matching chronological play.
        assert_eq!(cur_mp(6, 0, 3, M_HALF, false), 1);
    }

    #[test]
    fn mp_crit_sequence_p43() {
        // MV 8, heat 0: first crit loses half of current (8/2 = 4); the second loses 2 — the
        // loss is multiplicative at apply time, never count × k.
        let mut mp_lost = 0;
        let mut cur = cur_mp(8, 0, mp_lost, M_NONE, false);
        assert_eq!(cur, 8);
        mp_lost += bf_mp_crit_loss(cur); // −4
        cur = cur_mp(8, 0, mp_lost, M_NONE, false);
        assert_eq!(cur, 4);
        mp_lost += bf_mp_crit_loss(cur); // −2
        cur = cur_mp(8, 0, mp_lost, M_NONE, false);
        assert_eq!(cur, 2);
        mp_lost += bf_mp_crit_loss(cur); // −1 (jround(1.0))
        cur = cur_mp(8, 0, mp_lost, M_NONE, false);
        assert_eq!(cur, 1);
        // At current 1 the loss floors at 1 → 0 MP = cannot move.
        assert_eq!(bf_mp_crit_loss(cur), 1);
        mp_lost += 1;
        assert_eq!(cur_mp(8, 0, mp_lost, M_NONE, false), 0);
    }

    #[test]
    fn motive_effect_rows() {
        assert_eq!(bf_motive_effect(2), M_NONE);
        assert_eq!(bf_motive_effect(7), M_NONE);
        assert_eq!(bf_motive_effect(8), M_MINUS1);
        assert_eq!(bf_motive_effect(9), M_MINUS1);
        assert_eq!(bf_motive_effect(10), M_HALF);
        assert_eq!(bf_motive_effect(11), M_HALF);
        assert_eq!(bf_motive_effect(12), M_IMM);
        assert_eq!(bf_motive_effect(16), M_IMM); // modifiers can exceed 12
    }

    #[test]
    fn vehicle_engine_crit_mv_derives_live() {
        // 1st Engine hit on the Vehicle column: MV × 0.5 round down (p.43), derived live from
        // the hit count — never snapshotted into mp_lost.
        let v = Some(BfCritCol::Vehicle);
        assert_eq!(bf_current_mp(6, 0, 0, M_NONE, false, 1, v), 3);
        assert_eq!(bf_current_mp(7, 0, 0, M_NONE, false, 1, v), 3); // round down
        // Chronology-independence: Engine crit + motive ½ MV are both live halvings, so
        // whichever landed first the derived MV is ⌊⌊8/2⌋/2⌋ = 2 — the sequential table value
        // (8 → 4 → 2) in either order.
        assert_eq!(bf_current_mp(8, 0, 0, M_HALF, false, 1, v), 2);
        // The column gates the effect: a 'Mech's Engine hits never touch MV.
        assert_eq!(bf_current_mp(6, 0, 0, M_NONE, false, 1, Some(BfCritCol::Mech)), 6);
        assert_eq!(bf_current_mp(6, 0, 0, M_NONE, false, 1, None), 6);
        assert_eq!(BF_ENGINE_HITS_DESTROY, 2);
    }

    #[test]
    fn aero_engine_tp_derives_live() {
        assert_eq!(bf_aero_engine_tp_loss(7), 3); // −50% round down
        assert_eq!(bf_aero_engine_tp_loss(2), 1);
        assert_eq!(bf_aero_engine_tp_loss(1), 1); // min 1 lost
        // Live derivation on the Aerospace column: 1st hit halves the CURRENT (post-heat) TP;
        // 2nd hit is TP 0 + shutdown — permanent (p.42), so cooling heat afterwards cannot
        // resurrect thrust (the old mp_lost snapshot could).
        let a = Some(BfCritCol::Aerospace);
        assert_eq!(bf_current_mp(7, 2, 0, M_NONE, false, 1, a), 3); // 5 − max(1, 5/2)
        assert_eq!(bf_current_mp(7, 0, 0, M_NONE, false, 1, a), 4); // 7 − 3
        assert_eq!(bf_current_mp(7, 2, 0, M_NONE, false, 2, a), 0); // TP 0
        assert_eq!(bf_current_mp(7, 0, 0, M_NONE, false, 2, a), 0, "TP 0 survives cooling");
        assert_eq!(bf_current_mp(1, 0, 0, M_NONE, false, 1, a), 0); // min 1 lost
    }

    // ---- §1.4: the crit table ----

    #[test]
    fn crit_table_all_cells() {
        use BfCrit::*;
        use BfCritCol::*;
        // The full p.42 table, row by row: ['Mech, ProtoMech, Vehicle, Aerospace, DropShip].
        let rows: [(i32, [BfCrit; 5]); 11] = [
            (2, [Ammo, Weapon, Ammo, Fuel, KfBoom]),
            (3, [Engine, Weapon, CrewStunned, FireControl, DockingCollar]),
            (4, [FireControl, FireControl, FireControl, Engine, NoCrit]),
            (5, [NoCrit, Mp, FireControl, Weapon, FireControl]),
            (6, [Weapon, NoCrit, NoCrit, NoCrit, Weapon]),
            (7, [Mp, Mp, NoCrit, NoCrit, Thruster]),
            (8, [Weapon, NoCrit, NoCrit, NoCrit, Weapon]),
            (9, [NoCrit, Mp, Weapon, Weapon, Door]),
            (10, [FireControl, ProtoDestroyed, Weapon, Engine, NoCrit]),
            (11, [Engine, Weapon, CrewKilled, FireControl, Engine]),
            (12, [HeadBlownOff, Weapon, Engine, CrewKilled, CrewHit]),
        ];
        for (roll, want) in rows {
            for (col, w) in [Mech, ProtoMech, Vehicle, Aerospace, DropShip].into_iter().zip(want) {
                assert_eq!(bf_crit(roll, col), w, "roll {roll} col {col:?}");
            }
        }
    }

    #[test]
    fn crit_modified_off_table_rows() {
        // CR-modified ≤1 = No Crit (p.145); IRA >12 / RFA 13+ = Engine Hit (p.148/p.152).
        for col in [
            BfCritCol::Mech,
            BfCritCol::ProtoMech,
            BfCritCol::Vehicle,
            BfCritCol::Aerospace,
            BfCritCol::DropShip,
        ] {
            assert_eq!(bf_crit(1, col), BfCrit::NoCrit);
            assert_eq!(bf_crit(0, col), BfCrit::NoCrit);
            assert_eq!(bf_crit(13, col), BfCrit::Engine);
            assert_eq!(bf_crit(14, col), BfCrit::Engine);
        }
    }

    #[test]
    fn crit_roll_modifiers() {
        assert_eq!(bf_crit_roll_mod(&el("BM")), 0);
        assert_eq!(bf_crit_roll_mod(&with_sua(el("BM"), "CR")), -2);
        assert_eq!(bf_crit_roll_mod(&with_sua(el("BM"), "IRA")), 1);
        assert_eq!(bf_crit_roll_mod(&with_sua(el("CV"), "RFA")), 2);
        assert_eq!(bf_crit_roll_mod(&with_sua(with_sua(el("CV"), "RFA"), "CR")), 0);
    }

    #[test]
    fn crit_column_mapping_and_im_double_roll() {
        assert_eq!(bf_crit_col(&el("BM")), Some(BfCritCol::Mech));
        assert_eq!(bf_crit_col(&el("IM")), Some(BfCritCol::Mech));
        assert_eq!(bf_crit_col(&el("PM")), Some(BfCritCol::ProtoMech));
        assert_eq!(bf_crit_col(&el("CV")), Some(BfCritCol::Vehicle));
        assert_eq!(bf_crit_col(&el("BD")), Some(BfCritCol::Vehicle)); // gun emplacement
        assert_eq!(bf_crit_col(&el("SV")), Some(BfCritCol::Vehicle)); // ground SV
        let mut sv_air = el("SV");
        sv_air.primary_mode = "a".into();
        assert_eq!(bf_crit_col(&sv_air), Some(BfCritCol::Aerospace)); // fixed-wing SV
        assert_eq!(bf_crit_col(&el("AF")), Some(BfCritCol::Aerospace));
        // Small Craft roll the DropShips column ("‡Includes Small Craft", p.42/p.87) — a
        // mark-only path until small craft are baked (spec §Data-fidelity 7).
        assert_eq!(bf_crit_col(&el("SC")), Some(BfCritCol::DropShip));
        assert_eq!(bf_crit_col(&el("DS")), Some(BfCritCol::DropShip));
        // Infantry and BA never take crits (p.42).
        assert_eq!(bf_crit_col(&el("CI")), None);
        assert_eq!(bf_crit_col(&el("BA")), None);
        // IndustrialMechs roll twice and apply both (p.42).
        assert_eq!(bf_crit_rolls(&el("IM")), 2);
        assert_eq!(bf_crit_rolls(&el("BM")), 1);
        assert_eq!(bf_crit_rolls(&el("CV")), 1);
    }

    // ---- §1.5: physicals & air-to-ground ----

    #[test]
    fn physical_eligibility_by_type() {
        use BfPhysical::*;
        let bm = el("BM");
        assert!(bf_physical_eligible(Standard, &bm));
        assert!(!bf_physical_eligible(Melee, &bm)); // no melee weapon
        assert!(bf_physical_eligible(Charge, &bm));
        assert!(!bf_physical_eligible(Dfa, &bm)); // no jump
        assert!(!bf_physical_eligible(AntiMech, &bm));
        let jumper = AsElement { jump_move: 4, ..el("BM") };
        assert!(bf_physical_eligible(Dfa, &jumper));
        // MEL elements may not choose Standard instead (p.44).
        let mel = with_sua(el("BM"), "MEL");
        assert!(!bf_physical_eligible(Standard, &mel));
        assert!(bf_physical_eligible(Melee, &mel));
        // ProtoMechs: Standard only. Vehicles: Charge only. Infantry: Anti-'Mech only — BA
        // innately, conventional infantry only with the AM special (p.143).
        assert!(bf_physical_eligible(Standard, &el("PM")));
        assert!(!bf_physical_eligible(Charge, &el("PM")));
        assert!(bf_physical_eligible(Charge, &el("CV")));
        assert!(!bf_physical_eligible(Standard, &el("CV")));
        assert!(!bf_physical_eligible(AntiMech, &el("CI")), "CI without AM: ineligible");
        assert!(bf_physical_eligible(AntiMech, &with_sua(el("CI"), "AM")));
        assert!(bf_physical_eligible(AntiMech, &el("BA")));
        assert!(!bf_physical_eligible(Standard, &el("BA")));
        // Aerospace elements make no physical attacks.
        assert!(!bf_physical_eligible(Charge, &el("AF")));
        assert!(!bf_physical_eligible(Standard, &el("AF")));
    }

    #[test]
    fn charge_and_dfa_damage_p44() {
        // Size 2, MV 6: 6 × 0.50 = 3. Size 3, MV 5: 5 × 0.75 = 3.75 → 4 (round normally).
        let s2 = el("BM");
        assert_eq!(bf_physical_damage(BfPhysical::Charge, &s2, 6, 0), 3.0);
        let s3 = AsElement { size: 3, ..el("BM") };
        assert_eq!(bf_physical_damage(BfPhysical::Charge, &s3, 5, 0), 4.0);
        // DFA = charge damage + 1 (p.45).
        assert_eq!(bf_physical_damage(BfPhysical::Dfa, &s3, 5, 0), 5.0);
        // ENG/SAW round up instead: Size 1, MV 5 → 1.25 → 1 normally, 2 with ENG.
        let s1 = AsElement { size: 1, ..el("CV") };
        assert_eq!(bf_physical_damage(BfPhysical::Charge, &s1, 5, 0), 1.0);
        let eng = with_sua(AsElement { size: 1, ..el("CV") }, "ENG");
        assert_eq!(bf_physical_damage(BfPhysical::Charge, &eng, 5, 0), 2.0);
    }

    #[test]
    fn standard_melee_and_antimech_damage() {
        // Standard/Melee: attacker Size; MEL +1; TSM (heat ≥ 1) / TSMX / I-TSM each +1.
        let bm = el("BM"); // size 2
        assert_eq!(bf_physical_damage(BfPhysical::Standard, &bm, 6, 0), 2.0);
        let mel = with_sua(el("BM"), "MEL");
        assert_eq!(bf_physical_damage(BfPhysical::Melee, &mel, 6, 0), 3.0);
        let tsm = with_sua(el("BM"), "TSM");
        assert_eq!(bf_physical_damage(BfPhysical::Standard, &tsm, 6, 0), 2.0); // cold: no bonus
        assert_eq!(bf_physical_damage(BfPhysical::Standard, &tsm, 6, 1), 3.0); // hot: +1
        assert_eq!(bf_physical_damage(BfPhysical::Standard, &with_sua(el("BM"), "TSMX"), 6, 0), 3.0);
        assert_eq!(bf_physical_damage(BfPhysical::Standard, &with_sua(el("BM"), "I-TSM"), 6, 0), 3.0);
        // Anti-'Mech: the element's normal (Short) damage.
        let mut ci = el("CI");
        ci.std_damage.s = 1.0;
        assert_eq!(bf_physical_damage(BfPhysical::AntiMech, &ci, 0, 0), 1.0);
    }

    #[test]
    fn strafe_strike_and_bomb_damage() {
        // Strafing: overheat and rear +1 added BEFORE halving; round normally; min 1.
        let mut e = el("AF");
        e.std_damage.s = 5.0;
        assert_eq!(bf_strafe_damage(&e, 0, 2, false), 4.0); // (5+2)/2 = 3.5 → 4
        assert_eq!(bf_strafe_damage(&e, 0, 0, false), 3.0); // 5/2 = 2.5 → 3
        assert_eq!(bf_strafe_damage(&e, 0, 0, true), 3.0); // (5+1)/2 = 3
        e.std_damage.s = 1.0;
        assert_eq!(bf_strafe_damage(&e, 0, 0, false), 1.0); // jround(0.5) = 1, min 1 anyway
        // Striking: S + OV, +1 rear; weapon crits reduce S first.
        e.std_damage.s = 5.0;
        assert_eq!(bf_strike_damage(&e, 0, 2, false), 7.0);
        assert_eq!(bf_strike_damage(&e, 0, 2, true), 8.0);
        assert_eq!(bf_strike_damage(&e, 1, 0, false), 4.0);
        // Bombing: 2 damage per bomb (p.48).
        assert_eq!(bf_bomb_damage(1), 2.0);
        assert_eq!(bf_bomb_damage(3), 6.0);
    }

    // ---- §1.6: overheat, REAR, IF ----

    #[test]
    fn overheat_commit_and_ovl_gate() {
        let mut e = el("BM");
        e.overheat = 3;
        e.std_damage = DamageVector { s: 4.0, m: 4.0, l: None, e: None };
        // Max commit: min(OV, heat room); the scale tops out at 4.
        assert_eq!(bf_max_ov_commit(&e, 0), 3);
        assert_eq!(bf_max_ov_commit(&e, 2), 2); // heat-room cap
        assert_eq!(bf_max_ov_commit(&e, 4), 0);
        // OV 3, dmg 4/4/− → 7/7/− at max commit; overheat cannot create an L attack.
        assert_eq!(bf_shot_damage(&e, BfRange::Short, 0, 3, 0), Some(7.0));
        assert_eq!(bf_shot_damage(&e, BfRange::Medium, 0, 3, 0), Some(7.0));
        assert_eq!(bf_shot_damage(&e, BfRange::Long, 0, 3, 0), None);
        // Without OVL the commit applies at S/M only; OVL extends it to L.
        let mut l = el("BM");
        l.overheat = 2;
        l.std_damage = DamageVector { s: 4.0, m: 4.0, l: Some(2.0), e: None };
        assert_eq!(bf_shot_damage(&l, BfRange::Long, 0, 2, 0), Some(2.0));
        let ovl = with_sua(l, "OVL");
        assert_eq!(bf_shot_damage(&ovl, BfRange::Long, 0, 2, 0), Some(4.0));
        // Weapon crits reduce the base before the commit is added.
        assert_eq!(bf_shot_damage(&ovl, BfRange::Short, 1, 2, 0), Some(5.0));
        // A 'Mech's Engine hits never touch the damage line (the halving is Vehicle-column).
        assert_eq!(bf_shot_damage(&ovl, BfRange::Short, 0, 0, 1), Some(4.0));
    }

    #[test]
    fn vehicle_engine_crit_halves_damage_readouts() {
        // p.43: 1st Engine hit halves ALL damage values (round down, min 0) — applied after the
        // Weapon-crit subtraction and before the OV add, on every weapon-damage readout.
        let mut v = el("CV");
        v.overheat = 1;
        v.std_damage = DamageVector { s: 5.0, m: 3.0, l: Some(1.0), e: None };
        assert_eq!(bf_shot_damage(&v, BfRange::Short, 0, 0, 1), Some(2.0)); // 5 → 2
        assert_eq!(bf_shot_damage(&v, BfRange::Long, 0, 0, 1), None); // 1 → 0 = no attack
        assert_eq!(bf_shot_damage(&v, BfRange::Short, 1, 0, 1), Some(2.0)); // (5−1)/2
        assert_eq!(bf_shot_damage(&v, BfRange::Short, 0, 1, 1), Some(3.0)); // halve, THEN +OV
        // IF and REAR are damage values too (p.43 halves them all).
        v.suas.insert("IF".into(), SuaVal::Num(2.0));
        v.suas.insert(
            "REAR".into(),
            SuaVal::Dmg(DamageVector { s: 3.0, m: 1.0, l: None, e: None }),
        );
        assert_eq!(bf_indirect_damage(&v, 0, 1), Some(1.0));
        assert_eq!(bf_indirect_damage(&v, 0, 0), Some(2.0));
        assert_eq!(bf_rear_damage(&v, BfRange::Short, 0, 1), Some(1.0));
        assert_eq!(bf_rear_damage(&v, BfRange::Medium, 0, 1), None); // 1 → 0
    }

    #[test]
    fn rear_and_indirect_damage_readouts() {
        // REAR-weapons attack fires the REAR values, not the card S/M/L.
        let mut e = el("BM");
        e.suas.insert(
            "REAR".into(),
            SuaVal::Dmg(DamageVector { s: 1.0, m: 1.0, l: None, e: None }),
        );
        assert_eq!(bf_rear_damage(&e, BfRange::Short, 0, 0), Some(1.0));
        assert_eq!(bf_rear_damage(&e, BfRange::Long, 0, 0), None);
        assert_eq!(bf_rear_damage(&e, BfRange::Short, 1, 0), None); // weapon crit reduces
        assert_eq!(bf_rear_damage(&el("BM"), BfRange::Short, 0, 0), None); // no REAR ability
        // Indirect fire deals the IF value, bracket-independent, weapon-crit reduced.
        let mut f = el("BM");
        f.suas.insert("IF".into(), SuaVal::Num(2.0));
        assert_eq!(bf_indirect_damage(&f, 0, 0), Some(2.0));
        assert_eq!(bf_indirect_damage(&f, 1, 0), Some(1.0));
        assert_eq!(bf_indirect_damage(&f, 2, 0), None);
        assert_eq!(bf_indirect_damage(&el("BM"), 0, 0), None);
    }

    // ---- §1.7: Unit derived stats ----

    #[test]
    fn unit_mv_p52_examples() {
        // Three 3j jumpers + two MV-2 walkers: ground min 2; not jump-capable (walkers lack jump).
        let mixed = [
            (3, Some(3), true),
            (3, Some(3), true),
            (3, Some(3), true),
            (2, None, true),
            (2, None, true),
        ];
        assert_eq!(bf_unit_mv(&mixed), (2, None));
        // All-jump Unit: ground and jump minima computed separately (p.52's 5-ground/3-jump).
        let jumps = [(5, Some(4), true), (6, Some(3), true)];
        assert_eq!(bf_unit_mv(&jumps), (5, Some(3)));
        // Destroyed members drop out of both minima.
        let losses = [(1, Some(1), false), (4, Some(2), true)];
        assert_eq!(bf_unit_mv(&losses), (4, Some(2)));
        // Empty / fully destroyed Unit.
        assert_eq!(bf_unit_mv(&[]), (0, None));
        assert_eq!(bf_unit_mv(&[(4, None, false)]), (0, None));
    }

    #[test]
    fn unit_size_round_normally() {
        assert_eq!(bf_unit_size(&[1, 2, 1, 2]), 2); // mean 1.5 → 2 (round normally)
        assert_eq!(bf_unit_size(&[4, 3, 3, 3, 3, 4]), 3); // mean 3.33 → 3
        assert_eq!(bf_unit_size(&[3]), 3);
        assert_eq!(bf_unit_size(&[]), 0);
    }

    // ---- §1.8: skill-adjusted PV parity ----

    #[test]
    fn skill_pv_parity_with_the_printed_brackets() {
        use crate::engine::skill::skill_adjusted_pv;
        // The BF Skill PV table (p.50) IS the AS adjustment — the printed examples must fall out
        // of skill.rs unchanged (a failure here is an AS-mode bug too).
        assert_eq!(skill_adjusted_pv(35, 6), 27); // 35 − 2×(1 + (35−5)/10) = 35 − 8
        assert_eq!(skill_adjusted_pv(39, 2), 55); // 39 + 2×(1 + (39−3)/5) = 39 + 16
        assert_eq!(skill_adjusted_pv(30, 2), 42); // the p.52 Alice example: 30 + 2×6
    }

    // ---- Catalog sweeps (OQ 5 / OQ 6) — skip cleanly when the baked bundle is absent ----

    fn load_bundle() -> Option<crate::data::bundle::Bundle> {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/mechs.bin");
        match crate::data::bundle::Bundle::load(std::path::Path::new(path)) {
            Ok(b) => Some(b),
            Err(_) => {
                eprintln!("skip: {path} absent (bake the catalog to run the sweep)");
                None
            }
        }
    }

    fn display_name(m: &crate::domain::Mech) -> String {
        if m.model.is_empty() {
            m.chassis.clone()
        } else {
            format!("{} {}", m.chassis, m.model)
        }
    }

    #[test]
    fn catalog_tmm_sweep_oq5() {
        use crate::engine::alpha_strike::inches_to_hexes;
        use crate::engine::as_element::as_element;
        // OQ 5: live TMM comes from the Available-MP bracket table; at full health it should
        // equal the baked AS TMM (identical tables at 2″ = 1 hex). Swept 2026-07-05: ZERO
        // mismatches across 8710 ground elements — the spec's expected MASC/quirk divergences do
        // not exist in the baked data. Pinned at 0; grow only with a rule in hand.
        const PINNED_MISMATCHES: usize = 0;
        let Some(bundle) = load_bundle() else { return };
        let mut mismatches = Vec::new();
        let mut ground = 0usize;
        for m in &bundle.mechs {
            let e = as_element(&m.as_stats, &display_name(m), 4);
            if bf_is_aero(&e) {
                continue; // fn1: the bracket table does not apply to aerospace
            }
            ground += 1;
            let hexes = inches_to_hexes(e.primary_move);
            let live = bf_tmm(hexes);
            if live != i32::from(m.as_stats.tmm) {
                mismatches.push(format!(
                    "{}: {}\" -> {} hexes -> bf_tmm {} vs baked {}",
                    e.name, e.primary_move, hexes, live, m.as_stats.tmm
                ));
            }
        }
        for s in &mismatches {
            eprintln!("TMM mismatch: {s}");
        }
        eprintln!(
            "catalog_tmm_sweep_oq5: {} mismatches across {} ground elements",
            mismatches.len(),
            ground
        );
        // Population floor: the pin is only meaningful over the full catalog (8,710 ground
        // elements at last bake) — a filtered/partial bake clobbering data/mechs.bin would
        // otherwise make the sweep silently vacuous.
        assert!(
            ground >= 8_000,
            "sweep ran over only {ground} ground elements — filtered mechs.bin?"
        );
        // "≤ pinned" semantics; == while the pin sits at the 0 floor (clippy: absurd `<= 0`).
        assert_eq!(
            mismatches.len(),
            PINNED_MISMATCHES,
            "TMM mismatch count moved off the pin"
        );
    }

    #[test]
    fn catalog_mv_even_oq6() {
        // OQ 6 / Data-fidelity 1: the book says "divide by 2" with an even example;
        // `movement_hexes` rounds up. The two agree iff every inch-denominated MV in the catalog
        // is even — this assert pins that. If it ever fires, resolve OQ 6 before trusting ÷2.
        let Some(bundle) = load_bundle() else { return };
        // Population floor (see catalog_tmm_sweep_oq5): the pin must cover the full catalog.
        assert!(
            bundle.mechs.len() >= 9_000,
            "sweep ran over only {} elements — filtered mechs.bin?",
            bundle.mechs.len()
        );
        let mut odd = Vec::new();
        for m in &bundle.mechs {
            for tok in m.as_stats.movement.split('/') {
                if let Some(q) = tok.find('"') {
                    let v: u32 = tok[..q].trim().parse().unwrap_or(0);
                    if !v.is_multiple_of(2) {
                        odd.push(format!("{}: {}", display_name(m), m.as_stats.movement));
                    }
                }
            }
        }
        for s in &odd {
            eprintln!("odd MV: {s}");
        }
        assert!(odd.is_empty(), "{} odd-inch MV entries (OQ 6 fires)", odd.len());
    }
}
