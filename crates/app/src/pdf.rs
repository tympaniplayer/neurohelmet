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

//! Print-to-PDF record sheet export (ROADMAP §37; `docs/pdf-record-sheet-spec.md`).
//!
//! Covers all three BattleForce-family modes: **Strategic BattleForce** (a port of MegaMek's
//! `SBFRecordSheet`), **Standard BattleForce** (one page per Unit), and the **Abstract Combat
//! System** (a Combat-Unit sheet per Combat Unit + a Formation-Tracking sheet). We build
//! **neurohelmet's own** vector record sheet as an SVG (no CGL/BattleTech artwork — the official
//! sheets were a layout reference only) and convert it to a US-Letter PDF with `svg2pdf`. The SVG is
//! generated programmatically; every value comes from the same derived stats the TUI shows, so the
//! sheet matches the screen.
//!
//! The sheet is always a **pristine blank fill-in form** — a printout exists to take a clean sheet to
//! the table, so [`make_blank`] strips all live damage/heat/crits/morale before rendering. Pages are
//! assembled into one multi-page PDF ([`svgs_to_pdf`]) — one page per formation (SBF/ACS) or per Unit
//! (BF). Driven by the `--pdf <session>` CLI verb and the in-app `P` key.

use neurohelmet_core::domain::GameMode;
use neurohelmet_core::engine::as_element::{self, AsElement, DamageVector};
use neurohelmet_core::session::{self, MoraleStatus, SbfFormationState, Session, TrackedMech};
use std::fmt::Write as _;
use std::path::PathBuf;

/// Embedded sheet font — Roboto (Apache-2.0), the same face Mekbay renders its record sheets with.
const FONT_REGULAR: &[u8] = include_bytes!("../assets/fonts/Roboto-Regular.ttf");
const FONT_BOLD: &[u8] = include_bytes!("../assets/fonts/Roboto-Bold.ttf");
const FONT_FAMILY: &str = "Roboto";

/// BattleTech + Catalyst logos (from MegaMek's assets — CC-BY-NC-SA; trademarks, see NOTICE.md),
/// drawn on the sheet exactly as MegaMek's `SBFRecordSheet` does.
const BT_LOGO: &[u8] = include_bytes!("../assets/logos/BT_Logo_BW.png");
const CGL_LOGO: &[u8] = include_bytes!("../assets/logos/CGL_Logo.png");

/// US Letter in points (== px at svg2pdf's default 72 dpi).
const PAGE_W: f32 = 612.0;
const PAGE_H: f32 = 792.0;

/// The internal sheet space every record sheet is drawn in (MegaMek's `SBFRecordSheet` canvas),
/// scaled to fit a US-Letter page. BF/ACS reuse it so all three sheets share proportions.
const SHEET_W: f32 = 1435.0;
const SHEET_H: f32 = 2000.0;
/// Underline grey shared across sheets (Java `Color.LIGHT_GRAY`).
const LINE: &str = "rgb(192,192,192)";

/// Export the Strategic BattleForce record sheets of session `name` to a single multi-page PDF
/// (default `<sessions_dir>/<name>-sheets.pdf`, one page per formation). The sheet is always a
/// **pristine fill-in form** — a printout exists to take a clean sheet to the table, so it never
/// renders the session's live damage/crits/morale.
pub fn run(name: &str, outfile: Option<PathBuf>) -> color_eyre::Result<()> {
    let Some(sess) = session::load_named(name)? else {
        eprintln!("No saved session '{name}'.");
        std::process::exit(2);
    };
    let path = export_session(&sess, name, outfile)?;
    println!("Wrote record sheet for '{}' → {}", name, path.display());
    Ok(())
}

/// Whether `mode` has a PDF record sheet (the three BattleForce-family modes).
fn mode_supported(mode: GameMode) -> bool {
    matches!(
        mode,
        GameMode::StrategicBattleForce | GameMode::BattleForce | GameMode::AbstractCombatSystem
    )
}

/// Render `sess`'s SBF formations to one multi-page PDF and write it to `out` (default
/// `<sessions_dir>/<name>-sheets.pdf`). Shared by the `--pdf` CLI verb and the in-app `P` key;
/// returns the written path.
pub(crate) fn export_session(
    sess: &Session,
    name: &str,
    out: Option<PathBuf>,
) -> color_eyre::Result<PathBuf> {
    if !mode_supported(sess.mode) {
        color_eyre::eyre::bail!(
            "PDF export supports BattleForce, Strategic BattleForce, and ACS sessions only (session is {:?}).",
            sess.mode
        );
    }
    let pdf = render_session_pdf(sess)?;
    let path = out.unwrap_or_else(|| {
        session::sessions_dir().join(format!("{}-sheets.pdf", session::sanitize_name(name)))
    });
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, pdf)?;
    Ok(path)
}

/// A session's SBF formations as one multi-page PDF — one US-Letter page per formation, always a
/// **pristine fill-in sheet**: the roster's static stats + force org (names, COM/LEAD) with full
/// armor, no crits, Normal morale. The printout is for taking to a game, so it deliberately strips
/// all live battle state (via [`make_blank`]).
pub(crate) fn render_session_pdf(sess: &Session) -> color_eyre::Result<Vec<u8>> {
    let mut sess = sess.clone();
    make_blank(&mut sess);
    let svgs: Vec<String> = match sess.mode {
        GameMode::StrategicBattleForce => {
            sess.sbf.formations.iter().map(|fs| sbf_formation_svg(&sess, fs)).collect()
        }
        GameMode::BattleForce => bf_sheets(&sess),
        GameMode::AbstractCombatSystem => acs_sheets(&sess),
        other => color_eyre::eyre::bail!("PDF export does not support {other:?} sessions"),
    };
    if svgs.is_empty() {
        color_eyre::eyre::bail!("session has no formations/units to export");
    }
    svgs_to_pdf(&svgs, &svg_options())
}

/// usvg parse options with the embedded value font loaded and made the default sans-serif.
fn svg_options() -> svg2pdf::usvg::Options<'static> {
    let mut opt = svg2pdf::usvg::Options::default();
    let db = opt.fontdb_mut();
    db.load_font_data(FONT_REGULAR.to_vec());
    db.load_font_data(FONT_BOLD.to_vec());
    db.set_sans_serif_family(FONT_FAMILY);
    opt
}

/// Reset persisted SBF live state so the sheet renders as a pristine fill-in form: full armor, no
/// crits, Normal morale, round 0. COM/LEAD designations are kept (they are pre-game roles, not
/// battle damage).
pub(crate) fn make_blank(sess: &mut Session) {
    // SBF live state.
    sess.sbf.round = 0;
    for f in &mut sess.sbf.formations {
        f.morale = MoraleStatus::Normal;
        f.jump_used_this_turn = 0;
        f.is_done = false;
        for u in &mut f.units {
            u.armor_hits = 0;
            u.damage_crits = 0;
            u.targeting_crits = 0;
            u.mp_crits = 0;
        }
    }
    // Standard BF live state: full armor/heat/crits on every pool element + Normal Unit morale.
    sess.bf.round = 0;
    for u in &mut sess.bf.units {
        u.morale = session::BfMorale::Normal;
    }
    // ACS live state: full armor, no fatigue, Normal morale, round 0 (COM/LEAD roles preserved).
    sess.acs.round = 0;
    for f in &mut sess.acs.formations {
        f.morale = neurohelmet_core::engine::acs::AcsMorale::Normal;
        f.is_done = false;
        for u in &mut f.units {
            u.armor_hits = 0;
            u.fatigue_points_x2 = 0;
            u.morale = neurohelmet_core::engine::acs::AcsMorale::Normal;
        }
    }
    // Per-element live state (armor/heat/crits) shared by BF (its cards read TrackedMech directly).
    for tm in &mut sess.mechs {
        tm.as_armor_hits = 0;
        tm.as_struct_hits = 0;
        tm.as_heat = 0;
        tm.bf = session::BfLive::default();
    }
}

/// Assemble one multi-page PDF from per-page SVGs (each our own 612×792 US-Letter sheet). Each SVG
/// becomes an XObject (via `svg2pdf::to_chunk`), renumbered into a shared ref space and placed 1:1 on
/// its own page — the documented `svg2pdf` multi-page recipe, so no external PDF-merge crate.
fn svgs_to_pdf(svgs: &[String], opt: &svg2pdf::usvg::Options) -> color_eyre::Result<Vec<u8>> {
    use pdf_writer::{Chunk, Content, Finish, Name, Pdf, Rect, Ref};
    use std::collections::HashMap;

    let mut alloc = Ref::new(1);
    let catalog_id = alloc.bump();
    let page_tree_id = alloc.bump();

    struct PageObj {
        chunk: Chunk,
        svg_ref: Ref,
        page_id: Ref,
        content_id: Ref,
    }
    let mut pages: Vec<PageObj> = Vec::with_capacity(svgs.len());
    for svg in svgs {
        let tree = svg2pdf::usvg::Tree::from_str(svg, opt)
            .map_err(|e| color_eyre::eyre::eyre!("SVG parse: {e}"))?;
        let (chunk, svg_id) = svg2pdf::to_chunk(&tree, svg2pdf::ConversionOptions::default())
            .map_err(|e| color_eyre::eyre::eyre!("SVG→PDF: {e:?}"))?;
        // Rebase this chunk's refs into our shared allocator so pages don't collide.
        let mut map = HashMap::new();
        let chunk = chunk.renumber(|old| *map.entry(old).or_insert_with(|| alloc.bump()));
        let svg_ref = *map.get(&svg_id).expect("xobject ref survives renumber");
        pages.push(PageObj { chunk, svg_ref, page_id: alloc.bump(), content_id: alloc.bump() });
    }

    let mut pdf = Pdf::new();
    pdf.catalog(catalog_id).pages(page_tree_id);
    pdf.pages(page_tree_id)
        .kids(pages.iter().map(|p| p.page_id))
        .count(pages.len() as i32);

    let svg_name = Name(b"S1");
    for p in &pages {
        let mut page = pdf.page(p.page_id);
        page.media_box(Rect::new(0.0, 0.0, PAGE_W, PAGE_H));
        page.parent(page_tree_id);
        page.contents(p.content_id);
        let mut res = page.resources();
        res.x_objects().pair(svg_name, p.svg_ref);
        res.finish();
        page.finish();

        // svg2pdf's XObject is normalised to a unit square — scale it to fill the page.
        let mut content = Content::new();
        content.transform([PAGE_W, 0.0, 0.0, PAGE_H, 0.0, 0.0]).x_object(svg_name);
        pdf.stream(p.content_id, &content.finish());
        pdf.extend(&p.chunk);
    }

    Ok(pdf.finish())
}

/// Open a record-sheet SVG: the 612×792 page, a white fill, and a group that scales the
/// [`SHEET_W`]×[`SHEET_H`] sheet space to fit, with the BattleTech + Catalyst logos and the titled
/// banner already drawn. Content is written into the returned buffer (still inside the scaled
/// group); close with [`end_sheet`]. The banner takes one or two title lines. Shared by the BF and
/// ACS sheets; the SBF sheet predates this and inlines the same scaffold.
fn begin_sheet(title: &[&str]) -> String {
    let m = 18.0_f32;
    let scale = ((PAGE_W - 2.0 * m) / SHEET_W).min((PAGE_H - 2.0 * m) / SHEET_H);
    let tx = (PAGE_W - SHEET_W * scale) / 2.0;
    let ty = (PAGE_H - SHEET_H * scale) / 2.0;

    let mut b = String::with_capacity(24576);
    let _ = write!(
        b,
        r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="{PAGE_W}" height="{PAGE_H}" viewBox="0 0 {PAGE_W} {PAGE_H}">"#
    );
    let _ = write!(b, r#"<rect x="0" y="0" width="{PAGE_W}" height="{PAGE_H}" fill="white"/>"#);
    let _ = write!(b, r#"<g transform="translate({tx:.2} {ty:.2}) scale({scale:.4})">"#);

    // Logos: BattleTech (top-left) + Catalyst (top-right), as MegaMek's SBFRecordSheet draws them.
    image(&mut b, -23.0, 10.0, 722.0, 125.0, BT_LOGO);
    image(&mut b, 1287.0, 45.0, 125.0, 72.0, CGL_LOGO);

    // Title banner (same chamfered outline as the SBF sheet).
    stroke_poly(
        &mut b,
        &[(732.0, 60.0), (732.0, 55.0), (756.0, 32.0), (1401.0, 32.0), (1424.0, 55.0), (1424.0, 107.0), (1401.0, 130.0), (756.0, 130.0), (732.0, 107.0), (732.0, 60.0)],
        "black",
        5.0,
    );
    match title {
        [one] => txtw(&mut b, 1019.0, 81.0, 30.0, true, "middle", true, "black", 440.0, one),
        [one, two] => {
            txtw(&mut b, 1019.0, 63.0, 28.0, true, "middle", true, "black", 440.0, one);
            txtw(&mut b, 1019.0, 100.0, 28.0, true, "middle", true, "black", 440.0, two);
        }
        _ => {}
    }
    b
}

/// Close a sheet opened with [`begin_sheet`]: draw the verbatim Topps/CGL record-sheet notice (as
/// MegaMek prints it, so the sheet reads as the real thing) and close the group + SVG.
fn end_sheet(b: &mut String) {
    let _ = write!(b, r#"<g transform="translate(0 1960)">"#);
    txt(b, 717.0, 0.0, 18.0, false, "middle", false, "black", "(C) 2024 The Topps Company, Inc. BattleTech, 'Mech and BattleMech are trademarks of the Topps Company, Inc. All rights reserved.");
    txt(b, 717.0, 22.0, 18.0, false, "middle", false, "black", "Catalyst Game Labs and the Catalyst Game Labs logo are trademarks of InMediaRes Production, LLC. Permission to photocopy for personal use.");
    b.push_str("</g>");
    b.push_str("</g></svg>");
}

/// One formation's record sheet as an SVG document (612×792, US Letter).
fn sbf_formation_svg(sess: &Session, fs: &SbfFormationState) -> String {
    let form = sess.sbf_formation(fs);
    let aero = form.is_aerospace();

    // MegaMek's SBFRecordSheet draws in a fixed 1435×2000 sheet space; we reproduce it 1:1 inside a
    // group scaled to fit a US-Letter page (svg2pdf makes the PDF page == the SVG's own size).
    const W: f32 = 1435.0;
    const H: f32 = 2000.0;
    const SHADOW: &str = "rgb(213,213,215)";
    const LINE: &str = "rgb(192,192,192)"; // underline colour (Java Color.LIGHT_GRAY)
    let m = 18.0_f32;
    let scale = ((PAGE_W - 2.0 * m) / W).min((PAGE_H - 2.0 * m) / H);
    let tx = (PAGE_W - W * scale) / 2.0;
    let ty = (PAGE_H - H * scale) / 2.0;

    let mut b = String::with_capacity(24576);
    let _ = write!(
        b,
        r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="{PAGE_W}" height="{PAGE_H}" viewBox="0 0 {PAGE_W} {PAGE_H}">"#
    );
    let _ = write!(b, r#"<rect x="0" y="0" width="{PAGE_W}" height="{PAGE_H}" fill="white"/>"#);
    let _ = write!(b, r#"<g transform="translate({tx:.2} {ty:.2}) scale({scale:.4})">"#);

    // ── Logos: BattleTech (top-left) + Catalyst (top-right), as MegaMek's SBFRecordSheet draws them ──
    image(&mut b, -23.0, 10.0, 722.0, 125.0, BT_LOGO);
    image(&mut b, 1287.0, 45.0, 125.0, 72.0, CGL_LOGO);

    // Title banner
    stroke_poly(&mut b, &[(732.0, 60.0), (732.0, 55.0), (756.0, 32.0), (1401.0, 32.0), (1424.0, 55.0), (1424.0, 107.0), (1401.0, 130.0), (756.0, 130.0), (732.0, 107.0), (732.0, 60.0)], "black", 5.0);
    txtw(&mut b, 1019.0, 63.0, 28.0, true, "middle", true, "black", 440.0, "STRATEGIC BATTLEFORCE");
    txtw(&mut b, 1019.0, 100.0, 28.0, true, "middle", true, "black", 440.0, "FORMATION RECORD SHEET");

    // ── Formation block ──
    fill_poly(&mut b, &[(1425.0, 189.0), (1435.0, 199.0), (1435.0, 285.0), (1410.0, 312.0), (30.0, 312.0), (19.0, 301.0), (1402.0, 301.0), (1424.0, 276.0)], SHADOW);
    stroke_poly(&mut b, &[(104.0, 167.0), (1401.0, 167.0), (1424.0, 193.0), (1424.0, 276.0), (1401.0, 302.0), (24.0, 302.0), (0.0, 276.0), (0.0, 193.0), (24.0, 167.0), (104.0, 167.0)], "black", 5.0);
    fill_poly(&mut b, &[(11.0, 198.0), (30.0, 179.0), (214.0, 179.0), (233.0, 198.0), (214.0, 217.0), (30.0, 217.0), (11.0, 198.0)], "black");
    txt(&mut b, 470.0, 210.0, 22.0, true, "middle", true, "black", "Type");
    txt(&mut b, 535.0, 210.0, 22.0, true, "middle", true, "black", "Size");
    txt(&mut b, 604.0, 210.0, 22.0, true, "middle", true, "black", "Move");
    txt(&mut b, 683.0, 210.0, 22.0, true, "middle", true, "black", "Jump");
    txt(&mut b, 762.0, 185.0, 22.0, true, "middle", true, "black", "Transport");
    txt(&mut b, 762.0, 210.0, 22.0, true, "middle", true, "black", "Move");
    txt(&mut b, 839.0, 210.0, 22.0, true, "middle", true, "black", "TMM");
    txt(&mut b, 919.0, 210.0, 22.0, true, "middle", true, "black", "Tactics");
    txt(&mut b, 1012.0, 210.0, 22.0, true, "middle", true, "black", "Morale");
    txt(&mut b, 1088.0, 210.0, 22.0, true, "middle", true, "black", "Skill");
    txt(&mut b, 1150.0, 210.0, 22.0, true, "middle", true, "black", "PV");
    txt(&mut b, 1196.0, 210.0, 22.0, true, "start", true, "black", "Formation Specials");
    for &(x1, x2) in &[(21.0, 425.0), (446.0, 494.0), (511.0, 559.0), (575.0, 633.0), (654.0, 712.0), (727.0, 797.0), (812.0, 867.0), (884.0, 955.0), (972.0, 1052.0), (1064.0, 1113.0), (1125.0, 1175.0), (1188.0, 1403.0)] {
        line(&mut b, x1, 278.0, x2, 278.0, LINE, 3.0);
    }
    txtw(&mut b, 122.0, 198.0, 32.0, true, "middle", true, "white", 176.0, "FORMATION:");
    // Formation values (pristine, static stats)
    txtw(&mut b, 24.0, 260.0, 25.0, false, "start", true, "black", 390.0, &fs.name);
    txt(&mut b, 470.0, 260.0, 25.0, false, "middle", true, "black", &type_label(&format!("{:?}", form.sbf_type)));
    txt(&mut b, 535.0, 260.0, 25.0, false, "middle", true, "black", &form.size.to_string());
    txt(&mut b, 604.0, 260.0, 25.0, false, "middle", true, "black", &format!("{}{}", form.movement, form.move_mode.code()));
    txt(&mut b, 683.0, 260.0, 25.0, false, "middle", true, "black", &form.jump_move.to_string());
    txt(&mut b, 762.0, 260.0, 25.0, false, "middle", true, "black", &format!("{}{}", form.trsp_movement, form.trsp_mode.code()));
    if !aero {
        txt(&mut b, 839.0, 260.0, 25.0, false, "middle", true, "black", &form.tmm.to_string());
    }
    txt(&mut b, 919.0, 260.0, 25.0, false, "middle", true, "black", &form.tactics.to_string());
    txt(&mut b, 1012.0, 260.0, 25.0, false, "middle", true, "black", &form.morale_rating.to_string());
    txt(&mut b, 1088.0, 260.0, 25.0, false, "middle", true, "black", &form.skill.to_string());
    txt(&mut b, 1150.0, 260.0, 25.0, false, "middle", true, "black", &form.point_value.to_string());
    txtw(&mut b, 1196.0, 260.0, 25.0, false, "start", true, "black", 210.0, &suas(&form.suas));

    // ── Units overview (translate 0,325) ──
    let _ = write!(b, r#"<g transform="translate(0 325)">"#);
    fill_poly(&mut b, &[(1425.0, 22.0), (1435.0, 32.0), (1435.0, 245.0), (1410.0, 269.0), (443.0, 269.0), (431.0, 259.0), (1402.0, 259.0), (1424.0, 235.0)], SHADOW);
    fill_poly(&mut b, &[(30.0, 232.0), (20.0, 222.0), (390.0, 222.0), (400.0, 232.0)], SHADOW);
    stroke_poly(&mut b, &[(104.0, 0.0), (1401.0, 0.0), (1424.0, 26.0), (1424.0, 235.0), (1401.0, 259.0), (435.0, 259.0), (388.0, 222.0), (24.0, 222.0), (0.0, 196.0), (0.0, 26.0), (24.0, 0.0), (104.0, 0.0)], "black", 5.0);
    txt(&mut b, 388.0, 35.0, 22.0, true, "middle", true, "black", "Type");
    txt(&mut b, 451.0, 35.0, 22.0, true, "middle", true, "black", "Size");
    txt(&mut b, 514.0, 35.0, 22.0, true, "middle", true, "black", "Move");
    txt(&mut b, 577.0, 35.0, 22.0, true, "middle", true, "black", "Jump");
    txt(&mut b, 639.0, 17.0, 22.0, true, "middle", true, "black", "Trsp");
    txt(&mut b, 639.0, 35.0, 22.0, true, "middle", true, "black", "Move");
    txt(&mut b, 702.0, 35.0, 22.0, true, "middle", true, "black", "TMM");
    txt(&mut b, 764.0, 35.0, 22.0, true, "middle", true, "black", "Arm");
    txt(&mut b, 856.0, 35.0, 22.0, true, "middle", true, "black", "S/M/L/E");
    txt(&mut b, 948.0, 35.0, 22.0, true, "middle", true, "black", "Skill");
    txt(&mut b, 1010.0, 35.0, 22.0, true, "middle", true, "black", "PV");
    txt(&mut b, 1055.0, 35.0, 22.0, true, "start", true, "black", "Unit Specials");
    for row in 0..4 {
        let y = 85.0 + 41.0 * row as f32;
        for &(x1, x2) in &[(26.0, 351.0), (363.0, 414.0), (426.0, 476.0), (489.0, 539.0), (552.0, 602.0), (614.0, 664.0), (677.0, 727.0), (739.0, 789.0), (801.0, 911.0), (923.0, 973.0), (985.0, 1035.0), (1047.0, 1404.0)] {
            line(&mut b, x1, y, x2, y, LINE, 3.0);
        }
    }
    txt(&mut b, 21.0, 31.0, 26.0, true, "start", true, "black", "UNITS:");
    txt(&mut b, 443.0, 239.0, 22.0, true, "start", true, "black", "Notes:");
    line(&mut b, 524.0, 248.0, 1394.0, 248.0, LINE, 3.0);
    for (i, us) in fs.units.iter().take(4).enumerate() {
        let d = sess.sbf_unit(us);
        let y = 80.0 + 41.0 * i as f32;
        txtw(&mut b, 34.0, y, 23.0, false, "start", false, "black", 315.0, &us.name);
        txt(&mut b, 388.0, y, 23.0, false, "middle", false, "black", &type_label(&format!("{:?}", d.sbf_type)));
        txt(&mut b, 451.0, y, 23.0, false, "middle", false, "black", &d.size.to_string());
        txt(&mut b, 514.0, y, 23.0, false, "middle", false, "black", &format!("{}{}", d.movement, d.move_mode.code()));
        txt(&mut b, 577.0, y, 23.0, false, "middle", false, "black", &d.jump_move.to_string());
        txt(&mut b, 639.0, y, 23.0, false, "middle", false, "black", &format!("{}{}", d.trsp_movement, d.trsp_mode.code()));
        if !d.sbf_type.is_aerospace() {
            txt(&mut b, 702.0, y, 23.0, false, "middle", false, "black", &d.tmm.to_string());
        }
        txt(&mut b, 764.0, y, 23.0, false, "middle", false, "black", &d.armor.to_string());
        txt(&mut b, 856.0, y, 23.0, false, "middle", false, "black", &dmg_string(&d.damage));
        txt(&mut b, 948.0, y, 23.0, false, "middle", false, "black", &d.skill.to_string());
        txt(&mut b, 1010.0, y, 23.0, false, "middle", false, "black", &d.point_value.to_string());
        // COM/LEAD are player designations, not always baked SUAs — surface them in the Specials column.
        let mut sp = suas(&d.suas);
        for (flag, code) in [(us.is_commander, "COM"), (us.is_leader, "LEAD")] {
            if flag && !sp.split(' ').any(|t| t == code) {
                if !sp.is_empty() {
                    sp.push(' ');
                }
                sp.push_str(code);
            }
        }
        txtw(&mut b, 1055.0, y, 23.0, false, "start", false, "black", 340.0, &sp);
    }
    b.push_str("</g>");

    // ── Element sub-blocks (one per Unit, translate 0, 575+340*i) ──
    for i in 0..4 {
        let yoff = 575.0 + 340.0 * i as f32;
        let _ = write!(b, r#"<g transform="translate(0 {yoff:.1})">"#);
        fill_poly(&mut b, &[(1425.0, 61.0), (1435.0, 71.0), (1435.0, 317.0), (1410.0, 341.0), (407.0, 341.0), (397.0, 331.0), (1402.0, 331.0), (1424.0, 307.0)], SHADOW);
        fill_poly(&mut b, &[(30.0, 320.0), (20.0, 310.0), (376.0, 310.0), (386.0, 320.0)], SHADOW);
        stroke_poly(&mut b, &[(104.0, 0.0), (333.0, 0.0), (379.0, 39.0), (1401.0, 39.0), (1424.0, 65.0), (1424.0, 307.0), (1401.0, 331.0), (401.0, 331.0), (376.0, 310.0), (24.0, 310.0), (0.0, 286.0), (0.0, 26.0), (24.0, 0.0), (104.0, 0.0)], "black", 5.0);
        txt(&mut b, 21.0, 77.0, 22.0, true, "start", true, "black", "Alpha Strike Elements:");
        txt(&mut b, 388.0, 77.0, 22.0, true, "middle", true, "black", "Type");
        txt(&mut b, 451.0, 77.0, 22.0, true, "middle", true, "black", "Size");
        txt(&mut b, 529.0, 77.0, 22.0, true, "middle", true, "black", "Move");
        txt(&mut b, 607.0, 77.0, 22.0, true, "middle", true, "black", "Arm");
        txt(&mut b, 669.0, 77.0, 22.0, true, "middle", true, "black", "Str");
        txt(&mut b, 761.0, 77.0, 22.0, true, "middle", true, "black", "S/M/L/E");
        txt(&mut b, 853.0, 77.0, 22.0, true, "middle", true, "black", "OV");
        txt(&mut b, 915.0, 77.0, 22.0, true, "middle", true, "black", "Skill");
        txt(&mut b, 977.0, 77.0, 22.0, true, "middle", true, "black", "PV");
        txt(&mut b, 1019.0, 77.0, 22.0, true, "start", true, "black", "Element Specials");
        for row in 0..6 {
            let y = 129.0 + 32.0 * row as f32;
            for &(x1, x2) in &[(26.0, 351.0), (363.0, 414.0), (426.0, 476.0), (489.0, 570.0), (582.0, 632.0), (644.0, 694.0), (706.0, 816.0), (828.0, 878.0), (890.0, 940.0), (952.0, 1002.0), (1014.0, 1404.0)] {
                line(&mut b, x1, y, x2, y, LINE, 3.0);
            }
        }
        let label = ["One", "Two", "Three", "Four"][i];
        txtw(&mut b, 20.0, 37.0, 26.0, true, "start", false, "black", 90.0, &format!("Unit {label}:"));
        line(&mut b, 116.0, 41.0, 340.0, 41.0, LINE, 3.0);
        if let Some(us) = fs.units.get(i) {
            txtw(&mut b, 119.0, 37.0, 25.0, false, "start", false, "black", 215.0, &us.name);
            for (j, &idx) in us.elements.iter().take(6).enumerate() {
                let Some(tm) = sess.mechs.get(idx) else { continue };
                let el = element_of(tm);
                let y = 124.0 + 32.0 * j as f32;
                txtw(&mut b, 34.0, y, 21.0, false, "start", false, "black", 315.0, &tm.spec.display_name());
                txt(&mut b, 388.0, y, 21.0, false, "middle", false, "black", &el.as_type);
                txt(&mut b, 451.0, y, 21.0, false, "middle", false, "black", &el.size.to_string());
                txt(&mut b, 529.0, y, 21.0, false, "middle", false, "black", &format!("{}{}", el.primary_move, el.primary_mode));
                txt(&mut b, 607.0, y, 21.0, false, "middle", false, "black", &el.full_armor.to_string());
                txt(&mut b, 669.0, y, 21.0, false, "middle", false, "black", &el.full_structure.to_string());
                txt(&mut b, 761.0, y, 21.0, false, "middle", false, "black", &dmg_string(&el.std_damage));
                txt(&mut b, 853.0, y, 21.0, false, "middle", false, "black", &el.overheat.to_string());
                txt(&mut b, 915.0, y, 21.0, false, "middle", false, "black", &el.skill.to_string());
                txt(&mut b, 977.0, y, 21.0, false, "middle", false, "black", &el.base_pv.to_string());
                txtw(&mut b, 1019.0, y, 21.0, false, "start", false, "black", 380.0, &suas(&el.suas));
            }
        }
        b.push_str("</g>");
    }

    // ── Notice (translate 0, 1960) — the standard BattleTech record-sheet notice, verbatim as
    // MegaMek's SBFRecordSheet prints it, so the sheet reads as the real thing. ──
    let _ = write!(b, r#"<g transform="translate(0 1960)">"#);
    txt(&mut b, 717.0, 0.0, 18.0, false, "middle", false, "black", "(C) 2024 The Topps Company, Inc. BattleTech, 'Mech and BattleMech are trademarks of the Topps Company, Inc. All rights reserved.");
    txt(&mut b, 717.0, 22.0, 18.0, false, "middle", false, "black", "Catalyst Game Labs and the Catalyst Game Labs logo are trademarks of InMediaRes Production, LLC. Permission to photocopy for personal use.");
    b.push_str("</g>");

    b.push_str("</g></svg>");
    b
}

// ==== Standard BattleForce (BF) record sheet ===================================================

/// Standard BattleForce record sheets — **one page per BF Unit** (its lance of elements), plus a
/// page for any Unassigned pool elements (BF allows ungrouped single-element Units, p.51). A Unit
/// with more elements than a page's slots paginates onto a `(cont.)` page. Always blank
/// ([`make_blank`] has already reset live state), so each element prints its static AS-card stats
/// with empty armor/structure pip rows and heat track to fill in at the table.
fn bf_sheets(sess: &Session) -> Vec<String> {
    use std::collections::BTreeSet;
    const PER_PAGE: usize = 6;
    let mut sheets = Vec::new();
    let mut assigned: BTreeSet<usize> = BTreeSet::new();

    for u in &sess.bf.units {
        for &e in &u.elements {
            assigned.insert(e);
        }
        let els: Vec<usize> =
            u.elements.iter().copied().filter(|&i| i < sess.mechs.len()).collect();
        if els.is_empty() {
            continue; // an empty Unit (e.g. the default "Unit 1") has nothing to print
        }
        for (pi, chunk) in els.chunks(PER_PAGE).enumerate() {
            sheets.push(bf_unit_svg(sess, &u.name, chunk, Some(u), pi > 0));
        }
    }
    let unassigned: Vec<usize> =
        (0..sess.mechs.len()).filter(|i| !assigned.contains(i)).collect();
    for (pi, chunk) in unassigned.chunks(PER_PAGE).enumerate() {
        sheets.push(bf_unit_svg(sess, "Unassigned", chunk, None, pi > 0));
    }
    sheets
}

/// One BF Unit page: a header block (Unit Name / Move / Size / PV / Morale / Notes) over up to six
/// element cards. `unit` is `None` for the Unassigned page (no Unit-level aggregates).
fn bf_unit_svg(
    sess: &Session,
    name: &str,
    elements: &[usize],
    unit: Option<&session::BfUnitState>,
    cont: bool,
) -> String {
    use neurohelmet_core::engine::battleforce;
    let mut b = begin_sheet(&["STANDARD BATTLEFORCE", "UNIT RECORD SHEET"]);

    // ── Unit header block ──
    stroke_poly(&mut b, &[(104.0, 167.0), (1401.0, 167.0), (1424.0, 193.0), (1424.0, 276.0), (1401.0, 302.0), (24.0, 302.0), (0.0, 276.0), (0.0, 193.0), (24.0, 167.0), (104.0, 167.0)], "black", 5.0);
    fill_poly(&mut b, &[(11.0, 198.0), (30.0, 179.0), (174.0, 179.0), (193.0, 198.0), (174.0, 217.0), (30.0, 217.0), (11.0, 198.0)], "black");
    txtw(&mut b, 102.0, 198.0, 30.0, true, "middle", true, "white", 140.0, "UNIT:");
    for (x, lbl) in [(660.0, "Move"), (760.0, "Size"), (850.0, "PV"), (960.0, "Morale")] {
        txt(&mut b, x, 210.0, 22.0, true, "middle", true, "black", lbl);
    }
    txt(&mut b, 1070.0, 210.0, 22.0, true, "start", true, "black", "Notes");
    let title = if cont { format!("{name} (cont.)") } else { name.to_string() };
    txtw(&mut b, 210.0, 260.0, 26.0, false, "start", true, "black", 420.0, &title);
    if let Some(u) = unit {
        let members = bf_member_stats(sess, &u.elements);
        let (mv, jump) = battleforce::bf_unit_mv(&members);
        let mv_str = match jump {
            Some(j) => format!("{mv} (j{j})"),
            None => mv.to_string(),
        };
        let pv: u64 = u
            .elements
            .iter()
            .filter_map(|&i| sess.mechs.get(i))
            .map(|tm| tm.point_cost(GameMode::BattleForce))
            .sum();
        txt(&mut b, 660.0, 260.0, 24.0, false, "middle", true, "black", &mv_str);
        txt(&mut b, 760.0, 260.0, 24.0, false, "middle", true, "black", &u.size.to_string());
        txt(&mut b, 850.0, 260.0, 24.0, false, "middle", true, "black", &pv.to_string());
        txt(&mut b, 960.0, 260.0, 24.0, false, "middle", true, "black", u.morale.label());
        txtw(&mut b, 1070.0, 260.0, 22.0, false, "start", true, "black", 330.0, &u.notes);
    }

    // ── Element cards ──
    for (i, &idx) in elements.iter().enumerate() {
        let Some(tm) = sess.mechs.get(idx) else { continue };
        bf_element_card(&mut b, tm, 355.0 + 262.0 * i as f32);
    }

    end_sheet(&mut b);
    b
}

/// One BF element card at vertical offset `yoff` (sheet space): the static AS-card stat line, the
/// four-bracket damage row, blank Armor/Structure pip rows and Heat track, and the Specials line.
fn bf_element_card(b: &mut String, tm: &TrackedMech, yoff: f32) {
    use neurohelmet_core::engine::alpha_strike::movement_hexes;
    use neurohelmet_core::engine::battleforce::bf_is_aero;
    let a = &tm.spec.as_stats;
    let el = element_of(tm);
    let aero = bf_is_aero(&el);
    let _ = write!(b, r#"<g transform="translate(0 {yoff:.1})">"#);

    // Card outline.
    stroke_poly(b, &[(0.0, 0.0), (1435.0, 0.0), (1435.0, 240.0), (0.0, 240.0), (0.0, 0.0)], "black", 3.0);

    // Row 1: name + stat cells.
    txtw(b, 18.0, 40.0, 27.0, true, "start", true, "black", 470.0, &tm.spec.display_name());
    let tmm_lbl = if aero { "TH" } else { "TMM" };
    let tmm_val = if aero { a.threshold.to_string() } else { a.tmm.to_string() };
    let cells = [
        (560.0, "Type", el.as_type.clone()),
        (645.0, "Size", if a.size == 0 { "-".into() } else { a.size.to_string() }),
        (740.0, "MV", movement_hexes(&a.movement)),
        (880.0, tmm_lbl, tmm_val),
        (965.0, "OV", a.overheat.to_string()),
        (1045.0, "Skill", format!("{}+", tm.gunnery)),
        (1130.0, "PV", a.pv.to_string()),
    ];
    for (x, lbl, val) in &cells {
        txt(b, *x, 22.0, 18.0, true, "middle", true, "black", lbl);
        txtw(b, *x, 52.0, 24.0, false, "middle", true, "black", 90.0, val);
    }
    // Destroyed checkbox (top-right corner).
    let _ = write!(b, r#"<rect x="1240" y="18" width="26" height="26" fill="none" stroke="black" stroke-width="3"/>"#);
    txt(b, 1278.0, 37.0, 20.0, true, "start", true, "black", "DESTROYED");

    // Row 2: damage brackets S(+0) M(+2) L(+4) E(+6).
    txt(b, 18.0, 100.0, 22.0, true, "start", true, "black", "Damage");
    let dv = &el.std_damage;
    let dmg_cells = [
        (300.0, "S (+0)", num(dv.s)),
        (470.0, "M (+2)", num(dv.m)),
        (640.0, "L (+4)", opt_num(dv.l)),
        (810.0, "E (+6)", opt_num(dv.e)),
    ];
    for (x, lbl, val) in &dmg_cells {
        txt(b, *x, 92.0, 20.0, true, "middle", true, "black", lbl);
        txt(b, *x, 122.0, 24.0, false, "middle", true, "black", val);
    }

    // Rows 3–4: Armor / Structure pip rows (blank circles to strike off).
    txt(b, 18.0, 158.0, 22.0, true, "start", true, "black", "Armor");
    pips(b, 200.0, 152.0, u16::from(a.armor), 40);
    txt(b, 18.0, 195.0, 22.0, true, "start", true, "black", "Struct");
    pips(b, 200.0, 189.0, u16::from(a.structure), 40);

    // Row 5: heat track + specials.
    txt(b, 18.0, 226.0, 20.0, true, "start", true, "black", "Heat");
    for (j, lbl) in ["1", "2", "3", "S"].iter().enumerate() {
        let x = 110.0 + 46.0 * j as f32;
        let _ = write!(b, r#"<rect x="{x:.1}" y="212" width="30" height="24" fill="none" stroke="black" stroke-width="2"/>"#);
        txt(b, x + 15.0, 226.0, 17.0, false, "middle", true, "black", lbl);
    }
    let specials = suas(&el.suas);
    if !specials.is_empty() {
        txt(b, 360.0, 226.0, 20.0, true, "start", true, "black", "Specials:");
        txtw(b, 480.0, 226.0, 20.0, false, "start", true, "black", 930.0, &specials);
    }

    b.push_str("</g>");
}

/// The `(ground-hex, jump-hex, alive)` member tuples [`battleforce::bf_unit_mv`] needs, built from
/// the pool for the always-blank sheet (every element alive, no heat/crit degradation).
fn bf_member_stats(sess: &Session, elements: &[usize]) -> Vec<(u32, Option<u32>, bool)> {
    use neurohelmet_core::engine::alpha_strike::inches_to_hexes;
    use neurohelmet_core::engine::battleforce::bf_is_aero;
    elements
        .iter()
        .filter_map(|&i| sess.mechs.get(i))
        .map(|tm| {
            let el = element_of(tm);
            let ground = if bf_is_aero(&el) { el.primary_move } else { inches_to_hexes(el.primary_move) };
            let jump = (el.jump_move > 0).then(|| inches_to_hexes(el.jump_move));
            (ground, jump, true)
        })
        .collect()
}

/// Draw `n` blank pip circles from (x, y), wrapping every `per_row`. Used for BF armor/structure.
fn pips(b: &mut String, x: f32, y: f32, n: u16, per_row: u16) {
    const R: f32 = 8.0;
    const GAP: f32 = 22.0;
    for i in 0..n {
        let col = (i % per_row) as f32;
        let row = (i / per_row) as f32;
        let cx = x + col * GAP + R;
        let cy = y + row * GAP + R;
        let _ = write!(
            b,
            r#"<circle cx="{cx:.1}" cy="{cy:.1}" r="{R}" fill="none" stroke="black" stroke-width="2"/>"#
        );
    }
}

// ==== Abstract Combat System (ACS) record sheets ===============================================

/// ACS record sheets — one **Combat Unit** sheet per `AcsCombatUnitState` (p.18: the Armor pool +
/// damage line + Morale-Check triggers + its Combat-Teams summary), followed by a single **Formation
/// Tracking** sheet (p.19) for the whole force. Ground-only v1. Always blank.
fn acs_sheets(sess: &Session) -> Vec<String> {
    let mut sheets = Vec::new();
    for f in &sess.acs.formations {
        for cu in &f.units {
            sheets.push(acs_combat_unit_svg(sess, &f.name, cu));
        }
    }
    if sess.acs.formations.iter().any(|f| !f.units.is_empty()) {
        sheets.push(acs_formation_tracking_svg(sess));
    }
    sheets
}

/// One ACS Combat Unit record sheet (p.18).
fn acs_combat_unit_svg(sess: &Session, formation: &str, cu: &session::AcsCombatUnitState) -> String {
    let d = sess.acs_combat_unit(cu);
    let mut b = begin_sheet(&["ABSTRACT COMBAT SYSTEM", "COMBAT UNIT RECORD SHEET"]);

    // ── Combat Unit header block ──
    stroke_poly(&mut b, &[(104.0, 167.0), (1401.0, 167.0), (1424.0, 193.0), (1424.0, 355.0), (1401.0, 381.0), (24.0, 381.0), (0.0, 355.0), (0.0, 193.0), (24.0, 167.0), (104.0, 167.0)], "black", 5.0);
    fill_poly(&mut b, &[(11.0, 198.0), (30.0, 179.0), (280.0, 179.0), (299.0, 198.0), (280.0, 217.0), (30.0, 217.0), (11.0, 198.0)], "black");
    txtw(&mut b, 155.0, 198.0, 28.0, true, "middle", true, "white", 240.0, "COMBAT UNIT:");
    txtw(&mut b, 320.0, 200.0, 26.0, false, "start", true, "black", 500.0, &cu.name);
    txtw(&mut b, 900.0, 200.0, 20.0, false, "start", true, "black", 500.0, &format!("Formation: {formation}"));

    // Stat grid (two rows of labelled cells).
    let stat_cells = [
        (60.0, "Type", type_label(&format!("{:?}", d.acs_type))),
        (150.0, "Size", d.size.to_string()),
        (250.0, "Move", format!("{}{}", d.movement, d.move_mode.code())),
        (380.0, "TranspMP", format!("{}{}", d.trsp_movement, d.trsp_mode.code())),
        (510.0, "TMM", d.tmm.to_string()),
        (600.0, "ARM", d.armor.to_string()),
        (710.0, "S/M/L/E", dmg_string(&d.damage)),
        (860.0, "Tactics", d.tactics.to_string()),
        (960.0, "Morale", d.morale_rating.to_string()),
        (1060.0, "Skill", d.skill.to_string()),
        (1150.0, "PV", d.point_value.to_string()),
    ];
    for (x, lbl, val) in &stat_cells {
        txt(&mut b, *x, 250.0, 20.0, true, "middle", true, "black", lbl);
        txtw(&mut b, *x, 282.0, 24.0, false, "middle", true, "black", 110.0, val);
    }
    // Specials + Morale-check triggers.
    txt(&mut b, 24.0, 330.0, 20.0, true, "start", true, "black", "Specials:");
    txtw(&mut b, 140.0, 330.0, 20.0, false, "start", true, "black", 620.0, &suas(&d.suas));
    txt(&mut b, 800.0, 330.0, 20.0, true, "start", true, "black", "Morale Check Triggers (Armor):");
    let [t75, t50, t25] = d.damage_thresholds;
    txtw(&mut b, 1180.0, 330.0, 20.0, false, "start", true, "black", 230.0, &format!("75%: {t75}    50%: {t50}    25%: {t25}"));

    // ── Combat Teams summary ──
    let _ = write!(b, r#"<g transform="translate(0 420)">"#);
    stroke_poly(&mut b, &[(104.0, 0.0), (1401.0, 0.0), (1424.0, 26.0), (1424.0, 470.0), (1401.0, 496.0), (24.0, 496.0), (0.0, 470.0), (0.0, 26.0), (24.0, 0.0), (104.0, 0.0)], "black", 5.0);
    fill_poly(&mut b, &[(11.0, 31.0), (30.0, 12.0), (300.0, 12.0), (319.0, 31.0), (300.0, 50.0), (30.0, 50.0), (11.0, 31.0)], "black");
    txtw(&mut b, 165.0, 31.0, 26.0, true, "middle", true, "white", 260.0, "COMBAT TEAMS:");
    let hdrs = [
        (360.0, "Type"), (430.0, "Size"), (510.0, "Move"), (600.0, "Jump"), (680.0, "Trsp"),
        (760.0, "TMM"), (830.0, "Arm"), (930.0, "S/M/L/E"), (1050.0, "Skill"), (1130.0, "PV"),
        (1190.0, "Specials"),
    ];
    for (x, lbl) in hdrs {
        let anchor = if lbl == "Specials" { "start" } else { "middle" };
        txt(&mut b, x, 90.0, 20.0, true, anchor, true, "black", lbl);
    }
    for (i, team) in d.teams.iter().take(8).enumerate() {
        let y = 140.0 + 40.0 * i as f32;
        txtw(&mut b, 30.0, y, 22.0, false, "start", true, "black", 320.0, &team.name);
        txt(&mut b, 360.0, y, 22.0, false, "middle", true, "black", &type_label(&format!("{:?}", team.acs_type)));
        txt(&mut b, 430.0, y, 22.0, false, "middle", true, "black", &team.size.to_string());
        txt(&mut b, 510.0, y, 22.0, false, "middle", true, "black", &format!("{}{}", team.movement, team.move_mode.code()));
        txt(&mut b, 600.0, y, 22.0, false, "middle", true, "black", &team.jump_move.to_string());
        txt(&mut b, 680.0, y, 22.0, false, "middle", true, "black", &format!("{}{}", team.trsp_movement, team.trsp_mode.code()));
        txt(&mut b, 760.0, y, 22.0, false, "middle", true, "black", &team.tmm.to_string());
        txt(&mut b, 830.0, y, 22.0, false, "middle", true, "black", &team.armor.to_string());
        txt(&mut b, 930.0, y, 22.0, false, "middle", true, "black", &dmg_string(&team.damage));
        txt(&mut b, 1050.0, y, 22.0, false, "middle", true, "black", &team.skill.to_string());
        txt(&mut b, 1130.0, y, 22.0, false, "middle", true, "black", &team.point_value.to_string());
        txtw(&mut b, 1180.0, y, 22.0, false, "start", true, "black", 240.0, &suas(&team.suas));
    }
    b.push_str("</g>");

    // ── Live-tracking aids (blank boxes): Fatigue, Morale rung, COM/LEAD ──
    let _ = write!(b, r#"<g transform="translate(0 960)">"#);
    txt(&mut b, 24.0, 30.0, 22.0, true, "start", true, "black", "Fatigue (FP):");
    line(&mut b, 220.0, 34.0, 520.0, 34.0, LINE, 3.0);
    txt(&mut b, 560.0, 30.0, 22.0, true, "start", true, "black", "Morale:");
    for (j, lbl) in ["Normal", "Shaken", "Unsteady", "Broken", "Routed"].iter().enumerate() {
        let x = 700.0 + 150.0 * j as f32;
        let _ = write!(b, r#"<rect x="{x:.1}" y="14" width="20" height="20" fill="none" stroke="black" stroke-width="2"/>"#);
        txt(&mut b, x + 28.0, 30.0, 18.0, false, "start", true, "black", lbl);
    }
    for (j, lbl) in ["Force Commander (COM)", "Formation Leader (LEAD)"].iter().enumerate() {
        let y = 90.0 + 40.0 * j as f32;
        let _ = write!(b, r#"<rect x="24" y="{:.1}" width="22" height="22" fill="none" stroke="black" stroke-width="2"/>"#, y - 16.0);
        txt(&mut b, 60.0, y, 20.0, false, "start", true, "black", lbl);
    }
    b.push_str("</g>");

    end_sheet(&mut b);
    b
}

/// The ACS Formation Tracking sheet (p.19): a box per Formation with its ID / Name / Type / Move /
/// Tactics / Morale / Skill and a list of its Combat Units, plus force-level Round / PV / Leadership.
fn acs_formation_tracking_svg(sess: &Session) -> String {
    let mut b = begin_sheet(&["ABSTRACT COMBAT SYSTEM", "FORMATION TRACKING SHEET"]);

    // Force line.
    txt(&mut b, 24.0, 200.0, 24.0, true, "start", true, "black", "FORCE:");
    let force = format!(
        "Round ___    Force PV {}    Leadership {}",
        sess.acs_force_pv(),
        sess.acs.leadership_rating
    );
    txt(&mut b, 160.0, 200.0, 22.0, false, "start", true, "black", &force);

    // Two columns of formation boxes (empty formations — e.g. the default "Formation 1" — omitted).
    const BOX_W: f32 = 690.0;
    const BOX_H: f32 = 232.0;
    let formations: Vec<&session::AcsFormationState> =
        sess.acs.formations.iter().filter(|f| !f.units.is_empty()).collect();
    for (i, f) in formations.into_iter().enumerate() {
        let d = sess.acs_formation(f);
        let col = (i % 2) as f32;
        let row = (i / 2) as f32;
        if row >= 7.0 {
            break; // 2×7 grid; overflow would need another sheet (rare).
        }
        let x = 10.0 + col * (BOX_W + 25.0);
        let y = 240.0 + row * (BOX_H + 12.0);
        let _ = write!(b, r#"<g transform="translate({x:.1} {y:.1})">"#);
        stroke_poly(&mut b, &[(0.0, 0.0), (BOX_W, 0.0), (BOX_W, BOX_H), (0.0, BOX_H), (0.0, 0.0)], "black", 3.0);
        txt(&mut b, 16.0, 34.0, 20.0, true, "start", true, "black", &format!("#{}", i + 1));
        txtw(&mut b, 70.0, 34.0, 24.0, true, "start", true, "black", BOX_W - 90.0, &f.name);
        let meta = format!(
            "Type {}   Move {}   Tactics {}   Skill {}   Morale {}",
            type_label(&format!("{:?}", d.acs_type)),
            d.movement,
            d.tactics,
            d.skill,
            d.morale_rating,
        );
        txtw(&mut b, 16.0, 74.0, 20.0, false, "start", true, "black", BOX_W - 32.0, &meta);
        txt(&mut b, 16.0, 108.0, 20.0, true, "start", true, "black", "Combat Units:");
        for (j, cu) in f.units.iter().take(4).enumerate() {
            let cd = sess.acs_combat_unit(cu);
            let ly = 140.0 + 28.0 * j as f32;
            let mut tags = Vec::new();
            if cu.is_commander {
                tags.push("COM");
            }
            if cu.is_leader {
                tags.push("LEAD");
            }
            let tag = if tags.is_empty() { String::new() } else { format!("  [{}]", tags.join(" ")) };
            txtw(&mut b, 30.0, ly, 19.0, false, "start", true, "black", BOX_W - 60.0, &format!("• {} (ARM {}, PV {}){tag}", cu.name, cd.armor, cd.point_value));
        }
        b.push_str("</g>");
    }

    end_sheet(&mut b);
    b
}

// ---- small SVG helpers ----------------------------------------------------------------------

/// SBF element-type label, e.g. `"BM"` / `"AS"` / `"V"` — matches the TUI's `{:?}`-uppercased form.
fn type_label(debug: &str) -> String {
    debug.to_uppercase()
}

/// Space-joined SUA keys.
fn suas(map: &std::collections::BTreeMap<String, neurohelmet_core::engine::as_element::SuaVal>) -> String {
    map.keys().cloned().collect::<Vec<_>>().join(" ")
}

/// Format an SBF damage value: `0.5` (minimal) → `"0*"`, whole → integer, else one decimal.
fn num(f: f32) -> String {
    if (f - 0.5).abs() < 0.05 {
        "0*".to_string()
    } else if f.fract().abs() < 0.05 {
        format!("{}", f.round() as i64)
    } else {
        format!("{f:.1}")
    }
}

fn opt_num(o: Option<f32>) -> String {
    o.map(num).unwrap_or_else(|| "—".to_string())
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// The typed AS element behind a pool member (mirrors `app::bf_element_of`).
fn element_of(tm: &TrackedMech) -> AsElement {
    as_element::as_element(&tm.spec.as_stats, &tm.spec.display_name(), tm.gunnery)
}

/// SBF damage as `S/M/L` (+`/E` when an Extreme band is present).
fn dmg_string(dv: &DamageVector) -> String {
    let mut s = format!("{}/{}/{}", num(dv.s), num(dv.m), opt_num(dv.l));
    if let Some(e) = dv.e {
        if e > 0.0 {
            s.push('/');
            s.push_str(&num(e));
        }
    }
    s
}

fn points(pts: &[(f32, f32)]) -> String {
    pts.iter().map(|(x, y)| format!("{x:.1},{y:.1}")).collect::<Vec<_>>().join(" ")
}

/// Standard base64 (with padding), to inline the PNG logos as `data:` URIs.
fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = ((c[0] as u32) << 16)
            | ((*c.get(1).unwrap_or(&0) as u32) << 8)
            | (*c.get(2).unwrap_or(&0) as u32);
        out.push(T[(n >> 18 & 63) as usize] as char);
        out.push(T[(n >> 12 & 63) as usize] as char);
        out.push(if c.len() > 1 { T[(n >> 6 & 63) as usize] as char } else { '=' });
        out.push(if c.len() > 2 { T[(n & 63) as usize] as char } else { '=' });
    }
    out
}

/// Embed a PNG in sheet space as a base64 data URI.
fn image(b: &mut String, x: f32, y: f32, w: f32, h: f32, png: &[u8]) {
    let _ = write!(
        b,
        r#"<image x="{x:.1}" y="{y:.1}" width="{w:.1}" height="{h:.1}" xlink:href="data:image/png;base64,{}"/>"#,
        base64(png)
    );
}

/// A filled polygon in sheet space.
fn fill_poly(b: &mut String, pts: &[(f32, f32)], fill: &str) {
    let _ = write!(b, r#"<polygon points="{}" fill="{fill}"/>"#, points(pts));
}

/// A stroked, unfilled outline (MegaMek's `drawPolyline`).
fn stroke_poly(b: &mut String, pts: &[(f32, f32)], color: &str, width: f32) {
    let _ = write!(
        b,
        r#"<polyline points="{}" fill="none" stroke="{color}" stroke-width="{width}"/>"#,
        points(pts)
    );
}

fn line(b: &mut String, x1: f32, y1: f32, x2: f32, y2: f32, color: &str, width: f32) {
    let _ = write!(
        b,
        r#"<line x1="{x1:.1}" y1="{y1:.1}" x2="{x2:.1}" y2="{y2:.1}" stroke="{color}" stroke-width="{width}"/>"#
    );
}

/// Text in sheet space. `anchor` ∈ {"start","middle","end"}; `vc` vertically centers on `y`.
#[allow(clippy::too_many_arguments)]
fn txt(b: &mut String, x: f32, y: f32, size: f32, bold: bool, anchor: &str, vc: bool, fill: &str, s: &str) {
    txtw(b, x, y, size, bold, anchor, vc, fill, 0.0, s);
}

/// Like [`txt`] but shrinks the font so the text fits within `maxw` sheet units (approximating
/// MegaMek's `StringDrawer.maxWidth`), so long names/specials don't overflow their column.
#[allow(clippy::too_many_arguments)]
fn txtw(b: &mut String, x: f32, y: f32, size: f32, bold: bool, anchor: &str, vc: bool, fill: &str, maxw: f32, s: &str) {
    if s.is_empty() {
        return;
    }
    let est = s.chars().count() as f32 * size * 0.60;
    let size = if maxw > 0.0 && est > maxw { (size * maxw / est).max(6.0) } else { size };
    let yb = if vc { y + size * 0.35 } else { y };
    let weight = if bold { r#" font-weight="bold""# } else { "" };
    let _ = write!(
        b,
        r#"<text x="{x:.1}" y="{yb:.1}" text-anchor="{anchor}" font-family="{FONT_FAMILY}" font-size="{size:.1}" fill="{fill}"{weight}>{}</text>"#,
        esc(s)
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use neurohelmet_core::session::SbfUnitState;

    #[test]
    fn renders_valid_sbf_svg() {
        let sess = Session::new_with_mode(GameMode::StrategicBattleForce);
        let svg = sbf_formation_svg(&sess, &sess.sbf.formations[0]);
        assert!(svg.contains("STRATEGIC BATTLEFORCE"));
        assert!(svg.contains("</svg>"));
        // Must be valid SVG that usvg accepts.
        assert!(
            svg2pdf::usvg::Tree::from_str(&svg, &svg_options()).is_ok(),
            "generated SVG must parse"
        );
    }

    #[test]
    fn multipage_pdf_has_a_page_per_formation() {
        let mut sess = Session::new_with_mode(GameMode::StrategicBattleForce);
        sess.sbf
            .formations
            .push(SbfFormationState { name: "Formation 2".into(), ..Default::default() });
        let pdf = render_session_pdf(&sess).unwrap();
        assert!(pdf.starts_with(b"%PDF"), "output must be a PDF");
        let pages = String::from_utf8_lossy(&pdf).matches("/MediaBox").count();
        assert_eq!(pages, 2, "one page per formation");
    }

    #[test]
    fn make_blank_zeroes_sbf_state() {
        let mut sess = Session::new_with_mode(GameMode::StrategicBattleForce);
        sess.sbf.round = 5;
        let f = &mut sess.sbf.formations[0];
        f.morale = MoraleStatus::Broken;
        f.units.push(neurohelmet_core::session::SbfUnitState {
            name: "Alpha".into(),
            armor_hits: 7,
            damage_crits: 2,
            targeting_crits: 1,
            mp_crits: 3,
            is_commander: true,
            ..Default::default()
        });
        make_blank(&mut sess);
        assert_eq!(sess.sbf.round, 0);
        let f = &sess.sbf.formations[0];
        assert_eq!(f.morale, MoraleStatus::Normal);
        let u = &f.units[0];
        assert_eq!((u.armor_hits, u.damage_crits, u.targeting_crits, u.mp_crits), (0, 0, 0, 0));
        assert!(u.is_commander, "COM/LEAD designations are preserved");
    }

    #[test]
    fn populated_formation_renders_units() {
        // Real data: build an SBF Unit from baked elements and confirm the card path renders.
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/mechs.bin");
        let bundle = neurohelmet_core::data::bundle::Bundle::load(&path).expect("load baked bundle");
        let mut sess = Session::new_with_mode(GameMode::StrategicBattleForce);
        for i in 0..3 {
            if let Some(m) = bundle.mechs.get(i).cloned() {
                sess.add_mech(m);
            }
        }
        sess.sbf.formations[0].name = "Lance Command".into();
        sess.sbf.formations[0].units.push(neurohelmet_core::session::SbfUnitState {
            name: "Test Lance".into(),
            elements: (0..sess.mechs.len()).collect(),
            is_commander: true,
            ..Default::default()
        });

        let svg = sbf_formation_svg(&sess, &sess.sbf.formations[0]);
        assert!(svg.contains("Lance Command"), "formation name on the sheet");
        assert!(svg.contains("Test Lance"), "unit name on the sheet");
        assert!(svg.contains("COM"), "commander designation rendered in Specials");
        assert!(svg.contains("Alpha Strike Elements:"), "element sub-block rendered");

        let pdf = render_session_pdf(&sess).unwrap();
        assert!(pdf.starts_with(b"%PDF") && pdf.len() > 1500, "non-trivial PDF: {} bytes", pdf.len());

        // Manual-inspection hatch: `PDF_DUMP=/path cargo test populated_formation_renders_units`.
        if let Ok(p) = std::env::var("PDF_DUMP") {
            std::fs::write(p, &pdf).unwrap();
        }
    }

    /// A realistic two-company battalion (7 lances of 4 BattleMechs): a full 4-Unit formation plus
    /// a 3-Unit one. Doubles as a preview generator — set `PDF_PREVIEW_DIR=/dir` to dump
    /// `sbf-sheet.pdf` for eyeballing.
    #[test]
    fn preview_realistic_sheets() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/mechs.bin");
        let bundle = neurohelmet_core::data::bundle::Bundle::load(&path).expect("load baked bundle");
        let mut sess = Session::new_with_mode(GameMode::StrategicBattleForce);

        // 28 real BattleMechs → 7 lances of 4.
        let bm: Vec<usize> = bundle
            .mechs
            .iter()
            .enumerate()
            .filter(|(_, m)| m.as_stats.tp == "BM")
            .map(|(i, _)| i)
            .take(28)
            .collect();
        assert!(bm.len() >= 28, "need 28 'Mechs in the bundle");
        for &i in &bm {
            sess.add_mech(bundle.mechs[i].clone());
        }
        // Pool indices 0..28; a lance is 4 consecutive elements.
        let lance = |n: usize| -> Vec<usize> { (n * 4..n * 4 + 4).collect() };

        // 1st Company — the force's command formation (4 lances); the command lance carries the
        // force commander (COM) and the formation leader (LEAD).
        {
            let f = &mut sess.sbf.formations[0];
            f.name = "1st Company (Command)".into();
            f.units = vec![
                SbfUnitState { name: "Command Lance".into(), elements: lance(0), is_commander: true, is_leader: true, ..Default::default() },
                SbfUnitState { name: "Battle Lance".into(), elements: lance(1), ..Default::default() },
                SbfUnitState { name: "Fire Lance".into(), elements: lance(2), ..Default::default() },
                SbfUnitState { name: "Recon Lance".into(), elements: lance(3), ..Default::default() },
            ];
        }
        // 2nd Company — 3 lances.
        sess.sbf.formations.push(SbfFormationState {
            name: "2nd Company".into(),
            units: vec![
                SbfUnitState { name: "Assault Lance".into(), elements: lance(4), is_leader: true, ..Default::default() },
                SbfUnitState { name: "Striker Lance".into(), elements: lance(5), ..Default::default() },
                SbfUnitState { name: "Pursuit Lance".into(), elements: lance(6), ..Default::default() },
            ],
            ..Default::default()
        });

        let pdf = render_session_pdf(&sess).unwrap();
        assert!(pdf.starts_with(b"%PDF"));
        assert_eq!(
            String::from_utf8_lossy(&pdf).matches("/MediaBox").count(),
            2,
            "two companies → two pages"
        );
        if let Ok(dir) = std::env::var("PDF_PREVIEW_DIR") {
            std::fs::write(format!("{dir}/sbf-sheet.pdf"), &pdf).unwrap();
        }
    }

    /// Load N real 'Mechs of a given AS type into a session of `mode`.
    fn seed(mode: GameMode, n: usize) -> Session {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/mechs.bin");
        let bundle = neurohelmet_core::data::bundle::Bundle::load(&path).expect("load baked bundle");
        let mut sess = Session::new_with_mode(mode);
        for m in bundle.mechs.iter().filter(|m| m.as_stats.tp == "BM").take(n).cloned() {
            sess.add_mech(m);
        }
        sess
    }

    #[test]
    fn bf_unit_sheet_renders() {
        let mut sess = seed(GameMode::BattleForce, 4);
        let ui = sess.bf_new_unit("Battle Lance", 0..4);
        sess.bf.units[ui].notes = "hold the ridge".into();
        let svgs = bf_sheets(&sess);
        assert_eq!(svgs.len(), 1, "one Unit → one page");
        assert!(svgs[0].contains("STANDARD BATTLEFORCE"));
        assert!(svgs[0].contains("Battle Lance"));
        assert!(svgs[0].contains("Armor") && svgs[0].contains("Heat"));
        assert!(svg2pdf::usvg::Tree::from_str(&svgs[0], &svg_options()).is_ok(), "valid SVG");

        let pdf = render_session_pdf(&sess).unwrap();
        assert!(pdf.starts_with(b"%PDF") && pdf.len() > 1500);
        if let Ok(dir) = std::env::var("PDF_PREVIEW_DIR") {
            std::fs::write(format!("{dir}/bf-sheet.pdf"), &pdf).unwrap();
        }
    }

    #[test]
    fn bf_unassigned_elements_get_a_page() {
        // Pool 'Mechs with no BF Unit fall onto the "Unassigned" page.
        let sess = seed(GameMode::BattleForce, 2);
        let svgs = bf_sheets(&sess);
        assert_eq!(svgs.len(), 1);
        assert!(svgs[0].contains("Unassigned"));
    }

    #[test]
    fn bf_large_unit_paginates() {
        // A Unit with more than a page's element slots continues onto a `(cont.)` page.
        let mut sess = seed(GameMode::BattleForce, 8);
        sess.bf_new_unit("Big Company", 0..8);
        let svgs = bf_sheets(&sess);
        assert_eq!(svgs.len(), 2, "8 elements over 6 slots → 2 pages");
        assert!(svgs[1].contains("(cont.)"));
    }

    #[test]
    fn acs_sheets_render() {
        let mut sess = seed(GameMode::AbstractCombatSystem, 8);
        // Build one Formation → one Combat Unit → one Combat Team → two SBF Units of 4 elements.
        let fi = sess.acs_new_formation("Assault Formation", 0..8);
        assert!(!sess.acs.formations[fi].units.is_empty(), "formation seeded a combat unit");
        sess.acs.leadership_rating = 5;

        let svgs = acs_sheets(&sess);
        assert!(svgs.len() >= 2, "at least one Combat Unit sheet + a Formation Tracking sheet");
        assert!(svgs[0].contains("COMBAT UNIT RECORD SHEET"));
        assert!(svgs[0].contains("Morale Check Triggers"));
        assert!(svgs.last().unwrap().contains("FORMATION TRACKING SHEET"));
        assert!(svgs.last().unwrap().contains("Assault Formation"));
        for s in &svgs {
            assert!(svg2pdf::usvg::Tree::from_str(s, &svg_options()).is_ok(), "valid SVG");
        }

        let pdf = render_session_pdf(&sess).unwrap();
        assert!(pdf.starts_with(b"%PDF") && pdf.len() > 1500);
        if let Ok(dir) = std::env::var("PDF_PREVIEW_DIR") {
            std::fs::write(format!("{dir}/acs-sheets.pdf"), &pdf).unwrap();
        }
    }

    #[test]
    fn make_blank_zeroes_bf_and_acs_state() {
        let mut sess = seed(GameMode::AbstractCombatSystem, 4);
        let fi = sess.acs_new_formation("F", 0..4);
        sess.acs.round = 4;
        sess.acs.formations[fi].units[0].armor_hits = 9;
        sess.acs.formations[fi].units[0].fatigue_points_x2 = 6;
        sess.mechs[0].as_armor_hits = 3;
        make_blank(&mut sess);
        assert_eq!(sess.acs.round, 0);
        assert_eq!(sess.acs.formations[fi].units[0].armor_hits, 0);
        assert_eq!(sess.acs.formations[fi].units[0].fatigue_points_x2, 0);
        assert_eq!(sess.mechs[0].as_armor_hits, 0);
    }
}
