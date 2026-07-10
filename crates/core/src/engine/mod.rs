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

//! Pure BattleTech rules: damage application, internal-structure reference table, and the
//! heat-effects scale. No UI, no IO — exhaustively unit-testable.

pub mod acs;
pub mod alpha_strike;
pub mod as_element;
pub mod battleforce;
pub mod damage;
pub mod dice;
pub mod gator;
pub mod heat;
pub mod infantry;
pub mod internal;
pub mod large_craft;
pub mod movement;
pub mod override_conv;
pub mod pilot;
pub mod sbf;
pub mod skill;

pub use alpha_strike::{
    as_attacker_move_modifier, as_target_modifier, as_to_hit, as_to_hit_full, inches_to_hexes,
    movement_hexes, range_brackets_hexes,
};
pub use damage::{apply_damage, transfer_to, DamageOutcome, LocState};
pub use dice::{cluster_hits, cluster_profile, mech_hit_location, AttackDir, ClusterProfile, HitRow};
pub use gator::{parse_ranges, range_bracket, target_modifier, to_hit as gator_to_hit, RangeBracket};
pub use heat::{aero_heat_effects, dissipation, heat_effects, AeroHeatEffects, HeatEffects};
pub use infantry::{infantry_max_range, infantry_range_mod};
pub use internal::internal_structure;
pub use movement::{attacker_movement_modifier, target_movement_modifier, MoveMode};
pub use pilot::{consciousness_avoid, PILOT_MAX};
pub use skill::{bv_skill_multiplier, skill_adjusted_bv, skill_adjusted_pv};
