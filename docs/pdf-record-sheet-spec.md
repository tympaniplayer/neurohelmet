# Print-to-PDF Record Sheet Export — Design & Reference

neurohelmet writes a **print-ready PDF record sheet** for its three BattleForce game modes — Standard
BattleForce (BF, mode #5), Strategic BattleForce (SBF, #4), and the Abstract Combat System (ACS, #6).
Each formation renders to a PDF via `svg2pdf`. The SBF sheet is a **faithful port of MegaMek's
`SBFRecordSheet.java`**; BF and ACS follow the same MegaMek-parity approach. A single `--pdf <session>`
CLI verb (a sibling of `--export`/`--publish`) and an in-app export key drive it; there is **one page
per formation**, and the sheet is always a **pristine blank fill-in form** — a printout is for taking a
clean sheet to the table, so it never renders live battle state (see Status).

## Ecosystem context — a net-new export

No direct PDF record-sheet export exists for any of these three systems anywhere in the hobby's
reference toolchain (MegaMek, MekHQ, MegaMekLab):

- **SBF** has a sheet (`SBFRecordSheet.java`, a Java2D canvas), but it only implements
  `java.awt.print.Printable` → `PrinterJob.printDialog()`; a PDF is obtainable only via the OS "Save
  as PDF" dialog, and MegaMek writes no `.pdf`.
- **Standard BF** has no formation sheet at all — MegaMek renders it as per-unit Alpha Strike cards
  (`ASCardPrinter`, same print-dialog path).
- **ACS** has **nothing** — MegaMek's only "Abstract Combat" code is **ACAR** (Abstract Combat
  Auto-Resolve), a sim engine that emits an HTML log; there is no ACS record sheet in MegaMek, MekHQ,
  or MegaMekLab.
- MegaMekLab's genuine "Export to PDF" (Batik SVG → FOP `PDFTranscoder` → PDFBox) covers **Classic
  per-unit sheets only** — no Alpha Strike, SBF, or ACS class in its `printing` package.

A real PDF export for BF/SBF/ACS is therefore net-new across the whole ecosystem — strongest for ACS
(nothing exists), then BF (no formation sheet), then SBF (print-dialog-only).

**Design invariant (the whole doc rides on this):** neurohelmet generates **its own vector SVG record
sheets in code**, modeled on the official CGL *BattleForce Record Sheets* layout as a **visual
reference only**. The official sheets are never bundled or redistributed. The sheets carry the real
BattleTech and Catalyst logos and the verbatim Topps/CGL footer sourced from MegaMek's
**CC-BY-NC-SA-4.0** vector asset set (matching MegaMek and Mekbay), and are themselves licensed
**CC-BY-NC-SA 4.0**, matching `mechs.bin`'s NOTICE and MegaMek's own `data/images/recordsheets/`
precedent. Each export builds the sheet from session data and renders it with `svg2pdf` + `usvg` — the
same SVG→PDF route MegaMekLab uses, in pure Rust. The per-mode **field inventories** below are the
sheet field-maps.

---

## Status

- **SBF is shipped.** `crates/app/src/pdf.rs` implements `neurohelmet --pdf <session> [outfile]` and the
  in-app **`P`** key (SBF screen); both render every SBF formation and assemble **one multi-page
  US-Letter PDF** (a page per formation) via `svg2pdf` + `pdf-writer`. Covered by `pdf::tests::*`
  (including a real two-company battalion and the multi-page path) and verified end-to-end.
- **The SBF sheet is a faithful port of MegaMek's `SBFRecordSheet.java`** — drawn in its 1435×2000
  space and scaled to Letter, not an original layout.
- **Always blank, never current-state.** A printout is for taking a clean sheet to the table, so the
  sheet strips all live damage/crits/morale; there is no `--blank` flag and no current-state variant.
  SBF armor is a single **numeric value** the player writes down and reduces (IO:BF pp.170–171) — there
  are **no pips**.
- **Real BattleTech and Catalyst logos** (from MegaMek's CC-BY-NC-SA assets), the exact layout, and the
  **verbatim** Topps/CGL footer — matching MegaMek and Mekbay so the sheet reads as the real thing.
- **Roboto** is bundled for text (Apache-2.0) — the face Mekbay uses.
- Per-element AS sub-rows are rendered (via `element_of`); a `maxWidth` approximation clamps long names
  and specials to their columns.
- **BF and ACS are now shipped too (2026-07-12).** BF renders one page per Unit (+ an Unassigned page),
  each element a card with its AS stat line, four-bracket damage row, blank Armor/Structure pip rows, a
  `1 2 3 S` heat track, Specials, and a Destroyed box (`bf_sheets`/`bf_unit_svg`/`bf_element_card`). ACS
  renders a Combat Unit sheet per `AcsCombatUnitState` (stat grid, 75/50/25% Morale-Check triggers,
  Combat-Teams summary, blank Fatigue/Morale/COM/LEAD aids) plus one Formation Tracking sheet per force
  (`acs_sheets`/`acs_combat_unit_svg`/`acs_formation_tracking_svg`). BF/ACS share a `begin_sheet`/
  `end_sheet` scaffold with SBF. All three are on the `P` key and the `--pdf` verb. The ACS arm reads
  Combat-Unit/Formation state straight off the `Session`, so no `export::render_turn` ACS arm was needed.

---

## Layout-authority convention

This is an app-crate feature, not a rules module, so it cites **code** (file and function references)
and **template layout references**, not the rulebook. The layout authority for each mode is the
official CGL *BattleForce Record Sheets* PDF (a **vector** US-Letter set): **BF** pp.1–12 (one sheet
per unit type), **SBF** p17 (Formation Record Sheet), **ACS** p18 (Combat Unit Record Sheet) + p19
(Formation Tracking Record Sheet). Layout coordinates are measured off those pages when authoring the
SVGs, and field sets are cross-checked against IO:BF (BF pp.322–324; SBF; ACS pp.622-ff) and MegaMek's
`SBFRecordSheet.java`.

---

## Scope

The mode-scope doctrine (single force, boardless, manual-first) is inherited unchanged; this feature
adds an *output* and touches no rules.

**In scope:**
- Code-generated SVG sheets for BF (ground unit-type variants), SBF, and ACS (both sheets), licensed
  CC-BY-NC-SA-4.0, rendered entirely from the bundled binary (no external files at runtime).
- A `--pdf <session> [outdir]` CLI verb that loads a saved session, builds the right sheet(s), and
  writes one PDF via `svg2pdf`.
- One page per **formation** (SBF, ACS) or per **Unit** (BF), matching how `export::render_turn`
  already paginates each mode.
- An always-blank fill-in form (every live counter at its pristine default).
- An in-app export key on the BF/SBF/ACS play screens.

**Out of scope (v1):**
- Classic, Alpha Strike, and Override modes (MegaMekLab already does Classic unit sheets).
- Sheets for unit types neurohelmet doesn't field — DropShip/WarShip/JumpShip/Space Station/Mobile
  Structure/Squadron. BF fields only 'Mechs, ProtoMechs, vehicles, infantry/BA, and aerospace
  *fighters*; the capital/DropShip family falls to the `La` catch-all in `sbf_type_from_tp` with a
  dead/unbaked crit column, and neurohelmet's aerospace support (ROADMAP §21) fields fighters only.
  Each unit-type sheet is added **when** neurohelmet gains that unit.
- Aerospace ACS (neurohelmet's ACS is ground-only v1) and any OpFor/two-sided sheet.
- Bundling or redistributing the official CGL sheets (a hard non-goal — reference only).

---

## Approach — generate neurohelmet's own SVG sheets

Three approaches are possible; neurohelmet uses **path B (generate its own sheets in code)**.

| | Chosen: **B — own SVG, generated in code** | Rejected: **A — overlay on official PDF** | Rejected: **image-in-PDF** |
|---|---|---|---|
| How | generate our own SVG per mode, `svg2pdf`→PDF | stamp values onto the user's own CGL PDF (`lopdf`) | rasterize the TUI `Buffer`, embed via `printpdf` |
| Fidelity | print-quality, selectable text, our layout | print-quality, but the CGL layout | fixed-DPI bitmap, screenshot look |
| Ships? | **yes** — CC-BY-NC-SA assets, works out of the box | no — needs the user's CGL PDF present | yes, but ugly on paper |
| Licensing | clean (MegaMek's CC-BY-NC-SA assets, no CGL redistribution) | personal-use only, nothing shipped | clean |
| Precedent | **exactly MegaMek's model** | more conservative than MegaMek | neurohelmet's own §13 export |

**Rationale.** MegaMek is the precedent: it ships **no** official CGL PDF and instead recreates every
sheet itself — 54 hand-built SVG templates in MegaMek's `data/images/recordsheets/templates_us/`
(BATTLETECH/Catalyst logos redrawn as vector art, the `© Topps… photocopy for personal use` footer
reproduced), plus SBF drawn in Java2D — all licensed **CC-BY-NC-SA-4.0** (MegaMek's `LICENSE.assets`),
the same license as neurohelmet's `mechs.bin`. Recreating neurohelmet's own sheets is consistent with
both MegaMek and neurohelmet's existing NOTICE, is shippable, and stays print-quality. Because MegaMek
has **no** BF or ACS templates at all, neurohelmet's BF/SBF/ACS SVG set is a genuine first. Image-in-PDF
is rejected for looking like a screenshot; the overlay-on-the-user's-PDF path for shipping nothing and
requiring the CGL file to be present.

**Licensing rule for the sheets:** neurohelmet's SVGs reproduce *layout and labels* (which carry thin
copyright — blank forms) and carry the BattleTech and Catalyst logos and the verbatim Topps/CGL footer
sourced from MegaMek's **CC-BY-NC-SA-4.0** vector asset set, exactly as MegaMek and Mekbay do. They are
modeled on the official CGL layout as a visual reference only; the official CGL sheets are never
bundled or redistributed. Everything ships under CC-BY-NC-SA-4.0.

---

## Reference sources

**Layout references (measure from, never bundle):**
- Official CGL sheets — the CGL *BattleForce Record Sheets* PDF (vector, US Letter 612×792pt). Page
  map: BF p1 Inner Sphere/Periphery (3 Units × 4 element slots), p2 ComStar (×6), p3 Clan (×5), p4–12
  Aerospace/DropShip/WarShip/JumpShip/LgSupport/MobileStruct/Squadron; SBF p17; ACS p18 (Combat Unit) +
  p19 (Formation Tracking).
- MegaMek's construction precedent — MegaMek's `data/images/recordsheets/templates_us/*.svg` (how a
  filled-by-code BT record-sheet SVG is structured; the `mml-color-elements` id scheme) and
  `SBFRecordSheet.java` (the SBF sheet's field grouping).

**The render pipeline & code:**
- `crates/app/src/pdf.rs` — the SVG generation + `svg2pdf` render + PDF assembly; registered as
  `mod pdf;` in `main.rs` beside `mod export;` and `mod publish;`.
- `crates/app/assets/fonts/` — the bundled value font (Roboto), loaded via `include_bytes!`.
- `crates/app/src/main.rs` — the `--export`/`--publish` arg-scan pattern `--pdf` mirrors.
- `crates/core/src/session.rs` — `Session`, `new_with_mode`, load-by-name; the per-mode state.
- `crates/app/src/export.rs` — `render_turn`'s pagination logic, mirrored by `render_session_sheets`.
- `crates/app/src/tui/view.rs` — the per-mode draw fns that authoritatively enumerate every field
  (BF `bf_card_lines` / `bf_unit_header_line`; SBF `draw_sbf_*`; ACS `draw_acs_*`).

**Crates:** `svg2pdf` 0.13 + `usvg` 0.47 (typst ecosystem; MIT/Apache + MPL-2.0, GPL-3.0-compatible;
text embedded as real subsetted fonts since svg2pdf 0.11). A bundled free TTF (Roboto) supplies the
value text. Multi-page assembly uses `pdf-writer` with svg2pdf's chunk API.

---

## Data fidelity — the sheet field-maps

These inventories are the durable contract of what each sheet's SVG carries. Each row is `static`
(recomputed from the immutable baked `AsStats` — the `ov_card` doctrine, never persisted) or `tracked`
(persisted live state). Because the export is always blank, every `tracked` counter is at its pristine
default; the blank-zeroing set after each table lists exactly which counters are reset.

### Standard BattleForce (BF) — per element (`bf_card_lines`) + per Unit (`bf_unit_header_line`)

Template layout (official p1): **3 Units per page**, each Unit **4 (IS) / 6 (ComStar) / 5 (Clan)**
element slots — three ground variants to author, chosen by element type/faction.

| Field | Source | s/t | Notes |
|---|---|---|---|
| Element name | `Mech::display_name()` | static | card title |
| Type / Size | `AsStats.tp` / `AsStats.size` | static | `BM/CV/BA/…`; size feeds physicals |
| MV (hexes) | `AsStats.movement` → `movement_hexes()` | static | AS inches ÷2; aero keeps thrust |
| MV (live) | `Session::bf_current_mp(i)` | tracked | when heat/crit/motive degrade it |
| TMM (baked / live) | `AsStats.tmm` / `bf_live_tmm(i)` | static / tracked | ground only; aero prints TH |
| Threshold (TH) | `AsStats.threshold` | static | aero only, replaces TMM |
| Skill | `TrackedMech.gunnery` | tracked | base to-hit; player-editable |
| PV | `skill_adjusted_pv(pv, gunnery)` | tracked-derived | printed point value |
| Armor rem/max | `as_armor_remaining()` / `AsStats.armor` | tracked / static | pip row |
| Structure rem/max | `as_struct_remaining()` / `AsStats.structure` | tracked / static | pip row |
| Heat (1 2 3 S) | `TrackedMech.as_heat` (0–4) | tracked | 4 = shutdown |
| Overheat (OV) | `AsStats.overheat` | static | on the heat row |
| Damage S/M/L/E | `bf_current_damage(...)` over `DamageVector` | static base / tracked-eff | ground E = max(L−1,0) |
| Specials (SUAs) | `AsStats.specials` | static | the Specials row |
| Destroyed ☐ | `bf_destroyed()` | tracked-derived | the checkbox |
| Crits (Eng/FC/MP/Wpn, motive, stun, ARM, kill) | `TrackedMech.bf.*` | tracked | not on the blank sheet; add a crit note area |
| **Per-Unit header** | `BfUnitState.name`, Unit MV (`bf_unit_mv`), Size, Σ PV, morale, notes | mixed | Unit MV = slowest surviving dismounted element |

*Blank-zeroing set:* `as_armor_hits`, `as_struct_hits`, `as_heat`, the whole `BfLive` (`bf`),
`BfUnitState.morale` → Normal, drop the ephemeral `App.bf_shot` context.

### Strategic BattleForce (SBF) — formation + unit + element (official p17 / `SBFRecordSheet.java`)

Template layout: header row + a **4-Unit** summary block + per-Unit "Alpha Strike Elements"
sub-blocks. One page = one formation (≤4 units; ≤20 elements). neurohelmet carries all 12 formation and
12 unit columns as `SbfFormation`/`SbfUnit` fields (a golden-tested MegaMek converter port).

| Tier | Field | Source | s/t |
|---|---|---|---|
| Formation | Name / Type / Size | `SbfFormationState.name` / `SbfFormation.{sbf_type,size}` | tracked-label / static |
| Formation | Move (+mode) / JUMP / TrspMove | `SbfFormation.{movement,move_mode,jump_move,trsp_movement,trsp_mode}` | static |
| Formation | TMM / Tactics / Morale-rating / Skill / PV / Specials | `SbfFormation.{tmm,tactics,morale_rating,skill,point_value,suas}` | static |
| Formation | Morale rung / Jump-used / Activated / Round | `SbfFormationState.{morale,jump_used_this_turn,is_done}`, `SbfState.round` | tracked |
| Unit | Name / Type / Size / Move / JUMP / TrspMove / TMM | `SbfUnitState.name`, `SbfUnit.{sbf_type,size,movement,move_mode,jump_move,trsp_movement,trsp_mode,tmm}` | tracked-label / static |
| Unit | Armor rem/max | `SbfUnitState.armor_remaining()` / `SbfUnit.armor` | tracked / static |
| Unit | Damage S/M/L(/E), current | `SbfUnit.damage`, `SbfUnitState.current_damage()` | static / tracked-derived |
| Unit | Skill / PV / Specials | `SbfUnit.{skill,point_value,suas}`, `base_gunnery()` | static / tracked-derived |
| Unit | Crits DMG/TGT/MP; COM/LEAD | `SbfUnitState.{damage_crits,targeting_crits,mp_crits,is_commander,is_leader}` | tracked |
| Element | Type/Size/Move/Arm/Str/S/M/L/E/OV/Skill/PV/Specials | per-element `AsElement` via `Session::sbf_element` | static (skill tracked) |

*Blank-zeroing set:* `SbfUnitState.{armor_hits, damage_crits, targeting_crits, mp_crits}`, morale →
Normal, `is_done` = false, `jump_used_this_turn` = 0, `SbfState.round` = 0.

### Abstract Combat System (ACS) — Combat Unit sheet (p18) + Formation Tracking sheet (p19)

Two templates. **p18 Combat Unit sheet:** header (Type/Size/Move/TranspMP/TMM/ARM/S/M/L/E/Tactics/
Morale/Skill/PV) with the **75%/50%/25%-Armor "Morale Check Triggers"** = `damage_thresholds`, a
4-row Combat-Teams summary, and 4 Combat-Team unit sub-blocks. **p19 Formation Tracking sheet:** a
grid of formation boxes (ID/Name/Type/Move/Tactics/Morale/Skill + a Combat-Units list). Ground-only.

| Tier | Field | Source | s/t |
|---|---|---|---|
| Combat Unit | Name / Type / Size / Move / TranspMP / TMM | `AcsCombatUnitState.name`, `AcsCombatUnit.{acs_type,size,movement,move_mode,trsp_movement,tmm}` | tracked-label / static |
| Combat Unit | ARM rem/max | `armor_remaining()` / `AcsCombatUnit.armor` | tracked / static |
| Combat Unit | Damage S/M/L | `AcsCombatUnit.damage` | static |
| Combat Unit | Morale Check Triggers [75/50/25%] | `AcsCombatUnit.damage_thresholds` | static |
| Combat Unit | Tactics / Morale-value / Skill / PV / Specials | `AcsCombatUnit.{tactics,morale_rating,skill,point_value,suas}` | static |
| Combat Unit | Morale rung (7) / Fatigue FP+band / COM / LEAD | `AcsCombatUnitState.{morale,fatigue_points_x2,is_commander,is_leader}`, `acs_fatigue_band()` | tracked |
| Combat Teams | 4-row summary + unit sub-blocks | `AcsCombatTeam` (fold) / its `SbfUnit`s | static |
| Formation (p19) | ID / Name / Type / Move / Tactics / Morale / Skill / units | `AcsFormationState.{name,morale}`, `AcsFormation.{acs_type,movement,tactics,skill}` | tracked-label / static |
| Force | Round / Force PV / Leadership rating | `AcsState.{round,leadership_rating}`, `Session::acs_force_pv()` | tracked / derived |

*Blank-zeroing set:* `AcsCombatUnitState.{armor_hits, fatigue_points_x2}`, morale → Normal, COM/LEAD =
false, formation morale → Normal, `is_done` = false, `AcsState.round` = 0.

---

## Implementation

### Session → sheets

`export::render_turn`'s per-mode pagination is factored into a `Session`-based function both callers
share:

```rust
/// One (filename-stem, page-heading, generated-SVG) per printable sheet for `s`, paginated like
/// export::render_turn: SBF/ACS → per formation; BF → per Unit (+ implicit "Unassigned").
pub(crate) fn render_session_sheets(s: &Session) -> Vec<(String, String, String)>; // String = SVG
```

Each mode selects its layout (BF: by element type/faction → IS/CS/Clan/aero variant), fills the fields
from the field-map above, and returns the generated SVG. The ACS arm is net-new (see Cross-cutting
notes).

### SVG generation

The SVG is **generated programmatically** in `pdf.rs` — not authored as static template files with
placeholder substitution. A formation holds a variable 1–4 Units, so building the sheet in code is
cleaner than filling fixed slots (and is how MegaMek fills its own templates). Every value comes from
the derived stats the TUI already shows (`Session::sbf_formation` / `sbf_unit` plus the `SbfUnitState`
live accessors), so the sheet matches the screen. All text is XML-escaped. Because there are no static
template asset files, the only bundled asset is the value font.

### Always-blank export

The sheet is always a pristine fill-in form: it renders the static stat lines and leaves every tracked
counter at its default (full armor/structure, no heat/crits, Normal morale, round 0). `make_blank`
performs the reset:

```rust
/// Reset every persisted live counter to default so the sheet is a pristine fill-in form
/// (static stat lines, full armor/structure, no heat/crits, Normal morale, round 0). Per the
/// blank-zeroing sets in Data fidelity.
pub(crate) fn make_blank(s: &mut Session);
```

### Render + assemble

Per generated SVG: `usvg::Tree::from_str(&svg, &opt, &fontdb)`, then `svg2pdf` to a US-Letter page
(612×792pt — the canvas the sheets are authored at). The pages are assembled into one multi-page PDF
with `svg2pdf::to_chunk` + `pdf-writer` (both already in the tree — no merge crate): each SVG becomes
an XObject, renumbered into a shared ref space and scaled to fill its page. The XObject is a **unit
square**, so it must be scaled by `[PAGE_W, 0, 0, PAGE_H, 0, 0]` — identity placement renders blank.
The value font is loaded into `fontdb` from the bundled TTF so text embeds and subsets rather than
flattening to paths.

### CLI verb

```
neurohelmet --pdf <session> [outfile]
```

Loads the session by name, runs `render_session_sheets`, renders, assembles, and writes the PDF, then
prints the path. Default output is `<sessions_dir>/<sanitize_name(session)>.pdf`.

### In-app export key

The **`P`** key (Shift+p) on the SBF screen (`App::export_pdf`) renders the live `self.session` via
`pdf::export_session` to `<sessions_dir>/<name>-sheets.pdf` and shows a toast with the path. `P` is
unbound on the SBF screen and matches the capital-letter-for-actions convention (`S`/`D`); global
handling only intercepts Ctrl+C/Ctrl+T/z. BF and ACS reuse the same `P` action when their sheets land.

Any new keybinding lands in three places: the in-app `?` modals in `view.rs`,
`docs/keybindings-cheatsheet.html`, and the committed `docs/neurohelmet-keybindings.pdf` (re-rendered
via Chrome Headless).

### Per-mode sheet content

- **SBF (shipped).** `sbf_formation` layout from official p17: header
  (Type/Size/Move/JUMP/Transport Move/TMM/Tactics/Morale/Skill/PV/Formation Specials) + a 4-Unit
  summary block + per-Unit "Alpha Strike Elements" sub-blocks. `Session::sbf_element` supplies the
  per-element sub-rows MegaMek draws but the TUI abbreviates.
- **BF (pending).** Ground variants for Inner Sphere (4 element slots), ComStar (6), and Clan (5),
  plus an aero-fighter variant (AF/CF/SC), modeled on official pp.1–4. `render_session_sheets` picks
  the variant per Unit by element type/faction and fills the BF field-map, cross-checked against IO:BF
  pp.322–324 and the `bf_card_lines` inventory. DropShip/WarShip/JumpShip/Space Station/Mobile
  Structure/Squadron sheets are deferred until neurohelmet fields those units.
- **ACS (pending).** Two sheets: the Combat Unit sheet (p18) and the Formation Tracking sheet (p19),
  ground-only. One Combat-Unit sheet per `AcsCombatUnitState` plus a Formation-Tracking sheet per
  force. Adding ACS also requires the net-new ACS arm in `render_session_sheets` (see Cross-cutting
  notes).

---

## Known limitations & open items

- **PDF determinism for goldens.** Pinning the producer and creation date would make output
  byte-reproducible (neurohelmet's golden/snapshot discipline). This is not yet done: tests currently
  assert structure (`%PDF`, MediaBox count) rather than exact bytes.
- **Overflow / pagination.** When a force exceeds a sheet's fixed slots (a BF Unit beyond its slot
  count; ACS Formation Tracking beyond 14 formations), the extra content paginates onto another copy of
  the sheet. SBF is safe (≤4 units/formation, ≤20 elements). The per-mode rule is confirmed when BF/ACS
  land.

---

## Cross-cutting notes

- **A genuine first.** No BF/SBF/ACS PDF export exists elsewhere in the MegaMek ecosystem.
- **MegaMek precedent is the model.** MegaMek recreates its own record sheets (54 Classic SVGs +
  Java2D SBF) under CC-BY-NC-SA-4.0 and never bundles the official CGL PDF — but has **no** BF or ACS
  templates. neurohelmet follows the same licensing model, omits the CGL redistribution, and fills the
  BF/SBF/ACS gap MegaMek never covered.
- **The official sheets are reference, not payload.** Layout and field placement are measured off the
  CGL PDF; none of it ships. The sheets are neurohelmet's own CC-BY-NC-SA-4.0 art.
- **ACS export wiring is net-new.** The ACS mode (GameMode #6) is complete, but the game-log export
  pipeline (`export.rs`) predates it and has no ACS arm. Adding the ACS sheets requires an ACS branch
  in `render_session_sheets` that reads Combat-Unit and Formation state from the `Session` directly; BF
  and SBF need no such addition and ship first.
- **Offline and self-contained is why `svg2pdf`, not Chrome.** `svg2pdf` + `usvg` and the generated
  sheets live inside the binary; nothing to detect or install. Chrome Headless (used at *build* time
  for the cheat-sheet PDF) is wrong for a *runtime* export — it breaks the
  no-external-runtime-dependency charter.
- **This reverses a recorded non-goal.** The ROADMAP Mekbay-parity note originally listed
  "printing / roster PDF" as a web-platform non-goal; this feature (ROADMAP §37) reverses the
  record-sheet slice.

---

## Appendix A — official sheet layouts (construction reference)

Measured from the official CGL *BattleForce Record Sheets* PDF (vector, US Letter 612×792pt). This is
the layout authority when authoring the SVGs; **not shipped**.

- **BF ground (p1 IS/Periphery):** 3 Units/page. Per Unit: header `Unit Name / Unit MV / Size / Point
  Value / Notes`; 4 element rows (mech icon · `MV` · `S(+0) M(+2) L(+4) E(+6)` · `SZ` · `Skill` · `OV`
  · `Destroyed ☐` · `Armor/Structure` two-row pip block · `Heat Scale 1 2 3 S` · `Special Abilities`).
  p2 ComStar = 6 element rows, p3 Clan = 5. pp.4–12 = other unit types.
- **SBF (p17):** `FORMATION:` header row (Type/Size/Move/JUMP/Transport Move/TMM/Tactics/Morale/Skill/
  PV/Formation Specials) → `UNITS:` 4-row summary (…/Arm/S/M/L/E/… + Notes) → `Unit One`…`Unit Four`,
  each an `Alpha Strike Elements:` sub-block (Type/Size/Move/Arm/Str/S/M/L/E/OV/Skill/PV/Specials).
- **ACS Combat Unit (p18):** `COMBAT UNIT:` header (Type/Size/Move/Transport MP/TMM/ARM/S/M/L/E/Tactics
  /Morale/Skill/PV) + `Morale Check Triggers 75%/50%/25% Armor` + `No Supply? ☐☐☐☐☐` → `COMBAT TEAMS:`
  4 rows → `COMBAT TEAM 1…4` unit sub-blocks (Type/Size/Move/Jump/Trans/TMM/ARM/S/M/L/E/Skill/PV/Specials).
- **ACS Formation Tracking (p19):** 2×7 grid of formation boxes: `ID`, `Formation Name`, `Type / Move
  / Tactics / Morale / Skill`, and a `COMBAT UNITS` list column.

## Appendix B — rejected approaches

- **Image-in-PDF** (rasterize the TUI `Buffer` via `printpdf`): reuses §13's pipeline and covers all
  modes cheaply, but produces a fixed-DPI screenshot, not a record sheet. Rejected.
- **Overlay on the user's official PDF** (`lopdf` stamp): print-quality and least work, but ships
  nothing and only works if the user has the CGL PDF present. Rejected in favor of the shippable
  MegaMek-style recreation.
