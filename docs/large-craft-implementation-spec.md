# Large Craft & Capital-Scale Aerospace — Implementation Spec

Making DropShips, Small Craft, JumpShips, WarShips, and Space Stations **fieldable as units inside the
existing BattleForce-ladder modes** — Standard BattleForce (BF), Strategic BattleForce (SBF), and the
Abstract Combat System (ACS). This is the "Phase 2" aerospace initiative in `ROADMAP.md` ("Large craft &
capital-scale aerospace"). Fighters (Phase 1) already ship in Classic, Alpha Strike, BF, and SBF.

**Decisions (2026-07-09, recorded in ROADMAP):** target the **full ladder at faithful IO:BF fidelity**
(the multi-arc, capital-weapon-class Alpha Strike / BattleForce card), delivered in phases DropShips-first,
implemented as a **shared `crates/core/src/engine/large_craft.rs` layer** that BF, SBF, and ACS each
consume rather than triplicating the arc/capital logic.

> **Citations.** `IO:BF p.NN` = *Interstellar Operations: BattleForce*. As with the SBF/BF/ACS specs, the
> `p.NN` we cite is the **PDF page**, which runs **~2 ahead of the printed folio** (our p.191 = printed
> "189"). Values transcribed only from text extraction are collected in §9 (**NEEDS RULEBOOK**) and must be
> checked against a printed copy before coding.

---

## 0. Scope & doctrine

**The board stays at the table — same as every other mode.** neurohelmet tracks *record-sheet state* and
computes *numbers*. For large craft that means: the converted stat line (per-arc weapon-class damage, the
single Armor/Structure/Threshold pool), crits, and the to-hit / damage calculators. Everything positional
is the player's/GM's job and is **out of scope**: the Capital Radar Map and its zones (Central / Inner /
Middle / Outer / Peripheral), Engagement Maps, sector adjacency, capital movement rates, tailing /
engagement-control positioning, gravity / landing / liftoff / crash, altitude & velocity, fuel endurance,
and hyperspace-jump geometry. The player selects the *outcome* of that geometry (which arc(s) fired, the
struck arc, the range bracket, whether an engagement was won); the app crunches the rest. See §10.

**The fieldable ladder** is six Alpha-Strike type codes, all `type == "Aero"` in the source data:

| TP | Unit | Baked count | Weapon classes present in data |
|----|------|-------------|--------------------------------|
| `SC` | Small Craft (aerodyne + spheroid) | 39 | STD only |
| `DS` | Spheroid DropShip | 123 | STD, **SCAP**, **MSL** |
| `DA` | Aerodyne DropShip | 86 | STD, **SCAP**, **MSL** |
| `JS` | JumpShip | 29 | STD (+ little SCAP) |
| `SS` | Space Station (military + civilian) | 33 | STD, **CAP**, SCAP, MSL |
| `WS` | WarShip | 123 | STD, **CAP** (heavy), SCAP, MSL |

**RAW type separation (no mixing).** IO:BF p.262 Step 3B: *"Aerospace and Large Aerospace are both
classified as Aerospace (AS)… Aerospace and Ground types may not be mixed in Abstract Combat System play."*
The same ground/aero separation holds in BF and SBF. So large craft never join a ground Unit/Formation —
they field alongside fighters in aerospace groupings, and the app enforces this by construction (§6, §7).

**MegaMek is a data-model / conversion parity source ONLY — never a combat-resolution reference.** MegaMek
has no faithful IO:BF large-craft resolver: its only large-aero engine is the abstract SBF autoresolve
(`autoresolve/acar/…/FiringPhase.java:165`), which fires with formation-level `getStdDamage()` and **never
reads arcs, CAP/SCAP/MSL, or threshold**. MegaMek confirms the *data model* (arc → class → S/M/L/E), the
conversion rounding, the threshold formula, and the SUA decode — but every combat rule (threshold checks,
arc firing, the weapon-class to-hit table) is specced from IO:BF directly.

---

## 1. The data reality — transcription, not conversion

The single biggest de-risking finding: **the source `units.json` already carries the full multi-arc
BF card** for all 433 arc-using craft. Baking is transcription; no MegaMek `ASConverter` port is needed.

Each arc-using unit's `as` block has:

- `frontArc`, `leftArc`, `rightArc`, `rearArc` — **exactly 4 arcs** (the AS/BF card, *not* the 6-arc Total
  Warfare WarShip record sheet; confirmed by MegaMek `ASArcs{FRONT,LEFT,RIGHT,REAR}`).
- each arc split into weapon classes `STD`, `CAP`, `SCAP`, `MSL`, each with `dmgS/M/L/E` strings.
- each arc **also carries its own `specials` array** (e.g. front arc `["ENE","PNT1"]`) — arc-level specials,
  not just unit-level. The schema must preserve per-arc specials.
- a **single unit-wide** `Arm` / `Str` / `Th` pool (e.g. Aegis WarShip `Arm 193 / Str 75 / Th 16`) —
  **not** per-arc. Probe confirmed: arc blocks contain only `STD/CAP/SCAP/MSL/specials`, no per-arc `Th`.
- `usesArcs: true`, `usesTh: true`, `SZ`, `PV`, `OV`, `TMM`, and unit-level `specials`
  (`AT20-D4`, `CK86-D1`, `CRW3`, `DT4`, `KF`, `LF`, `MFB2`, `SPC`, `ST10-D2`, `PNT#`, `CT#`, `VLG`, …).

**No capital↔standard damage rescale.** The arc damage numbers are already final BF-scale (a WarShip front
`CAP.dmgS` is `170`–`335`). There is **no ×10 / ÷10 step** when a capital arc hits a standard-scale target,
and none the other way. The CAP/SCAP/MSL classification drives only (a) the **to-hit weapon-class modifier**
(§4, the p.191 table) and (b) the **Random Weapon Class** crit selection (§6). This is confirmed by the data,
by MegaMek's `ASArcedDamageConverter` (these *are* the final AS values), and by IO:BF p.95's per-arc
attack structure. → §9 NEEDS RULEBOOK item 1 (confirm no BF-scale multiplier on a printed copy).

**Preserve the `0*` minimal-damage token.** 53 arc cells carry `0*` (MegaMek `ASDamage.minimal`: a value
`0 < v < 0.5`, stored `damage=0, minimal=true`). It is **not** zero — the schema, threshold test, and UI
must carry it as a distinct state (renders `0*`, rolls minimal damage on hit, cannot itself meet a
threshold). Do not collapse to `0`.

**Movement (MV) strings by TP** (thrust, already how `parse_movement` tokenizes value+mode):
`DS = "3p"` (spheroid), `DA = "5a"` (aerodyne), `SC = "5p"`, `WS = "2"` (bare — the `Warship` move mode),
`JS`/`SS = "0.2k"` (fractional thrust + station-keeping `k`). JumpShips/Space Stations are Thrust-0 /
station-keeping by rule; the fractional `0.2` clamps to 0 for the tracker (they auto-fail engagement
positioning, IO:BF p.190).

**Derivation formulas (parity only — our data is pre-baked, but these anchor "faithful"):** MegaMek
`ASArmStrConverter` — Threshold = `roundUp(fullArmor / 3 / arcs)` with `arcs = 4` for large aero; Armor =
`round(0.33 × total TW armor)` (capital-scale); Structure = `SI` for WarShips, `ceil(0.5 × SI)` for
DropShips/Small Craft, `1` for JumpShips/Space Stations. Arc damage rounds **up** (`createUpRndDmgMinus`).

---

## 2. The shared engine — `crates/core/src/engine/large_craft.rs`

One module owns the arc/weapon-class model, the (rescale-free) damage selector, the threshold gate, the
crit columns, and the capital-scale to-hit modifier table. BF, SBF, and ACS each build only their
mode-specific to-hit context and call the shared functions; the crit / threshold / damage-selection code is
identical across modes.

```rust
/// The four AS/BF firing arcs (card arcs, NOT the TW 6-arc WarShip sheet).
pub enum Arc { Nose, Left, Right, Aft }   // maps front/left/right/rear from the source card

/// AS/BF weapon classes. Order fixed; append only.
pub enum WeaponClass { Std, Cap, ScAp, Msl }

/// One weapon-class line within an arc: an S/M/L/E vector (DamageVector already models 0*/minimal).
pub struct ArcLine { pub class: WeaponClass, pub dmg: DamageVector }

/// One arc: its class lines plus arc-level specials (e.g. ENE, PNT#).
pub struct ArcCard { pub lines: Vec<ArcLine>, pub specials: Vec<String> }

/// The transcribed large-craft card. Single Arm/Str/Th pool (data-confirmed), 4 arcs, attack budget.
pub struct LargeCraftCard {
    pub arcs: [ArcCard; 4],       // indexed by Arc
    pub armor: u16,
    pub structure: u16,
    pub threshold: u8,            // SINGLE pool, not per-arc
    pub tp: LargeCraftKind,       // Sc|Ds|Da|Js|Ss|Ws
    pub attack_limit: u8,         // Large Aerospace Attack Limits (§ per-mode): DS/JS/Sat 4, SS 6, WS 8
    pub specials: Vec<String>,    // unit-level SUAs (KF, LF, DT#, CRW#, PNT#, SCR#, …)
}

/// Damage from a set of (arc, class) selections at one range band. Pure selector+SUM — NO capital rescale.
pub fn arc_attack_damage(card: &LargeCraftCard, shots: &[(Arc, WeaponClass)], band: Range) -> DamageVector;

/// Threshold gate (IO:BF p.40): a single attack whose damage >= threshold triggers a crit roll.
/// Armor still absorbs the damage (threshold does NOT bypass armor into structure). 0* never triggers.
pub fn threshold_triggered(single_attack_dmg: u16, card: &LargeCraftCard) -> bool;

/// Crit column selection + the multi-class arc resolver.
pub enum CritColumn { Aerospace, DropShip, JumpShip }
pub fn crit_column(tp: LargeCraftKind) -> CritColumn;   // §5 routing table
pub fn random_weapon_class(d6: u8) -> WeaponClass;       // p.190: 1-2 Std, 3-4 Cap(non-missile), 5-6 Msl

/// The Capital-Scale Aerospace To-Hit Modifiers table (IO:BF p.191), shared by BF/SBF-LA/ACS-aero.
pub enum CapitalToHitMod { /* range, weapon-class, atmospheric, target-type, misc rows — §4 */ }
pub fn capital_to_hit(ctx: &CapitalShotCtx) -> i64;

/// Weapon-class to-hit penalty, WAIVED when the target is itself large craft (the p.191 footnote).
pub fn weapon_class_mod(class: WeaponClass, target_is_large_craft: bool) -> i64; // CAP +3/SCAP +2 else 0
```

`DamageVector` is the existing `engine/as_element.rs` type and already carries the `0*`/minimal state, so it
is reused verbatim for arc lines.

---

## 3. Bake & schema (the shared foundation — do first)

**Bake** (`crates/bake`):

- `join.rs:310` `is_aero_fighter` → add a sibling `is_large_craft(unit)` = `type == "Aero" && subtype ∈
  {Spheroid/Aerodyne DropShip, Aerodyne/Spheroid Small Craft, JumpShip, WarShip, Military/Civilian Space
  Station}` (later phases may add Mobile Structure / large Support Vehicles).
- `main.rs:117` top filter → admit `is_large_craft` (currently dropped alongside the fighter gate).
- `join.rs:251` `parse_as_stats` → when `a["usesArcs"] == true`, parse the four `frontArc/leftArc/rightArc/
  rearArc` × `{STD,CAP,SCAP,MSL}` `dmgS/M/L/E` (**preserving `0*`**) and per-arc `specials` into the new
  `AsStats.arcs`; keep the single-`dmg` path for non-arc units.
- `join.rs:554` `build_aero` parses *fighter* armor arcs from the record-sheet SVG; large-craft arc data
  comes from the AS card (`usesArcs`), not the SVG — route large craft to a new `build_large_craft` that
  carries the transcribed `ArcCard`s and the single `Arm/Str/Th` pool.

**Domain** (`crates/core/src/domain.rs`):

- `UnitType` (`:522`, flat `Mech/Vehicle/Infantry/BattleArmor/Aerospace`) → append `LargeCraft` **last**
  (serde discriminant stability, same rationale as the `Aerospace` note at `:533`), or reuse `Aerospace`
  with a `LargeCraftKind` discriminator — decide during Phase 1.
- `AsStats` (`:649`, single `dmg_s/m/l/e` + `threshold`) → add `arcs: Option<ArcCard4>` + `uses_arcs: bool`,
  both `#[serde(default)]` so pre-existing bundles/sessions still load. Keep the flat fields for non-arc
  units (fighters, ground).
- `Location` (enum `:28`; aero arcs ~`:66`: `Nose/LeftWing/RightWing/Aft/AeroSI`) already models 4 aero arcs
  + an SI pool; reuse
  `Nose / LeftWing→Left / RightWing→Right / Aft` as the large-craft arc labels for the single armor pool +
  crit tracking (large craft use one armor pool, so the doll is a display of the single pool, not 4 pools).

**Type routing** (`crates/core/src/engine/as_element.rs`): `sbf_type_from_tp` (`:186`) already funnels
`DS/DA/WS/JS/SS` to `SbfElementType::La` via the `_ =>` arm (`SC → As` at `:193`). Make the `WS/DS/DA/JS/SS`
arms **explicit** (documents reachability + makes the `Warship` move mode reachable), and ensure
`as_element()` (`:202`) populates the `ArcCard` rather than flattening to a single `DamageVector`.

**Bundle:** this bumps the bundle format → re-bake + application rebuild, and a **`data/mechs.bin` ~10 MB
regression check** (a filtered bake clobbers it — project memory). Add ≥1 golden/snapshot test per phase
driving a real DropShip/WarShip end-to-end.

---

## 4. The damage-resolution sequence (the spine)

The one pipeline all three modes share, built on `large_craft.rs`. Steps marked **⚠** transcribe a rule that
must be verified against a printed copy (§9).

```
1. ATTACK DECLARATION (player-driven; the table resolves geometry):
     pick firing arc(s) + weapon class(es)  — bounded by the unit's Attack Limit (DS/JS/Sat 4, SS 6, WS 8),
       one to-hit roll per (arc, class); one primary target, extras are secondary (+1).       [IO:BF p.190]
     pick range band S/M/L/E  — for CAP/SCAP/MSL, reduce the chosen bracket by 1 (min Short).  [IO:BF p.190]⚠
2. TO-HIT (mode-specific wrapper, §5/§6/§7) = base skill
     + range mod (aero: S+0 / M+1 / L+2 / E+3 — NOT the ground ladder)
     + weapon_class_mod(class, target_is_large_craft)   // CAP +3 / SCAP +2, WAIVED vs large-craft targets ⚠
     + target-type / atmospheric / misc rows (the p.191 table, §4a)
3. ON HIT: damage = arc_attack_damage(card, shots, band)   // NO rescale — the card value IS the damage
4. APPLY DAMAGE to the SINGLE armor pool, then structure  [IO:BF p.40 "Applying Damage"; ASCE p.49 Q4]⚠
     armor first: subtract from Arm until 0; overflow into Str; craft destroyed when Str exhausted.
5. THRESHOLD → CRIT: if this single attack's damage >= Threshold, owe a crit roll  [IO:BF p.40; TW p.239]
     (threshold TRIGGERS a crit; it does NOT bypass armor. 0*/minimal can never meet a threshold.)
     roll 2D6 on crit_column(tp) (§5). On a Weapon result vs a multi-class arc, roll Random Weapon Class
       (1D6: 1-2 Std / 3-4 Cap / 5-6 Msl) to pick which class line is knocked out.               [IO:BF p.190]
```

Consistent with the manual-dice doctrine, the app **tells the player a crit roll is owed** (mirrors SBF's
`crit_check_due`) and applies the effect the player enters — it never auto-rolls.

**Display-vs-resolved contract (keeps every phase honest).** A unit's card may carry class lines the current
phase does not yet *resolve* (e.g. a Phase-1 DropShip's `SCAP`/`MSL` lines before capital resolution lands).
Every such line is **transcribed and displayed faithfully but flagged read-only** with an explicit
"capital-weapon resolution deferred — resolve at table" note, so a player is never misled into thinking a
capital line was computed. The calculator resolves only the classes the active phase supports.

### 4a. Capital-Scale Aerospace To-Hit Modifiers Table (IO:BF p.191) ⚠

The shared `CapitalToHitMod`. **This is a distinct table from the already-built p.179 standard-scale
Strategic-Aerospace table** (§6 warning). Transcribed from text extraction — verify the misc block (§9).

- **Range:** S +0 / M +1 / L +2 / E +3.
- **Weapon class used:** Capital non-missile (CAP / SDS-C) **+3**; Sub-capital non-missile (SCAP / SDS-SC)
  **+2**; Capital or sub-capital missiles (MSL / SDS-CM) **+0**; Standard **+0**. *Applies only when the
  target is a Squadron type **other than** DropShips/JumpShips/stations/WarShips* — waived vs large craft.
- **General:** Atmospheric Combat +2 (only if both attacker and target are at/below the atmosphere
  interface); Attacker is a Grounded DropShip −2.
- **Target type:** Airborne Aerospace +2 (only if the attacker is *not* itself an airborne aerospace
  Squadron); Airborne DropShip −2; Airborne VTOL/WiGE +1; Small Craft −1; Target Crippled/Drifting (thrust
  loss / shutdown) −2.
- **Misc:** High-Speed Attack +8; Point Defense (`PNT#`) vs cap/sub-cap missiles: 1 pt → +1, 2+ pts →
  auto-fail; Screen Launchers (`SCR#`) → +SCR (max +4, counts against the attack limit); Secondary Target
  +1; Attacker Fire-Control damaged +2 per hit (cumulative); Support-Vehicle/Satellite fire control
  AFC +0 / BFC +2 / neither +2; Attacker Behind the target +1 (fighters/small craft only; not vs
  station-keeping); Teleoperated missiles −1; Advanced Capital Missile vs same sector +0 / adjacent +2.
- **Air-to-ground:** Altitude Bombing +3 / Dive Bombing +2 / Strafing +4 / Striking +2.
- **Ground attack (SDS / orbit-to-surface):** Surface-to-Surface (non-stationary) +2; Orbit-to-surface
  base +3; SDS vs Central Zone +0 / other zone +3; TAG-designated −2. ⚠ (zone splits extract ambiguously.)

---

## 5. Standard BF integration

BF is the closest to ready — most hooks exist and light up on bake.

- **Crit columns.** `bf_crit_col` (`battleforce.rs:604`) already routes `SC|DS|DA|JS|WS|SS → BfCritCol::
  DropShip`. The `BfCrit` enum (`:577`) already declares the full large-craft vocabulary (`Engine`,
  `FireControl`, `Weapon`, `Fuel`, `CrewStunned`, `CrewKilled`, `KfBoom`, `DockingCollar`, `Thruster`,
  `Door`, `CrewHit`, `Ammo`). **Add:** `BfCritCol::JumpShip` + `BfCrit::KfDrive`, and re-route
  `JS/WS/SS → JumpShip` (keep `SC/DS/DA → DropShip`; Satellites → `Aerospace`). ⚠ Confirm the p.87 Expanded
  crit table's JumpShip column + KF-drive row against a printed copy (§9).
    - DropShip column (2D6, verified footnote-correct at `battleforce.rs:691`): 2 KF-Boom, 3 Docking-Collar,
      4 —, 5 FCS, 6 Weapon, 7 Thruster, 8 Weapon, 9 Door, 10 —, 11 Engine, 12 Crew-Hit.
    - JumpShip column (2D6, **new** ⚠): 2 Door, 3 Dock, 4 FCS, 5 —, 6 Weapon, 7 Weapon, 8 Thruster, 9 —,
      10 K-F-Drive, 11 Engine, 12 Crew-Hit.
- **Crit EFFECTS** (`bf_apply_crit_row`, `app.rs:3813`) — today a bare mark; wire real effects. Several need
  **new persistent per-unit stage counters** on `TrackedMech.bf` (a save-format addition):
    - **Weapon** = one weapon-class line in one arc ×0.5 (round down) — with the Random Weapon Class 1D6
      pick on a multi-class arc.
    - **Engine** (DS/SC) = the 3-stage thrust ladder −25% / −50% / shutdown (IO:BF p.43); (large SV) = roll
      2D6 per hit, 8+ → −1 MP.
    - **Crew Hit** ladders ⚠ (differ by type): DropShip / JumpShip / WarShip / Space Station / large SV /
      Satellite each have their own +2 / +4-total / eliminate progression — verify against p.87.
    - **K-F Drive** = −2 drive integrity per hit, 0 → no jump; **KF Boom** = KF drive destroyed (no
      hyperspace); **Docking Collar / Dock** = −1 DT rating per hit; **Door** = −1 transport bay door;
      **Thruster** = maneuver/thrust loss (positional — a mark).
- **To-hit — no new modifier code needed for large craft as targets.** `BfTargetKind::AirborneDropship`
  (−2, `:518`) is already a hand-set target row; `bf_to_hit` (`:429`) already has the grounded-aerospace
  branch (`DS|DA → −2` else `+2`) and the FLK ground-to-air row (`:539`). The work is (a) baking large craft
  as fieldable **attackers** and (b) the crit effects above.
- **Shot builder** (`view.rs`): a per-arc UI — pick arc → weapon class → range band → BF to-hit → pull the
  arc/class `dmgS/M/L/E` — bounded by the Attack Limit.

---

## 6. SBF integration

- **`Large Aerospace (LA)` is a first-class IO:BF type** (p.183): *"a new unit type… Large Aerospace (LA),
  which includes DropShips, JumpShips, WarShips and Space Stations."* This is exactly the existing
  `SbfElementType::La` (`as_element.rs:58`); `sbf_type_from_tp` already collapses the codes to `La`, and the
  dead `SbfMoveMode::Warship` (`sbf.rs:43,147`) becomes reachable on bake. `sbf_can_convert` (`session.rs:
  2884`) already partitions ground/aero — extend it to recognize `La` and keep the 4-arc card (do **not**
  collapse to a single `DamageVector`).
- **Squadron composition (tracked)** (p.183): DropShip Squadron ≤ 6 (one DropShip per Flight); WarShip
  Squadron = 1 WS + ≤ 4 more Flights (total 5); station-keeping Squadron ≤ 2 Space-Station or 6 JumpShip
  Flights. **Large Aerospace Attack Limits** (p.190): per Flight per turn — DropShips 4, JumpShips 4,
  Satellites 4, Space Stations 6, WarShips 8. ⚠ verify limits.
- **⚠ Use a NEW capital-scale to-hit path — do NOT reuse `SbfAeroShot`.** IO:BF p.190 Step 4: *"The
  Capital-Scale Aerospace To-Hit Modifiers Table (p.191) replaces the standard Strategic Aerospace To-Hit
  Modifier Table."* The existing `SbfAeroShot`/`SbfAeroKind`/`SbfAeroTarget` machinery (`sbf.rs:935-1090`)
  is the **standard-scale p.179 table** for AS Squadrons and lacks the weapon-class rows — extending it
  would silently apply the wrong table. Add an `SbfCapitalShot` (or a `capital: bool` + `WeaponClass` leg on
  a shared enum) keyed to the shared `capital_to_hit` (§4a). The existing `SbfAeroTarget` variants
  (`AirborneDropship`/`SmallCraft`/`GroundedSquadron`) are reused.
- Capital/sub-capital ranges reduce the chosen bracket by 1 (min Short); the Maneuver-Roll that *selects*
  the bracket is table-side. `SCR#` counts against the attack limit and modifies both sides' to-hit.
- **Table-side (out of scope):** Capital Radar Map zones/sectors, Engagement Maps, capital movement rates
  (Thrust ×0.5/0.25/0.1 by zone, p.186), fuel endurance, gravity/landing/liftoff/crash, hyperspace jumps
  (pp.193–194), tailing/engagement-control. Firing arcs do **not** pick targets in Strategic Aerospace
  (no facing) — "firing arcs determine the number of attacks" (p.189); the player picks which arcs fired.

---

## 7. ACS integration

- **Already half-wired.** IO:BF p.262: aerospace + large aerospace both classify as `AS` and never mix with
  ground. `merge_type_phase3` (`acs.rs:264`) already does `La → As`; `combat_unit_from_teams` (`:282`) sets
  `is_aero` for `As|La`. Aerospace Formations are 1–4 Combat Units (ground are 2–6; p.243).
- **Wire the dead guard.** `AcsFormation::is_aerospace` (`acs.rs:127`) is referenced only by a test (`:1053`)
  and `pdf.rs` — it never gates anything in prod, so aero elements today would **silently ground-aggregate**.
  Make it the live branch key: `acs_new_formation` (`session.rs:3403`) must (1) enforce As/La-only vs ground-only
  composition, (2) size-limit 1–4 for aero, (3) route aero Formations to the aero to-hit/damage path + the
  Ground-Support missions. *(This guard fix is a Phase-0 cheap win, valuable independent of the rest — §8.)*
- **Range: add Extreme, distinct ladder.** `AcsRange` (`acs.rs:576`) is ground-only `Short(−1)/Medium(+2)/
  Long(+4)`, no Extreme. The ACS **Aerospace** table (p.252) uses `S+0 / M+1 / L+2 / E+3`. Add a separate
  aero range path — **do not reuse `AcsRange::to_hit_mod`** (it would silently mis-modify every aero shot).
- **Calculators:** `AcsToHitCtx` (`:681`) / `AcsDamageCtx` (`:750`) have no aero fields. Add an
  `AcsAeroToHitCtx` mirroring the p.252 aero to-hit table and the p.242–243 **Master Modifier Table
  (Aerospace)** — weapon-class rows, orbit-to-surface, high-speed +8, point-defense, and the large-craft
  combat rows: aero-vs-WarShip −3, aero-vs-DropShip −2, DropShip-vs-aero +2, DropShip-vs-WarShip −2,
  WarShip-vs-aero +5, WarShip-vs-DropShip −1 (p.243). ⚠ The Master Modifier table is multi-column and
  extracts poorly — verify per-cell values on a printed copy (§9).
- **Ground-Support missions (calculators, geometry-free)** (p.253): CAP (−1 engagement); Ground Strike →
  Strike (½ Short-range damage; may trade attacks for −1 TN each, max −3, or +0.1 dmg each, max +0.5) or
  Bomb (BOMB rating in 5-pt clusters); Aerial Recon (−4 recon); Orbit-to-Surface / Surface-to-Orbit (per
  SBF p.190: primary = ¼ Combat-Unit damage +1, min 1; secondary = ½ primary; scatter 5–6); Combat Drop
  (the Combat Drop Results Table, MoS → drop value + modifier + drop-damage %). Combat-Drop and Orbital-
  Bombardment **morale triggers** on the ground side stay as the existing manual morale-calculator entries.

---

## 8. Phasing

Each phase is independently shippable, testable, and quarantines the capital-weapon rulebook uncertainty
into Phase 2. Every phase adds a golden/snapshot test driving a real unit end-to-end, and re-verifies
`data/mechs.bin` stays ~10 MB.

**Phase 0 — two decoupled cheap wins** (small, no schema change, valuable now):
1. Wire the never-called `AcsFormation::is_aerospace` guard (fixes the silent ground-aggregation latent bug).
2. `isAerospaceSV` routing (`SV → As` vs `V`) — needs a baked data flag.

**Phase 1 — DropShips + Small Craft in Standard BF.** The shared foundation (bake + `AsStats.arcs` +
`large_craft.rs` skeleton: `ArcCard`, `WeaponClass`, `arc_attack_damage`, `threshold_triggered`, the
DropShip crit column + effects). Small Craft are STD-only; DropShips carry SCAP/MSL — those lines are
**transcribed + displayed** under the §4 display-vs-resolved contract, with the calculator resolving STD
(and, if cheap, SCAP/MSL via the same rescale-free selector — decide during build). Reuses the most existing
hooks (DropShip crit column, `AirborneDropship −2`, grounded-DropShip, A2G kinds). Highest value / lowest
rulebook risk; needs no capital-scaling verification.

**Phase 2 — capital weapons + JumpShip / WarShip / Space Station in BF. ✅ COMPLETE (2026-07-10).**
Baked 185 new large craft (JS 29, WS 123, Mil SS 22, Civ SS 11 → 433 total; bundle v23, ~10.9 MB). Added the
`BfCritCol::JumpShip` column + `BfCrit::{KfDrive, Dock}`, re-routed JS/WS/SS → JumpShip, and made the
large-craft crit arms stateful (`BfLive`: `crew_hit`/`kf_drive`/`dock_hits`/`door_hits` counters +
`kf_boom`/`docking_collar`/`thruster` flags) with the verified ladders: DropShip/SC crew-hit 2-stage
(+2/eliminate), JS/WS/SS crew-hit 3-stage (+2/+4/eliminate); K-F Drive −2/hit
(inert on Space Stations); Dock −1 DT/hit; Door −1/hit — all seeded from baked `AsStats.{dt_rating, door_count}`
(new `DT#` / `-D#` parse). The **Weapon crit** (halve one random arc ×0.5) is described + resolved at the
table (the random-arc pick is table-side, spec §10), not auto-applied. Capital classes (CAP/SCAP/MSL) now
RESOLVE their to-hit in the shot builder. No
damage rescale — capital is a to-hit + crit-class distinction only.

> **KEY CORRECTION to §4a/§9 (verified on the rulebook 2026-07-10).** Standard BF does **not** use the
> Capital-Scale Aerospace To-Hit table (p.191) — that table belongs to the *Advanced Strategic Aerospace*
> (SBF) subsystem and is deferred to Phase 3. Standard BF large-craft fire uses the **Advanced Combat
> Modifiers Table (IO:BF p.83)**: the existing `+0/+2/+4/+6` range ladder (unchanged) plus **"Capital Weapon
> vs. Small Target +5 / Sub-Capital +3"** (footnote 28 — applies only vs an aerospace fighter / fighter
> squadron / Small Craft / Satellite; `+0` vs large craft and vs ground). Missiles and standard weapons take
> `+0`. There is **no capital bracket-reduction and no 8/6/4 attack cap in standard BF** (a BF WarShip may
> make up to 16 attacks: 4 arcs × 4 classes, p.95); those are SBF rules. `WeaponClass::to_hit_mod` (CAP+3/
> SCAP+2, p.191) is retained but marked SBF-only; the BF path uses the new `WeaponClass::bf_vs_small_mod`.

**Phase 3 — SBF Advanced Strategic Aerospace. ✅ COMPLETE (2026-07-10).** Threaded the 4-arc card through
the SBF Unit model (`SbfUnit.arcs`, populated in `convert_unit` **only for `La`** — Small Craft are baked
with a card but are a standard `As` Squadron in SBF, p.183). The capital-scale p.191 to-hit is a **`capital:
Option<SbfCapital>` leg on the existing `SbfAeroShot`** (not a parallel `SbfCapitalShot` — the p.191 table
shares the range ladder, target-type rows, and SV-fire-control ladder with the built p.179 table). Full
p.191 legs: weapon-class `WeaponClass::to_hit_mod` (CAP+3/SCAP+2, waived vs large craft), the `capital_range`
bracket-reduction −1 (to-hit **and** damage), High-Speed +8 (suppresses Naval-C3/Teleoperated/Point-Defense/
Screen incl. the auto-fail), Atmospheric +2, Point-Defense (PNT#, 1→+1 / 2+→auto-fail vs missiles), Screen
(SCR# max +4), Naval-C3 −1, Teleoperated −1, Crippled −2, Grappled −4, ACM same/adjacent (+0/+2), and the
LA-tailer −1 behind adjustment. `random_weapon_class(1D6)` + a crit-modal describe note (table-side per the
manual-dice doctrine — no persisted per-arc knockout). LA composition (WarShip ≤1, Space Station ≤2, each a
1-element Flight) is advisory in `sbf_can_convert`; the per-Flight Attack Limits (DS/JS 4, SS 6, WS 8) are an
advisory readout. `SbfMoveMode::Warship` is now reachable (WarShips bake). **SBF has NO Threshold** — the
existing below-half-armor `crit_check_due` gate + generic SBF crit table already cover LA (single armor pool).

**Phase 4 — ACS Abstract Combat Aerospace.** The aero range path (+Extreme), `AcsAeroToHitCtx`/damage
mirroring p.252 + the Master Modifier (Aerospace) pp.242–243, Ground-Support missions, and the live
`is_aerospace` gate/composition rules. Last — depends on the Phase-3 capital-scale to-hit shape and has the
worst-extracting source tables.

---

## 9. NEEDS RULEBOOK — verify against a printed copy

Every value below reached the spec via btrules **text extraction** (PDF folio runs ~2 ahead) or an
inference, and must be confirmed on a printed IO:BF before it is coded.

**✅ Verified & resolved in Phase 2 (2026-07-10, btrules re-read):** Item **3** (crit columns) — DropShip &
JumpShip columns confirmed cell-for-cell on p.85/PDF 87; WarShips + Space Stations use the JumpShip column
(footnote **); Satellites use the DropShip column (footnote ‡). Item **4** (crit ladders) — Weapon = halve
one random arc (×0.5, table-side); DropShip/SC crew-hit 2-stage, JS/WS/SS 3-stage; K-F Drive −2/hit (inert on Space Stations);
Dock −1 DT; Door per-bay. Item **7** (Random Weapon Class 1D6: 1-2 Std / 3-4 Cap-non-missile / 5-6 Msl) —
confirmed; the DS/JS/Sat 4, SS 6, WS 8 **attack limits are SBF (Strategic-Aerospace) caps, NOT standard BF**
(a BF WarShip gets 16). Item **1/5** (to-hit) — **BF uses the p.83 Advanced Combat Modifiers Table, not the
p.191 capital-scale table** (see the Phase-2 §8 correction); confirmed no damage rescale. Discrepancy fixed:
p.191 "behind target" is −2, not the earlier +1. Items **2, 6, 8** remain open (SBF/ACS phases).

Remaining to confirm against a printed IO:BF before the later phases code them:

1. **Capital↔standard damage — no rescale.** Confirm BF applies the arc's `dmgS/M/L/E` directly (the class
   is a to-hit + crit-class distinction, not a damage multiplier). Data + MegaMek strongly support this;
   verify there is no BF-scale ×10 step.
2. **IO:BF p.40 "Applying Damage"** — transcribe the numbered yes/no steps verbatim; confirm armor-absorbs-
   first + threshold-triggers-crit (does **not** bypass armor). (TW p.239 + ASCE p.49 Q4 corroborate.)
3. **p.87 Expanded crit table** — confirm the separate JumpShip column set and the K-F-Drive row, and that
   Satellites use the Aerospace column / Space Stations use WarShip-style crits.
4. **Crew / Engine / K-F crit LADDERS** — the per-type stage counts (+2 / +4 / eliminate), the DS/SC
   −25%/−50%/shutdown engine ladder, the large-SV 2D6≥8 → −1 MP, and K-F −2/hit.
5. **Capital-Scale To-Hit table (p.191) misc block** — the Point-Defense / Screen / Secondary / Crippled /
   Teleoperated / orbital-artillery-zone rows extracted as an unaligned list; confirm each condition→value.
6. **ACS Aerospace To-Hit table (p.252) + Master Modifier Table (Aerospace) pp.242–243** — the multi-column
   Master table extracts poorly; verify the aero cell values and the orbit-to-surface zone splits (note the
   BF p.191 and ACS p.242 orbit-to-surface ladders differ — do not conflate).
7. **Large Aerospace Attack Limits** (DS/JS/Sat 4, SS 6, WS 8) and the **Random Weapon Class** 1D6 mapping
   (1-2 Std / 3-4 Cap / 5-6 Msl), applied only to multi-class arcs.
8. **DropShip/Small-Craft CAP** — MegaMek's card omits CAP for SC/DS/DA (data agrees: 0 CAP cells) but the
   converter still processes CAP; confirm the printed DS card layout is STD/SCAP/MSL only.

**Already resolved by data probe (not open):** Threshold is a **single pool**, not per-arc (arc blocks hold
only STD/CAP/SCAP/MSL). DropShips **do** carry SCAP + MSL (not just STD); only Small Craft are STD-only.
`0*` minimal tokens are present (53 cells) and must be preserved.

---

## 10. Out of scope / known limitations

- **All positional/table machinery:** Capital Radar Map & zones, Engagement Maps, sector adjacency, capital
  movement rates, altitude/velocity, tailing/engagement-control, gravity, landing/liftoff/crash, hyperspace-
  jump geometry, fuel endurance. The player selects outcomes; the app crunches numbers. (Same doctrine as
  terrain/LOS/facing in every other mode.)
- **Which arc is struck** is table geometry (attack direction) — the player picks the struck arc; the app
  tracks the single armor pool + per-arc weapon crits.
- **Inter-unit transport / jump dependencies** (a KF-Boom on a carrier blocking the elements it transports
  from jumping; bay-door throttling of launch/recovery) are **left to the table** — neurohelmet tracks
  units in isolation. The `DT` rating and `-D#` door counts *are* baked so the Dock/Door crit effects can
  decrement a number, but the cross-unit consequence is the player's to adjudicate.
- **MegaMek** is used only to confirm the data model, conversion math, and SUA decode — **never** as a
  combat-resolution reference (it has none for large craft; §0).
- **Three DISTINCT to-hit tables, kept separate by design:** BF single-Element uses the p.39 ground rows;
  SBF-LA uses the **new** capital-scale p.191 table (not the built p.179); ACS-aero uses the **new** p.252
  table + Master Modifier (not `AcsRange`). Each existing structure carries a "do not extend/reuse" note.
- **Serde/enum stability:** append new `UnitType` / `WeaponClass` / `BfCritCol` / `BfCrit` / `SbfMoveMode` /
  `AcsRange` variants **last**; new `AsStats`/`TrackedMech.bf` fields are `#[serde(default)]`.

---

## 11. New persistent state (save-format additions)

Called out explicitly because these are `session.json` migrations (all `#[serde(default)]`):

- `AsStats.arcs: Option<ArcCard4>` + `uses_arcs: bool` (spec snapshot; refreshed by `relink_specs`).
- Per-unit crit stage counters on `TrackedMech.bf`: crew-hit stage, engine/thrust stage, K-F-Drive
  integrity, DT rating remaining, bay-door count remaining — the laddered/decrementing crit effects.
- The single large-craft armor + structure pools + threshold (reusing the aero doll surface).
