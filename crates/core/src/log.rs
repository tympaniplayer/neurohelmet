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

//! Optional game log: append-only per-turn snapshots of a session's mechs, for later export to a
//! transcript or images. Read-only with respect to gameplay — writing a snapshot never changes any
//! tracked state, and a session that's never snapshotted has no log file at all.

use crate::domain::GameMode;
use crate::error::Result;
use crate::session::{sanitize_name, sessions_dir, BfState, SbfState, TrackedMech};
use serde::{Deserialize, Serialize};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

/// One logged snapshot: every tracked mech's full state at a chosen moment. The `turn` is just the
/// snapshot count — the log imposes no turn/phase structure. `TrackedMech` embeds its own spec, so
/// each entry is self-contained and re-renderable offline without the baked bundle.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogEntry {
    pub turn: u32,
    pub label: String,
    /// Wall-clock stamp when captured (free-form; `None` if not recorded).
    #[serde(default)]
    pub ts: Option<String>,
    /// The session's game mode when captured, so the export re-renders the right card (Classic
    /// record sheet / Alpha Strike card / Override doll / SBF formation sheet). Defaulted to
    /// Classic for old logs.
    #[serde(default)]
    pub mode: GameMode,
    pub mechs: Vec<TrackedMech>,
    /// SBF grouping + live formation state at capture (empty for other modes and for logs
    /// written before this field existed — such SBF entries export as per-element AS cards).
    /// `mechs` is the element pool its indices reference, so the entry stays self-contained.
    #[serde(default)]
    pub sbf: SbfState,
    /// Standard BF Unit grouping + round state at capture (empty for other modes and for logs
    /// written before this field existed). `mechs` carries the per-element live state
    /// (`TrackedMech.bf`), so the entry stays self-contained.
    #[serde(default)]
    pub bf: BfState,
}

/// The JSONL log file for a named session (a sibling of its `<name>.json`).
pub fn log_file(name: &str) -> PathBuf {
    sessions_dir().join(format!("{}.log.jsonl", sanitize_name(name)))
}

/// Append one snapshot to a session's log (creates the file on first use).
pub fn append_log(name: &str, entry: &LogEntry) -> Result<()> {
    append_to(&log_file(name), entry)
}

/// Read all snapshots from a session's log, oldest first. Empty if the log doesn't exist.
pub fn read_log(name: &str) -> Result<Vec<LogEntry>> {
    read_from(&log_file(name))
}

/// Append one snapshot as a single compact JSON line to a specific path.
pub fn append_to(path: &Path, entry: &LogEntry) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut line = serde_json::to_string(entry)?;
    line.push('\n');
    let mut f = OpenOptions::new().create(true).append(true).open(path)?;
    f.write_all(line.as_bytes())?;
    Ok(())
}

/// Read all snapshots from a specific JSONL path (empty if it doesn't exist).
pub fn read_from(path: &Path) -> Result<Vec<LogEntry>> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).map_err(Into::into))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_then_read_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("game.log.jsonl");

        assert!(read_from(&path).unwrap().is_empty(), "missing log reads empty");

        let e1 = LogEntry {
            turn: 1,
            label: "Turn 1".into(),
            ts: None,
            mode: GameMode::Classic,
            mechs: vec![],
            sbf: SbfState::default(),
            bf: BfState::default(),
        };
        let mut sbf = SbfState::default();
        sbf.formations.push(crate::session::SbfFormationState {
            name: "Binary 1".into(),
            ..Default::default()
        });
        sbf.round = 3;
        let e2 = LogEntry {
            turn: 2,
            label: "Turn 2".into(),
            ts: Some("now".into()),
            mode: GameMode::StrategicBattleForce,
            mechs: vec![],
            sbf,
            bf: BfState::default(),
        };
        let mut bf = BfState::default();
        bf.units.push(crate::session::BfUnitState {
            name: "Fire Lance".into(),
            ..Default::default()
        });
        bf.round = 2;
        let e3 = LogEntry {
            turn: 3,
            label: "Turn 3".into(),
            ts: None,
            mode: GameMode::BattleForce,
            mechs: vec![],
            sbf: SbfState::default(),
            bf,
        };
        append_to(&path, &e1).unwrap();
        append_to(&path, &e2).unwrap();
        append_to(&path, &e3).unwrap();

        assert_eq!(read_from(&path).unwrap(), vec![e1, e2, e3]);
    }

    #[test]
    fn old_log_lines_without_sbf_still_parse() {
        // Logs written before the `sbf`/`bf` fields default to empty state.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("old.log.jsonl");
        std::fs::write(&path, r#"{"turn":1,"label":"Turn 1","mechs":[]}"#).unwrap();
        let entries = read_from(&path).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].sbf, SbfState::default());
        assert_eq!(entries[0].bf, BfState::default());
        assert_eq!(entries[0].mode, GameMode::Classic);
    }
}
