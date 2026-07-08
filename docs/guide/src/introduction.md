# Neurohelmet

**Neurohelmet** is a keyboard-driven [ratatui](https://ratatui.rs) BattleTech tracker — a terminal
"paper doll" for running a game from a single screen. It's sized for a 7" Raspberry Pi display
(~100×30 cells) but is at home in any terminal, and it is **fully offline**: the entire unit catalog
is baked into the binary, so the app never touches the network at runtime.

![The Classic tracker: an Atlas's armor/structure paper doll with heat, pilot, movement, and weapons panels.](images/classic-tracker.png)

Neurohelmet tracks **six BattleTech game systems**, each a first-class mode with live state — damage,
heat, ammo, criticals, pilots, morale, fatigue. You roll the dice and mark the results; the app keeps
the sheet and surfaces the consequences.

## Philosophy: manual first

Neurohelmet is a *tracker*, not a rules engine that plays the game for you. Manual control is the primary
flow — you stay in charge of every roll and result. Where the app helps, it helps deliberately: it
does the bookkeeping (armor cascade, heat, crit consequences, to-hit modifiers) and offers reference
tables, but it never rolls for you or hides a decision. Any automation is opt-in and warns before it
would discard something you entered by hand.

## What's here

- **[Install & first run](getting-started.md)** — get it building and create your first session.
- **[Game systems](modes/overview.md)** — the six modes, what each tracks, and how to start one.
- **Guides** — [sessions](guides/sessions.md), the [game log & publishing](guides/game-log.md),
  [force generation](guides/force-generation.md), and [display options](guides/display.md).
- **Reference** — [keybindings](reference/keybindings.md), [configuration](reference/configuration.md),
  and [license & attribution](reference/attribution.md).

## Unofficial & non-commercial

Neurohelmet is an unofficial, non-commercial, fan-made tool, not affiliated with or endorsed by
Microsoft, Topps, Catalyst Game Labs, Death From Above Wargaming, or the MegaMek project. See
[License & attribution](reference/attribution.md) for the full picture.
