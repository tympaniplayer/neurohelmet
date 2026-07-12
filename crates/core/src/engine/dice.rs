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

//! Dice-reference tables: Cluster Hits and 'Mech Hit Location. Pure reference data — this module
//! *rolls nothing* and changes no state. It exists so the tracker can show the two tables a Classic
//! player reaches for the rulebook for most (cf. the no-auto-roll philosophy in `engine` generally).
//!
//! Tables are transcribed from a cited source, never recalled:
//! - Cluster Hits Table: MegaMek `megamek/src/megamek/common/compute/Compute.java`
//!   (`clusterHitsTable`), verified against the canonical Total Warfare table (e.g. SRM-2 → 1 hit on
//!   2–7 / 2 hits on 8–12).
//! - 'Mech Hit Location Table: Total Warfare p.114 (BattleMech, front/left/right/rear columns).

use crate::domain::Location;

/// The MegaMek Cluster Hits Table, verbatim. Each row is `[cluster_size, r2, r3, …, r12]`: the
/// first element is the rack/cluster size and the remaining eleven are the number of hits for a
/// 2d6 result of 2 through 12. Sizes are contiguous 1..=30, then 40.
#[rustfmt::skip]
const CLUSTER_TABLE: &[[u8; 12]] = &[
    [ 1,  1,  1,  1,  1,  1,  1,  1,  1,  1,  1,  1],
    [ 2,  1,  1,  1,  1,  1,  1,  2,  2,  2,  2,  2],
    [ 3,  1,  1,  1,  2,  2,  2,  2,  2,  3,  3,  3],
    [ 4,  1,  2,  2,  2,  2,  3,  3,  3,  3,  4,  4],
    [ 5,  1,  2,  2,  3,  3,  3,  3,  4,  4,  5,  5],
    [ 6,  2,  2,  3,  3,  4,  4,  4,  5,  5,  6,  6],
    [ 7,  2,  2,  3,  4,  4,  4,  4,  6,  6,  7,  7],
    [ 8,  2,  3,  3,  4,  4,  5,  5,  6,  7,  8,  8],
    [ 9,  3,  3,  4,  5,  5,  5,  5,  7,  7,  9,  9],
    [10,  3,  3,  4,  6,  6,  6,  6,  8,  8, 10, 10],
    [11,  4,  4,  5,  7,  7,  7,  7,  9,  9, 11, 11],
    [12,  4,  4,  5,  8,  8,  8,  8, 10, 10, 12, 12],
    [13,  4,  4,  5,  8,  8,  8,  8, 11, 11, 13, 13],
    [14,  5,  5,  6,  9,  9,  9,  9, 11, 11, 14, 14],
    [15,  5,  5,  6,  9,  9,  9,  9, 12, 12, 15, 15],
    [16,  5,  5,  7, 10, 10, 10, 10, 13, 13, 16, 16],
    [17,  5,  5,  7, 10, 10, 10, 10, 14, 14, 17, 17],
    [18,  6,  6,  8, 11, 11, 11, 11, 14, 14, 18, 18],
    [19,  6,  6,  8, 11, 11, 11, 11, 15, 15, 19, 19],
    [20,  6,  6,  9, 12, 12, 12, 12, 16, 16, 20, 20],
    [21,  7,  7,  9, 13, 13, 13, 13, 17, 17, 21, 21],
    [22,  7,  7,  9, 14, 14, 14, 14, 18, 18, 22, 22],
    [23,  7,  7, 10, 15, 15, 15, 15, 19, 19, 23, 23],
    [24,  8,  8, 10, 16, 16, 16, 16, 20, 20, 24, 24],
    [25,  8,  8, 10, 16, 16, 16, 16, 21, 21, 25, 25],
    [26,  9,  9, 11, 17, 17, 17, 17, 21, 21, 26, 26],
    [27,  9,  9, 11, 17, 17, 17, 17, 22, 22, 27, 27],
    [28,  9,  9, 11, 17, 17, 17, 17, 23, 23, 28, 28],
    [29, 10, 10, 12, 18, 18, 18, 18, 23, 23, 29, 29],
    [30, 10, 10, 12, 18, 18, 18, 18, 24, 24, 30, 30],
    [40, 12, 12, 18, 24, 24, 24, 24, 32, 32, 40, 40],
];

/// Number of hits a cluster weapon of the given rack `size` scores on a 2d6 `roll` (clamped to
/// 2..=12). For a size not in the table (only 31..=39 are absent — no real weapon falls there) the
/// nearest defined column is used; full MegaMek interpolation isn't needed for our data.
pub fn cluster_hits(size: u16, roll: u8) -> u8 {
    let col = roll.clamp(2, 12) as usize - 1; // row[0] is the size; roll 2 → index 1.
    let row = CLUSTER_TABLE
        .iter()
        .find(|r| u16::from(r[0]) == size)
        .or_else(|| {
            CLUSTER_TABLE
                .iter()
                .min_by_key(|r| u16::from(r[0]).abs_diff(size))
        });
    row.map_or(0, |r| r[col])
}

/// Whether a weapon rolls on the cluster table, and how many projectiles are in play.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClusterProfile {
    /// A single projectile — no cluster roll (AC, Gauss, energy, artillery, plain MG, …).
    Single,
    /// All `n` projectiles hit with no roll — Streak launchers, when locked on.
    AllHit(u16),
    /// Roll 2d6 and read the cluster column for this rack size.
    Table(u16),
}

/// Classify a weapon for cluster-table purposes from its baked `ammo_type`
/// ([`crate::domain::WeaponMount::ammo_type`]) prefix, rack size, and loaded munition name.
///
/// The `ammoType` vocabulary is MegaMek's (e.g. `LRM`, `SRM_STREAK`, `AC_LBX`, `AC_ULTRA`). Notes:
/// - Streak launchers (`*_STREAK`) are all-or-nothing — every missile hits once locked, no roll.
/// - LB-X autocannons roll on the table only with **Cluster** munition loaded; Slug/Standard is a
///   single slug. This honors the loaded munition (the `t` munition picker).
/// - Ultra / Rotary ACs roll on the table for their *shots* (2 / up to 6), not a missile rack.
/// - A plain machine gun is a single hit; MG *arrays* (cluster) aren't distinguishable from the
///   ammo type alone, so they're treated as single here.
pub fn cluster_profile(ammo_type: &str, rack: u16, munition: &str) -> ClusterProfile {
    // Streak first — `LRM_STREAK`/`SRM_STREAK` also start with LRM/SRM, so order matters.
    if ammo_type.ends_with("_STREAK") {
        return ClusterProfile::AllHit(rack);
    }
    match ammo_type {
        "AC_LBX" => {
            if munition.eq_ignore_ascii_case("cluster") {
                ClusterProfile::Table(rack)
            } else {
                ClusterProfile::Single
            }
        }
        "AC_ULTRA" => ClusterProfile::Table(2),
        "AC_ROTARY" => ClusterProfile::Table(6),
        _ if is_missile_rack(ammo_type) => ClusterProfile::Table(rack),
        _ => ClusterProfile::Single,
    }
}

/// Missile / rocket / arrow-style launchers that roll on the cluster table (non-Streak).
fn is_missile_rack(t: &str) -> bool {
    t.starts_with("LRM")
        || t.starts_with("SRM")
        || t.starts_with("NLRM")
        || matches!(
            t,
            "MRM"
                | "ATM"
                | "IATM"
                | "MML"
                | "EXLRM"
                | "ROCKET_LAUNCHER"
                | "RL_BOMB"
                | "HAG"
                | "MEK_MORTAR"
        )
}

/// Which way the attack comes in, for the hit-location table. `Rear` strikes the same locations as
/// `Front` but lands on the rear armor of the torsos.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AttackDir {
    Front,
    Left,
    Right,
    Rear,
}

impl AttackDir {
    /// The four directions, in table-column order.
    pub const ALL: [AttackDir; 4] = [
        AttackDir::Front,
        AttackDir::Left,
        AttackDir::Right,
        AttackDir::Rear,
    ];

    /// Short column label for the table header.
    pub fn label(self) -> &'static str {
        match self {
            AttackDir::Front => "Front",
            AttackDir::Left => "Left",
            AttackDir::Right => "Right",
            AttackDir::Rear => "Rear",
        }
    }
}

/// One cell of the hit-location table: the location struck and whether a floating critical applies
/// (the natural-2 result — the attacker may also inflict a critical hit on that location).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HitRow {
    pub loc: Location,
    pub floating_crit: bool,
}

/// The 'Mech Hit Location Table (Total Warfare p.114). `roll` is the 2d6 total 2..=12; out-of-range
/// values clamp. `Rear` uses the same locations as `Front` (rear armor on the torsos).
pub fn mech_hit_location(dir: AttackDir, roll: u8) -> HitRow {
    use Location::*;
    let roll = roll.clamp(2, 12);
    let floating_crit = roll == 2;
    // Index by roll - 2 (0..=10).
    let i = (roll - 2) as usize;
    let loc = match dir {
        AttackDir::Front | AttackDir::Rear => [
            CenterTorso, // 2  (+ floating crit)
            RightArm,    // 3
            RightArm,    // 4
            RightLeg,    // 5
            RightTorso,  // 6
            CenterTorso, // 7
            LeftTorso,   // 8
            LeftLeg,     // 9
            LeftArm,     // 10
            LeftArm,     // 11
            Head,        // 12
        ][i],
        AttackDir::Left => [
            LeftTorso,   // 2  (+ floating crit)
            LeftLeg,     // 3
            LeftArm,     // 4
            LeftArm,     // 5
            LeftLeg,     // 6
            LeftTorso,   // 7
            CenterTorso, // 8
            RightTorso,  // 9
            RightArm,    // 10
            RightLeg,    // 11
            Head,        // 12
        ][i],
        AttackDir::Right => [
            RightTorso,  // 2  (+ floating crit)
            RightLeg,    // 3
            RightArm,    // 4
            RightArm,    // 5
            RightLeg,    // 6
            RightTorso,  // 7
            CenterTorso, // 8
            LeftTorso,   // 9
            LeftArm,     // 10
            LeftLeg,     // 11
            Head,        // 12
        ][i],
    };
    HitRow { loc, floating_crit }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Location::*;

    #[test]
    fn cluster_known_cells() {
        // SRM-2: 1 hit on 2..=7, 2 hits on 8..=12.
        for roll in 2..=7 {
            assert_eq!(cluster_hits(2, roll), 1, "srm2 @ {roll}");
        }
        for roll in 8..=12 {
            assert_eq!(cluster_hits(2, roll), 2, "srm2 @ {roll}");
        }
        // SRM-6 column, verbatim from MegaMek.
        assert_eq!(cluster_hits(6, 2), 2);
        assert_eq!(cluster_hits(6, 7), 4);
        assert_eq!(cluster_hits(6, 12), 6);
        // LRM-20 corners.
        assert_eq!(cluster_hits(20, 2), 6);
        assert_eq!(cluster_hits(20, 7), 12);
        assert_eq!(cluster_hits(20, 12), 20);
        // Size 40 (MRM-40) max.
        assert_eq!(cluster_hits(40, 12), 40);
    }

    #[test]
    fn cluster_roll_clamps_and_nearest_size() {
        assert_eq!(cluster_hits(20, 0), cluster_hits(20, 2));
        assert_eq!(cluster_hits(20, 99), cluster_hits(20, 12));
        // 31..=39 aren't in the table → nearest (30) used; never panics.
        assert_eq!(cluster_hits(35, 12), cluster_hits(30, 12));
    }

    #[test]
    fn profile_classification() {
        assert_eq!(
            cluster_profile("LRM", 20, "Standard"),
            ClusterProfile::Table(20)
        );
        assert_eq!(
            cluster_profile("SRM", 6, "Inferno"),
            ClusterProfile::Table(6)
        );
        assert_eq!(cluster_profile("MRM", 40, ""), ClusterProfile::Table(40));
        assert_eq!(cluster_profile("ATM", 12, ""), ClusterProfile::Table(12));
        // Streak: all-or-nothing, checked before the LRM/SRM prefix rule.
        assert_eq!(
            cluster_profile("SRM_STREAK", 6, ""),
            ClusterProfile::AllHit(6)
        );
        assert_eq!(
            cluster_profile("LRM_STREAK", 20, ""),
            ClusterProfile::AllHit(20)
        );
        // LB-X depends on the loaded munition.
        assert_eq!(
            cluster_profile("AC_LBX", 10, "Cluster"),
            ClusterProfile::Table(10)
        );
        assert_eq!(
            cluster_profile("AC_LBX", 10, "Standard"),
            ClusterProfile::Single
        );
        // Ultra / Rotary roll for shots.
        assert_eq!(cluster_profile("AC_ULTRA", 5, ""), ClusterProfile::Table(2));
        assert_eq!(
            cluster_profile("AC_ROTARY", 5, ""),
            ClusterProfile::Table(6)
        );
        // Single-projectile weapons.
        assert_eq!(cluster_profile("AC", 20, ""), ClusterProfile::Single);
        assert_eq!(cluster_profile("GAUSS", 1, ""), ClusterProfile::Single);
        assert_eq!(cluster_profile("NARC", 1, ""), ClusterProfile::Single);
    }

    #[test]
    fn hit_location_corners() {
        // Front: natural 2 = CT + floating crit; 7 = CT; 12 = head.
        let two = mech_hit_location(AttackDir::Front, 2);
        assert_eq!(two.loc, CenterTorso);
        assert!(two.floating_crit);
        assert_eq!(mech_hit_location(AttackDir::Front, 7).loc, CenterTorso);
        assert!(!mech_hit_location(AttackDir::Front, 7).floating_crit);
        assert_eq!(mech_hit_location(AttackDir::Front, 12).loc, Head);
        // Side natural-2 floating crits land on the near torso.
        assert_eq!(mech_hit_location(AttackDir::Left, 2).loc, LeftTorso);
        assert!(mech_hit_location(AttackDir::Left, 2).floating_crit);
        assert_eq!(mech_hit_location(AttackDir::Right, 2).loc, RightTorso);
        // Rear uses the front locations.
        assert_eq!(mech_hit_location(AttackDir::Rear, 6).loc, RightTorso);
        // Heads on a 12 in every direction.
        for dir in AttackDir::ALL {
            assert_eq!(mech_hit_location(dir, 12).loc, Head, "{dir:?}");
        }
    }
}
