# Security Policy

## Reporting a vulnerability

Please report security issues privately to **<neurohelmet@natedpalm.com>** rather than opening a
public issue, so a fix can ship before the details are public.

Helpful things to include: the Neurohelmet version (`neurohelmet --version`), your OS, and the
smallest steps or input file that reproduce it.

Neurohelmet is a hobby project maintained by one person, so responses are best-effort — expect an
acknowledgement within about a week. Fixes land in the next release; there is no separate backport
process. Only the **latest release** is supported.

## Scope

Neurohelmet is an offline terminal application. It has no server, no accounts, no authentication, and
it makes **no network requests at runtime** — the unit catalog is compiled into the binary. That rules
out most of the usual categories.

What is realistically in scope:

- **Malformed input causing memory-unsafety, a crash, or unbounded resource use** — a hand-edited or
  corrupted session file, a `NEUROHELMET_DATA` bundle, or a game-log file.
- **Path handling** around the data directory and `NEUROHELMET_DIR`, `--export`, and `--pdf` output
  (for example, a session name that escapes its intended directory).
- **The publishing flow** (`--publish`), which is the one feature that shells out to `git` and the
  GitHub CLI and writes into a repository you configure.
- **Anything in the release pipeline** that would let a third party alter the published binaries.

Out of scope: the `neurohelmet-bake` developer tool fetching data over the network (it runs on a
maintainer's machine, not a player's), and the content of the BattleTech data itself.

A plain crash or panic with no security impact isn't a vulnerability — please file those as a normal
[issue](https://github.com/tympaniplayer/neurohelmet/issues), they're very welcome.
