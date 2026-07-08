# Keybindings

Keybindings differ per system, so the authoritative reference is **always in the app**: press **`?`**
in any mode for that mode's exact keymap. There's also a one-page
[cheat sheet PDF](https://github.com/tympaniplayer/neurohelmet/blob/main/docs/neurohelmet-keybindings.pdf)
in the repository covering every mode.

## Keys shared across modes

These behave the same everywhere:

| Key | Action |
|-----|--------|
| `?` | help — the current mode's keymap |
| `a` / `D` | add / delete a unit |
| `,` / `.` (or `[` / `]`) | previous / next unit |
| `g` | edit skills (gunnery / piloting, etc.) — re-costs the unit |
| `L` | [log snapshot](../guides/game-log.md) |
| `S` | [Sessions browser](../guides/sessions.md) |
| `z` | undo (50 deep) |
| `Ctrl-T` | [display picker](../guides/display.md) (theme / layout / icons) |
| `q` | quit (asks first) |

## Per-mode keys

Because each system tracks different state, the action keys — damaging a location, firing a weapon
group, adjusting heat, ending a turn — vary by mode. Rather than duplicate them here (where they can
drift out of date), open the mode and press **`?`**; the help is generated from the same bindings the
app actually uses.
