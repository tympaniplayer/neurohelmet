// Neurohelmet — Copyright (C) 2026 Nate Palmer
//
// This file is part of Neurohelmet.
//
// Neurohelmet is free software: you can redistribute it and/or modify it under
// the terms of the GNU General Public License as published by the Free Software
// Foundation, either version 3 of the License, or (at your option) any later
// version.
//
// Neurohelmet is distributed in the hope that it will be useful, but WITHOUT ANY
// WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR
// A PARTICULAR PURPOSE. See the GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License along with
// Neurohelmet. If not, see <https://www.gnu.org/licenses/>.

//! Publish a session's game log to a public GitHub repo (`neurohelmet-logs`) as PNG frames + a
//! markdown gallery (rendered by GitHub Pages). Decoupled from play — invoked offline via
//! `neurohelmet --publish <session>`. Auth rides on the existing `gh` login.

use crate::export::{montage, render_turn, SCALE};
use crate::render::rasterize;
use crate::tui::config::Config;
use color_eyre::eyre::{bail, Result, WrapErr};
use neurohelmet_core::log::{self, LogEntry};
use neurohelmet_core::session::{neurohelmet_dir, sanitize_name};
use std::path::Path;
use std::process::Command;

const REPO: &str = "neurohelmet-logs";

/// Paths (relative to the repo root) that publish owns and is allowed to stage — never `git add -A`,
/// so a `log_repo` pointed at a directory shared with other files leaves those files untouched.
const OWNED_PATHS: &[&str] = &["games", "README.md"];

/// Publish `name`'s log. With `dry_run`, render the gallery into a local staging dir and skip all
/// git/network work. Otherwise the destination depends on config ([`Config::resolved_log_repo`]): a
/// user-managed repo when set, else the built-in `gh`-managed `neurohelmet-logs` repo + GitHub Pages.
pub fn run(name: &str, dry_run: bool) -> Result<()> {
    let entries = log::read_log(name)?;
    if entries.is_empty() {
        bail!("no game log for '{name}' — press L in-game to capture turns first");
    }
    let game = sanitize_name(name);

    if dry_run {
        let root = neurohelmet_dir().join("logs-staging");
        let game_dir = root.join("games").join(&game);
        write_game(name, &entries, &game_dir)?;
        write_root_index(&root)?;
        println!(
            "Dry run: wrote {} turn(s) to {}",
            entries.len(),
            game_dir.display()
        );
        return Ok(());
    }

    let cfg = Config::load();
    let auto_push = cfg.resolved_auto_push();
    let n = entries.len();
    let msg = format!("{name}: {n} turn(s)");

    // Bring-your-own-repo: publish into a user-managed git working copy and skip all `gh` plumbing
    // (no repo-create, no clone, no Pages) — the user owns the remote and its hosting.
    if let Some(root) = cfg.resolved_log_repo() {
        if !root.join(".git").is_dir() {
            bail!(
                "log_repo `{}` is not a git repository — clone or `git init` it first \
                 (see docs/logging-setup.md)",
                root.display()
            );
        }
        write_game(name, &entries, &root.join("games").join(&game))?;
        write_root_index(&root)?;
        git_commit(&root, &msg, OWNED_PATHS)?;
        let rd = root.display();
        if auto_push {
            git_push(&root)?;
            println!("Published {n} turn(s) to {rd}");
        } else {
            println!(
                "Committed {n} turn(s) to {rd} — push when you're ready (log_auto_push is off)."
            );
        }
        return Ok(());
    }

    // Default: the `gh`-managed public `neurohelmet-logs` repo, served via GitHub Pages.
    let owner = gh_user()?;
    ensure_repo(&owner)?;
    let root = neurohelmet_dir().join("logs-repo");
    ensure_clone(&owner, &root)?;

    write_game(name, &entries, &root.join("games").join(&game))?;
    write_root_index(&root)?;
    git_commit(&root, &msg, OWNED_PATHS)?;
    if auto_push {
        git_push(&root)?;
        ensure_pages(&owner); // best-effort; needs the pushed branch to exist
        println!("Published https://{owner}.github.io/{REPO}/games/{game}/");
    } else {
        println!(
            "Committed {n} turn(s) to {rd} — run `git -C {rd} push` to publish (log_auto_push is off).",
            rd = root.display()
        );
    }
    Ok(())
}

/// Render the game's PNGs (per-mech + per-turn montage) and its README into `game_dir`.
fn write_game(name: &str, entries: &[LogEntry], game_dir: &Path) -> Result<()> {
    std::fs::create_dir_all(game_dir)?;
    for e in entries {
        let tdir = game_dir.join(format!("turn-{:02}", e.turn));
        std::fs::create_dir_all(&tdir)?;
        let mut images = Vec::new();
        for (stem, _heading, buf) in render_turn(e) {
            let img = rasterize(&buf, SCALE);
            img.write_png(&tdir.join(format!("{stem}.png")))?;
            images.push(img);
        }
        montage(&images).write_png(&game_dir.join(format!("turn-{:02}.png", e.turn)))?;
    }
    std::fs::write(game_dir.join("README.md"), game_readme(name, entries))?;
    Ok(())
}

/// Markdown gallery for one game: each turn's montage, newest first.
fn game_readme(name: &str, entries: &[LogEntry]) -> String {
    let mut s = format!("# {name}\n\nTurn-by-turn log — newest first.\n\n");
    for e in entries.iter().rev() {
        s.push_str(&format!(
            "## {} ({} mech{})\n\n![{}](turn-{:02}.png)\n\n",
            e.label,
            e.mechs.len(),
            if e.mechs.len() == 1 { "" } else { "s" },
            e.label,
            e.turn
        ));
    }
    s
}

/// A minimal repo landing page (created once; not overwritten if present).
fn write_root_index(root: &Path) -> Result<()> {
    let readme = root.join("README.md");
    if !readme.exists() {
        std::fs::create_dir_all(root)?;
        std::fs::write(
            &readme,
            "# neurohelmet game logs\n\nTurn-by-turn BattleTech logs exported by \
             [neurohelmet](https://github.com/tympaniplayer/neurohelmet). Browse `games/`.\n",
        )?;
    }
    Ok(())
}

// ----- gh / git plumbing -----

fn run_ok(cmd: &mut Command) -> Result<std::process::Output> {
    let out = cmd.output().wrap_err("failed to launch command")?;
    if !out.status.success() {
        bail!(
            "command failed ({}): {}",
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(out)
}

fn gh_user() -> Result<String> {
    let out = run_ok(Command::new("gh").args(["api", "user", "-q", ".login"]))
        .wrap_err("`gh` not authenticated? run `gh auth login`")?;
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Create the public logs repo (+ enable Pages) if it doesn't exist yet.
fn ensure_repo(owner: &str) -> Result<()> {
    let exists = Command::new("gh")
        .args(["repo", "view", &format!("{owner}/{REPO}")])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if exists {
        return Ok(());
    }
    run_ok(Command::new("gh").args([
        "repo",
        "create",
        &format!("{owner}/{REPO}"),
        "--public",
        "--description",
        "neurohelmet turn-by-turn game logs",
    ]))?;
    Ok(())
}

/// Clone the repo on first use, otherwise pull the latest.
fn ensure_clone(owner: &str, dir: &Path) -> Result<()> {
    if dir.join(".git").is_dir() {
        let _ = Command::new("git")
            .args(["-C", &dir.to_string_lossy(), "pull", "--ff-only"])
            .output();
        return Ok(());
    }
    if let Some(parent) = dir.parent() {
        std::fs::create_dir_all(parent)?;
    }
    run_ok(Command::new("gh").args([
        "repo",
        "clone",
        &format!("{owner}/{REPO}"),
        &dir.to_string_lossy(),
    ]))?;
    Ok(())
}

/// Enable GitHub Pages (main branch, root) so the markdown gallery is browsable. Best-effort:
/// requires the branch to already exist (post-push), and a no-op once already enabled.
fn ensure_pages(owner: &str) {
    let _ = Command::new("gh")
        .args([
            "api",
            "-X",
            "POST",
            &format!("repos/{owner}/{REPO}/pages"),
            "-f",
            "source[branch]=main",
            "-f",
            "source[path]=/",
        ])
        .output();
}

/// Stage only the paths publish manages (never `-A`) and commit. `paths` are relative to the repo
/// root; an empty commit (nothing changed since last publish) is treated as success.
fn git_commit(dir: &Path, msg: &str, paths: &[&str]) -> Result<()> {
    let d = dir.to_string_lossy().to_string();
    run_ok(
        Command::new("git")
            .args(["-C", &d, "add", "--"])
            .args(paths),
    )?;
    // `commit` exits non-zero when there's nothing new — that's fine; any other failure is real.
    let committed = Command::new("git")
        .args(["-C", &d, "commit", "-m", msg])
        .output()?;
    let stdout = String::from_utf8_lossy(&committed.stdout);
    if !committed.status.success() && !stdout.contains("nothing to commit") {
        bail!("git commit failed: {}", stdout.trim());
    }
    Ok(())
}

/// Push the current branch. Errors surface to the caller — a missing remote or upstream is the
/// user's to configure (see `docs/logging-setup.md`).
fn git_push(dir: &Path) -> Result<()> {
    run_ok(Command::new("git").args(["-C", &dir.to_string_lossy(), "push"]))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use neurohelmet_core::session::TrackedMech;

    #[test]
    fn readme_lists_turns_newest_first() {
        use neurohelmet_core::domain::GameMode;
        let entries = vec![
            LogEntry {
                turn: 1,
                label: "Turn 1".into(),
                ts: None,
                mode: GameMode::Classic,
                mechs: vec![],
                sbf: Default::default(),
                bf: Default::default(),
            },
            LogEntry {
                turn: 2,
                label: "Turn 2".into(),
                ts: None,
                mode: GameMode::Classic,
                mechs: vec![],
                sbf: Default::default(),
                bf: Default::default(),
            },
        ];
        let md = game_readme("my game", &entries);
        assert!(md.starts_with("# my game"));
        let t2 = md.find("Turn 2").unwrap();
        let t1 = md.find("Turn 1").unwrap();
        assert!(t2 < t1, "newest turn first");
        assert!(md.contains("![Turn 2](turn-02.png)"));
    }

    #[test]
    fn write_game_produces_pngs() {
        use neurohelmet_core::domain::{
            AsStats, HeatSinkType, Location, LocationArmor, Mech, MechConfig, UnitType,
        };
        use std::collections::BTreeMap;
        // A minimal valid mech (armor on one location is enough to render).
        let mut armor = BTreeMap::new();
        armor.insert(
            Location::CenterTorso,
            LocationArmor {
                armor_max: 10,
                rear_max: 3,
                internal_max: 5,
            },
        );
        let mech = Mech {
            chassis: "Test".into(),
            model: "X".into(),
            tonnage: 20,
            tech_base: "IS".into(),
            role: "Scout".into(),
            weight_class: "Light".into(),
            subtype: "BattleMek".into(),
            year: 3000,
            bv: 0,
            cost: 0,
            armor_type: "Standard".into(),
            structure_type: "Standard".into(),
            walk: 6,
            run: 9,
            jump: 0,
            heat_sinks: 10,
            heat_sink_type: HeatSinkType::Single,
            dissipation: 10,
            equipment: vec![],
            config: MechConfig::Biped,
            unit_type: UnitType::Mech,
            motive: None,
            internal: 0,
            dpt: 0,
            transport: vec![],
            armor,
            weapons: vec![],
            ammo: vec![],
            crit_slots: BTreeMap::new(),
            as_stats: AsStats::default(),
            availability: BTreeMap::new(),
        };
        let entries = vec![LogEntry {
            turn: 1,
            label: "Turn 1".into(),
            ts: None,
            mode: neurohelmet_core::domain::GameMode::Classic,
            mechs: vec![TrackedMech::new(mech)],
            sbf: Default::default(),
            bf: Default::default(),
        }];
        let dir = tempfile::tempdir().unwrap();
        let game_dir = dir.path().join("games").join("g");
        write_game("g", &entries, &game_dir).unwrap();
        assert!(game_dir.join("turn-01.png").exists(), "montage png");
        assert!(game_dir.join("README.md").exists());
        let png = std::fs::read(game_dir.join("turn-01.png")).unwrap();
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n", "valid PNG signature");
    }

    #[test]
    fn root_index_is_written_once_and_not_clobbered() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("logs");
        // First call creates the landing page.
        write_root_index(&root).unwrap();
        let readme = root.join("README.md");
        assert!(readme.exists());
        assert!(std::fs::read_to_string(&readme)
            .unwrap()
            .starts_with("# neurohelmet game logs"));
        // A user edit must survive a second call (it only writes when absent).
        std::fs::write(&readme, "custom landing page").unwrap();
        write_root_index(&root).unwrap();
        assert_eq!(
            std::fs::read_to_string(&readme).unwrap(),
            "custom landing page"
        );
    }

    #[test]
    fn git_commit_stages_only_owned_paths() {
        // The whole point of a custom `log_repo`: publishing into a repo shared with other files
        // must never sweep those files into our commit. Skip gracefully where git is unavailable.
        if Command::new("git")
            .arg("--version")
            .output()
            .map(|o| !o.status.success())
            .unwrap_or(true)
        {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let git = |args: &[&str]| {
            let out = Command::new("git")
                .args(["-C", &root.to_string_lossy()])
                .args(args)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "git {args:?}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        };
        git(&["init", "-q"]);
        git(&["config", "user.email", "t@t"]); // local identity so the test is self-contained
        git(&["config", "user.name", "t"]);
        git(&["config", "commit.gpgsign", "false"]);

        // A file we do NOT own sits alongside the tree we do.
        std::fs::write(root.join("secret.txt"), "private").unwrap();
        std::fs::create_dir_all(root.join("games/g")).unwrap();
        std::fs::write(root.join("games/g/turn-01.png"), "png").unwrap();
        std::fs::write(root.join("README.md"), "# logs").unwrap();

        git_commit(root, "publish", OWNED_PATHS).unwrap();

        let out = Command::new("git")
            .args(["-C", &root.to_string_lossy(), "ls-files"])
            .output()
            .unwrap();
        let tracked = String::from_utf8_lossy(&out.stdout);
        assert!(
            tracked.contains("games/g/turn-01.png"),
            "committed the frames: {tracked}"
        );
        assert!(
            tracked.contains("README.md"),
            "committed the index: {tracked}"
        );
        assert!(
            !tracked.contains("secret.txt"),
            "must not commit unrelated files: {tracked}"
        );
    }
}
