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

//! neurohelmet-bake: download Mekbay data and bake it into a single self-contained bundle.
//!
//! Usage:
//!   neurohelmet-bake [--out data/mechs.bin] [--cache .bake-cache]
//!                 [--limit N] [--filter SUBSTR] [--jobs N]
//!
//! Downloads `units.json` + `equipment2.json` once (cached), then each Mek's record-sheet
//! SVG (cached), parses per-location armor from the SVG, joins weapon/ammo/heat data, and
//! writes a bincode `Bundle` of all BattleMeks to `--out`.

mod avail;
mod extras;
mod fetch;
mod join;
mod svg;

use fetch::Fetcher;
use neurohelmet_core::data::bundle::Bundle;
use neurohelmet_core::domain::Mech;
use rayon::prelude::*;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

struct Args {
    out: PathBuf,
    /// Whether `--out` was given explicitly (vs. the default `data/mechs.bin`). A filtered/limited
    /// bake produces a *subset*, so we refuse to overwrite the default committed bundle with one.
    out_explicit: bool,
    cache: PathBuf,
    limit: Option<usize>,
    filter: Option<String>,
    jobs: Option<usize>,
    print: bool,
}

fn parse_args() -> Args {
    let mut args = Args {
        out: PathBuf::from("data/mechs.bin"),
        out_explicit: false,
        cache: PathBuf::from(".bake-cache"),
        limit: None,
        filter: None,
        jobs: None,
        print: false,
    };
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--out" => {
                args.out = PathBuf::from(it.next().expect("--out needs a value"));
                args.out_explicit = true;
            }
            "--cache" => args.cache = PathBuf::from(it.next().expect("--cache needs a value")),
            "--limit" => {
                args.limit = Some(it.next().expect("--limit needs a value").parse().unwrap())
            }
            "--filter" => args.filter = Some(it.next().expect("--filter needs a value")),
            "--jobs" => args.jobs = Some(it.next().expect("--jobs needs a value").parse().unwrap()),
            "--print" => args.print = true,
            other => {
                eprintln!("unknown arg: {other}");
                std::process::exit(2);
            }
        }
    }
    args
}

fn main() {
    let args = parse_args();
    if let Some(j) = args.jobs {
        rayon::ThreadPoolBuilder::new()
            .num_threads(j)
            .build_global()
            .ok();
    }

    let fetcher = Fetcher::new(args.cache.clone()).expect("create cache dir");

    eprintln!("downloading catalogs (cached at {}) ...", args.cache.display());
    let units_text = fetcher.get_text("units.json").expect("fetch units.json");
    let eq_text = fetcher.get_text("equipment2.json").expect("fetch equipment2.json");

    let eq_index = join::build_equipment_index(&eq_text).expect("build equipment index");
    eprintln!(
        "equipment index: {} weapons, {} ammo",
        eq_index.weapons.len(),
        eq_index.ammo.len()
    );

    // §35: faction/era availability (era + faction catalogs + the RATGenerator-derived table).
    let avail = avail::fetch(&fetcher).expect("fetch availability catalogs");
    eprintln!(
        "availability: {} eras, {} factions, {} units with RAT data",
        avail.eras.len(),
        avail.factions.len(),
        avail.by_name.len()
    );

    let units = join::parse_units(&units_text).expect("parse units");
    // 'Mechs + combat vehicles + infantry/BA + aerospace fighters + large craft (DropShips + Small
    // Craft; Phase 1 of the large-craft initiative). WarShips/JumpShips/Space Stations and
    // protomechs are still excluded.
    let mut meks: Vec<serde_json::Value> = units
        .into_iter()
        .filter(|u| {
            join::is_mek(u)
                || join::is_vehicle(u)
                || join::is_infantry(u)
                || join::is_aero_fighter(u)
                || join::is_large_craft(u)
        })
        .collect();
    let n_veh = meks.iter().filter(|u| join::is_vehicle(u)).count();
    let n_inf = meks.iter().filter(|u| join::is_infantry(u)).count();
    let n_aero = meks.iter().filter(|u| join::is_aero_fighter(u)).count();
    let n_large = meks.iter().filter(|u| join::is_large_craft(u)).count();
    eprintln!(
        "found {} units ({} vehicles, {} infantry/BA, {} aero fighters, {} large craft) in catalog",
        meks.len(),
        n_veh,
        n_inf,
        n_aero,
        n_large
    );

    if let Some(f) = &args.filter {
        let f = f.to_lowercase();
        meks.retain(|u| {
            let name = format!(
                "{} {}",
                u.get("chassis").and_then(|v| v.as_str()).unwrap_or(""),
                u.get("model").and_then(|v| v.as_str()).unwrap_or("")
            )
            .to_lowercase();
            name.contains(&f)
        });
        eprintln!("after --filter {:?}: {} meks", args.filter, meks.len());
    }
    if let Some(n) = args.limit {
        meks.truncate(n);
        eprintln!("after --limit: {} meks", meks.len());
    }

    let total = meks.len();
    let done = AtomicUsize::new(0);
    let unresolved = AtomicUsize::new(0);

    let results: Vec<Result<Mech, String>> = meks
        .par_iter()
        .map(|unit| {
            let name = format!(
                "{} {}",
                unit.get("chassis").and_then(|v| v.as_str()).unwrap_or(""),
                unit.get("model").and_then(|v| v.as_str()).unwrap_or("")
            );
            // The units.json `name` (e.g. `BMAtlas_AS7D`) is the join key into the availability table.
            let join_key = unit.get("name").and_then(|v| v.as_str()).unwrap_or("");
            // 'Mechs and vehicles parse per-location armor from the record-sheet SVG (vehicles
            // have no crit-slot table — their criticals are a manual rolled table, not slots).
            // Battle Armor parses per-trooper pips from its sheet; conventional infantry has
            // no pips (its sheet is a strength/damage table) and bakes from JSON only.
            let outcome = if join::is_large_craft(unit) {
                // Large craft field on the AS/BF card only (multi-arc damage + single Arm/Str/Th
                // pool from JSON); no SVG doll, no Classic loadout.
                join::build_large_craft(unit).map_err(|e| format!("{name}: {e}"))?
            } else if join::is_aero_fighter(unit) {
                // Aerospace fighters: armor arcs + SI from the record-sheet SVG (like vehicles),
                // plus the printed heat-sink count.
                let rel = join::sheet_rel(unit).ok_or_else(|| format!("{name}: no sheet"))?;
                let svg_text = fetcher.get_text(&rel).map_err(|e| format!("{name}: {e}"))?;
                let armor = svg::parse_armor(&svg_text).map_err(|e| format!("{name}: {e}"))?;
                let (heat_sinks, dissipation) = svg::parse_aero_heat(&svg_text);
                join::build_aero(unit, armor, heat_sinks, dissipation, &eq_index)
                    .map_err(|e| format!("{name}: {e}"))?
            } else if join::is_infantry(unit) {
                let armor = if unit.get("subtype").and_then(serde_json::Value::as_str)
                    == Some("Battle Armor")
                {
                    let rel = join::sheet_rel(unit).ok_or_else(|| format!("{name}: no sheet"))?;
                    let svg_text = fetcher.get_text(&rel).map_err(|e| format!("{name}: {e}"))?;
                    svg::parse_ba_armor(&svg_text).map_err(|e| format!("{name}: {e}"))?
                } else {
                    Default::default()
                };
                join::build_infantry(unit, armor, &eq_index).map_err(|e| format!("{name}: {e}"))?
            } else {
                let rel = join::sheet_rel(unit).ok_or_else(|| format!("{name}: no sheet"))?;
                let svg_text = fetcher.get_text(&rel).map_err(|e| format!("{name}: {e}"))?;
                let armor = svg::parse_armor(&svg_text).map_err(|e| format!("{name}: {e}"))?;
                if join::is_vehicle(unit) {
                    let transport = svg::parse_transport(&svg_text);
                    join::build_vehicle(unit, armor, transport, &eq_index)
                        .map_err(|e| format!("{name}: {e}"))?
                } else {
                    let crit_slots =
                        svg::parse_crit_slots(&svg_text).map_err(|e| format!("{name}: {e}"))?;
                    join::build_mech(unit, armor, crit_slots, &eq_index)
                        .map_err(|e| format!("{name}: {e}"))?
                }
            };
            if !outcome.unresolved_heat.is_empty() {
                unresolved.fetch_add(outcome.unresolved_heat.len(), Ordering::Relaxed);
            }
            let mut mech = outcome.mech;
            if let Some(av) = avail.by_name.get(join_key) {
                mech.availability = av.clone();
            }
            let n = done.fetch_add(1, Ordering::Relaxed) + 1;
            if n.is_multiple_of(200) || n == total {
                eprintln!("  baked {n}/{total}");
            }
            Ok(mech)
        })
        .collect();

    let mut mechs = Vec::new();
    let mut failures = 0usize;
    for r in results {
        match r {
            Ok(m) => mechs.push(m),
            Err(e) => {
                failures += 1;
                if failures <= 25 {
                    eprintln!("  SKIP {e}");
                }
            }
        }
    }
    // Merge hand-entered units that aren't in Mekbay's catalog (gun emplacements / Battlefield
    // Support AS cards — see `extras.rs`). Appended before the sort so they file in alphabetically.
    let extra = extras::load_extra_units(std::path::Path::new("data/extra_units.json"));
    if !extra.is_empty() {
        eprintln!("merging {} hand-entered extra unit(s) from data/extra_units.json", extra.len());
        mechs.extend(extra);
    }

    // Sort by chassis/model, ignoring leading punctuation so quoted nicknames (e.g. `'Wing'
    // Wraith`, `'Gestalt'`) file under their first letter instead of clumping at the top.
    let sort_key = |s: &str| s.trim_start_matches(|c: char| !c.is_alphanumeric()).to_string();
    mechs.sort_by(|a, b| {
        (sort_key(&a.chassis), sort_key(&a.model)).cmp(&(sort_key(&b.chassis), sort_key(&b.model)))
    });

    eprintln!(
        "baked {} meks ({} skipped, {} weapons with unresolved heat)",
        mechs.len(),
        failures,
        unresolved.load(Ordering::Relaxed)
    );

    if args.print {
        for m in &mechs {
            println!(
                "\n{} ({}t, {}, walk {}/run {}/jump {})",
                m.display_name(),
                m.tonnage,
                m.role,
                m.walk,
                m.run,
                m.jump
            );
            println!(
                "  heat sinks: {} {:?} -> dissipation {}",
                m.heat_sinks, m.heat_sink_type, m.dissipation
            );
            for (loc, a) in &m.armor {
                println!(
                    "  {:<3} armor {:>2} rear {:>2} internal {:>2}",
                    loc.code(),
                    a.armor_max,
                    a.rear_max,
                    a.internal_max
                );
            }
            for w in &m.weapons {
                let to_hit = m.weapon_to_hit(w);
                let th = if to_hit != 0 { format!(" to-hit {to_hit:+}") } else { String::new() };
                println!(
                    "  WPN {:<18} {} heat {} dmg {} rng {}{}{}",
                    w.name,
                    w.location.code(),
                    w.heat,
                    w.damage,
                    w.range,
                    if w.rear { " (rear)" } else { "" },
                    th
                );
            }
            for b in &m.ammo {
                let munitions = b
                    .base_ammo
                    .as_deref()
                    .map(|g| {
                        let n = eq_index.munition_catalog.get(g).map_or(0, Vec::len);
                        format!("  [munition {}: {} options]", b.munition_name(), n)
                    })
                    .unwrap_or_default();
                println!(
                    "  AMM {:<22} {} {}x{} = {} shots{}",
                    b.name,
                    b.location.code(),
                    b.tons,
                    b.shots_per_ton,
                    b.shots_max(),
                    munitions
                );
            }
            for e in &m.equipment {
                println!("  EQP {:<22} {}", e.name, e.location.code());
            }
        }
    }

    // Guard the footgun: `--filter`/`--limit` bake a *subset*, so writing it to the default
    // committed bundle would silently truncate the real dataset. Require an explicit `--out`.
    if (args.filter.is_some() || args.limit.is_some()) && !args.out_explicit {
        eprintln!(
            "refusing to overwrite {} with a filtered/limited subset ({} units).\n\
             pass an explicit --out <path> (e.g. --out /tmp/subset.bin) for a partial bake.",
            args.out.display(),
            mechs.len()
        );
        std::process::exit(2);
    }

    let with_avail = mechs.iter().filter(|m| !m.availability.is_empty()).count();
    eprintln!("availability joined onto {with_avail}/{} units", mechs.len());

    let mut bundle = Bundle::new(mechs);
    bundle.munitions = eq_index.munition_catalog;
    bundle.eras = avail.eras;
    bundle.factions = avail.factions;
    eprintln!(
        "munition groups: {} | eras: {} | factions: {}",
        bundle.munitions.len(),
        bundle.eras.len(),
        bundle.factions.len()
    );
    let bytes = bundle.encode().expect("encode bundle");
    if let Some(parent) = args.out.parent() {
        std::fs::create_dir_all(parent).expect("create out dir");
    }
    std::fs::write(&args.out, &bytes).expect("write bundle");
    eprintln!(
        "wrote {} ({:.1} MB)",
        args.out.display(),
        bytes.len() as f64 / 1_048_576.0
    );
}
