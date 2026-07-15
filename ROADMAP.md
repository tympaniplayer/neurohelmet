# Neurohelmet — Roadmap & developer notes

This document tracks forward-looking work and orients contributors to where each subsystem lives. For
build, run, and control instructions see [README.md](README.md); this file covers what remains and
where to start.

The planned work below is grouped by area. Items are candidates rather than commitments, and the size
hints (small / medium / large) are indicative. Neurohelmet's design invariants — an offline terminal
sized for a ~100×30 Raspberry Pi screen, manual dice, and a play-tracking focus — bound what is in
scope; see [Non-goals](#non-goals).

## Where things live

Neurohelmet is split into three crates: a UI-free `core` (pure, unit-tested rules), a `bake` data
pipeline, and the `app` TUI.

```
crates/core/src/
  domain.rs            Mech (the unit container: 'Mechs, vehicles, infantry/BA — see UnitType)
                       + CritSlot, MechConfig (biped/quad/tripod), GameMode, AsStats, MotiveType,
                       Location (mech + vehicle + Trooper1-6/Platoon), Facing, LocationArmor,
                       WeaponMount, AmmoBin, Equipment, bv/cost
  engine/damage.rs     LocState, apply_damage (armor→internal overflow), transfer_to (cascade)
  engine/heat.rs       dissipation + heat_effects (the standard TW scale)
  engine/internal.rs   internal-structure-by-tonnage table (validation/fallback)
  engine/movement.rs   MoveMode + attacker-movement modifier + the TMM table
  engine/pilot.rs      consciousness_avoid table + PILOT_MAX
  engine/skill.rs      skill-adjusted point cost: Classic BV multiplier table + Alpha Strike PV
                       skill adjustment (both verbatim from MegaMek; force-budget math)
  engine/alpha_strike.rs  AS 1:1 ground (hex) scale: movement_hexes + range_brackets_hexes
                       (2" = 1 hex, halve-round-up)
  engine/infantry.rs   conventional-infantry weapon range: infantry_max_range + infantry_range_mod
                       (range class × 3 hexes + per-hex to-hit, from MegaMek getInfantryRangeMods)
  engine/dice.rs       reference tables: cluster_hits (MegaMek, full table 1-30+40), cluster_profile,
                       mech_hit_location
  session.rs           TrackedMech (live state: damage, heat, ammo, crit_hits, pilot, movement
                       this turn, vehicle motive/crew/crits, AS) + AsCrits/AsCritKind;
                       Session (roster + GameMode + force_total); named-session storage
  log.rs               game-log entries (JSONL append/read) for the turn-by-turn export
  data/bundle.rs       Bundle = bincode(version + Vec<Mech> + munition catalog); the baked
                       dataset format (see BUNDLE_VERSION, bumped with each new baked field)
crates/bake/src/
  fetch.rs             caching HTTP client (db.mekbay.com), 429 backoff
  svg.rs               parse record-sheet SVG → armor/internal pip counts + crit-slot tables
                       (+ parse_ba_armor: per-trooper pips, dark pip = the trooper;
                        + parse_transport: storage bays from the "Features" line)
  join.rs              units.json + equipment2.json + SVG → Mech: build_mech / build_vehicle /
                       build_infantry, shared parse_loadout, crit-slot id→display, AS stats
  main.rs              CLI orchestration (--filter/--limit/--print/--jobs); per-unit-type branch
  extras.rs            merge hand-entered AS-only units (data/extra_units.json) not in Mekbay —
                       gun emplacements / Battlefield Support
crates/app/src/
  main.rs              entry: load bundle (include_bytes! data/mechs.bin) + session, run TUI;
                       --selftest / --export / --publish modes
  render.rs/export.rs/publish.rs   game-log rasterizer (cell buffer → PPM/PNG) + Pages publisher
  tui/app.rs           App state, Screen / Focus / Modal enums, handle_key + per-screen *_key
                       dispatch, grid_pos (doll layout)
  tui/view.rs          all rendering: doll box-grid (mech/vehicle/squad/platoon), heat/pilot/
                       move/squad panels, AS card grid, picker preview, sessions, modals
  tui/picker.rs        nucleo-matcher fuzzy unit picker
  tui/mod.rs           run loop + the test suite (incl. E2E snapshots)
```

Design rule: `core` has no UI/terminal dependencies; all rules are pure and unit-tested. ratatui is
confined to `app/tui`. The TUI follows an Elm-style loop: `handle_key` mutates `App`, `view::draw`
renders, and every state change sets `app.dirty`, which triggers an autosave in the run loop.

## Development workflows

- Run the test suite with `cargo test`; lint with `cargo clippy --all-targets -- -D warnings`.
- After an intentional UI change, update snapshots with `INSTA_UPDATE=always cargo test -p neurohelmet`,
  then review `git diff crates/app/src/tui/snapshots/`.
- Render a single headless frame of real data with `cargo run -p neurohelmet -- --selftest`.
- Re-bake the dataset with `cargo run --release -p neurohelmet-bake -- --jobs 4 --out data/mechs.bin`
  (cached under `.bake-cache/`; rebuild the app afterward to re-embed).
- Every new screen or state should get an E2E snapshot test in `tui/mod.rs`, following the existing
  `e2e_*` tests.

## Combat & tracking fidelity

### Critical hits — remaining work

Manual critical-hit tracking and its consequences already ship: marking slots by hand per location,
auto-marking all of a destroyed location's slots, engine crits adding +5 heat/turn each, weapons
rendered as disabled when their slot is destroyed, and 'Mech destruction on CT/head internal loss,
cockpit hit, three or more engine crits, or two or more gyro crits. Remaining and deferred items:

- 2d6 auto-rolling of critical *locations* is intentionally not provided; crit locations stay
  hand-entered, consistent with the manual-first design.
- XL gyros are destroyed at three hits rather than two; all gyros currently use a two-hit destruction
  threshold.
- Ammo-bin explosions, and the "destroyed ammo is unusable" rule, are not modeled.
- Life Support critical heat damage to the pilot is not modeled.

### Pilot tracking — remaining work

The six-box pilot track, consciousness-avoid numbers, and pilot death already ship. Remaining work:

- Piloting-skill rolls and falling.
- Unconscious-then-recovery tracking (a downed pilot regaining consciousness across turns).

### Munition effects — remaining work

Ammo-bin selection and munition-type selection/labeling already ship: choosing the active bin per
weapon, a shared munition catalog on the bundle (Standard / Inferno / Semi-Guided / Fragmentation /
Dead-Fire / Thunder, etc.), and the loaded munition surfaced on the crit popup and weapon detail line.
Remaining work:

- Model the mechanical *effects* of the selected munition — for example Inferno rounds adding heat and
  Fragmentation's anti-infantry behavior — rather than only selecting and labeling them.

### Munition-based weapon to-hit modifiers

Equipment-derived to-hit modifiers (pulse, VSP, heavy laser, and the Targeting Computer −1 on eligible
direct-fire weapons) are baked from `equipment2.json` and summed per weapon via `Mech::weapon_to_hit`.
Remaining work concerns munition-dependent adjustments:

- **Targeting Computer ammo-based exclusions** (LB-X cluster, flak, SB Gauss) are not applied; they
  depend on the loaded munition.
- **Ammo-based to-hit modifiers** carried by some munitions in `equipment2.json` are not yet baked.

Chassis accuracy quirks are a separate un-baked source, addressed by the quirks work item.

### Movement-mode: auto-applied heat, terrain, and VTOL altitude

Per-turn movement mode (stationary/walked/ran/jumped, with vehicle relabeling to cruised/flanked), the
attacker to-hit modifier, and the unit's own TMM are tracked and editable (`v`). Remaining work:

- **Movement heat** (+1 walk / +2 run / jump MP) is not auto-applied at end of turn; heat entry stays
  manual, consistent with the manual-dice philosophy, so auto-application is optional.
- **Road/pavement MP bonuses** are not modeled, so the hex cap equals the raw MP value.
- **VTOL altitude** (climb/descend as its movement) is unmodeled; VTOLs currently use cruise/flank like
  any vehicle. Shared with the combat-vehicle VTOL work item.
- Range and terrain to-hit remain the player's to add.

### Ammo-explosion and heat-threshold prompts

Ammo-bin explosions are not yet surfaced, and the heat scale drives events that are currently only
partially exposed: ammo-explosion checks (heat 19 → 4+, 23 → 6+, 28 → 8+) and shutdown checks
(14 → 4+ through 30 auto).

- **Scope to ship (prompts with manual rolls, following the existing PSR pattern):** When heat crosses
  a threshold, the heat panel flags `ammo-exp 6+` or `shutdown 8+` as a due roll. An ammo-bin crit
  (rolled by hand in the crit popup) marks that bin destroyed — rendering its shots unusable — and
  raises an explosion prompt that is CASE-aware (CASE in the location vents it). No auto-rolling;
  reminders and bookkeeping only.
- **Deferred:** Applying the explosion damage (a large internal hit) remains a manual damage entry, and
  Life-Support heat-to-pilot damage likewise stays deferred.

Size: small-medium.

### Equipment active effects, physical & artillery attack rules

Equipment, physical weapons (Hatchet, Sword, Claw, etc.; component type `P`), and artillery (Sniper,
Thumper; type `A`) are baked and listed alongside weapons — physicals resolving tonnage-based damage at
heat 0, artillery resolving heat and linking to its ammo bin. Heat sinks are summarized in the HEAT
panel. Remaining work:

- **Physical and artillery attacks** have no phase or damage-resolution rules; they are
  display-plus-heat/ammo only.
- **Equipment is display-only:** active effects are not modeled (ECM range, jump MP, MASC/Supercharger,
  and similar).

### Equipment active-state toggles ✅ (2026-07-12)

Manual per-row toggles for the equipment states that matter at the table, on the `J` key in the Classic
tracker (modeled on Mekbay's per-equipment state handlers). What shipped:

- **UAC/RAC jam** — `J` on an Ultra/Rotary-AC row marks it jammed (amber `JAM`, `WeaponMount::can_jam`);
  a jammed weapon reads amber and refuses to fire (`primary_action` guards on `TrackedMech::is_jammed`)
  until cleared by hand. The jam persists across turns (clearing a UAC/RAC jam costs the unit a turn) —
  jam rolls stay manual, per the manual-dice philosophy.
- **MASC / Supercharger engaged** — `J` on the gear row engages the booster; `TrackedMech::movement`
  then lifts Running MP from the baked base (⌈walk×1.5⌉) to ⌈walk×2⌉ (one booster) or ⌈walk×2.5⌉ (both),
  recomputed from the heat/crit-reduced walk, via `run_from_walk` (MegaMek `MPBoosters`). The MOVE panel
  flags it `MASC↑` / `SC↑`.
- **ECM / Stealth on-off** — `J` flips a display-only marker (`● ON`, accent) for ECM suites and stealth
  systems (Stealth Armor, Null/Void Signature, Chameleon LPS).

Detection is name-based off the baked equipment list (`TrackedMech::has_masc/has_supercharger/has_ecm/
has_stealth`); a toggle on gear the unit doesn't mount is inert. New `TrackedMech` fields (`jammed`,
`masc_engaged`, `supercharger_engaged`, `ecm_active`, `stealth_active`) are `#[serde(default)]`, so no
bundle bump and old sessions load unchanged.

**Out of scope (unchanged):** C3 networks (a force-level construct under the large force-management
axis); BAP and other tags stay display-only.

### Dice-reference popups: non-'Mech hit location and additional tables

The read-only dice-reference popup (`r`) provides the per-weapon Cluster Hits column, the full Cluster
Hits Table, and the 'Mech Hit Location Table across all four attack directions — it rolls nothing and
changes nothing. Remaining work:

- **Vehicle, Battle Armor, and infantry hit-location tables** — the hit-location tab is 'Mech-only.
- **Punch and kick columns** for the hit-location table.
- **Cluster mutators** beyond LB-X and Streak.

### Crit-location and crit-roll reference for non-'Mechs

The dice-reference popups (cluster and hit-location tables) are currently 'Mech-only. This adds
read-only 2d6 reference tables for the remaining unit types.

- **Scope to ship (reference only, no auto-roll):** Add hit-location tables for vehicle, aerospace,
  battle armor, and infantry to the Dice modal tabs, along with the per-type critical-hit tables
  (vehicle motive/crew/weapon, aerospace engine/avionics, and so on) as read-only references.
  Neurohelmet already applies graded crit effects (vehicle motive, aerospace); this reference simply
  removes the need to consult the rulebook to learn which slot a 2d6 result maps to. Tables should be
  sourced from MegaMek (`Compute` and related `*.java`).

Size: small-medium.

## Units & catalog

### Combat vehicles — remaining work

Alpha Strike and Classic combat-vehicle support already ship (tanks, VTOL, and naval, roughly 1,441
units): vehicles reuse the mech per-location armor→internal damage model, with a Front/Sides/Rear/Turret
doll, CRITS and CREW panels, the Motive System Damage table (TW p.193), and transport-capacity display.
Remaining work (large):

- VTOL flight and altitude tracking.
- Per-location internal-structure nuance: vehicles share a single internal-structure pool by rule,
  whereas the tracker currently uses the record sheet's per-location pips.
- A vehicle piloting-skill-roll engine: the graded motive to-hit / steering penalty is displayed but not
  composed into a driving-roll target, and other vehicle crit modifiers remain manual.
- Aerospace units are tracked as separate planned work, with their own structural-integrity and altitude
  sheet model.

### Large craft & capital-scale aerospace (Phase 2)

Large — a cross-cutting initiative, not a single mode. Phase 1 (aerospace + conventional fighters, in
Classic and Alpha Strike) is complete, and fighters are already first-class in Standard BattleForce and
SBF. Phase 2 makes the **capital-scale craft fieldable across the BattleForce ladder** — DropShips (209:
spheroid + aerodyne), Small Craft (39), JumpShips (29), Space Stations (33), and WarShips (123) — as
units *inside* BF / SBF / ACS (targets, air-to-ground / orbit-to-surface attackers, grounded units), not
as a separate space-combat game. Full spec: `docs/large-craft-implementation-spec.md`.

**Decisions (2026-07-09).** Target the **full ladder at faithful IO:BF fidelity** (the multi-arc,
capital-weapon-class Alpha Strike / BattleForce card), delivered in phases DropShips-first. Implement it
as a **shared `crates/core/src/engine/large_craft.rs` layer** (arc model + capital/sub-capital damage +
threshold) that BF, SBF, and ACS each consume, rather than triplicating the logic per mode.

**Key data finding.** The source `units.json` **already carries the full multi-arc card** for the ~450
arc-using craft — each has `frontArc`/`leftArc`/`rightArc`/`rearArc`, every arc split into `STD`/`CAP`/
`SCAP`/`MSL` weapon classes with `dmgS/M/L/E`, over a single `Arm`/`Str`/`Th` pool, plus `usesArcs`,
`TP`, `SZ`, `PV`. The bake simply discards it: `is_aero_fighter` (`bake/join.rs`) drops every large-craft
subtype, and `parse_as_stats` reads only the single `dmg` block. So this is **transcription, not
conversion** — no MegaMek `ASConverter` port is needed; the AS/BF card is 4 firing arcs (front/left/
right/rear), not the 6-arc Total Warfare WarShip record sheet.

**Shared foundation (unblocks all modes; do first):**

- **Bake:** add an `is_large_craft` admit path alongside `is_aero_fighter` (`bake/join.rs`, `bake/main.rs`);
  extend `parse_as_stats` to parse the four arcs × four weapon classes when `usesArcs` is set.
- **Schema:** a multi-arc damage/armor representation on `AsStats` (today single-`dmg` only,
  `domain.rs`); new `UnitType` variant(s) / subtype enum (today the flat `UnitType::Aerospace`); the
  bundle-format bump + re-bake + ~10 MB bundle-size check that implies.
- **Routing:** wire the `sbf_type_from_tp` large-craft arms (`DS/DA/JS/WS/SS`, today → the dead `La`
  default) and the `Warship` move mode (coded but unreachable); resolve `isAerospaceSV` (`SV → As` vs `V`).

**Phasing:** Phase 0 ✅ — the `AcsFormation::is_aerospace()` guard + `isAerospaceSV` routing (commit 014fb14).
Phase 1 ✅ — DropShips + Small Craft in Standard BF (multi-arc card, DropShip crit column, per-arc shot
builder; commits 8b0a5c9…543c255). **Phase 2 ✅ (2026-07-10)** — JumpShips / WarShips / Space Stations baked
(433 large craft total, bundle v23); `BfCritCol::JumpShip` + `BfCrit::{KfDrive, Dock}` with the stateful
crew/K-F/Dock/Door ladders; capital (CAP/SCAP/MSL) to-hit resolved via the **p.83 Advanced Combat Modifiers
Table** (CAP+5/SCAP+3 vs small aerospace targets — *not* the p.191 capital-scale table, which is SBF). **Phase 3 ✅ (2026-07-10)** — SBF Advanced Strategic Aerospace: the 4-arc card threaded through the SBF Unit
model (`SbfUnit.arcs`), the p.191 capital-scale to-hit as a `capital` leg on `SbfAeroShot` (full table +
`capital_range` bracket-reduction + Random-Weapon-Class describe), advisory LA Squadron composition + attack
limits, and the now-reachable `Warship` move mode (SBF has no Threshold — it reuses the below-half-armor crit
gate). **Phase 4 ✅ (2026-07-10) — the initiative is COMPLETE.** ACS Abstract Combat Aerospace:
`AcsAeroToHitCtx` + `acs_aero_to_hit` (folio p.250 aero table + p.241 cross-type rows), reusing the SBF
capital pieces; `AcsCombatUnit.arcs` for per-arc capital fire; all five Ground-Support missions (CAP /
Ground Strike / Aerial Recon / Orbit-to-Surface / the full Combat Drop table); `is_aerospace` is now the
live routing key (the "not yet supported" banner is gone). The whole DropShip→WarShip ladder is now
fieldable across Standard BF, SBF, and ACS.

Positional/table-side machinery stays out of scope as everywhere else: capital radar map, orbital
mechanics, jump-point/space movement, altitude/velocity. Fuel consumption (Advanced/StratOps) remains
out of scope. What ships is the record-sheet state + the number-crunchers, per the whole-app doctrine.

### Infantry / Battle Armor: remaining attacks and ProtoMechs

Conventional infantry and Battle Armor are supported in both Alpha Strike and Classic, including Battle
Armor per-suit armor tracks and firing, conventional-infantry platoon strength and damage, and infantry
weapon counts and range classes. Remaining work:

- **Battle Armor anti-'Mech attacks** (swarm and leg attacks) stay manual.
- **ProtoMechs** are not yet supported as a unit type.
- **Infantry point-blank weapon flags** at hex 0 (F_INF_BURST −1, point-blank/encumbrance +1) are left
  to the player, as with range and terrain to-hit; modeling them would require baking a weapon-kind flag.

Aerospace units are covered by their own work item.

### Hand-entered units: remaining work

Neurohelmet bakes from Mekbay's catalog, a subset of the canonical Master Unit List (MUL), so some
official units are missing from both — most clearly gun emplacements / Battlefield Support assets, which
have printed Alpha Strike cards but appear in neither Mekbay's `units.json` nor the MUL, and whose
MegaMek `.blk` files carry no armor/AS stats (a gun emplacement's durability comes from the building it
occupies). An initial hand-transcribed set ships in `data/extra_units.json`, merged at bake time
(`bake/extras.rs`); these are Alpha-Strike-only (empty `armor` map → `Mech::is_as_only()`) and are
blocked from Classic sessions. To add more: transcribe a card into `data/extra_units.json`, re-bake, and
rebuild. Remaining items:

- **A dedicated `GunEmplacement` unit type** (these are currently typed `Vehicle`) with
  Construction-Factor-based damage tracking, giving them a Classic representation instead of
  Alpha-Strike-only.
- **A MUL-vs-catalog roster diff** to find the remaining missing units, baselined against the MUL
  (comparing only to Mekbay confirms faithfulness but cannot surface gaps).

### Quirks (display-only) ✅ (2026-07-13)

Chassis design quirks are baked onto `Mech` and shown throughout. What shipped: `Mech.quirks`
(`Vec<String>`, `#[serde(default)]`) baked verbatim from Mekbay's `unit.quirks` — a flat array of
display-ready strings, e.g. `["Command Mech", "Narrow/Low Profile"]` — for all 5 unit build paths
(`bake/join.rs`), carried on ~3,300 of 9,724 units (bundle v24). Displayed as a "Quirks" row in the
unit-picker preview (`preview_lines`) and as a dim, truncated footer in the tracker's WEAPONS/AMMO/EQUIP
panel when it isn't focused (reusing the detail-line slot — the unobtrusive tracker home, working for
every unit type).

Remaining hook: the accuracy quirks (Improved/Poor Targeting) are a natural later addition to the
per-weapon to-hit assembly (`Mech::weapon_to_hit`) — the names are now baked and ready to match on.

### Unit-picker filtering — remaining work

The unit picker already ships a preview popup, inline intro-year display in the list, and faceted
filtering (Type, Tech, Class, Role, Era, inclusive intro-year range, and Family, composing with the
fuzzy query). Remaining filter enhancements:

- Multi-select within a single facet.
- Tonnage and BV range entry.
- Persisting filters across sessions.

### Picker: numeric range filters and query syntax

The picker currently offers 7 facets plus a typed year range and fuzzy text search. Mekbay exposes
roughly 40 filter dimensions, most as range sliders (BV, tonnage, armor, damage-per-turn, heat,
dissipation, walk/run/jump, and AS PV/size/TMM/damage/threshold) together with a semantic query language
(`field>=x`, `field=a-b`, wildcards such as `AC*`, AND/OR/NOT, and virtual keys like `dmg=50/30/20`).

- **Scope to ship:** Add numeric range facets to `tui/filters.rs` for the already-baked scalars (BV,
  tonnage/internal, total armor, PV, damage-per-turn), reusing the existing typed-bound widget
  (Year ≥/≤). This covers the common case, e.g. "light 'Mechs 30–55t under 1200 BV".
- **Optional stretch:** A minimal query-prefix syntax in the search box (`bv<1200`, `tons>=80`) parsed
  before fuzzy scoring.
- **Out of scope:** Multi-select within a single facet, and a full AST / worker-thread search engine,
  which is unnecessary for a baked offline catalog.

Size: small-medium.

## Alpha Strike, to-hit & force building

### Alpha Strike — remaining work

Alpha Strike mode already ships: a 2×2 grid of unit cards, armor/structure pip rows, a heat dial with
Heat-4 shutdown, damage-by-range, per-unit-type crit sets (including a combat-vehicle Motive track and an
aerospace armor Threshold), the optional 1:1 ground (hex) scale, and a per-card pilot skill that adjusts
PV. Remaining work:

- Aerospace Alpha Strike units beyond vehicles and infantry, as part of the separate aerospace effort.
- Alpha Strike firing arcs (the `*Arc` stat fields).
- Decoding Alpha Strike special-ability tags (`specials`) into mechanical effects; they are currently
  displayed as tags only.

Heat and crit application remain manual by design.

### Alpha Strike to-hit: terrain modifiers

The Alpha Strike card already computes and displays a per-range to-hit number via `engine::alpha_strike`,
folding in Skill, the range bracket (S 0 / M +2 / L +4 / E +6), AS heat-to-hit, Fire-Control and Crew
criticals, attacker jump (+2), and a hand-entered target's modifiers (TMM 0–6, +1 if the target jumped,
−4 if immobile). The remaining deferred work is terrain: woods and cover modifiers currently stay the
player's responsibility, matching the stance taken for the Classic to-hit line. Supporting terrain
modifiers on the AS to-hit would complete the calculation.

Size: small.

### GATOR to-hit assembly: remaining work

Phase 1 is complete: a hand-entered target model plus per-weapon target numbers assembling gunnery +
attacker movement + target movement + range bracket + other (equipment + heat), kept manual (number
only), via `engine::gator` and a `t` GATOR target editor. Remaining items:

- **Minimum-range penalty:** the engine helper (`minimum_range_modifier`) exists, but per-weapon minimum
  range is not baked — it is absent from the `s/m/l` range cell the bundle is built from (it is read off
  the unit SVG in the source catalog), so callers pass `min = 0`. Baking it is a bundle version bump,
  wanted for LRM/PPC close-range accuracy.
- **Terrain and line-of-sight:** woods / partial cover, and secondary-target / indirect fire, stay the
  player's to apply.
- **Aimed shots:** immobile / targeting-computer eligibility, riding on the same target model — a small
  later addition.
- **VSP pulse range-based to-hit:** the −3/−2/−1-by-bracket modifier; pulse is currently baked as a flat
  value. Minor.

### Force-building budget: remaining work

The BV/PV budget with skill-adjusted point cost is implemented (skill-adjusted cost engine, optional
per-session point limit, and a pre-add skill/cost preview). Remaining items:

- **Unmodeled cost sources:** MD-implant and quirk BV modifiers, and the Alpha Strike firing-arc PV
  components, are not yet folded into the point cost.
- **Budget editing from the sessions browser:** setting or editing a session's point limit for a
  non-loaded session is not wired; the limit can currently be set only once the session is loaded.

By design, the point limit is advisory: adding a unit over budget is warned but not blocked.

### Special Pilot Abilities (SPAs)

Neurohelmet currently shows Alpha Strike specials as tags only and has no pilot-ability concept. A
complete reference catalog exists in Mekbay: 73 Classic abilities (id/name/cost, per-skill limits —
Green 0 through Ace 5 abilities — unit-type eligibility, and BV impact) and 159 Alpha Strike specials
(tag/name, Standard vs. Optional, consumable/exhaustible flags), plus an effect-hook registry covering
adjustHeat / adjustMovement / adjustCriticalHits / adjustRollModifier.

- **Phase 1:** Bake the ability catalog (name, cost, summary, eligibility) into the bundle; let a pilot
  carry a list of SPAs via an editor modeled on the existing Skills modal; display them on the pilot
  panel and the AS card; and fold ability cost into the force BV/PV budget, enforcing per-skill ability
  limits. Effects remain labels, matching how AS specials are handled today.
- **Phase 2 (optional):** Decode the abilities with clean mechanical effects (heat and movement
  adjusters) into the engine, following the approach already used for aerospace criticals.

Size: medium. High value for Alpha Strike and force-building; supplies the crew half of the force-builder
budget.

### Force composition radar / roster analysis

Builds on the force BV/PV budget. Mekbay's force-radar panel scores a force on Mobility / Endurance /
Range / Damage (Classic, plus Alpha Strike variants), with per-unit contribution measured against
population min/avg/max.

- **Scope to ship:** A compact roster-summary view (a help-style overlay or a Sessions-browser panel)
  showing the force's totals and averages — tonnage, BV/PV (already summed), total armor and structure,
  aggregate damage (AS damage / Classic damage-per-turn), average walk/jump, and maximum range. Rendered
  as a text "radar" (bars), not a chart.

Size: small-medium. A natural companion to the force budget.

### BV/PV budget optimizer

An optional extension to the force budget. Mekbay's budget-optimizer dialog substitutes units to hit a
target BV/PV (skill-adjusted and C3-tax-aware) and reports the delta. A Neurohelmet version would suggest
skill tweaks or catalog swaps to land a roster on its `Session::limit`. This is lower priority:
assembling a force by hand is the common path, and the existing budget readout already guides it.

Size: medium; optional.

### Faction/era availability filter + weighted random force generation

The catalog supports availability-aware filtering and a weighted single-force draw for all catalog
modes: per-unit availability scores (derived from MegaMek's RATGenerator data, keyed era → faction) are
baked into the bundle, an "availability lens" filter tints and re-sorts the unit list by a chosen faction
and era across six rarity tiers plus an "unknown" tier (it sorts, never hides), and a seeded,
reproducible force generator draws weighted-random units that honor hard facets (unit type / tonnage /
role are hard gates; availability then weights the draw within survivors) up to a target count under a
hard budget ceiling. The following phases extend this foundation.

**Phase 1.5 — Formation (lance) types (planned; medium–large).** Layer BattleTech tactical formation
types onto the weighted draw as composition constraints. The canonical definitions live in MegaMek's
`FormationType` (in the `ratgenerator` package), covering families such as Battle (Light/Medium/Heavy,
Rifle, Berserker/Close), Assault (Anvil, Fast Assault, Hunter), Fire (Direct Fire, Fire Support,
Anti-Air, Artillery Fire, Light Fire), Pursuit (Probe, Sweep), Recon, Command (Order, Vehicle Command),
and Anti-'Mech. Each type carries an ideal role, a minimum weight class, a main-criteria predicate,
count/percent "other" constraints, and a grouping criterion. Because `FormationType` sits in the same
RATGenerator backbone that supplies the availability scores, a formation type is a constraint layer on
the existing weighted draw. Porting the definitions as data is straightforward; the substantive work is a
good-enough solver — greedy fill toward each constraint's minimum with best-effort fallback — since
MegaMek solves composition combinatorially via constraint bitmask/group search. Licensing is
unencumbered: the project is GPL-3.0-or-later, so porting the GPLv3 MegaMek code is fine.

**Phase 2 — Mixed-force composition (planned; medium).** Beyond all-'Mech forces, offer combined-arms
draws (mixed 'Mech / vehicle / aerospace in chosen proportions) plus pure all-vehicle and all-aerospace
forces. The baked availability data already spans vehicles, aerospace, and infantry, so the work is
chiefly a composition spec (proportions per unit type), supporting UI, and drawing from multiple type
pools. Designed to compose with Phase 1.5, since a formation type may itself admit non-'Mechs.

**Phase 3 — Campaign tracking (exploratory; future).** A heavier, distinct axis: generate a large force
(e.g. a 'Mech battalion), select a subset (one lance) to bring up into live record sheets, and persist
damage, ammo use, and destruction across multiple games. This extends the current single-game live
tracking to durable, multi-session per-unit history for a roster. It is recorded so the availability and
force-roll data model is designed not to preclude it, and overlaps the broader force-management axis. The
current roster cap of 12 units (one Company) means larger generated formations depend on this phase.

**Deferred generator knobs (small).** Additional draw controls not in the first cut: a role-bias slider,
and a requisition-vs-salvage weighting mix — the baked availability score currently collapses
RATGenerator's separate requisition and salvage weights into a single value.

## Presentation & export

### Display profiles: further theming and layout density

Neurohelmet presents the same application well on both a Raspberry Pi panel (~100×30 terminal) and a
roomier laptop terminal by diverging presentation on two axes — color (themes) and screen size (layout
density) — while all game and domain logic stays shared. The semantic theme palette, the fifteen shipped
themes (including faction liveries), background painting, the in-app display picker (Ctrl-T), a persisted
`config.json`, the `Pi`/`Modern` display profiles, the persistent play-screen force sidebar, and the
space-adaptive Alpha Strike card grid are in place. The following extensions remain:

- **Graded multi-stop heat ramp.** The current heat coloring uses a three-band good/warning/danger
  scheme; a five-stop ramp would shift the band thresholds and is a follow-up.
- **High-contrast / colorblind-safe theme.**
- **In-app configuration editor with live file hot-reload**, folding into the broader configuration-file
  work.
- **Additional `Modern`-profile density wins:** lift the preview/modal row caps and widen columns when
  space allows; larger unit "doll" boxes with inline critical-slot lists; and make the random
  force-generation UI profile-aware.

Size: small-to-medium; render-layer only, with no data re-bake.

### Print-to-PDF record-sheet export ✅ (2026-07-12 — all three modes)

Neurohelmet renders print-ready record sheets to PDF for **all three** BattleForce-family modes. No
direct PDF record-sheet export exists elsewhere in the MegaMek ecosystem for these systems — Strategic
BattleForce and Alpha Strike cards are print-dialog "Save as PDF" only, and the Abstract Combat System
(ACS) has no printable sheet at all — so this capability is net-new (strongest for ACS: nothing existed
anywhere). Status:

- **SBF** ✅ — a faithful port of MegaMek's `SBFRecordSheet.java` (one page per formation).
- **BF** ✅ — one page per Unit (lance) + an "Unassigned" page for ungrouped pool elements; each
  element prints its AS-card stat line, the four-bracket damage row, blank Armor/Structure pip rows,
  a `1 2 3 S` heat track, Specials, and a Destroyed box. A Unit past six elements paginates onto a
  `(cont.)` page. (`bf_sheets` / `bf_unit_svg` / `bf_element_card` in `pdf.rs`.)
- **ACS** ✅ — a **Combat Unit** sheet per `AcsCombatUnitState` (header stat grid, 75/50/25%
  Morale-Check triggers, the Combat-Teams summary, and blank Fatigue/Morale/COM/LEAD aids) plus one
  **Formation Tracking** sheet per force (a box per non-empty Formation + force PV / Leadership).
  (`acs_sheets` / `acs_combat_unit_svg` / `acs_formation_tracking_svg`.)

**Approach (shipped).** Neurohelmet generates its own vector SVG sheets in code — modeled on the
official CGL BattleForce Record Sheets as a layout reference only, never bundled — filled from the same
derived stats the TUI shows and rendered via `svg2pdf` + `usvg` (pure-Rust, offline), assembled into one
multi-page PDF with `pdf-writer`. BF/ACS share a `begin_sheet`/`end_sheet` scaffold (page, scaled
1435×2000 sheet space, BT+CGL logos, titled banner, Topps/CGL notice) with the SBF sheet. The sheet is
**always a pristine blank fill-in form** ([`make_blank`] strips all live state — the decided design, no
`--blank` flag / no current-state variant). Exposed as the `--pdf <session> [outfile]` CLI verb and the
in-app **`P`** key on all three screens. Full field inventories in
[docs/pdf-record-sheet-spec.md](docs/pdf-record-sheet-spec.md).

**ACS wiring note (resolved).** The ACS sheets read Combat-Unit and Formation state from the `Session`
directly (`acs_combat_unit` / `acs_formation`), so — unlike the game-log export — no `export::render_turn`
ACS arm or `LogEntry.acs` field was needed for the PDF.

**Deferred:** unit-type sheets for units neurohelmet doesn't field (DropShip/WarShip/JumpShip/Space
Station/Squadron); byte-reproducible golden PDFs (tests assert structure — `%PDF`, MediaBox count, valid
SVG — not exact bytes).

### Game log: additional export options

The turn-by-turn game log — live snapshot capture (`L`), image and transcript export (`--export`), and
publishing to a GitHub Pages gallery (`--publish`) — is built for the Classic tracker and is fully opt-in
and read-only. Potential enhancements:

- Extend logging to the Alpha Strike screen (currently Classic-only).
- Optional per-snapshot labels (typed names instead of "Turn N").
- A grid montage layout for exported turns.
- A `--replay` viewer for recorded sessions.
- Configurable logs repository and image scale.

### Custom configuration: keybinds and theme

Medium size; currently deprioritized behind game-aid features. A `config.json` under
`dirs::config_dir()/neurohelmet/` (sessions already use `dirs` plus JSON), loaded at startup, with a key
to hot-reload. Motivated by small / ortholinear keyboards (e.g. Planck) where some symbol defaults are
awkward. A stopgap has already shipped — base-layer aliases (`,`/`.` for mech switch, `o`/`i` for heat)
alongside the originals; this item is the full, user-editable system.

- **Keybinds:** remap the tracker gameplay actions (mech next/prev, heat ±, end turn, fire/primary,
  repair, facing, crit, shutdown, pilot hit/heal, add/delete, sessions, quit). List/modal navigation
  (arrows, j/k, Enter, Esc) stays built-in. Define an `Action` enum plus default keymap and dispatch
  `tracker_key` through it instead of a hardcoded `match`; parse key strings (`"]"`, `"tab"`,
  `"shift+tab"`, `"ctrl+s"`). Guardrail: never allow unbinding quit.
- **Theme:** a semantic palette (accent, danger, dim, warn, good, selection, heat, …) with the current
  look as `default`, a `high-contrast` preset for the Raspberry Pi console, and per-color overrides.
- **UI:** edit the file externally; a reload key applies it live. An in-app editor is a later nicety.

## Larger & exploratory

### Smaller polish

A set of optional tracker refinements:

- **Two-phase pending→commit damage** (Mekbay-style): stage hits, then confirm before applying. An undo
  action currently covers the same need, so this remains optional.
- **"Fire all" volley (Classic mode):** a single key fires every unfired, ready weapon at once — summing
  heat and spending ammo — cleared at end of turn. Distinct from the Alpha Strike game mode.
- **UAC/RAC jam marker:** ✅ shipped as part of [Equipment active-state toggles](#equipment-active-state-toggles-2026-07-12)
  (the `J` key).
- **Canvas silhouette doll:** an alternate `view.rs` rendering using ratatui `Canvas` (Braille) as a
  toggle; cosmetic, aimed at legibility on small screens.
- **Status-line persistence:** keep the last status message visible slightly longer (it is currently
  cleared on the next keypress).

### Large force-management axis

This is the army-building and -management half of Mekbay, distinct from Neurohelmet's play-tracking
focus — a whole product axis rather than a single feature. It is retained as a candidate but ranked last.
Constituent pieces:

- **Random force generator** — by faction / era / BV-PV budget / role, with skill optimization to budget.
- **Hierarchical TO&E templates** — 8 faction org systems (IS, Clan, ComStar, WoB, Society, Merc, CC,
  SLDF), lance → company → battalion, driven by a rules engine (org-solver, org-namer, org-tier, and
  per-faction definitions).
- **Formations and formation bonuses** — formation definitions, requirement validation, and SPA/effect
  distribution per formation.
- **C3 networks** — master/peer topologies plus the per-unit BV tax (this is where the equipment-toggle
  C3 item lands).
- **Multi-force operations** — snapshotting two or more forces with friendly/enemy alignment for scenario
  setup.
- **Org-chart spatial layout** — forces placed on a 2D canvas.
- **Force packs** — pre-built named compositions.
- **Force tagging / collections** — free-form labels across forces.

**Open question:** Most of this is multi-pane, mouse- and canvas-oriented UI that may not suit a ~100×30
terminal; a flat roster plus budget already covers the tabletop need. Before building any of it, a
decision is required on whether Neurohelmet's scope should grow from single-screen tracker toward force
manager, and which pieces survive the screen constraint — the generator and force packs are the most
plausible TUI fits, org-charts the least.

Size: large; low priority.

## Non-goals

Neurohelmet's scope is bounded by three design invariants: an offline terminal targeting a ~100×30
Raspberry Pi screen; manual dice, where the application surfaces targets and prompts but never
auto-rolls; and a tracker-first focus rather than a full army-management suite. The following are
deliberately out of scope:

- **Web-platform features:** cloud accounts and OAuth, real-time multiplayer synchronization,
  browser-side storage, QR/URL force sharing, browser print-option dialogs, and touch zoom/pan/swipe
  navigation. The offline analog for sharing is the game-log image export with optional GitHub Pages
  publishing.
- **Automatic dice rolling** and 2d6 crit-roll dialogs. Neurohelmet provides reference tables and
  computed to-hit targets (including GATOR and piloting-skill rolls) instead. An optional in-app dice
  roller is the only concession that would fit the design, and it is intentionally not planned.
- **Two-phase pending/commit damage.** Undo covers this need.
- **Turn/phase tracking** (movement/weapon/physical/heat phases). Neurohelmet defines no turn structure.
- **Reference-data richness with low tracker value and high bundle cost:** unit fluff/lore, manufacturer
  and factory data, sourcebook metadata with store links, and Sarna page links. Design quirks are the
  exception and are tracked separately.

## Known limitations

- **Zero-heat weapons:** weapons whose `equipment2.json` entry omits a `heat` value (machine guns,
  B/M-pods, Narc/iNarc, Fluid Gun, Nail/Rivet Gun) are treated as 0 heat and displayed as `—`. The bake
  reports "unresolved heat" only for weapons genuinely absent from `equipment2.json` (currently none).
- **Ammo linking** matches on `ammoType:rackSize` and uses the first compatible non-empty bin.
- **Internal structure:** runtime values come from the SVG-derived bundle; the tonnage table in
  `engine/internal.rs` is a cross-check and fallback (for example, for a future MTF-only bake path).
- **Bundle format:** positional bincode keyed by `BUNDLE_VERSION`. New `Mech`/`WeaponMount`/`AmmoBin`
  fields use `#[serde(default)]` so existing `session.json` files still load; changing the bundle format
  requires a re-bake and an application rebuild.
- **Session spec migration:** a `session.json` embeds a snapshot of each unit's `Mech` spec, so a session
  saved before a re-bake would otherwise retain stale baked data. `Session::relink_specs(&Bundle)` runs
  on every load (startup and the sessions browser) to refresh each tracked unit's spec from the current
  bundle, matched by display name, while preserving all live state (damage, heat, ammo counts, crits,
  pilot, Alpha Strike, and bin/munition choices). Ammo bins new to a refreshed spec start full; a unit no
  longer in the bundle keeps its old spec. The refresh persists via autosave and reports a status line.
- **Bake politeness:** `db.mekbay.com` rate-limits (HTTP 429); use `--jobs 4`. The `.bake-cache/`
  directory means re-bakes mostly hit local files.
- **Test-coverage gap:** the raw terminal I/O path (raw-mode entry, crossterm event reads) is not covered
  by tests — snapshots exercise rendering and `handle_key`, not the real TTY. New interactive flows
  should be sanity-checked on the Raspberry Pi.
