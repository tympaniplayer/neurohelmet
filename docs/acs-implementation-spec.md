# Abstract Combat System (ACS) — Design and Reference

The **Abstract Combat System** (ACS) is the top rung of the BattleForce ladder — the *planetary-invasion, multi-regiment* scale of *Interstellar Operations: BattleForce* (IO:BF pp.236–264, aerospace pp.251–255). Where SBF fuses a lance into one stat line and tracks formations of a few units, ACS fuses **whole battalions** into one stat line and fights army-group actions across a star system. It is designed for campaign play (Inner Sphere at War) but plays standalone.

**Ladder position** (from the SBF spec's "The BattleForce ladder"): Classic → Alpha Strike → Standard BF (lance stack, per-element) → SBF (formation piece, lance-fused stat line) → **ACS** (abstract army-group, battalion-fused stat line). ACS is built *directly on top of SBF*: per IO:BF p.258 the whole conversion pipeline bottoms out at Alpha Strike Elements and passes **through SBF Units**, so ACS reuses neurohelmet's existing `engine/sbf.rs` converter wholesale and adds three aggregation tiers on top.

ACS stats derive purely from data neurohelmet already bakes. Every conversion formula consumes only Alpha Strike Element fields (Size, MV/TMM, Armor+Structure, S/M/L/E/IF/FLK damage, Overheat, Skill, PV, SUAs) → SBF Unit → up the tiers by average/total/divide. There is no bake change, no new inputs, and no MegaMek port: MegaMek has no ACS engine at all, so IO:BF is the sole rules authority, as with the reference-free portions of the SBF and Standard BF converters.

---

## Page-citation convention

All `p.NN` citations reference *Interstellar Operations: BattleForce*, using the same numbering as the SBF and Standard-BF specs. The citation numbers run **+2 ahead of the printed book folio** (a citation of p.258 corresponds to the printed "256"); the book's own cross-references use the printed folios. Chapter map: ACS core rules pp.236–256, Abstract Combat Aerospace pp.251–255, ACS conversion (the five conversion phases) pp.256–264, ACS record sheets pp.622-ff in the back-of-book quick-reference.

---

## Scope — single force, boardless, manual-first, **ground-only v1**

The SBF/Standard-BF Scope doctrine applies wholesale and then ACS adds one large, deliberate cut.

### Inherited (unchanged from SBF/BF)

neurohelmet tracks **one force — yours.** Two-sided tracking, OpFor rosters, per-side initiative, and turn interleave are **non-goals** — an artifact of a *battle simulator*, which ACS-the-tabletop-game is not (each player keeps their own Formation record sheets; there is no two-sided sheet, and ACS explicitly assumes a Gamemaster adjudicates the hidden/positional half). The only cross-side data is the target's numbers, hand-entered at combat time.

**The board stays at the table.** ACS's positional half is enormous — the Star-System Radar Map, the Planetary Combat Map, five aerospace zones, Blip counters, movement/MP costs, wrap-around, facing, stacking limits, hex terrain, Fortress hexes, Initiative, Detection, Reconnaissance, hidden Formations, Deployment, Formation adjustment/role-switching, Engagement Control, Supply, Victory Points, and the entire ISW campaign layer. **All of it is TABLE + GM concern** and out of scope, exactly as movement/LOS/facing are for every other neurohelmet mode. What survives into the app is what a player writes on a **Formation record sheet and updates as combat is announced**: the converted stat lines, each Combat Unit's armor pool, morale state, and fatigue — plus the number-crunchers (to-hit, fractional damage, morale-check TN + result, fatigue effects).

**Damage applies immediately** (the SBF/BF ruling carries over). The book defers Combat-Phase damage to the End Phase (p.249); that simultaneity is a table procedure. neurohelmet records outcomes as announced, like every other mode.

### The v1 cut: **ground only; aerospace deferred**

**Abstract Combat Aerospace (ACA, pp.251–255) is a deliberate v1 non-goal.** This is the single biggest scoping decision and it is well-founded:

- ACA does **not** build on the light SBF-scale "Strategic Aerospace" (pp.177–179, Atmospheric Radar Map). Per p.251 it rests on **Advanced Strategic BattleForce (pp.180–194)** *and* the **entire Capital-Scale Strategic Aerospace chapter (pp.194–229)** — hyperspace travel, jump-point/space combat, orbital mechanics, gravity, pirate points, WarShip/DropShip capital-weapon classes (CAP/SCAP/MSL/SDS), screen launchers, point defense. neurohelmet implements **none** of that capital-scale chapter today. ACA is not a phase on top of ground ACS; it is a second, larger project that pulls in an unimplemented rulebook chapter.
- The cut is **RAW-legal and clean**: p.262 Step 3B states *"Aerospace and Ground types may not be mixed in Abstract Combat System play."* Aerospace and ground Combat Units are already separate objects in the rules, so shipping ground-only omits a whole *type*, not half of a shared mechanic.
- What ground ACS loses without aero: Combat Air Patrol, Ground Strike/Bombing, Aerial Recon, Orbit-to-Surface / Surface-to-Orbit, Combat Drops, and the aerospace rows of the Master Modifier Table. Every one is a *table/positional* action or an aero-only calculator; none is load-bearing for tracking a ground Formation's attrition. Combat Drops and Orbital Bombardment *do* trigger ground-side morale checks — those triggers survive as manual entries on the morale calculator (the player says "we just got orbitally bombarded, roll morale at −2"), the aero resolution that produced them does not.

Aerospace ACS is a named non-goal (Limitation 1) and is called out in the smoke test, to be picked up only if/after the capital-scale aero chapter is ever implemented as its own mode.

### In scope for v1 (the deliverables)

1. **The ACS converter** (`engine/acs.rs`): SBF Units → Combat Team → Combat Unit → Formation (conversion Phases 2–5, pp.261–264), layered on the existing `convert_unit`. Pure, golden-free (no MegaMek reference), validated against the worked examples printed in the book.
2. **Live ground tracking**: per **Combat Unit** — a single Armor pool (attrition, not per-element structure/crits), three auto-derived **Damage Thresholds** (75/50/25%), **Fatigue Points**, and a manual **Morale** rung. The Formation is the grouping/activation/morale-rollup tier.
3. **Combat calculators**: to-hit (base 4 + range + skill/experience + Combat Tactics + situational Master-Modifier toggles); **fractional damage** (Combat-Unit range damage × `1.0 ± Σmods`, round normal); secondary-target −0.25.
4. **Morale calculator + manual rung**: derive the check TN from Morale Value + triggers/modifiers; on a hand-entered 2D6 roll (or margin of failure) read the **Morale Failure Results Table** to the resulting rung; apply the rung's action modifiers; recovery ("Recovering Nerve") readout. The rung itself is a settable label (manual-first, like SBF); the calculator is a *readout*, same footing as to-hit — neurohelmet never rolls for you.
5. **Fatigue tracking**: per-Combat-Unit FP accrual by experience rating each turn it fought, the Fatigue Effects bands (combat/damage/morale modifiers), and FP recovery.
6. **Grouping editor**: the SBF grouping editor pointed at the ACS three-tier hierarchy, with an opt-in doctrine auto-grouper (IS battalion / Clan Trinary).
7. **TUI GameMode #6**, cheat-sheet section, and PDF re-render.

### Explicitly out of scope for v1 (each is a known limitation below or a smoke-test non-goal)

Everything positional/GM/campaign listed under "the board stays at the table"; **all aerospace** (Limitation 1); Fortress hexes (Limitation 2); the ISW campaign layer — Supply, experience progression, salvage/repair, Leadership Rating as a live economy (Limitation 3; LR appears only as a static per-force number the player may type for the morale/adjustment readouts); Victory Points (Limitation 4); artillery *targeting* resolution (the ART damage still converts and displays; the +2-to-hit-instead-of-range artillery attack is a calculator toggle — Limitation 5); nuclear weapons; the Combat Drop *results* calculator (deferred with aero — Limitation 1).

---

## The ACS hierarchy (the object model)

The rulebook nests five tiers; the conversion pipeline (p.258) makes the relationship exact:

```
AS Element  ──Phase 1──►  SBF Unit  ──Phase 2──►  Combat Team  ──Phase 3──►  Combat Unit  ──Phase 4──►  Formation  ──►  Force
 (baked)      (sbf.rs)     (1–6 elem)  (acs.rs)     (1–4 units,    (acs.rs)     (2–4 teams,    (acs.rs)      (1–8 units)   (roster)
                                                       ≤30 elem)                    ≤48 elem)
```

- **SBF Unit** — already built by `convert_unit` in `engine/sbf.rs`. ACS consumes it unchanged. (In IO:BF's "Scaled BattleForce" naming an SBF Unit is renamed a *Combat Team's* building block; the pipeline is what matters, not the vocabulary.)
- **Combat Team** (conversion Phase 2, p.261) — 1–4 SBF Units, ≤30 Elements. Same aggregation as an SBF Formation *plus* an armor step (Step 2E) and a damage ÷3. A **build-time intermediate**: it is where the ×⅓ battalion-fusion happens, but it is not the atom of play.
- **Combat Unit** (conversion Phase 3, p.262) — 2–4 Combat Teams, ≤48 Elements. **This is the atom of ACS combat and the tracked object**: it has the Armor pool, the S/M/L damage line, TMM, Tactics, Morale Value, Skill, the three Damage Thresholds, and it accrues Fatigue. Combat rolls are "2D6 per Combat Unit"; damage targets a Combat Unit. *Clan Combat Units are Trinaries* — a Clan SBF Formation of 2–4 Units already **is** the Combat Unit, so **Phase 3 is skipped for Clan** (p.262).
- **Formation** (conversion Phase 4, p.262) — 1–8 Combat Units (setup rule p.243 tightens this to **Ground 2–6**; see Limitation 6 for the 8-vs-6 conflict). **The grouping + activation + morale-rollup tier.** Its record sheet (the "Formation Tracking Sheet", p.237) holds Move (= slowest Combat Unit's transport MP), Tactics, Morale, Skill. Ground or Aerospace only, never mixed.
- **Force** — every Formation you field; the roster. Carries the Force Commander (COM) designation and the Leadership Rating. Not a converted object; the top of the tracker tree.

**Mapping to the tracker UI** (mirrors SBF's three-pane Formation/Unit/detail): the panes are **Formation → Combat Unit → detail+calculators**. Combat Teams are a *derivation detail* of the Combat Unit (shown in the detail pane's "how this stat line was assembled" breakdown), not a separate navigable pane — they carry no independent live state. This keeps the SBF three-pane UX intact.

---

## Reference sources

**Rulebook (the only rules authority):** *Interstellar Operations: BattleForce*. Load-bearing pages: 236–239 (scale, hierarchy, command, sequence of play), 240–243 (**Master Modifier Table** — the equipment/faction rows are prone to column-misalignment when transcribed; the experience/morale/fatigue/tactics/range rows are clean and are what v1 uses), 248–249 (combat resolution, range/tactics/damage), 249–250 (**Fatigue** + **Morale** tables), 258–260 (conversion Phase 1, the SBF-Unit tables neurohelmet already implements — cross-check only), 261 (**Phase 2** Combat Team), 261–262 (**Phase 3** Combat Unit), 262 (**Phase 4** Formation), 262–264 (**Phase 5** special abilities). Aerospace pp.251–255 are read but **deferred** (Limitation 1).

**neurohelmet (source layout):**
- `crates/core/src/domain.rs` — `GameMode`, with the `AbstractCombatSystem` variant.
- `crates/core/src/engine/sbf.rs` — **the engine ACS builds on.** `convert_unit` → `SbfUnit` is conversion Phase 1, reused verbatim. `convert_formation` → `SbfFormation` is the Phase-2 precedent (Combat Team ≈ SBF Formation + armor step + ÷3). The `DamageVector` + `band` fractional-damage machinery, `reduced_by`, `SbfRange`, the `SbfToHitCtx`/`sbf_to_hit` ctx-struct pattern, the SUA aggregation vocabularies (`FORM_IF_ANY` etc.), and `art_damage` are all reused; ACS Phase 5 adds its own ACS-column table alongside these.
- `crates/core/src/session.rs` — `AcsState`/`AcsFormationState`/`AcsCombatUnitState` mirror `SbfState`/`SbfFormationState`/`SbfUnitState`/`MoraleStatus`; `AcsAssign` extends the `SbfAssign` grouping ops (`sbf_assign_element` and the SBF grouping block); the `new_with_mode` seams, `point_cost`, `mech_cap`, and the `remove_mech` element-index remap all follow the SBF/BF pattern.
- `crates/core/src/engine/as_element.rs` — `as_element`, `AsElement`, `DamageVector`, `jround` (round-normal helper): the typed access layer that already feeds `convert_unit`.
- `crates/app/src/tui/{app.rs,view.rs}` — the GameMode wiring surface; ACS mirrors `Screen::Sbf`'s three panes. `export.rs`, `forcegen.rs`.
- Related docs: `docs/sbf-implementation-spec.md`, `docs/standard-bf-implementation-spec.md`, `docs/*-smoke-test.md`, `docs/keybindings-cheatsheet.html` (keybinding changes require re-rendering the committed cheat-sheet PDF).

---

## Data fidelity & the "derives purely from AS data" claim

The mode's input is the existing baked `AsStats` + `TrackedMech.gunnery`, read through `as_element()` → `convert_unit()`. Key properties:

1. **No new baked inputs** (p.258). Conversion Phases 2–5 are pure arithmetic over already-converted SBF Units: average / total / ÷3 / round-normal. The only "new" data is *grouping choices* (which SBF Units form a Combat Team, which Teams a Combat Unit, which Units a Formation) and *roster metadata* (COM designation, LEAD per Formation, optional Leadership Rating and experience-rating overrides for the morale/fatigue math). All player-set, none baked.
2. **Round-normal everywhere** (p.259 "round normal"). Reuse `jround` (the AS/SBF round-half-up helper). Phase-specific divisors: Combat Team armor ÷3, Team damage ÷3, Combat Unit PV ÷3 — apply once each; the p.261 top-of-page "÷3, round normal" is the *same* Unit→Team divisor restated, **not** a second division.
3. **Damage is fractional at the wire, integer on the sheet.** Combat Unit S/M/L are integers (each aggregation rounds normal). *Inflicted* damage = `value × (1.0 ± Σmods)` then round normal → integer armor loss (p.249 example: 25 × 1.3 = 32.5 → 33). Reuse SBF's f32 `DamageVector`; the multiplier is the novel term.
4. **The tracked object carries armor only — no structure, no per-element crits.** At ACS scale a Combat Unit is a single Armor pool that attrites; there is no internal structure bar and no crit table (those live at AS/Standard-BF scale). This makes ACS live-tracking *simpler* than SBF/AS: `current_armor = derived_armor − armor_hits`, three thresholds, done. (Contrast SBF's `SbfUnitState`, which tracks `damage_crits`/`targeting_crits`/`mp_crits` — ACS has none of these; damage → armor → thresholds → morale.)
5. **Tactics, Morale Value, and Damage Thresholds are derived stats** the SBF converter does not produce, computed in Phases 2–4:
   - **Tactics** (p.261 Step 2H / p.262 Steps 3G/4D): base `10 − standard Move`, `+1 per Skill point above 4`, `−1 per below 4`; if any constituent has an `MHQ#` rating, subtract `min(MHQ÷2 round-normal, cap)` where cap = 3 at Team, 2 at Combat Unit; floor at 0.
   - **Morale Value** (p.261 Step 2I / p.262 Step 3H): `Skill value + 3`. Formation Morale = the **lowest** Combat Unit's Morale (Step 4E).
   - **Damage Thresholds** (p.262 Step 3H): `step = floor(Armor × 0.25)`; the three thresholds are `Armor − step`, `Armor − 2·step`, `Armor − 3·step` (i.e. 75/50/25% marks). Worked ex.: Armor 20 → step 5 → thresholds 15/10/5.
6. **SUAs use the ACS column** (p.262–264 "SBF & ACS Special Abilities Table"). Phase 5 promotes SBF specials up the tiers with the same U1/U½/UA → F1/F⅔/FA → sum-of-# machinery neurohelmet already has for SBF, filtered to abilities whose ACS column = "Yes". v1 renders promoted SUAs as reference text on the Combat Unit / Formation; only the handful that feed the v1 calculators are live (see the calculators section below). The rest is display, exactly like AS/SBF mode.

---

## The converter (`crates/core/src/engine/acs.rs`)

Pure functions, no session state, building three tiers on top of `SbfUnit`. There is no MegaMek ACS engine to golden against, so the converter is anchored to the book's **printed worked examples** and to internal-consistency checks over the real catalog.

### Types

```rust
pub struct AcsCombatTeam {   // conversion Phase 2, p.261
    pub name: String,
    pub acs_type: AcsType,        // predominant type, ≥2/3 rule else Mixed Ground (reuse SBF type logic)
    pub size: i64,                // avg of unit sizes, round normal
    pub movement: i64, pub move_mode: SbfMoveMode, pub jump_move: i64,
    pub trsp_movement: i64, pub trsp_mode: SbfMoveMode,
    pub tmm: i64,
    pub armor: i64,               // Step 2E: total unit armor (+SCL#, +PNT#) ÷3 round normal
    pub damage: DamageVector,     // Step 2F: total each range ÷3; IF÷3 folds into L&E if ≥1; FLK≥2 → FLK special
    pub tactics: i64,             // Step 2H
    pub morale_rating: i64,       // Step 2I: skill+3
    pub skill: i64,               // Step 2G: avg unit skill
    pub point_value: i64,         // Step 2J: total unit PV ÷3 round normal
    pub suas: BTreeMap<String, SuaVal>,
    pub units: Vec<SbfUnit>,
}

pub struct AcsCombatUnit {   // conversion Phase 3, pp.261–262
    pub name: String,
    pub acs_type: AcsType,
    pub size: i64,
    pub movement: i64, pub move_mode: SbfMoveMode, pub trsp_movement: i64, pub trsp_mode: SbfMoveMode,
    pub tmm: i64,                 // Step 3D: avg team TMM + (avg team JUMP ÷3) — JUMP folds INTO tmm here
    pub armor: i64,               // Step 3E: TOTAL (not avg) of team armors
    pub damage: DamageVector,     // Step 3F: TOTAL each range across teams
    pub tactics: i64,             // Step 3G (MHQ cap 2)
    pub morale_rating: i64,       // Step 3H: skill+3
    pub damage_thresholds: [i64; 3], // Step 3H: 75/50/25% of armor
    pub skill: i64,               // Step 3I: avg of teams
    pub point_value: i64,         // Step 3J: total team PV ÷3 round normal
    pub suas: BTreeMap<String, SuaVal>,
    pub teams: Vec<AcsCombatTeam>,
}

pub struct AcsFormation {    // conversion Phase 4, p.262
    pub name: String,
    pub acs_type: AcsType,        // Ground | Aerospace (v1: Ground only)
    pub movement: i64,            // Step 4C: transport MP of the SLOWEST combat unit
    pub tactics: i64,             // Step 4D (MHQ: subtract highest, cap 2)
    pub morale_rating: i64,       // Step 4E: morale of the LOWEST-morale combat unit
    pub skill: i64,               // Step 4F: avg of combat units
    pub point_value: i64,         // total combat-unit PV
    pub units: Vec<AcsCombatUnit>,
}
```

`AcsType` = `{ BattleMech, Vehicle, Infantry, ProtoMech, MobileStructure, MixedGround, Aerospace, LargeAerospace }` — reuse/wrap `SbfElementType`; the ≥⅔-else-Mixed predominant-type logic already exists for SBF (`convert_formation` type step) and is lifted, with the p.262 3B merges (Aero+LargeAero→Aero, BA+CI→CI). v1 rejects/flags Aerospace at Formation build.

### Key entry points

- `pub fn convert_combat_team(name, units: &[SbfUnit]) -> AcsCombatTeam` — Phase 2. Mirrors `convert_formation` for the movement/size/skill/tactics/morale steps, then **adds** Step 2E armor (÷3, +½/+1 already done at SBF-Unit level so here it's just total-then-÷3 with SCL#/PNT#), Step 2F damage ÷3 (+IF fold, +FLK grant).
- `pub fn convert_combat_unit(name, teams: &[AcsCombatTeam]) -> AcsCombatUnit` — Phase 3. **Clan path**: a helper `combat_unit_from_clan_formation(SbfFormation)` skips Phase 3 (the Clan Trinary already is the Combat Unit). Note the two divergences from Phase 2: armor is **total not averaged** (3E), damage is **total not ÷3** (3F), JUMP folds into TMM (3D).
- `pub fn convert_formation(name, units: &[AcsCombatUnit]) -> AcsFormation` — Phase 4. Movement = slowest transport MP, Morale = lowest, Tactics/Skill per steps.
- `promote_suas_acs(...)` — Phase 5, reusing the SBF U/F promotion helpers filtered to the ACS-"Yes" ability set.

### Validation and invariants

The converter reproduces the book's printed worked examples: the p.259 armor example, the p.259–260 damage/skill/PV examples, the p.262 threshold example (Armor 20 → 15/10/5), and the JUMP-into-TMM example — each transcribed as a fixture from the printed inputs to the printed result. Across the full unit catalog it holds the structural invariants: no panic building a Combat Team from any 1–4 SBF Units, `armor ≥ 0`, `size` in range, thresholds monotone decreasing, PV ≥ the constituent floor. Team damage is divided by 3 exactly once, and `jround` (round-half-up, not truncation) is used at every divisor — an X.5 case is pinned per step.

---

## Session state and GameMode

The session state mirrors the SBF state shapes in `session.rs`.

```rust
GameMode::AbstractCombatSystem,   // in domain.rs

pub struct AcsState {
    pub formations: Vec<AcsFormationState>,
    pub active_formation: usize,
    pub active_unit: usize,        // selected COMBAT UNIT within the formation
    pub round: u32,
    /// Force-level roster metadata (player-set; feeds morale/adjustment readouts). Static in v1.
    pub leadership_rating: i64,    // 0 if unset
}

pub struct AcsFormationState {
    pub name: String,
    pub units: Vec<AcsCombatUnitState>,
    pub morale: MoraleStatus,      // manual rung — but ACS has 6 rungs, see below
    pub is_done: bool,
}

pub struct AcsCombatUnitState {
    pub name: String,
    /// Grouping: the SBF Units composing this Combat Unit, as (team_index, element_indices).
    /// Elements are indices into Session::mechs, same convention as SbfUnitState::elements.
    pub teams: Vec<AcsTeamGrouping>,   // preserves the Combat-Team sub-structure for re-derivation
    pub armor_hits: u32,               // current_armor = derived − armor_hits
    pub fatigue_points_x2: u16,        // FP tracked ×2 (values are half-integers: 0.5 FP etc.)
    pub morale: MoraleStatus,          // per-unit rung (formation rung is the rollup)
    pub is_commander: bool,            // COM — one per force
    pub is_leader: bool,               // LEAD — one per formation
}
```

**Morale rungs — ACS needs a wider enum than SBF's.** SBF's `MoraleStatus` has 4 rungs (Normal/Shaken/Broken/Routed). ACS's Morale Failure Results Table (p.250) produces **six failure rungs** atop `Normal`: `Shaken, Unsteady, Broken, Retreating, Routed, Surrender`. ACS uses a separate `AcsMorale` enum rather than widening SBF's — SBF's 4-rung choice is deliberate (the SBF spec drops `Unsteady` as an ACAR artifact), but in *ACS* `Unsteady` is RAW (p.250). Ordinals run worst-last, with `Surrender` = combat-ineffective / counting as destroyed for VP.

Fatigue is stored ×2 as an integer (`fatigue_points_x2`) because FP accrue in halves (0.5/turn for most, table p.249); this avoids float in persisted state, the same spirit as the SBF `jump_used_this_turn` byte.

**Derive-on-demand** methods on `Session` (mirroring `sbf_unit`/`sbf_formation`): `acs_combat_unit(fi, ui) -> AcsCombatUnit`, `acs_formation(fi) -> AcsFormation`, rebuilt from the grouping + baked stats each call; live state (armor_hits/fatigue/morale) overlays the derived stats. `mech_cap` = None (roster mode, like AS/SBF/BF), and `point_cost` routes ACS → the Combat-Unit PV the way SBF routes to AS PV. The `new_with_mode`, save/load, and `GameMode::` match sites are wired the same way SBF and BF are.

**Grouping** re-points the SBF grouping editor at three tiers. `AcsAssign` extends `SbfAssign` with the extra tier: assign an SBF-Unit-worth-of-elements into an existing Combat Team, a new Team (split), a new Combat Unit, a new Formation, or unassign. Any pool deletion remaps every `AcsTeamGrouping.element_indices`, following the `remove_mech` element-index remap from the SBF path.

---

## Combat, morale, and fatigue calculators (`engine/acs.rs`)

Where neurohelmet owns the rules. All pure functions plus ctx structs, following the `SbfToHitCtx`/`sbf_to_hit` pattern.

### To-hit (p.248–249)

`base = 4`. `2D6 per Combat Unit ≥ modified TN` hits; **a natural 2 always fails**. Modifiers (from the clean rows of the Master Modifier Table pp.240–241 + the range/tactics rows p.248):

- **Range** (ground): Short **−1**, Medium **+2**, Long **+4** (no Extreme, no Indirect at ACS ground scale).
- **Skill / Experience** (the "Wet Behind the Ears … Legendary" Attacker-To-Hit column, p.241): +2/+1/0/−1/−2/−3/−4/−4 for WBTE/ReallyGreen/Green/Regular/Veteran/Elite/Heroic/Legendary. This is the *experience rating* row, distinct from the numeric Skill value; v1 derives the rating band from the Combat Unit's Skill value via the p.260 Experience Skill Value table (Skill 7=WBTE … 0=Legendary) unless the player overrides.
- **Target Movement Modifier**: `+TMM` (the target's, hand-entered).
- **Combat Tactics** (p.248, per Formation): Aggressive `+1..+5` to-hit (→ `+0.1..+0.5` damage on hit), Defensive `+1..+5` to-hit (→ `−0.1..−0.5` damage *received* on hit), Standard `+0`.
- **Target morale** (target's rung, p.241 Morale rows): Shaken +1 / Unsteady +2 / Broken +3 / Retreating-Routed +4 to the attacker (target easier to hit as it breaks). Hand-set from the known target rung.
- **Situational toggles** (hand-set, the clean Master-Modifier rows): attacked-from-behind −1, secondary-target +2, no-supply +3, fatigue band +1/+2/+3/+5, infantry/proto in urban +1, ambush −1. Artillery attack: **+2 instead of the range modifier** (Limitation 5 toggle).

`pub struct AcsToHitCtx { range, experience, target_tmm, tactics: AcsTactics, target_morale, from_behind, secondary, ... }` → `pub fn acs_to_hit(&AcsToHitCtx) -> i64`. Same shape as `sbf_to_hit`, no board terms.

### Damage (p.249)

`inflicted = round_normal( combat_unit.damage.band(range) as f32 * (1.0 + Σ damage_mods) )`, floored at 0.

- **Damage Inflicted Modifier** = `1.0 ±` the sum of applicable *damage* modifiers: Combat Tactics (Aggressive success `+0.1/step`; Defensive reduces *received*), Attack-From-Behind **+0.2**, secondary-target **−0.25**, ambush **+0.2** (or **−0.5** on a Combat-Phase attack), no-supply **−0.1 cumulative**, fatigue band **−0.1/−0.2/−0.4**, morale Broken **−0.2** / Retreating-Routed **−0.4** (dealt). Signs and which side (dealt vs received) come from the two-part `Damage Modifier*` column (p.240 footnote: value before slash = dealt, after = received).
- **Apply**: `AcsCombatUnitState.armor_hits += inflicted` on the chosen target Combat Unit. When `armor_hits` crosses one or more of the three `damage_thresholds`, **flag "morale check(s) due"** (one per threshold crossed this turn) — the app surfaces the trigger; the player rolls (below). There is **no spillover** at ACS scale (unlike SBF Phase-4 — damage targets one Combat Unit and stops; a Combat Unit at 0 armor is destroyed).
- **Secondary target** convenience: a "this is my 2nd target this Formation" toggle bakes the −0.25.

### Morale (p.249–250) — calculator + manual rung

Manual-first, like SBF, **but** ACS morale is fully RAW (not a MegaMek artifact) so neurohelmet provides a real **check readout**:

1. **Triggers** the app auto-flags: a Combat Unit's `armor_hits` crossing a Damage Threshold (one check per threshold, p.249–250); the player also manually triggers Combat-Drop / Orbital-Bombardment / Force-morale checks (the aero half is out of scope but its *morale consequence* is a manual entry).
2. **Check TN** = `2D6 ≥ Morale Value + Σ modifiers`. Modifiers (p.250 Morale-specific table + the Master-Modifier Morale column): 3rd threshold (75% dmg) **+2**, ⅔ of Formation at 50%+ **+1**, force suffered orbital attack **+2** (COM rolls), ⅔ of Formation Shaken+ **−2**, experience-rating Morale column (WBTE +2 … Legendary −5), fatigue band Morale-check column (+1/+2/+3/+4). Formation/Force checks use the **LEAD/COM** unit's Morale Value.
3. **Result**: player enters the 2D6 (or the margin of failure); neurohelmet reads the **Morale Failure Results Table** (p.250, MoF × current Damage-Threshold band → Shaken/Unsteady/Broken/Retreating/Routed/Surrender) and sets the rung (or the player sets it by hand — the rung is always hand-settable).

```
Morale Failure Results Table (p.250) — rows = margin of failure, cols = current damage band:
 MoF   | 25% Armor  | 50% Armor  | 75% Armor  | No Threshold | No Damage
 1–3   | Broken     | Unsteady   | Unsteady   | Shaken       | Shaken
 4–6   | Retreating | Retreating | Broken     | Unsteady     | Shaken
 7–9   | Routed     | Routed     | Retreating | Broken       | Unsteady
 10+   | Surrender  | Surrender  | Routed     | Retreating   | Broken
```

4. **Rung effects** (readout applied to the calculators, p.250): Shaken −1 / Unsteady −2 / Broken −3 (no Aggressive tactics, no Force/Overrun) / Retreating −4 / Routed −4 (must withdraw) / Surrender (combat-ineffective). These feed back into `acs_to_hit`/damage as the target-morale term for the enemy and as an own-action penalty.
5. **Recovering Nerve** (End Phase, Shaken+): a standard Morale Check + the Nerve-Recovery modifiers (friendly commander in hex −2, sub-commander −1, unattacked-this-turn −1, …, p.250) → success improves the rung one level. neurohelmet shows the recovery TN; the player rolls and steps the rung.

`pub struct AcsMoraleCtx { morale_value, experience, fatigue_band, third_threshold, formation_half_damaged, orbital, ... }` → `acs_morale_tn(&ctx) -> i64` and `acs_morale_result(current_band, margin_of_failure) -> AcsMorale`.

### Fatigue (p.249) — live per-turn counter

Fatigue is deterministic bookkeeping (not a simulation of the enemy), so neurohelmet **tracks it live**:

- **Accrual** (Fatigue Points Earned, p.249): each turn a Combat Unit is in combat, add FP by experience rating — WBTE 2, ReallyGreen 1, Green/Regular/Veteran/Elite/Heroic/Legendary 0.5 — with the **ignore-first-N** for Elite (2) / Heroic (3) / Legendary (4) / Clan (2) / WoB (1), capped at 4 ignored. `acs_fatigue_earned(experience) -> f32`.
- **Effects bands** (Fatigue Effects, p.249): FP 0–4.5 Rested (—), 5–8.5 Tired (+1 combat), 9–12.5 Flagging (+2, −0.1 dmg, morale check +2), 13–16.5 Exhausted (+3, −0.2, +3), 17+ Spent (+5, −0.4, +4). `acs_fatigue_band(fp) -> AcsFatigueBand` feeds the to-hit/damage/morale ctx structs.
- **Recovery**: a Combat Unit that neither moves, attacks, nor is attacked in a turn reduces FP by 1 (p.249). A per-turn "rested" toggle.

The app surfaces "advance turn", which (opt-in, manual-first) prompts which Combat Units were in combat, accrues their FP, and offers the −1 rest reduction for the others — never auto-applied without the player marking who fought.

---

## TUI (`crates/app`)

`Screen::Acs`, three-pane **Formation / Combat Unit / detail+calculators**, mirroring `Screen::Sbf`:

- **Formation pane**: name, type, Move (slowest CU transport MP), Tactics, Morale rung (rollup = lowest CU), Skill, PV, `is_done`.
- **Combat Unit pane**: per CU — name, type, Size, S/M/L damage, TMM, Armor bar with the three threshold marks, Fatigue band, Morale rung, COM/LEAD flags.
- **Detail pane**: the to-hit calculator (range/tactics/target-TMM/target-morale/situational toggles), the damage readout (band × multiplier → armor loss with a threshold-crossing warning), the morale-check panel (TN + failure-table result entry), the fatigue panel (FP + band + accrue/rest), and a "derivation" fold showing the Combat Teams / SBF Units this CU was assembled from.
- **Grouping editor** (`g`): the SBF editor re-pointed at the 3-tier hierarchy; opt-in doctrine auto-grouper (`a`) with an ACS picker — IS/Periphery **battalions → Combat Units → Formations**, Clan **Trinaries** (Phase-3-skip path), with the itemized-loss confirmation on regroup. `r` renames.
- Full keyset modeled on SBF (`Space` activate, `c` combat, `m` morale, `f` fatigue, `t` tactics, `C`/`l` COM/LEAD, `D` delete, `A` new ACS session, `n` next).
- **Game log** parity: `LogEntry.acs` captures Formation state; export renders one Formation sheet per formation; old logs fall back gracefully.
- **Cheat sheet**: an ACS section in `docs/keybindings-cheatsheet.html`, the committed PDF re-rendered via Chrome Headless, and the in-app `?` modal in `view.rs`.
- **Smoke test**: `docs/acs-smoke-test.md` (mirroring the SBF/BF ones), whose v1 non-goals section names all aerospace, all positional/GM/campaign machinery, Fortress hexes, Victory Points, Supply, and immediate-damage-timing.

---

## Non-goals and known limitations

1. **Aerospace ACS (ACA, pp.251–255)** — deferred non-goal. It reopens only if the Capital-Scale Strategic Aerospace chapter (pp.194–229) is implemented as its own mode. The ground↔aero coupling that is preserved even without it: Combat-Drop and Orbital-Bombardment morale triggers, kept as manual morale-check entries.
2. **Fortress hexes (pp.254–255)** — a rich sub-system (Standard/Capital/Castle Brian × 5 levels, damage-division, STO weapons, nuclear table). Positional/GM, and out of v1. A defender-side toggle (e.g. "my Formation is in a Level-N Standard fortress → damage received ÷4") is the natural extension point.
3. **ISW campaign layer** — Supply economy, experience progression, salvage/repair, and Leadership Rating as a live resource are out of v1. LR is a static typed number feeding the morale/adjustment readouts.
4. **Victory Points (p.251)** — the AS:CE VP rules ×0.1 for Formation-destroyed. Scorekeeping, out of v1.
5. **Artillery to-hit** — ART damage converts and displays; the "+2 to-hit instead of range, one attack per Formation per artillery type" (p.248) is a calculator toggle, not a separate resolution engine.
6. **Formation size (8 vs 6)** — the source tables conflict: p.236 says "up to 15 Combat Units", p.238 says "2–8", and the setup rules p.243 say **Ground 2–6 / Aero 1–4**. v1 treats p.243 as operative and uses **Ground 2–6** as a soft warning — never a hard cap, since neurohelmet does not block a grouping the player insists on (manual-first doctrine).
7. **Master Modifier Table transcription** — the experience/morale/fatigue/tactics/range rows (what v1's calculators use) are clean and column-verified. The equipment/faction/loyalty/combat-doctrine rows (Probes/ECM/C³/Loyalty/Doctrine/Engineers/Orders) are prone to column-misalignment when transcribed, and are positional/GM modifiers anyway — none feed a v1 calculator. Any such row is verified against the printed PDF before it is surfaced.
8. **Combat Team as a navigable tier** — v1 treats Combat Teams as a derivation detail of the Combat Unit (no independent live state, shown in the detail fold), because the rules track damage at the Combat Unit. Per-Team tracking is the extension point if it is ever wanted.
9. **Experience rating vs numeric Skill** — the Master Modifier rows key off the named experience rating (Green/Veteran/…), while the converter produces a numeric Skill value. v1 maps Skill→rating via the p.260 table and lets the player override the rating per Combat Unit (an elite crew in a green battalion). Rating is first-class in the rules — the Leadership Rating is set by the COM's experience rating (p.239), with Skill derived from it rather than the reverse.
