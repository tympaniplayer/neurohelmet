# Configuration

Neurohelmet keeps everything it persists — settings, sessions, game logs — in one **data
directory**, and its settings in a single small JSON file there: **`<data-dir>/config.json`**.
Every key is optional. A missing, partial, or even corrupt file simply loads as defaults and never
blocks startup, and writes are atomic (write-to-temp, then rename), so a crash can't leave you with
half a config.

You rarely need to touch the file directly: the **`Ctrl+T`** display picker saves the three display
settings for you (see [Themes & layout](../guides/display.md)). The two logging keys are the only
ones you edit by hand.

## The data directory

By default the data directory is `neurohelmet` under your platform's standard data location:

| OS | Default location |
|----|------------------|
| Linux | `~/.local/share/neurohelmet/` (respects `$XDG_DATA_HOME`) |
| macOS | `~/Library/Application Support/neurohelmet/` |
| Windows | `%APPDATA%\neurohelmet\` |

Setting the **`NEUROHELMET_DIR`** environment variable relocates the whole directory — config,
sessions, logs, and publish clones move together. Two legacy fallbacks exist from the app's
pre-rename days: the old **`MECHDOLL_DIR`** variable is still honored (checked after
`NEUROHELMET_DIR`), and if no `neurohelmet` data directory exists yet but an old `mechdoll` one
does, that old directory keeps being used so nothing gets lost. Once a `neurohelmet` directory
exists, it always wins.

### What lives inside

| Path | What it is |
|------|------------|
| `config.json` | Your settings (this page). |
| `current` | Plain-text pointer: the name of the active session. |
| `sessions/<name>.json` | One saved [session](../guides/sessions.md) per file. |
| `sessions/<name>.log.jsonl` | That session's [game log](../guides/game-log.md) — created on the first **`L`** snapshot, never before. |
| `sessions/<name>-log/` | Default output of `--export` (PPM frames + transcript). |
| `sessions/<name>-sheets.pdf` | Default output of `--pdf` ([record sheets](../guides/pdf-record-sheets.md)). |
| `logs-repo/` | The `gh`-managed clone of your `neurohelmet-logs` publish repo. |
| `logs-staging/` | Output of `--publish --dry-run`. |

Session filenames are sanitized (letters, digits, spaces, `-`, `_` are kept; anything else becomes
`_`), so a session name can never write outside the sessions directory. To back up or move your
setup, copy the whole data directory — that's all of it.

## config.json keys

These five keys are the entire config surface:

| Key | Type | Default | Meaning |
|-----|------|---------|---------|
| `theme` | string | terminal-auto | Color [theme](../guides/display.md). Auto means: truecolor theme if `COLORTERM` reports `truecolor`/`24bit`, else `pi`. |
| `profile` | string | `pi` | Layout profile: `pi` (compact) or `modern` (roomy, with the Force sidebar). |
| `icons` | string | `ascii` | Icon set: `ascii` (plain text) or `nerd` (Nerd Font glyphs). |
| `log_repo` | string | *(unset)* | Local git working copy to [publish](../guides/game-log.md) game logs into. Unset = the `gh`-managed flow. A leading `~/` expands to your home dir; an empty string means unset. |
| `log_auto_push` | bool | `true` | Whether publishing pushes after committing. `false` = commit locally and stop; push when you're ready. |

Names for `theme`, `profile`, and `icons` are case-insensitive and accept aliases (`mocha` is also
`catppuccin`, `modern` is also `laptop` or `wide`, `nerd` is also `nerdfont` — the full alias list
is in [Themes & layout](../guides/display.md)). An unrecognized value isn't an error; it just falls
through to the default.

Two different writers touch this file, and they don't step on each other:

- **The `Ctrl+T` picker** saves `theme`, `profile`, and `icons` when you press **`Enter`**. It
  reloads the file first, so hand-edited keys like `log_repo` survive.
- **You**, by hand, for `log_repo` and `log_auto_push` — no in-app UI sets them, deliberately.

## Environment variables

| Variable | Effect |
|----------|--------|
| `NEUROHELMET_DIR` | Relocates the entire data directory (config + sessions + logs). |
| `MECHDOLL_DIR` | Legacy alias for `NEUROHELMET_DIR`; still honored, checked second. |
| `NEUROHELMET_DATA` | Path to a data bundle file to load instead of the embedded catalog — a developer override for testing a fresh [bake](data.md) without rebuilding the app. |
| `NEUROHELMET_THEME` | Theme for this launch. |
| `NEUROHELMET_PROFILE` | Layout profile (`pi`/`modern`) for this launch. |
| `NEUROHELMET_ICONS` | Icon set (`ascii`/`nerd`) for this launch. |
| `COLORTERM` | Read, not set, by Neurohelmet: your terminal sets it, and the auto theme choice keys off it. |

For the three display settings, resolution order is **environment variable → saved config →
built-in default**. An env override wins for that launch only — you can still change and save
settings with `Ctrl+T` during the session, but the next launch under the same env var is shadowed
again.

The logging keys are **config-only**: `log_repo` has no env override on purpose — it's a set-once,
hand-edited path, not something to vary per launch.

One quirk worth knowing: the headless verbs (`--selftest`, `--export`, `--publish`, `--pdf`) never
load the display settings — exported images always render in the default pi/ascii look, whatever
your config says (and [record sheets](../guides/pdf-record-sheets.md) are a fixed print layout that
themes never touch). See the [command-line reference](cli.md).

## Example

```json
{
  "theme": "mocha",
  "profile": "modern",
  "icons": "nerd",
  "log_repo": "~/battle-logs",
  "log_auto_push": false
}
```
