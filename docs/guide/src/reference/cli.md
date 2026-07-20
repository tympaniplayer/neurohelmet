# Command-line reference

Neurohelmet is one binary. Run it with no arguments and you get the TUI; a handful of `--verbs`
run headless jobs instead — diagnostics, game-log export, publishing, and PDF record sheets. Only
one verb runs per invocation, and there is no `--help` — an unrecognized flag is ignored and the TUI
starts.

| Invocation | What it does |
|------------|--------------|
| `neurohelmet` | Start the TUI. |
| `neurohelmet --version` | Print the version and exit. |
| `neurohelmet --selftest` | Headless smoke test — no terminal needed. |
| `neurohelmet --term-size` | Print the terminal size the app detects. |
| `neurohelmet --export <session> [outdir]` | Render the session's game log to image files, offline. |
| `neurohelmet --publish <session> [--dry-run]` | Publish the game log as a browsable web gallery. |
| `neurohelmet --pdf <session> [outfile]` | Export blank, print-ready record sheets (BF/SBF/ACS). |

**Exit behavior**: a verb missing its required `<session>` argument prints a `usage:` line to
stderr and exits with code **2**. So does `--pdf` when the named session doesn't exist
(`No saved session '<name>'.`).

## `neurohelmet` — the TUI

With no arguments, Neurohelmet loads the baked unit bundle, migrates any pre-rename legacy
session, reads the current-session pointer (falling back to `default`), and drops you into the
app — a fresh install lands in the unit picker of an empty default Classic session. See
[Your first session](../first-session.md).

One caveat worth knowing: the legacy-session migration (old single `session.json` →
`sessions/default.json`) runs **only on TUI startup**. If you upgraded from a very old install and
go straight to `--export`, `--publish`, or `--pdf` without ever launching the TUI, the verb won't find your old
session under its new name — launch the app once first.

## `--version`

```
neurohelmet --version      # -V works too
```

Prints `neurohelmet <version>` and exits — checked before anything that needs a terminal, so it
works over SSH, in a pipe, or anywhere without a TTY. Worth including in a
[bug report](https://github.com/tympaniplayer/neurohelmet/issues).

## `--selftest`

```
neurohelmet --selftest
```

A headless smoke test that needs no terminal: it loads the bundle, prints `loaded 9724 mechs`,
builds a demo session around an Atlas AS7-D, applies scripted damage and heat, fires the AC/20
three times to exercise weapon→ammo auto-linking, and renders one 100×30 frame to stdout. If this
runs, your install works. Handy after a [build from source](../install.md) or a
[re-bake](data.md).

## `--term-size`

```
neurohelmet --term-size
```

Prints what the terminal backend detects: `crossterm detects: {cols} cols x {rows} rows` (or an
error if detection fails). A sizing diagnostic — Neurohelmet wants roughly **100×30** or better,
and this tells you what you actually have. Useful when tuning a
[Raspberry Pi display](../raspberry-pi.md) or debugging a cramped layout (see
[Troubleshooting](troubleshooting.md)).

## `--export <session> [outdir]`

```
neurohelmet --export tukayyid
neurohelmet --export tukayyid ~/battle-reports/tukayyid
```

Renders every turn of a session's [game log](../guides/game-log.md) to image files, fully offline
— no terminal, no network, and no re-linking against the bundle (log entries embed their own unit
specs). Frames are drawn by the real tracker renderer, so they look exactly like the live screen.

- **Output** goes to `outdir` if given, else `<data-dir>/sessions/<name>-log/`.
- Per turn you get a `turn-NN/` directory with one **PPM** image per frame — per mech, or per
  formation (SBF) / per Unit (BF) — plus a stacked montage `turn-NN.ppm` at the top level.
- A `transcript.txt` holds a plain-text render of every frame.
- Frames re-render in the mode each snapshot was taken in: an Override snapshot exports the
  Override card, an Alpha Strike snapshot the AS card, and so on.
- An empty log is not an error: it warns `No game log for session '<name>' (nothing to export).`
  and exits 0.

Exported images always use the Pi theme and Pi layout, regardless of your saved
[display settings](../guides/display.md) — exports are deterministic, not themed.

Note the format split: `--export` writes PPM (a dead-simple uncompressed format); PNG is what
`--publish` produces. If you want PNGs locally without any git involvement, use
`--publish <session> --dry-run` instead.

## `--publish <session> [--dry-run]`

```
neurohelmet --publish tukayyid
neurohelmet --publish tukayyid --dry-run
```

Publishes the session's game log as **PNG** frames plus a markdown gallery — a browsable
turn-by-turn battle report. Unlike `--export`, an empty log is a hard error:
`no game log for '<name>' — press L in-game to capture turns first`.

Two flows, chosen by your [configuration](configuration.md):

- **Default (`gh`-managed)** — needs the GitHub CLI, authenticated (`gh auth login`). Creates a
  public `neurohelmet-logs` repo under your account on first use, keeps a working clone at
  `<data-dir>/logs-repo/`, writes `games/<name>/`, commits, pushes, enables GitHub Pages
  (best-effort), and prints the gallery URL:
  `Published https://<you>.github.io/neurohelmet-logs/games/<name>/`.
- **Bring your own repo** — set `log_repo` in `config.json` to a local git working copy and
  publishing commits there instead, no `gh` required. The path must already be a git repository.
  Publish only ever stages the paths it owns (`games/` and `README.md`), so a shared repo's other
  files are never touched.

`--dry-run` renders the same gallery into `<data-dir>/logs-staging/games/<name>/` and does no git
or network work at all — good for a preview, or as a local PNG export.

With `log_auto_push` set to `false` in config, either flow commits locally and stops, telling you
to push when you're ready. The full walkthrough lives in
[Game log & publishing](../guides/game-log.md).

## `--pdf <session> [outfile]`

```
neurohelmet --pdf tukayyid
neurohelmet --pdf tukayyid ~/print/tukayyid.pdf
```

Renders a session's record sheets to one multi-page, US-Letter **PDF** — always as pristine blank
fill-in forms (live damage, heat, crits, and morale are stripped; the printout exists to take a
clean sheet to the table). One page per formation (SBF), per Unit (BF), or per Combat Unit plus a
Formation Tracking sheet (ACS).

- **Only BattleForce, Strategic BattleForce, and ACS sessions** are supported; other modes error
  with `PDF export supports BattleForce, Strategic BattleForce, and ACS sessions only`.
- The optional second argument is an out**file**, not a directory. Default:
  `<data-dir>/sessions/<name>-sheets.pdf`. Parent directories of an explicit outfile are created
  for you.
- On success: `Wrote record sheet for '<name>' → <path>`.

The same export is one keypress in-app: **`P`** on the BF, SBF, and ACS screens. Details, sheet
anatomy, and a sample PDF are in [PDF record sheets](../guides/pdf-record-sheets.md).

## Environment variables

These affect any invocation — TUI or verb. Full semantics in
[Configuration](configuration.md); the data pipeline ones are covered in
[Data & re-baking](data.md).

| Variable | Effect |
|----------|--------|
| `NEUROHELMET_DIR` | Relocate the whole data directory — sessions, logs, `config.json`, publish clones. (`MECHDOLL_DIR` is the legacy pre-rename alias, still honored.) |
| `NEUROHELMET_DATA` | Load a bundle file from disk instead of the embedded dataset — for testing a fresh [bake](data.md) without rebuilding. |
| `NEUROHELMET_THEME` | Theme for this launch; beats saved config. |
| `NEUROHELMET_PROFILE` | Layout profile (`pi` / `modern`) for this launch; beats saved config. |
| `NEUROHELMET_ICONS` | Icon set (`ascii` / `nerd`) for this launch; beats saved config. |

For the display settings the resolution order is **environment variable → saved config →
built-in default**.
