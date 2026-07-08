# SBF Smoke Test

A ~5-minute manual pass that touches every Strategic BattleForce system once.
Run after any SBF change. `?` opens the key reference at any point; `z` undoes —
**every mutation should unwind in exactly one `z`** (two = bug).

## Setup

- [ ] `cargo run` → `S` → `B` → type a name → `Enter` → `Enter` (blank PV limit)
  - *Expect: picker opens; status "New Strategic BattleForce session '<name>'".*
- [ ] Add ~8 units (search, `Enter`, `Enter` in the add modal, repeat), then `Esc`
  - *Expect: the add modal shows **Skill + PV** (not BV/piloting).*
  - *Expect: SBF screen with the seeded empty "Formation 1" — "(no units — [g] to assign)" — and a "+8 ungrouped" hint. (The bare "No formations" message only appears if every formation is deleted.)*

## Grouping

- [ ] `g` — grouping editor lists all elements "— unassigned", cursor on the last-added
- [ ] `a` → pick **Clan** → `Enter`
  - *Expect: applies immediately (nothing hand-entered yet): "Binary 1" = Star 1 ×5 + Star 2 ×3. (IS → Company of Lances-of-4; ComStar → Level III of Level IIs-of-6. Aerospace elements always land in their own Flights/Squadron.)*
  - *Later, after renaming/damaging/marking: re-running `g`→`a`→`Enter` instead shows an **itemized confirmation** ("Discards 2 custom name(s), 5 armor hit(s), the COM mark — z undoes"); `n` cancels untouched, `y` applies, one `z` restores everything.*
- [ ] `g` again, on one element exercise every verb:
  - [ ] `f` → it moves to a fresh "Formation 2"
  - [ ] `→` → back into an existing Star; **the emptied Formation 2 stays as "(no units)"** — never rendered as destroyed — and `←` can move an element back into it (empty formations are grouping stops)
  - [ ] `n` → splits into a new unit of its formation
  - [ ] `u` → unassigned ("+1 ungrouped — [g] to assign" hint behind the modal)
  - [ ] `→` → re-assigned
  - [ ] `s` / `S` → Skill worsens/improves; **note:** unit PV only moves when the unit's
        *rounded average* skill moves (SBF averages element skills — one bump in a 4-element
        lance rounds away; try it on a 1-element unit to see the ×0.9/×1.2 scaling)
  - [ ] `x` → removed from the force entirely (pool count drops; indices renumber)
  - [ ] `Esc` closes — empty **units** come off the sheet; empty **formations** stay until `D`
- [ ] `r` rename the formation; `R` rename the active unit
- [ ] `C` on unit 1 → **COM** tag on the unit *and* the formation header
- [ ] `j` then `l` on unit 2 → **LEAD** tag
  - *Expect: detail pane shows "defender +2 Tactics (COM/LEAD)".*
  - *Expect: `C`/`l` again toggles off; marking a second unit moves the tag (never two COM).*

## Combat loop

- [ ] `n` → "Round 1 begun"
- [ ] `t` → to-hit editor (10 rows, printed p.172 table):
  - [ ] Range → Long: number = formation Skill + 2
  - [ ] Target TMM 2: +2 more
  - [ ] Extreme range is legal (+3), never "Impossible"
  - [ ] Formation JUMP row mutates the persisted jump count (+1 each)
  - [ ] Withholding units subtracts, floored at −2 total
  - [ ] `Esc` — the detail pane keeps showing the entered shot
- [ ] `Space` repeatedly: armor bar drains; **below half** the status says "⚠ crit check (2d6, c)" and the unit gets a "⚠crit due" tag
- [ ] `c` → crit modal states "below half armor — roll 2d6" + the single crit table
  - [ ] Mark a Damage crit → the unit's damage bands (incl. E = L−1) drop by 1
  - [ ] Mark a Targeting crit → the to-hit number rises by 1
  - [ ] Mark an MP crit → MV drops (can reach 0 = immobile; no floor of 1)
- [ ] `u` repairs a point; `z` undoes one step at a time
- [ ] `Space` a unit to death → **✖ DESTROYED** + spillover status ("spill remaining damage onto another unit"); `j` to the next unit and keep marking
- [ ] `m` cycles morale Normal → Shaken → Broken → Routed → Normal
  - *Expect: Routed (non-infantry) shows "⚠withdraw" on the formation; the rung never changes on its own (crits/destruction do NOT touch it).*
- [ ] `e` → formation marked done ✓; `n` → new round: ✓ clears, jump resets, **morale/armor/crits persist**
- [ ] `L` → "Logged Turn N" (captures formation state; `neurohelmet --export <session>` renders one formation sheet per formation with all live marks)
- [ ] Destroy every unit → formation shows the eliminated glyph but stays listed
- [ ] `D` → confirm → formation and its elements removed (picker if the force is now empty)

## Persistence

- [ ] Quit (`q` → `y`), relaunch
  - *Expect: everything restored — armor, crits, morale rung, COM/LEAD, round, names, skills.*

## Known non-goals (don't file these)

- Morale is a manual label: no checks, no recovery rolls, no auto-withdrawal, and it never feeds the to-hit.
- The Step-5b Tactics roll, Concentration of Fire (max 2 hits/unit/exchange), and terrain
  validity are table concerns — the tracker records outcomes, it doesn't referee them.
