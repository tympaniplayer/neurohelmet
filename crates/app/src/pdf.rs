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
//! Phase 1 + Strategic BattleForce. We build **neurohelmet's own** vector record sheet as an SVG
//! (no CGL/BattleTech artwork — the official sheets were a layout reference only) and convert it to
//! a US-Letter PDF with `svg2pdf`. The SVG is generated programmatically because a formation holds a
//! variable 1–4 Units; every value comes from the same derived stats the TUI shows
//! (`Session::sbf_formation` / `sbf_unit` + the `SbfUnitState` live-state accessors), so the sheet
//! matches the screen. Two variants: the live current-state sheet (default) and a pristine
//! `--blank` fill-in form ([`make_blank`]).
//!
//! One PDF per formation is written into the output dir (mirroring `--export`); combining them into a
//! single multi-page "record-sheet book" is the follow-up (spec Open Q5), as are the BF and ACS
//! sheets and per-element sub-rows.

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
    println!("Wrote SBF record sheet for '{}' → {}", name, path.display());
    Ok(())
}

/// Render `sess`'s SBF formations to one multi-page PDF and write it to `out` (default
/// `<sessions_dir>/<name>-sheets.pdf`). Shared by the `--pdf` CLI verb and the in-app `P` key;
/// returns the written path.
pub(crate) fn export_session(
    sess: &Session,
    name: &str,
    out: Option<PathBuf>,
) -> color_eyre::Result<PathBuf> {
    if sess.mode != GameMode::StrategicBattleForce {
        color_eyre::eyre::bail!(
            "PDF export currently supports Strategic BattleForce sessions only (session is {:?}).",
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
    let svgs: Vec<String> =
        sess.sbf.formations.iter().map(|fs| sbf_formation_svg(&sess, fs)).collect();
    if svgs.is_empty() {
        color_eyre::eyre::bail!("session has no SBF formations to export");
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
}
