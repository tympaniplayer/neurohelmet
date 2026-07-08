# Configuration

Neurohelmet's settings live in a small JSON file at **`<data-dir>/config.json`**, where `<data-dir>` is:

| OS | Default location |
|----|------------------|
| Linux | `~/.local/share/neurohelmet/config.json` |
| macOS | `~/Library/Application Support/neurohelmet/config.json` |
| Windows | `%APPDATA%\neurohelmet\config.json` |

Setting the **`NEUROHELMET_DIR`** environment variable relocates the whole data directory — config,
sessions, and logs — together. Edit the file by hand; a missing or partial file is fine (every key is
optional, and unknown keys are ignored).

## Keys

| Key | Type | Default | Meaning |
|-----|------|---------|---------|
| `log_repo` | string | *(unset)* | Local git working copy to [publish](../guides/game-log.md) into. Unset = use the `gh`-managed flow. A leading `~/` expands to your home dir. |
| `log_auto_push` | bool | `true` | Whether `--publish` pushes after committing. `false` = commit and stop; push it yourself. |
| `theme` | string | terminal-auto | Color [theme](../guides/display.md) (also set live via `Ctrl-T`). |
| `profile` | string | `pi` | Layout profile (`pi` / `modern`). |
| `icons` | string | `ascii` | Icon set (`ascii` / `nerd`). |

## Environment overrides

The display settings can also be set per-launch via environment variables, which win over the saved
config for that run:

| Variable | Overrides |
|----------|-----------|
| `NEUROHELMET_THEME` | `theme` |
| `NEUROHELMET_PROFILE` | `profile` |
| `NEUROHELMET_ICONS` | `icons` |
| `NEUROHELMET_DIR` | the entire data directory (config + sessions + logs) |

Resolution order for the display settings is **environment variable → saved config → built-in
default**. The logging keys (`log_repo`, `log_auto_push`) are config-only.

## Example

```json
{
  "theme": "mocha",
  "profile": "modern",
  "log_repo": "~/battle-logs",
  "log_auto_push": false
}
```
