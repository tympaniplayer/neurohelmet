//! Shared large-craft (DropShip→WarShip) combat helpers: the multi-arc AS/BF card, arc /
//! weapon-class damage selection, and the damage-threshold gate. Consumed by BattleForce now, and
//! by SBF/ACS as their capital layers land. See `docs/large-craft-implementation-spec.md`.
//!
//! Doctrine reminder: the arc *values* are already final BF-scale — the capital classes
//! (CAP/SCAP/MSL) drive the to-hit weapon-class modifier and the crit weapon-class pick, NOT a
//! damage rescale.

use super::as_element::{as_damage, DamageVector};
use crate::domain::{ArcCard, ArcDamage, FiringArc};

/// AS/BF weapon classes. `Std` is standard-scale; `Cap`/`ScAp`/`Msl` are capital, sub-capital, and
/// capital/sub-capital missiles. Small Craft carry only `Std`; DropShips add `ScAp`/`Msl`; capital
/// craft (Phase 2) add `Cap`. Order fixed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum WeaponClass {
    #[default]
    Std,
    Cap,
    ScAp,
    Msl,
}

impl WeaponClass {
    pub const ALL: [WeaponClass; 4] = [Self::Std, Self::Cap, Self::ScAp, Self::Msl];

    pub fn label(self) -> &'static str {
        match self {
            Self::Std => "STD",
            Self::Cap => "CAP",
            Self::ScAp => "SCAP",
            Self::Msl => "MSL",
        }
    }

    /// Capital-scale weapon-class to-hit modifier (IO:BF p.191): CAP +3, SCAP +2, MSL/STD +0. This
    /// is WAIVED when the target is itself large craft — applied by the to-hit layer, not here.
    pub fn to_hit_mod(self) -> i32 {
        match self {
            Self::Cap => 3,
            Self::ScAp => 2,
            Self::Std | Self::Msl => 0,
        }
    }

    pub fn is_capital(self) -> bool {
        matches!(self, Self::Cap | Self::ScAp | Self::Msl)
    }

    /// This class's raw damage strings within a firing arc.
    pub fn of(self, arc: &FiringArc) -> &ArcDamage {
        match self {
            Self::Std => &arc.std,
            Self::Cap => &arc.cap,
            Self::ScAp => &arc.scap,
            Self::Msl => &arc.msl,
        }
    }
}

/// The four AS/BF firing arcs. `Nose`/`Aft` are the card's front/rear.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Arc {
    #[default]
    Nose,
    Left,
    Right,
    Aft,
}

impl Arc {
    pub const ALL: [Arc; 4] = [Self::Nose, Self::Left, Self::Right, Self::Aft];

    pub fn label(self) -> &'static str {
        match self {
            Self::Nose => "Nose",
            Self::Left => "Left",
            Self::Right => "Right",
            Self::Aft => "Aft",
        }
    }

    /// This arc's per-class damage + specials on the card.
    pub fn of(self, card: &ArcCard) -> &FiringArc {
        match self {
            Self::Nose => &card.front,
            Self::Left => &card.left,
            Self::Right => &card.right,
            Self::Aft => &card.rear,
        }
    }
}

/// Parse one arc/class's S/M/L/E damage into a typed vector (`"0*"` → minimal `0.5`, `"-"`/`""` → 0).
pub fn arc_damage(card: &ArcCard, arc: Arc, class: WeaponClass) -> DamageVector {
    let ad = class.of(arc.of(card));
    DamageVector {
        s: as_damage(&ad.s),
        m: as_damage(&ad.m),
        l: Some(as_damage(&ad.l)),
        e: Some(as_damage(&ad.e)),
    }
}

/// Whether a damage vector is entirely zero across all bands (a minimal `0.5` counts as non-zero).
pub fn is_zero(d: &DamageVector) -> bool {
    d.s == 0.0 && d.m == 0.0 && d.l.unwrap_or(0.0) == 0.0 && d.e.unwrap_or(0.0) == 0.0
}

/// The weapon-class lines an arc actually carries, in `WeaponClass::ALL` order, skipping all-zero
/// classes. For card rendering and the shot-builder's class picker.
pub fn arc_lines(card: &ArcCard, arc: Arc) -> Vec<(WeaponClass, DamageVector)> {
    WeaponClass::ALL
        .into_iter()
        .map(|c| (c, arc_damage(card, arc, c)))
        .filter(|(_, d)| !is_zero(d))
        .collect()
}

/// Whether a single attack's damage meets the card's damage Threshold (triggers a crit roll — it
/// does NOT bypass armor; IO:BF p.40). A `0`/absent threshold never triggers; minimal `0.5` damage
/// can never meet an integer threshold ≥ 1.
pub fn threshold_triggered(single_attack_dmg: f32, threshold: u8) -> bool {
    threshold > 0 && single_attack_dmg >= threshold as f32
}

/// The printed form of one class's damage in an arc (`"4/3/2/0*"`), preserving `0*` and rendering
/// empty/`"-"` bands as `"0"`. Returns `None` for a fully-zero line (nothing to display). Unlike the
/// parsed [`arc_damage`], this keeps the source strings for faithful card rendering.
pub fn arc_class_display(card: &ArcCard, arc: Arc, class: WeaponClass) -> Option<String> {
    let ad = class.of(arc.of(card));
    let bands = [&ad.s, &ad.m, &ad.l, &ad.e];
    if bands.iter().all(|s| matches!(s.trim(), "" | "0" | "-")) {
        return None;
    }
    fn band(s: &str) -> &str {
        let t = s.trim();
        if t.is_empty() { "0" } else { t }
    }
    Some(format!("{}/{}/{}/{}", band(&ad.s), band(&ad.m), band(&ad.l), band(&ad.e)))
}

/// The non-empty printed class lines for an arc, in class order — for card rendering.
pub fn arc_display_lines(card: &ArcCard, arc: Arc) -> Vec<(WeaponClass, String)> {
    WeaponClass::ALL
        .into_iter()
        .filter_map(|c| arc_class_display(card, arc, c).map(|s| (c, s)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dmg(s: &str, m: &str, l: &str, e: &str) -> ArcDamage {
        ArcDamage { s: s.into(), m: m.into(), l: l.into(), e: e.into() }
    }

    /// A Union-like spheroid DropShip: Nose STD + MSL, an empty Left arc, a light Aft.
    fn union() -> ArcCard {
        ArcCard {
            front: FiringArc {
                std: dmg("4", "3", "2", "0*"),
                msl: dmg("1", "1", "1", "1"),
                specials: vec!["PNT1".into()],
                ..Default::default()
            },
            rear: FiringArc { std: dmg("1", "0", "0", "0"), ..Default::default() },
            ..Default::default()
        }
    }

    #[test]
    fn arc_damage_parses_and_preserves_minimal() {
        let c = union();
        let front_std = arc_damage(&c, Arc::Nose, WeaponClass::Std);
        assert_eq!((front_std.s, front_std.m, front_std.l), (4.0, 3.0, Some(2.0)));
        assert_eq!(front_std.e, Some(0.5), "0* → minimal 0.5");
        assert!(is_zero(&arc_damage(&c, Arc::Nose, WeaponClass::Cap)), "absent class = all zero");
    }

    #[test]
    fn arc_lines_skips_empty_classes() {
        let c = union();
        let lines: Vec<WeaponClass> = arc_lines(&c, Arc::Nose).into_iter().map(|(w, _)| w).collect();
        assert_eq!(lines, vec![WeaponClass::Std, WeaponClass::Msl], "Nose carries STD + MSL only");
        assert!(arc_lines(&c, Arc::Left).is_empty(), "an empty arc has no lines");
    }

    #[test]
    fn to_hit_mods_and_threshold() {
        assert_eq!(WeaponClass::Cap.to_hit_mod(), 3);
        assert_eq!(WeaponClass::ScAp.to_hit_mod(), 2);
        assert_eq!(WeaponClass::Msl.to_hit_mod(), 0);
        assert!(WeaponClass::Msl.is_capital() && !WeaponClass::Std.is_capital());
        assert!(threshold_triggered(5.0, 5));
        assert!(!threshold_triggered(4.0, 5));
        assert!(!threshold_triggered(0.5, 1), "minimal never meets a threshold");
        assert!(!threshold_triggered(10.0, 0), "no threshold → never triggers");
    }

    #[test]
    fn arc_display_preserves_printed_strings() {
        let c = union();
        assert_eq!(arc_class_display(&c, Arc::Nose, WeaponClass::Std).as_deref(), Some("4/3/2/0*"));
        assert_eq!(arc_class_display(&c, Arc::Nose, WeaponClass::Cap), None, "empty class → None");
        assert_eq!(
            arc_display_lines(&c, Arc::Nose),
            vec![
                (WeaponClass::Std, "4/3/2/0*".to_string()),
                (WeaponClass::Msl, "1/1/1/1".to_string()),
            ]
        );
        assert!(arc_display_lines(&c, Arc::Left).is_empty());
    }
}
