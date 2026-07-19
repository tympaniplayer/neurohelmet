# Troubleshooting

Quick answers to the problems people actually hit. Each one links to the page with the full
story. If your problem isn't here, the [command-line reference](cli.md) and
[Configuration](configuration.md) pages cover most of the machinery.

## The screen looks cramped, or panels are missing

Neurohelmet's layout is tuned for a terminal of at least **~100×30 cells**. Below that, panels
truncate — nothing crashes, but it isn't pretty. Check what the app actually sees:

```sh
neurohelmet --term-size
```

That prints `crossterm detects: N cols x N rows`. If the numbers are smaller than you expect,
enlarge the window or shrink the font. Two layout rules worth knowing:

- The **Modern layout's Force sidebar** needs at least **96 columns** (and a non-empty roster);
  below that it silently falls back to the single-pane layout. That's by design, not a bug.
- SBF and ACS never show the sidebar at any width — their panes already list the force.

See [Themes & layout](../guides/display.md) for the layout profiles.

## I see empty boxes or garbage glyphs

That's **tofu** — your font doesn't have the glyphs the **Nerd Font** icon set uses. The app
can't detect whether a Nerd Font is installed, so icons are strictly opt-in. Either:

- Switch icons back to **Text**: press **`Ctrl+T`**, arrow down to the Icons row, toggle with
  **`←`**/**`→`**, and press **`Enter`** to save. Text mode uses only characters every
  monospace font carries.
- Or install an actual Nerd Font (MesloLGS NF, JetBrainsMono Nerd Font, Iosevka Nerd Font) and
  keep the richer glyphs.

For one launch you can also force it with `NEUROHELMET_ICONS=ascii`. If even the box-drawing
lines (`┌─┐`) look broken, the problem is the terminal font itself — pick any standard
monospace font. Details in [Themes & layout](../guides/display.md).

## The colors look flat or washed out

With no saved theme, Neurohelmet picks a default from your terminal: if the `COLORTERM`
environment variable is `truecolor` or `24bit` you get the 24-bit **truecolor** theme,
otherwise the 16-color **pi** theme. Some terminals support 24-bit color but don't set
`COLORTERM`, so you land on the muted 16-color palette.

Fix it by choosing a theme explicitly: **`Ctrl+T`** opens the display picker on any screen,
arrowing over a theme live-previews it, **`Enter`** saves. Note that `pi` and `truecolor`
deliberately keep your terminal's own background; the other themes paint their own. The full
gallery is in [Themes & layout](../guides/display.md).

## I saved a theme but it doesn't stick between launches

Check your environment: `NEUROHELMET_THEME`, `NEUROHELMET_PROFILE`, and `NEUROHELMET_ICONS`
each beat the saved config **every launch** while set. The `Ctrl+T` picker still saves your
choice — the env var just shadows it. Precedence is env var → saved config → built-in default;
see [Configuration](configuration.md).

## macOS says the app "can't be opened"

The binary is unsigned, and a zip downloaded in a browser gets quarantined by Gatekeeper.
Clear it once:

```sh
xattr -dr com.apple.quarantine neurohelmet
```

or right-click the binary and choose Open, once. Installs via Homebrew, Scoop, or apt are not
quarantined. See [Installation](../install.md).

## My Raspberry Pi build dies at the link step

On a 2 GB Pi the linker can get killed by the out-of-memory reaper. Build with fewer parallel
jobs — `cargo build --release -j 2` — or add swap. A 4 GB Pi 4/5 builds comfortably. The
no-compile alternative is the apt repo (arm64 `.deb`). Full walkthrough in
[Running on a Raspberry Pi](../raspberry-pi.md).

## Where did my session go?

There is no save key — Neurohelmet autosaves after every change, and reopens the last active
session on launch. If a session seems to have vanished, it's almost always a data-directory
question. Sessions live at `<data-dir>/sessions/<name>.json`:

| OS | Default data dir |
|----|------------------|
| Linux | `~/.local/share/neurohelmet/` |
| macOS | `~/Library/Application Support/neurohelmet/` |
| Windows | `%APPDATA%\neurohelmet\` |

Things that change where the app looks:

- **`NEUROHELMET_DIR`** overrides the whole data directory. If it was set for one launch and
  not the next, your sessions are in the other location — nothing is lost.
- Installs from before the rename may still be using a legacy `mechdoll` data directory; the
  app keeps using it until a `neurohelmet` directory exists.

Press **`S`** on any play screen to open the Sessions browser and see everything the app can
find. More in [Sessions & autosave](../guides/sessions.md).

## `--export`, `--publish`, or `--pdf` can't find a session I know exists

Two common causes:

- **A very old install that never ran the new TUI.** The one-time migration of the legacy
  single-session file runs only on plain TUI startup, not before the CLI verbs. Launch
  `neurohelmet` once, quit, and the verbs will find it.
- **`--publish` with an empty log**: publishing needs snapshots — `no game log for '<name>' —
  press L in-game to capture turns first`. See [Game log & publishing](../guides/game-log.md).

## Publishing fails

The default flow drives the GitHub CLI. If you see a hint about `gh` not being authenticated,
run `gh auth login` first. If you've set `log_repo` in the config to publish into your own
repo, that path must already be a git working copy — clone it or `git init` it; the app won't
create one for you. `neurohelmet --publish <session> --dry-run` stages everything locally with
no git or network work, which is the easiest way to see what would be published. Details in
[Game log & publishing](../guides/game-log.md).

## The picker can't find a unit I know is in the catalog

- **Check the title bar for `N of 9724` and a `filters:` summary** — active **`Ctrl+F`**
  filters narrow the list, and it's easy to forget one is set. Open the filter editor and
  press **`c`** to clear every facet.
- **Try another name.** Search is fuzzy and case-insensitive, but the catalog uses combined
  names for units with Clan and Inner Sphere designations — the Mad Cat is listed as
  `Mad Cat (Timber Wolf)`, so either half matches.
- A handful of Alpha Strike-only entries (gun emplacements, Battlefield Support cards) are
  refused in Classic and Override sessions with a status message — add them to an AS session
  instead.

More on searching and filtering in [Building a force](../guides/force-generation.md).

## Why is my roster full at 12 units?

The 12-unit cap applies to **Classic and Override** sessions only — Alpha Strike, BattleForce,
Strategic BattleForce, and ACS rosters are uncapped. If you need a bigger Classic force, split
it across sessions.

## Does the mouse work? Where's `--help`?

Two honest limitations:

- Neurohelmet is **keyboard only** — there is no mouse support. Press **`?`** on any play
  screen for that mode's key reference; the unit picker and Sessions browser show their keys
  in a footer hint line instead.
- There is currently no `--help` or `--version` flag; unknown flags are silently ignored and
  the TUI starts. The verb list lives in the [command-line reference](cli.md).

## I want to start completely fresh

Quit the app, then move (or delete) the data directory listed above — sessions, logs, config,
and the current-session pointer all live inside it. Moving it aside instead of deleting means
you can always put it back. To experiment without touching your real data, point
`NEUROHELMET_DIR` at an empty directory for that launch.
