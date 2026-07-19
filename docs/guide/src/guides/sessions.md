# Sessions & autosave

Everything in Neurohelmet happens inside a **session** — a named, saved game locked to one
[game system](../modes/overview.md). A Classic session tracks record sheets, an Alpha Strike
session tracks cards, and so on — the roster shape is different in each system, so a session
never changes modes. You can keep as many sessions as you like side by side: tonight's Classic
duel, the ongoing SBF campaign, a scratch Override roster.

On first launch Neurohelmet creates an empty default Classic session and drops you straight
into the unit picker — see [Your first session](../first-session.md). Press **`Esc`** from that
empty picker (or **`S`** from any play screen, any time) to reach the Sessions browser.

## The Sessions browser

![The Sessions browser listing six saved sessions spanning all six game systems, each row showing a mode tag, unit count, chassis summary, and force total, with the create/rename/delete keys in the footer.](../images/sessions-browser.png)

The header shows which session is currently active. Each row gives you a session at a glance:

- a **`●`** dot marks the active session, `▶` the selected one;
- a two-letter **mode tag** — `CL` Classic, `AS` Alpha Strike, `OV` Override, `SB` Strategic
  BattleForce, `BF` BattleForce, `AC` ACS;
- the name, unit count, a short chassis summary (up to three chassis, then `+N`), and the
  force's BV or PV total — with the limit, if one is set.

Sessions are sorted by name, case-insensitively. An empty list says so and points you at
**`n`** to start one.

### Browser keys

| Key | Action |
|-----|--------|
| `↑ ↓` / `k j` | select a session |
| `Enter` | load the selected session |
| `r` | rename (the input comes pre-filled with the old name) |
| `D` | delete, with a y/n confirm |
| `Esc` / `q` | back to where you were |

Note that `q` here means **back**, not quit — the browser is a detour, not an exit. Loading
persists whatever you were doing first, so switching sessions never loses work; loading the
session you're already in just reports "Already active". Deleting is confirmed, and the
**active session can't be deleted** at all — switch away first.

### Creating a session

One key per system, each prompting you to type a name:

| Key | New session |
|-----|-------------|
| `n` | Classic |
| `A` | Alpha Strike |
| `O` | Override |
| `B` | Strategic BattleForce |
| `F` | BattleForce |
| `C` | Abstract Combat System |

After the name, Neurohelmet chains straight into the force-limit prompt (BV for
Classic/Override, PV for the rest — leave it blank for no limit) and then opens the unit picker
so you can start [building a force](force-generation.md). Names are sanitized to
filesystem-safe characters (letters, digits, spaces, `-`, `_`); anything else becomes `_`.

## Autosave

There is no save key, because there is nothing to save manually: **every change is written to
disk within a fraction of a second** of you making it. Add a unit, mark damage, bump heat —
it's already on disk. Writes are atomic (written to a temp file, then renamed into place), so a
crash or power cut mid-write can't corrupt a session. Quitting — whether via **`q`** and its
confirm prompt or an immediate **`Ctrl+C`** — never loses anything, and the session you were in
reopens automatically on the next launch.

## Undo

**`z`** steps backward, up to **50** changes deep, in all six modes. Undo covers anything that
changes the session — damage, heat, crits, roster edits, skills. It does *not* cover things
that aren't session state: cursor position, panel focus, theme and layout, or the picker's
search query. An empty stack reports "Nothing to undo".

The undo stack is **cleared** when you create or load a session — undo never crosses session
boundaries. It also can't un-append a [game log](game-log.md) snapshot: undoing after **`L`**
rolls back the turn counter but not the logged line.

## On disk

Sessions are plain, pretty-printed JSON files in the data directory (per-OS locations in
[Configuration](../reference/configuration.md)):

| Path | What |
|------|------|
| `sessions/<name>.json` | one file per session |
| `sessions/<name>.log.jsonl` | that session's [game log](game-log.md) — only exists once you've pressed `L` |
| `current` | plain-text pointer naming the active session |
| `config.json` | display and publishing [configuration](../reference/configuration.md) |

The `--export` and `--pdf` [command-line verbs](../reference/cli.md) also write their default
output here, as `sessions/<name>-log/` and `sessions/<name>-sheets.pdf`.

### Backing up and moving

Because a session is one file, backing it up is copying it — grab the `.log.jsonl` sibling too
if you want its game log. Copying a session file into another machine's data directory just
works. To relocate *everything* — sessions, logs, config — set **`NEUROHELMET_DIR`** to a
directory of your choice; it's also handy for keeping a throwaway sandbox separate from your
real games.

### Sessions survive data updates

Each unit in a session references its record-sheet spec in the baked
[data bundle](../reference/data.md). When a session loads under a newer bundle, those specs are
re-linked automatically and the update is reported ("Updated N mech spec(s) to the latest
data") — your damage, heat, and crits carry over. Old sessions keep working after an upgrade or
a [re-bake](../reference/data.md).

### Upgrading from older versions

Installs from before the Neurohelmet rename keep working: a legacy `mechdoll` data directory is
still found automatically, and a pre-sessions single `session.json` is migrated to
`sessions/default.json` the first time the TUI starts. Both are automatic and silent.
