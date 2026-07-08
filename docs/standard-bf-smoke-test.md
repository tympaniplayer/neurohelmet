# Standard BattleForce Smoke Test

A ~5-minute manual pass that touches every Standard BF system once. Run after any
BF change. `?` opens the key reference at any point; `z` undoes — **every mutation
should unwind in exactly one `z`** (two = bug). The mode is AS mode's sibling: same
damage/heat verbs, hex-native everything, lance Units as header rows.

## Setup

- [ ] `cargo run` → `S` → `F` → type a name → `Enter` → `Enter` (blank PV limit)
  - *Expect: picker opens; status "New BattleForce session '<name>'".*
- [ ] Add ~5 'Mechs (search, `Enter`, `Enter` in the add modal, repeat), then `Esc`
  - *Expect: the add modal shows **Skill + PV** (not BV/piloting).*
  - *Expect: the BF sheet — the seeded empty "Unit 1" header ("(no elements — [g]
    assigns, [a] adds)") plus every added element under "▸ Unassigned".*
  - *Expect: cards are hex-native: an 8″ 'Mech reads MV 4; the card footer reads
    "Rng S 0-1  M 2-4  L 5-8  E 9-10\*" (never the AS inch brackets); the Damage
    row's E is L−1 for ground elements (aero keep their printed E).*

## Grouping

- [ ] `g` — grouping editor lists all elements, cursor on the last-added
- [ ] `a` → pick **Clan** → `Enter`
  - *Expect: applies immediately (nothing hand-entered yet): "Star 1" ×5. (IS →
    Lances of 4; ComStar → Level IIs of 6. Aerospace elements pair off into their
    own "Point"/"Air Lance" Units of 2 — never mixed with ground.)*
  - *Later, after renaming/morale-marking: re-running `g`→`a`→`Enter` instead shows
    an **itemized confirmation** ("Discards 1 custom name(s), 1 morale rung(s) —
    z undoes"); `n` cancels untouched, `y` applies, one `z` restores everything.
    Element damage/heat/crits live on the cards and are NOT on the bill.*
- [ ] `g` again, on one element exercise every verb:
  - [ ] `→` / `←` → moves it between Units (Unit Size restamps on membership edits)
  - [ ] `n` → splits it into a fresh Unit
  - [ ] `u` → unassigned (it renders under "▸ Unassigned" — legal, never forced back)
  - [ ] `s` / `S` → Skill worsens/improves; the card's PV moves per the printed
        Skill-PV brackets (p.50 — the AS table)
  - [ ] `x` → removed from the force entirely (pool renumbers; groupings remap)
  - [ ] `Esc` closes — element-less Units come off the sheet
- [ ] `r` → rename the active element's Unit
- [ ] `m` cycles that Unit's morale Normal → Broken → Routed → Normal
  - *Expect: the rung colors on the header and never changes on its own.*

## Combat loop

- [ ] `n` → "Round 1 begun (crew-stunned cleared)"
- [ ] `t` → to-hit editor (the p.39 table, 21 rows):
  - [ ] Range Medium: To-Hit = Skill + 2; Target TMM 2: +2 more
  - [ ] Attacker move → stood still: −1 (no AS analogue; no floor — 3+ at Skill 4 is right)
  - [ ] Attack kind cycles only what the element can declare (Indirect needs IF,
        rear-weapons needs REAR, physicals gate by type/MEL/jump, A2G needs aero)
  - [ ] OV commit is bounded by min(OV, heat room) and shows the damage delta
  - [ ] Target immobile: flat −4 overriding the move row
  - [ ] `Esc` — the card's To-Hit row shows `To-Hit*` and folds the context into
        every bracket (dash where the card has no damage)
- [ ] `Space` repeatedly: armor then structure; the first structure point flags
      "⚠ crit check (2D6, c)" in the status
- [ ] `o` — heat 1: the card reads "MV 4→3 (heat 1)" and, if the bracket moved,
      "TMM n (live n−1)"; the Unit header MV drops with its slowest member
- [ ] `o` to heat S → "*** SHUTDOWN (heat) ***" on the card and **CANNOT MOVE
      (shutdown)** on the Unit header; `i` back down recovers
- [ ] `c` → crit modal shows the element's own column of the p.42 table (2D6 rows):
  - [ ] Roll 4 (Fire Control) → card Crits row FC1; To-Hit +2 everywhere
  - [ ] Roll 7 (MP) → −half CURRENT MP, min 1 — twice in a row loses less the
        second time (multiplicative, never count × k)
  - [ ] Roll 6/8 (Weapon) → every damage value −1 (E recomputes off the reduced L)
  - [ ] Roll 2 (Ammo) → outcome auto-selects from the card: CASE = 1 damage,
        CASEII/ENE = ignored, CASEP = a 1D6 prompt, bare = **DESTROYED**
  - [ ] Roll 3/11 twice (Engine ×2) on a 'Mech/vehicle → DESTROYED; on aero → TP 0
        + shutdown banner instead
  - [ ] Vehicles: the three once-per-game motive effects below the table (−1 MV /
        ½ MV / immobile) — each is an independent spent-flag: picking a marked one
        again is a no-op ("once per game"), but different effects STACK (−1 then ½
        on MV 8 → 3), and marked rows show `✓ spent`
  - [ ] ARM elements: `a` marks the first-crit-chance checkbox spent
  - [ ] Infantry/BA: `c` refuses — "Infantry and BA never take critical hits (p.42)"
- [ ] `u` repairs a point; `z` undoes one step at a time
- [ ] `Space` an element to death → ***** DESTROYED *** banner; it stops counting
      toward the Unit's live MV; the Unit's static SZ does NOT change
- [ ] `L` → "Logged Turn N" (captures Unit state; `neurohelmet --export <session>`
      renders one BF sheet frame per Unit with all live marks)
- [ ] `D` → confirm → element removed (groupings remap; picker if the force empties)

## Persistence

- [ ] Quit (`q` → `y`), relaunch
  - *Expect: everything restored — armor, heat, BF crits (incl. MP loss, motive
    flags, ARM spent), morale rungs, round, Unit names, skills. The shot context
    (`t`) is deliberately ephemeral — it resets, like reading the opponent's
    sheet fresh.*

## Known non-goals (by design)

- **Damage timing:** the book defers Combat-Phase damage to the End Phase (p.49);
  neurohelmet applies it the moment it's entered, like every other mode (a deliberate scope choice).
- **Heat cooldown is manual:** the "heat → 0 in any End Phase without a weapon
  attack" wipe and the shutdown auto-restart are table procedures — `i` is the tool.
- Morale is a manual label: no checks, no recovery rolls (Appendix A stays paper).
- No board: movement costs, terrain/LOS, firing arcs, facing, artillery resolution,
  transports/mounted state — table concerns; terrain enters as to-hit toggles only.
- The crit CHANCE procedure (1D6 motive chance roll, aero TH comparison, hull-breach
  checks) stays at the table; the app applies the outcome you enter.
