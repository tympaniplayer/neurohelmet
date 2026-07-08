# Sessions & autosave

Everything in Neurohelmet happens inside a **session** — a named, saved game locked to one
[system](../modes/overview.md). You can keep as many as you like side by side.

## The Sessions browser

Press **`S`** from any mode to open the browser. From there:

| Key | Action |
|-----|--------|
| `↑ ↓` | select a session |
| `Enter` | load the selected session |
| `n` / `A` / `O` / `B` / `F` / `C` | new session (Classic / AS / Override / SBF / BF / ACS) |
| `r` | rename the selected session |
| `D` | delete the selected session |
| `Esc` | back |

## Autosave & undo

- **Autosave** — the session is written to disk after every change; there's no "save" key to remember.
- **Undo** — `z` steps back up to **50** changes deep.
- **Last active** — the session you were in is reopened automatically next launch.

## On disk

Sessions are individual files under the data directory (see
[Configuration](../reference/configuration.md)). Because each is a plain file, backing up or moving a
session is just copying it. Set `NEUROHELMET_DIR` to relocate all sessions, logs, and config together.
