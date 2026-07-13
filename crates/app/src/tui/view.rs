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

//! Rendering: the unit picker and the tracker (box-grid paper doll, heat panel, equipment).

use super::app::{
    bf_element_of, bf_kind_eligible, bf_kind_label, App, BfShotUi, DiceTab, EquipRow, Focus,
    ForceGen, Modal, Screen,
};
use super::filters::Facet;
use super::forcegen;
use super::icons;
use super::profile::{profile, DisplayProfile};
use super::theme::{theme, THEMES};
use neurohelmet_core::domain::{
    Facing, GameMode, Location, Mech, Rarity, UnitType, WeaponMount, STANDARD_MUNITION,
};
use neurohelmet_core::engine::acs::{
    acs_aero_range, acs_aero_to_hit, acs_bomb_clusters, acs_combat_drop_result, acs_damage,
    acs_damage_band, acs_fatigue_band, acs_ground_strike_damage, acs_morale_tn,
    acs_orbit_to_surface_primary, acs_orbit_to_surface_secondary, acs_to_hit, AcsCombatUnit,
    AcsDamageBand, AcsDamageCtx, AcsExperience, AcsFatigueBand, AcsMorale, AcsMoraleCtx, AcsRange,
    ACS_AERIAL_RECON_MOD, ACS_CAP_ENGAGEMENT_MOD, ACS_GROUND_STRIKE_TOHIT,
};
use neurohelmet_core::engine::as_element::SbfElementType;
use neurohelmet_core::engine::battleforce::{
    self, BfA2G, BfAeroAngle, BfAttackKind, BfMotive, BfMove, BfPhysical, BfRange, BfTargetKind,
    BfTargetMove,
};
use neurohelmet_core::engine::large_craft;
use neurohelmet_core::engine::override_conv::{self, ArmorRegion, OverrideCard};
use neurohelmet_core::engine::sbf::{
    self as sbf_engine, SbfA2G, SbfAeroKind, SbfAeroTarget, SbfRange,
};
use neurohelmet_core::engine::{
    as_to_hit_full, cluster_hits, gator_to_hit, inches_to_hexes, infantry_max_range,
    infantry_range_mod, mech_hit_location, movement_hexes, parse_ranges, range_bracket,
    range_brackets_hexes, skill_adjusted_bv, skill_adjusted_pv, target_modifier, AttackDir,
    ClusterProfile, MoveMode, RangeBracket, PILOT_MAX,
};
use neurohelmet_core::session::{
    AsCritKind, BfLive, BfMorale, CritRow, MoraleStatus, MotiveLevel, TrackedMech, CREW_MAX,
    OV_HEAT_MAX, VEHICLE_CRITS,
};
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, BorderType, Borders, Clear, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState,
};
use ratatui::Frame;

/// §35 rarity → tint, resolved through the active [`theme`] (§36 display-profile palette).
fn rarity_color(r: Rarity) -> Color {
    theme().rarity(r)
}

pub fn draw(f: &mut Frame, app: &mut App) {
    // Paint the base fg/bg so a self-contained theme (cockpit/tokyo/mocha) owns the whole screen.
    // `Color::Reset` slots (pi, truecolor) leave the terminal's own colors — and skipping the paint
    // entirely there keeps headless/Pi renders and the snapshot tests byte-identical. Patched with
    // `Option`-style styles, untouched text cells inherit this base; explicit span colors override.
    let t = theme();
    if t.bg != Color::Reset || t.fg != Color::Reset {
        let mut style = Style::default();
        if t.fg != Color::Reset {
            style = style.fg(t.fg);
        }
        if t.bg != Color::Reset {
            style = style.bg(t.bg);
        }
        f.render_widget(Block::default().style(style), f.area());
    }
    match app.screen {
        // Picker and Sessions are full-screen lists of their own — no sidebar.
        Screen::Picker => draw_picker(f, app),
        Screen::Sessions => draw_sessions(f, app),
        // The play screens get the §36 Modern force sidebar (when the profile + width allow).
        Screen::Tracker => {
            let (area, sidebar) = play_content(f, app);
            draw_tracker(f, area, sidebar, app);
        }
        Screen::AlphaStrike => {
            let (area, sidebar) = play_content(f, app);
            draw_alpha_strike(f, area, sidebar, app);
        }
        Screen::Override => {
            let (area, sidebar) = play_content(f, app);
            draw_override(f, area, sidebar, app);
        }
        // Standard BF is AS mode's sibling: the same play-screen chrome, sidebar included (the
        // roster cursor IS the card cursor here, unlike SBF).
        Screen::BattleForce => {
            let (area, sidebar) = play_content(f, app);
            draw_battleforce(f, area, sidebar, app);
        }
        // SBF skips the force sidebar entirely: its roster cursor can't be moved on this screen
        // (formation/unit selection lives in the panes), and the detail pane lists the active
        // unit's elements — the sidebar would only duplicate that with a dead highlight.
        Screen::Sbf => draw_sbf(f, f.area(), app),
        // ACS is the same shape as SBF (three panes own the selection); no force sidebar.
        Screen::Acs => draw_acs(f, f.area(), app),
    }
    if app.modal.is_some() {
        draw_modal(f, app);
    }
}

/// Width reserved for the Modern-profile force sidebar, and the minimum width the main view needs
/// for the sidebar to be worth carving (below this we fall back to single-pane even in Modern).
const SIDEBAR_W: u16 = 26;
const SIDEBAR_MIN_MAIN: u16 = 70;

/// For a play screen: in the Modern profile (and with enough width + a non-empty roster), draw the
/// force sidebar down the left and return `(main_area, true)` for the screen proper. Otherwise return
/// `(full_frame, false)` — the Pi single-pane layout, and a graceful fallback on narrow terminals.
/// The `bool` lets a screen drop its now-redundant top roster tabs when the sidebar already lists the
/// whole force.
fn play_content(f: &mut Frame, app: &App) -> (Rect, bool) {
    let area = f.area();
    if profile() == DisplayProfile::Modern
        && !app.session.mechs.is_empty()
        && area.width >= SIDEBAR_W + SIDEBAR_MIN_MAIN
    {
        let parts = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(SIDEBAR_W),
                Constraint::Min(SIDEBAR_MIN_MAIN),
            ])
            .split(area);
        draw_sidebar(f, parts[0], app);
        (parts[1], true)
    } else {
        (area, false)
    }
}

/// A centered rectangle `w`x`h` within `area`.
fn centered_rect(w: u16, h: u16, area: Rect) -> Rect {
    let w = w.min(area.width);
    let h = h.min(area.height);
    Rect {
        x: area.x + (area.width - w) / 2,
        y: area.y + (area.height - h) / 2,
        width: w,
        height: h,
    }
}

fn draw_modal(f: &mut Frame, app: &App) {
    let Some(modal) = &app.modal else { return };
    let (title, lines): (String, Vec<Line>) = match modal {
        Modal::Confirm { prompt, .. } => (
            " Confirm ".into(),
            prompt
                .lines()
                .map(Line::from)
                .chain(std::iter::once(Line::from(Span::styled(
                    "y = yes    n / Esc = no",
                    Style::default().fg(theme().dim),
                ))))
                .collect(),
        ),
        Modal::Input { prompt, buffer, .. } => (
            " Input ".into(),
            vec![
                Line::from(prompt.as_str()),
                Line::from(Span::styled(
                    format!("{buffer}\u{2588}"),
                    Style::default().fg(theme().accent),
                )),
                Line::from(Span::styled(
                    "Enter = ok    Esc = cancel",
                    Style::default().fg(theme().dim),
                )),
            ],
        ),
        Modal::Crit { loc, sel } => (
            format!(" {} — critical slots ", loc.label()),
            crit_modal_lines(app, *loc, *sel),
        ),
        Modal::Munition { bin, sel, .. } => (
            " Load munition ".into(),
            munition_modal_lines(app, *bin, *sel),
        ),
        Modal::AsCrit { sel } => (
            " Alpha Strike crits ".into(),
            as_crit_modal_lines(app, *sel),
        ),
        Modal::Skills { sel } => (" Pilot skills ".into(), skills_modal_lines(app, *sel)),
        Modal::AddUnit {
            idx,
            gunnery,
            piloting,
            sel,
        } => {
            let title = match app.bundle.get(*idx) {
                Some(m) => format!(" Add {} ", m.display_name()),
                None => " Add unit ".into(),
            };
            (
                title,
                add_unit_modal_lines(app, *idx, *gunnery, *piloting, *sel),
            )
        }
        Modal::Move { sel } => (" Movement this turn ".into(), move_modal_lines(app, *sel)),
        Modal::Shot { sel } => (" To-hit target ".into(), shot_modal_lines(app, *sel)),
        Modal::Gator { sel } => (
            " To-hit target (GATOR) ".into(),
            gator_modal_lines(app, *sel),
        ),
        Modal::VehicleCrit { sel } => {
            let title = if app
                .session
                .active_mech()
                .is_some_and(|tm| tm.spec.is_aerospace())
            {
                " Aerospace criticals "
            } else {
                " Vehicle criticals "
            };
            (title.into(), vehicle_crit_modal_lines(app, *sel))
        }
        Modal::OvCrit { loc, sel } => (
            " Override criticals ".into(),
            ov_crit_modal_lines(app, *loc, *sel),
        ),
        Modal::OvShot { sel } => (" To-hit shot ".into(), ov_shot_modal_lines(app, *sel)),
        Modal::SbfGroup { sel } => (" Group force ".into(), sbf_group_modal_lines(app, *sel)),
        Modal::AcsGroup { sel } => (" Group force ".into(), acs_group_modal_lines(app, *sel)),
        Modal::SbfDoctrine { sel } => (
            " Auto-group (doctrine) ".into(),
            sbf_doctrine_modal_lines(*sel),
        ),
        Modal::SbfCrit { sel } => (" SBF criticals ".into(), sbf_crit_modal_lines(app, *sel)),
        Modal::SbfShot { sel } => (" SBF to-hit ".into(), sbf_shot_modal_lines(app, *sel)),
        Modal::BfCrit { sel } => (
            " BattleForce criticals ".into(),
            bf_crit_modal_lines(app, *sel),
        ),
        Modal::BfShot { sel } => (" BF to-hit ".into(), bf_shot_modal_lines(app, *sel)),
        Modal::BfGroup { sel } => (" Group force ".into(), bf_group_modal_lines(app, *sel)),
        Modal::BfDoctrine { sel } => (
            " Auto-group (doctrine) ".into(),
            bf_doctrine_modal_lines(*sel),
        ),
        Modal::Motive { sel } => (
            " Motive system damage ".into(),
            motive_modal_lines(app, *sel),
        ),
        Modal::Dice { tab } => (" Dice reference ".into(), dice_modal_lines(app, *tab)),
        Modal::Filters { sel } => (" Filters ".into(), filters_modal_lines(app, *sel)),
        Modal::FactionPick { query, sel } => (
            " Pick faction ".into(),
            faction_pick_lines(app, query, *sel),
        ),
        Modal::GenerateForce(fg) => (" Generate force ".into(), force_gen_modal_lines(app, fg)),
        Modal::ThemePicker { sel, .. } => (" Display (Ctrl-T) ".into(), theme_picker_lines(*sel)),
        Modal::Help => (
            " Keys ".into(),
            match app.screen {
                Screen::AlphaStrike => as_help_modal_lines(),
                Screen::Override => ov_help_modal_lines(),
                Screen::Sbf => sbf_help_modal_lines(),
                Screen::BattleForce => bf_help_modal_lines(),
                Screen::Acs => acs_help_modal_lines(),
                _ => help_modal_lines(),
            },
        ),
    };
    // Width fits the widest line (for wide tables like the Cluster Hits reference), min 60.
    let content_w = lines.iter().map(Line::width).max().unwrap_or(0) as u16;
    let area = centered_rect(
        content_w.saturating_add(4).max(60),
        lines.len() as u16 + 2,
        f.area(),
    );
    f.render_widget(Clear, area);
    f.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Double)
                .border_style(Style::default().fg(theme().accent))
                .title(title),
        ),
        area,
    );
}

/// Body lines for the Ctrl-T display picker: a theme row per preset (label + config name, swatched
/// in that theme's accent), then a layout-profile toggle row. The selection is highlighted; moving
/// onto a theme row live-previews it, and the profile row toggles Pi/Modern live — so both choices
/// are visible behind the modal.
fn theme_picker_lines(sel: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for (i, (name, label, t)) in THEMES.iter().enumerate() {
        let selected = i == sel;
        let marker = if selected { "▶ " } else { "  " };
        let style = if selected {
            Style::default()
                .fg(theme().on_accent)
                .bg(theme().accent)
                .add_modifier(Modifier::BOLD)
        } else {
            // A little swatch: render the unselected label in that theme's accent color.
            Style::default().fg(t.accent)
        };
        lines.push(Line::from(vec![
            Span::styled(format!("{marker}{label:<18}"), style),
            Span::styled(format!(" ({name})"), Style::default().fg(theme().dim)),
        ]));
    }
    // The layout-profile toggle row (the last selectable row).
    let prof_selected = sel == THEMES.len();
    let marker = if prof_selected { "▶ " } else { "  " };
    let (prof_name, prof_label) = match profile() {
        DisplayProfile::Pi => ("pi", "Pi (compact)"),
        DisplayProfile::Modern => ("modern", "Modern (roomy)"),
    };
    let prof_style = if prof_selected {
        Style::default()
            .fg(theme().on_accent)
            .bg(theme().accent)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme().accent)
    };
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled(format!("{marker}{:<10}", "Layout:"), prof_style),
        Span::styled(format!("‹ {prof_label} ›"), prof_style),
        Span::styled(format!("  ({prof_name})"), Style::default().fg(theme().dim)),
    ]));
    // The icon-set toggle row (Text vs Nerd Font glyphs).
    let icon_selected = sel == THEMES.len() + 1;
    let marker = if icon_selected { "▶ " } else { "  " };
    let cur = icons::icons();
    let icon_style = if icon_selected {
        Style::default()
            .fg(theme().on_accent)
            .bg(theme().accent)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme().accent)
    };
    lines.push(Line::from(vec![
        Span::styled(format!("{marker}{:<10}", "Icons:"), icon_style),
        Span::styled(format!("‹ {} ›", cur.label()), icon_style),
        Span::styled(
            format!("  ({})", cur.config_name()),
            Style::default().fg(theme().dim),
        ),
    ]));
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "↑↓ move · ←→ toggle · Enter keep · Esc cancel",
        Style::default().fg(theme().dim),
    )));
    lines
}

/// Body lines for the critical-slot popup: one row per slot (slot index + name), a red
/// `×` on destroyed slots, the selected row highlighted, and a footer hint.
fn crit_modal_lines(app: &App, loc: Location, sel: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    match app.session.active_mech() {
        Some(tm) if tm.spec.crit_slots.get(&loc).is_some_and(|s| !s.is_empty()) => {
            let mut prev_col2 = false;
            for (i, cs) in tm.spec.crit_slots[&loc].iter().enumerate() {
                // Paper sheets number each column 1-6; locations with 12 slots have a
                // second column that restarts at 1. A divider marks that break.
                let col2 = cs.slot >= 6;
                if col2 && !prev_col2 && i > 0 {
                    lines.push(Line::from(Span::styled(
                        "     ──────────",
                        Style::default().fg(theme().dim),
                    )));
                }
                prev_col2 = col2;
                let num = (cs.slot % 6) + 1;
                let hit = tm.is_crit_hit(loc, cs.slot);
                let selected = i == sel;
                let marker = if selected { "▶ " } else { "  " };
                let label = format!("{marker}{num}  {}", cs.name);
                let mut style = if hit {
                    Style::default().fg(theme().danger)
                } else {
                    Style::default()
                };
                if selected {
                    style = Style::default()
                        .fg(theme().on_accent)
                        .bg(theme().accent)
                        .add_modifier(Modifier::BOLD);
                }
                let mut spans = vec![Span::styled(label, style)];
                // Ammo slots: show remaining/max shots, loaded munition, and the active flag.
                if let Some(bin) = tm.bin_at(loc, &cs.name) {
                    let (rem, max) = (tm.ammo_remaining(bin), tm.ammo_max(bin));
                    spans.push(Span::styled(
                        format!("  {rem}/{max}"),
                        Style::default().fg(frac_color(rem, max)),
                    ));
                    // Loaded munition (only bins with a real choice carry `base_ammo`).
                    let has_choice = tm
                        .spec
                        .ammo
                        .iter()
                        .any(|b| b.id == bin && b.base_ammo.is_some());
                    if has_choice {
                        let m = tm.bin_munition(bin);
                        let style = if m == STANDARD_MUNITION {
                            Style::default().fg(theme().dim)
                        } else {
                            Style::default().fg(theme().warning)
                        };
                        spans.push(Span::styled(format!("  {m}"), style));
                    }
                    if tm.is_active_bin(bin) {
                        spans.push(Span::styled(
                            "  ◀ active",
                            Style::default()
                                .fg(theme().accent)
                                .add_modifier(Modifier::BOLD),
                        ));
                    }
                }
                if hit {
                    spans.push(Span::styled(
                        "  ×",
                        Style::default()
                            .fg(theme().danger)
                            .add_modifier(Modifier::BOLD),
                    ));
                }
                lines.push(Line::from(spans));
            }
        }
        _ => lines.push(Line::from(Span::styled(
            "(no critical-slot data)",
            Style::default().fg(theme().dim),
        ))),
    }
    lines.push(Line::from(Span::styled(
        "[Spc] hit  [a] active bin  [t] munition  [Esc] close",
        Style::default().fg(theme().dim),
    )));
    lines
}

/// Body lines for the munition picker: a scroll-windowed list of the bin's loadable munitions,
/// the currently-loaded one marked `●`, the selected row highlighted, plus `▲/▼ more` hints.
fn munition_modal_lines(app: &App, bin: u32, sel: usize) -> Vec<Line<'static>> {
    /// Rows of the list shown at once (the group can hold up to ~40 munitions).
    const WINDOW: usize = 12;
    let mut lines = Vec::new();
    let (list, loaded, header): (Vec<String>, String, String) = match app.session.active_mech() {
        Some(tm) => {
            let b = tm.spec.ammo.iter().find(|b| b.id == bin);
            let list = b
                .map(|b| app.bundle.munitions_for(b.base_ammo.as_deref()).to_vec())
                .unwrap_or_default();
            let header = b.map(|b| b.name.clone()).unwrap_or_default();
            (list, tm.bin_munition(bin).to_string(), header)
        }
        None => (Vec::new(), String::new(), String::new()),
    };

    lines.push(Line::from(Span::styled(
        header,
        Style::default().fg(theme().dim),
    )));

    // Window the list so a long group (e.g. 40 LRM munitions) stays on screen around the cursor.
    let start = sel
        .saturating_sub(WINDOW / 2)
        .min(list.len().saturating_sub(WINDOW));
    let end = (start + WINDOW).min(list.len());
    if start > 0 {
        lines.push(Line::from(Span::styled(
            format!("  ▲ {} more", start),
            Style::default().fg(theme().dim),
        )));
    }
    for (i, m) in list.iter().enumerate().take(end).skip(start) {
        let selected = i == sel;
        let is_loaded = *m == loaded;
        let marker = if selected { "▶ " } else { "  " };
        let dot = if is_loaded { "● " } else { "  " };
        let label = format!("{marker}{dot}{m}");
        let style = if selected {
            Style::default()
                .fg(theme().on_accent)
                .bg(theme().accent)
                .add_modifier(Modifier::BOLD)
        } else if is_loaded {
            Style::default().fg(theme().warning)
        } else {
            Style::default()
        };
        lines.push(Line::from(Span::styled(label, style)));
    }
    if end < list.len() {
        lines.push(Line::from(Span::styled(
            format!("  ▼ {} more", list.len() - end),
            Style::default().fg(theme().dim),
        )));
    }
    lines.push(Line::from(Span::styled(
        "[↑↓] select   [Enter] load   [Esc] cancel",
        Style::default().fg(theme().dim),
    )));
    lines
}

/// Body lines for the read-only dice-reference popup (§18): a tab header plus either the selected
/// weapon's Cluster Hits column or the 'Mech Hit Location table. Rolls/changes nothing.
fn dice_modal_lines(app: &App, tab: DiceTab) -> Vec<Line<'static>> {
    let active = Style::default()
        .fg(theme().on_accent)
        .bg(theme().accent)
        .add_modifier(Modifier::BOLD);
    let inactive = Style::default().fg(theme().dim);
    let mut lines = vec![
        Line::from(vec![
            Span::styled(
                " Cluster ",
                if tab == DiceTab::Cluster {
                    active
                } else {
                    inactive
                },
            ),
            Span::raw("  "),
            Span::styled(
                " Full Table ",
                if tab == DiceTab::Table {
                    active
                } else {
                    inactive
                },
            ),
            Span::raw("  "),
            Span::styled(
                " Hit Location ",
                if tab == DiceTab::HitLoc {
                    active
                } else {
                    inactive
                },
            ),
        ]),
        Line::from(""),
    ];
    match tab {
        DiceTab::Cluster => dice_cluster_lines(app, &mut lines),
        DiceTab::Table => dice_cluster_table_lines(app, &mut lines),
        DiceTab::HitLoc => dice_hitloc_lines(app, &mut lines),
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "[Tab] switch page   [Esc] close",
        Style::default().fg(theme().dim),
    )));
    lines
}

/// Body lines for the picker filter editor: one row per facet (`Type:  Mech`), the selected facet
/// highlighted, `(any)` dimmed. ←→ cycles the highlighted facet's value (handled in app).
fn filters_modal_lines(app: &App, sel: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for (i, &facet) in Facet::ALL.iter().enumerate() {
        let value = app.filters.value_label(facet);
        let is_any = value == "(any)";
        let selected = i == sel;
        let marker = if selected { "▶ " } else { "  " };
        let style = if selected {
            Style::default()
                .fg(theme().on_accent)
                .bg(theme().accent)
                .add_modifier(Modifier::BOLD)
        } else if is_any {
            Style::default().fg(theme().dim)
        } else {
            Style::default().fg(theme().warning)
        };
        lines.push(Line::from(Span::styled(
            format!("{marker}{:<7} {value}", format!("{}:", facet.label())),
            style,
        )));
    }
    lines.push(Line::from(""));
    // The Year facet is typed; the §35 availability lens tints rather than hides; everything else
    // is a plain cycle — show the relevant hint.
    let hint = if Facet::ALL[sel].is_year() {
        "[0-9] year  [⌫] del  [←→] ±1  [c] clear  [Esc] apply"
    } else if Facet::ALL[sel] == Facet::Faction {
        "[Enter] search & set faction (82)   [c] clear   [Esc] apply"
    } else if Facet::ALL[sel].is_avail() {
        "[←→] cycle (tints + sorts by rarity, never hides)   [c] clear   [Esc] apply"
    } else {
        "[↑↓] facet   [←→] cycle   [c] clear   [Esc] apply"
    };
    lines.push(Line::from(Span::styled(
        hint,
        Style::default().fg(theme().dim),
    )));
    lines
}

/// Body lines for the faction combo box ([`Modal::FactionPick`]): a live search line over the typed
/// query, then a windowed, scrollable list of matching factions with the selection highlighted.
fn faction_pick_lines(app: &App, query: &str, sel: usize) -> Vec<Line<'static>> {
    const MAX: usize = 12; // visible rows before scrolling
    let list = app.faction_pick_list(query);
    let mut lines = vec![
        Line::from(vec![
            Span::styled("search: ", Style::default().fg(theme().dim)),
            Span::styled(format!("{query}_"), Style::default().fg(theme().warning)),
        ]),
        Line::from(""),
    ];
    if list.is_empty() {
        lines.push(Line::from(Span::styled(
            "  (no faction matches)",
            Style::default().fg(theme().dim),
        )));
    } else {
        // Window the list so the selection stays on screen.
        let start = sel
            .saturating_sub(MAX - 1)
            .min(list.len().saturating_sub(MAX));
        let end = (start + MAX).min(list.len());
        if start > 0 {
            lines.push(Line::from(Span::styled(
                format!("  ▲ {start} more"),
                Style::default().fg(theme().dim),
            )));
        }
        for (i, choice) in list.iter().enumerate().take(end).skip(start) {
            let label = match choice {
                Some((_, name)) => name.clone(),
                None => "(any — clear faction)".to_string(),
            };
            let selected = i == sel;
            let marker = if selected { "▶ " } else { "  " };
            let style = if selected {
                Style::default()
                    .fg(theme().on_accent)
                    .bg(theme().accent)
                    .add_modifier(Modifier::BOLD)
            } else if choice.is_none() {
                Style::default().fg(theme().dim)
            } else {
                Style::default().fg(theme().warning)
            };
            lines.push(Line::from(Span::styled(format!("{marker}{label}"), style)));
        }
        if end < list.len() {
            lines.push(Line::from(Span::styled(
                format!("  ▼ {} more", list.len() - end),
                Style::default().fg(theme().dim),
            )));
        }
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "[type] filter   [↑↓] select   [Enter] set   [Esc] cancel",
        Style::default().fg(theme().dim),
    )));
    lines
}

/// §35 force-generator modal: the config form, or — once rolled — the force preview.
fn force_gen_modal_lines(app: &App, fg: &ForceGen) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let pt = match app.session.mode {
        GameMode::AlphaStrike
        | GameMode::StrategicBattleForce
        | GameMode::BattleForce
        | GameMode::AbstractCombatSystem => "PV",
        GameMode::Classic | GameMode::Override => "BV",
    };
    let era_id = fg.era.as_ref().map(|(id, _)| *id);
    let fac_id = fg.faction.as_ref().map(|(id, _)| *id);

    if !fg.rolled {
        let any = |o: &Option<(u16, String)>| {
            o.as_ref()
                .map_or_else(|| "(any)".to_string(), |(_, n)| n.clone())
        };
        let size = match forcegen::formation_name(fg.count) {
            Some(name) => format!("{} ({name})", fg.count),
            None => fg.count.to_string(),
        };
        let budget = match (fg.use_budget, app.session.limit) {
            (true, Some(l)) => format!("on — {l} {pt}"),
            (true, None) => "on (no limit set — ^b)".into(),
            (false, _) => "off".into(),
        };
        let values = [
            any(&fg.faction),
            any(&fg.era),
            size,
            budget,
            if fg.allow_rare {
                "yes".into()
            } else {
                "no".into()
            },
            fg.class_bias.clone().unwrap_or_else(|| "(any)".into()),
        ];
        for (i, label) in ForceGen::ROWS.iter().enumerate() {
            let selected = i == fg.field;
            let marker = if selected { "▶ " } else { "  " };
            let dim = matches!(values[i].as_str(), "(any)" | "no" | "off");
            let style = if selected {
                Style::default()
                    .fg(theme().on_accent)
                    .bg(theme().accent)
                    .add_modifier(Modifier::BOLD)
            } else if dim {
                Style::default().fg(theme().dim)
            } else {
                Style::default().fg(theme().warning)
            };
            lines.push(Line::from(Span::styled(
                format!("{marker}{:<11} {}", format!("{label}:"), values[i]),
                style,
            )));
        }
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "[↑↓] field   [←→] change   [Enter] roll   [Esc] cancel",
            Style::default().fg(theme().dim),
        )));
    } else if fg.preview.is_empty() {
        lines.push(Line::from(Span::styled(
            fg.note.clone(),
            Style::default().fg(theme().warning),
        )));
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "[⌫] back to config   [Esc] cancel",
            Style::default().fg(theme().dim),
        )));
    } else {
        let mut total = 0u64;
        for &idx in &fg.preview {
            let Some(m) = app.bundle.get(idx) else {
                continue;
            };
            let cost = forcegen::unit_cost(m, app.session.mode);
            total += cost;
            lines.push(Line::from(vec![
                Span::styled(
                    format!("  {:<24}", m.display_name()),
                    Style::default().fg(rarity_color(m.rarity(era_id, fac_id))),
                ),
                Span::styled(
                    format!("{:>3}t  ", m.tonnage),
                    Style::default().fg(theme().dim),
                ),
                Span::styled(format!("{cost} {pt}"), Style::default().fg(theme().dim)),
            ]));
        }
        lines.push(Line::from(""));
        let over = app.session.limit.is_some_and(|l| total > l);
        let summary = match app.session.limit {
            Some(l) => format!("{} unit(s)  ·  {total}/{l} {pt}", fg.preview.len()),
            None => format!("{} unit(s)  ·  {total} {pt}", fg.preview.len()),
        };
        lines.push(Line::from(Span::styled(
            summary,
            Style::default()
                .fg(if over { theme().danger } else { theme().good })
                .add_modifier(Modifier::BOLD),
        )));
        // A budget-capped roll explains why it came up short of the requested size.
        if !fg.note.is_empty() {
            lines.push(Line::from(Span::styled(
                fg.note.clone(),
                Style::default().fg(theme().warning),
            )));
        }
        lines.push(Line::from(Span::styled(
            "[Enter] accept (append)   [r] reroll   [⌫] back   [Esc] cancel",
            Style::default().fg(theme().dim),
        )));
    }
    lines
}

/// Cluster-hits column for the currently selected weapon.
fn dice_cluster_lines(app: &App, lines: &mut Vec<Line<'static>>) {
    let dim = |s: String| Line::from(Span::styled(s, Style::default().fg(theme().dim)));
    let Some(tm) = app.session.active_mech() else {
        lines.push(dim("No active unit.".into()));
        return;
    };
    let Some(id) = app.selected_weapon_id() else {
        lines.push(dim("Select a weapon in the WEAPONS panel".into()));
        lines.push(dim("(Tab to focus it) to see its cluster column.".into()));
        return;
    };
    let weapon = tm.spec.weapons.iter().find(|w| w.id == id);
    let name = weapon.map_or("", |w| w.name.as_str());
    let munition = tm
        .weapon_bin(id)
        .map_or(STANDARD_MUNITION, |b| tm.bin_munition(b));
    match tm.weapon_cluster_profile(id) {
        Some(ClusterProfile::Table(size)) => {
            let suffix = if munition.is_empty() || munition == STANDARD_MUNITION {
                String::new()
            } else {
                format!(", {munition}")
            };
            lines.push(Line::from(Span::styled(
                format!("{name}  (rack {size}{suffix})"),
                Style::default()
                    .fg(theme().fg_strong)
                    .add_modifier(Modifier::BOLD),
            )));
            lines.push(dim("  2d6    hits".into()));
            for roll in 2..=12u8 {
                // The most common result (a natural 7) reads in the accent color.
                let style = if roll == 7 {
                    Style::default().fg(theme().accent)
                } else {
                    Style::default()
                };
                lines.push(Line::from(Span::styled(
                    format!("  {roll:>3}    {:>3}", cluster_hits(size, roll)),
                    style,
                )));
            }
        }
        Some(ClusterProfile::AllHit(n)) => {
            lines.push(Line::from(name.to_string()));
            lines.push(dim(format!(
                "Streak: all {n} hit if locked — no cluster roll."
            )));
        }
        Some(ClusterProfile::Single) | None => {
            lines.push(Line::from(name.to_string()));
            lines.push(dim("Single hit — no cluster roll.".into()));
        }
    }
}

/// The full Cluster Hits Table as a reference grid: every rack size (2–30, then 40) down the side,
/// 2d6 result across the top, laid out in two side-by-side halves to stay compact. The selected
/// weapon's rack size (if any) is highlighted.
fn dice_cluster_table_lines(app: &App, lines: &mut Vec<Line<'static>>) {
    // Highlight the active weapon's rack size, if it rolls on the table.
    let active = app.session.active_mech().and_then(|tm| {
        app.selected_weapon_id()
            .and_then(|id| match tm.weapon_cluster_profile(id) {
                Some(ClusterProfile::Table(size)) => Some(size),
                _ => None,
            })
    });
    const ROLLS: [u8; 11] = [2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12];
    // One half-row of spans: the size label then its 11 hit counts (or the roll header).
    let half = |size: u16, header: bool| -> Vec<Span<'static>> {
        let hot = !header && active == Some(size);
        let label_style = if header {
            Style::default()
                .fg(theme().accent)
                .add_modifier(Modifier::BOLD)
        } else if hot {
            Style::default()
                .fg(theme().warning)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme().dim)
        };
        let label = if header {
            "sz".to_string()
        } else {
            size.to_string()
        };
        let mut spans = vec![Span::styled(format!("{label:>3}"), label_style)];
        for r in ROLLS {
            let cell = if header {
                format!("{r:>3}")
            } else {
                format!("{:>3}", cluster_hits(size, r))
            };
            let style = if hot {
                Style::default().fg(theme().warning)
            } else if r == 7 {
                Style::default().fg(theme().accent) // the median roll
            } else {
                Style::default()
            };
            spans.push(Span::styled(cell, style));
        }
        spans
    };
    let sep = || Span::styled(" │ ", Style::default().fg(theme().dim));
    // Left column 2–16, right column 17–30 then 40 — 15 rows each.
    let left: Vec<u16> = (2..=16).collect();
    let right: Vec<u16> = (17..=30).chain(std::iter::once(40)).collect();
    let row = |l: u16, r: u16, header: bool| -> Line<'static> {
        let mut spans = half(l, header);
        spans.push(sep());
        spans.extend(half(r, header));
        Line::from(spans)
    };
    lines.push(row(0, 0, true));
    for i in 0..left.len() {
        lines.push(row(left[i], right[i], false));
    }
    lines.push(Line::from(Span::styled(
        "  7 = median roll; yellow = selected weapon's rack",
        Style::default().fg(theme().dim),
    )));
}

/// The 2d6 'Mech Hit Location table, all four attack directions. Shown for every unit — most
/// targets in play are 'Mechs anyway; a vehicle/other table + a per-open toggle is a later add.
fn dice_hitloc_lines(_app: &App, lines: &mut Vec<Line<'static>>) {
    let dim = |s: String| Line::from(Span::styled(s, Style::default().fg(theme().dim)));
    lines.push(dim("  2d6   Front  Left  Right  Rear".into()));
    let mut floating = false;
    for roll in 2..=12u8 {
        let mut spans = vec![Span::raw(format!("  {roll:>3}   "))];
        for dir in AttackDir::ALL {
            let hit = mech_hit_location(dir, roll);
            if hit.floating_crit {
                floating = true;
            }
            let mark = if hit.floating_crit { "*" } else { " " };
            spans.push(Span::raw(format!("{:<3}{mark}  ", hit.loc.code())));
        }
        lines.push(Line::from(spans));
    }
    if floating {
        lines.push(dim("* natural 2: inflict a critical hit too".into()));
    }
}

/// The full keybinding reference shown by the [?] help modal.
/// One entry in a help modal: a section `Header`, a key `Row`, or a `Blank` spacer.
enum HelpItem {
    Header(&'static str),
    Row(&'static str, &'static str),
    Blank,
}

/// Lay two columns of [`HelpItem`]s side by side into rendered lines (left padded to a fixed width,
/// then a gutter, then the right column). Keeps a busy keymap inside the ~30-row Pi screen without
/// clipping the footer. `left`/`right` are zip-padded so the shorter column just leaves blanks.
fn two_column_help(left: &[HelpItem], right: &[HelpItem]) -> Vec<Line<'static>> {
    // Width of one column's text: "  {keys:<9}{desc}". The longest desc sets it; cap so two columns
    // plus the gutter fit a 100-wide screen.
    const COL_W: usize = 46;
    let render = |item: &HelpItem| -> Line<'static> {
        match item {
            HelpItem::Header(s) => Line::from(Span::styled(
                (*s).to_string(),
                Style::default()
                    .fg(theme().accent)
                    .add_modifier(Modifier::BOLD),
            )),
            HelpItem::Row(keys, desc) => Line::from(vec![
                Span::styled(format!("  {keys:<9}"), Style::default().fg(theme().accent)),
                Span::styled((*desc).to_string(), Style::default().fg(theme().dim)),
            ]),
            HelpItem::Blank => Line::from(""),
        }
    };
    let pad = |line: Line<'static>| -> Line<'static> {
        let w: usize = line.width();
        let mut spans = line.spans;
        if w < COL_W {
            spans.push(Span::raw(" ".repeat(COL_W - w)));
        }
        Line::from(spans)
    };
    let n = left.len().max(right.len());
    (0..n)
        .map(|i| {
            let mut spans = pad(render(left.get(i).unwrap_or(&HelpItem::Blank))).spans;
            spans.push(Span::raw("  "));
            spans.extend(render(right.get(i).unwrap_or(&HelpItem::Blank)).spans);
            Line::from(spans)
        })
        .collect()
}

fn help_modal_lines() -> Vec<Line<'static>> {
    use HelpItem::{Blank, Header, Row};
    let left = [
        Header("Doll"),
        Row("Space", "damage current location"),
        Row("u", "repair (internal first)"),
        Row("c", "crit slots (a=bin · t=munition)"),
        Row("r", "dice reference (cluster · hit loc)"),
        Row("f", "toggle front / rear armor"),
        Row("←↑↓→", "move cursor"),
        Blank,
        Header("Equipment"),
        Row("Space", "fire weapon (marks ✓) / spend ammo"),
        Row("u", "un-fire / refill"),
        Row("J", "toggle state: jam UAC/RAC · MASC/ECM"),
        Blank,
        Header("Pilot / force"),
        Row("g", "edit gunnery / piloting skills"),
        Row("b", "set force point limit (BV)"),
        Row("p / P", "pilot / crew hit / heal"),
    ];
    let right = [
        Header("General"),
        Row("Tab", "switch panel"),
        Row("o / i", "heat up / down"),
        Row("e", "end turn (heat, clear fired ✓ + PSR)"),
        Row("x / X", "toggle shutdown / pilot KO / wake"),
        Row("d", "toggle prone (knocked down)"),
        Row("v", "movement this turn (mode + hexes)"),
        Row("t", "to-hit target (GATOR)"),
        Row("m / M", "motive damage (vehicles) / repair"),
        Row("L", "log snapshot (game log)"),
        Row("z", "undo"),
        Row(", / .", "previous / next mech  ([ / ] also)"),
        Row("a / D", "add / delete mech"),
        Row("S", "sessions browser"),
        Row("^t", "display picker (theme + layout)"),
        Row("q", "quit"),
    ];
    let mut lines = two_column_help(&left, &right);
    lines.push(Line::from(Span::styled(
        "  press any key to close",
        Style::default().fg(theme().dim),
    )));
    lines
}

/// Body lines for the Alpha Strike crit popup: the four crit types with their counts.
fn as_crit_modal_lines(app: &App, sel: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    if let Some(tm) = app.session.active_mech() {
        for (i, &kind) in tm.as_crit_kinds().iter().enumerate() {
            let n = tm.as_crit(kind);
            let selected = i == sel;
            let marker = if selected { "▶ " } else { "  " };
            let style = if selected {
                Style::default()
                    .fg(theme().on_accent)
                    .bg(theme().accent)
                    .add_modifier(Modifier::BOLD)
            } else if n > 0 {
                Style::default().fg(theme().danger)
            } else {
                Style::default()
            };
            lines.push(Line::from(Span::styled(
                format!("{marker}{:<14} {}/{}", kind.label(), n, kind.cap()),
                style,
            )));
        }
    }
    lines.push(Line::from(Span::styled(
        "[↑↓] select   [←→] adjust   [Esc] close",
        Style::default().fg(theme().dim),
    )));
    lines
}

/// Body lines for the pilot-skills editor: the unit's two skills (or the single Alpha Strike
/// Skill), the selected one highlighted.
fn skills_modal_lines(app: &App, sel: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    if let Some(tm) = app.session.active_mech() {
        // AS and BF have one Skill (the gunnery field); Classic uses the unit-type's two labels.
        let rows: Vec<(&str, u8)> = if matches!(
            app.session.mode,
            GameMode::AlphaStrike | GameMode::BattleForce
        ) {
            vec![("Skill", tm.gunnery)]
        } else {
            let (g, p) = tm.spec.unit_type.skill_labels();
            vec![(g, tm.gunnery), (p, tm.piloting)]
        };
        for (i, (name, val)) in rows.into_iter().enumerate() {
            let selected = i == sel;
            let marker = if selected { "▶ " } else { "  " };
            let style = if selected {
                Style::default()
                    .fg(theme().on_accent)
                    .bg(theme().accent)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            lines.push(Line::from(Span::styled(
                format!("{marker}{name:<10} {val}+"),
                style,
            )));
        }
    }
    lines.push(Line::from(Span::styled(
        "lower is better   [↑↓] select   [←→] adjust   [Esc] close",
        Style::default().fg(theme().dim),
    )));
    lines
}

/// Body lines for the pre-add skill + cost preview (picker Enter): the crew skill rows, this
/// unit's skill-adjusted point cost, and the resulting force total against any budget — so you
/// can see what a unit will cost (and whether it busts the limit) before committing the add.
fn add_unit_modal_lines(
    app: &App,
    idx: usize,
    gunnery: u8,
    piloting: u8,
    sel: usize,
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let Some(m) = app.bundle.get(idx) else {
        return vec![Line::from("(no unit)")];
    };
    // SBF and Standard BF elements are AS elements: same single Skill row and PV cost.
    let is_as = matches!(
        app.session.mode,
        GameMode::AlphaStrike
            | GameMode::StrategicBattleForce
            | GameMode::BattleForce
            | GameMode::AbstractCombatSystem
    );
    let unit = if is_as { "PV" } else { "BV" };

    // Skill rows. Alpha Strike has a single Skill; Classic has the unit-type's two skills.
    let (g_label, p_label) = m.unit_type.skill_labels();
    let rows: Vec<(&str, u8)> = if is_as {
        vec![("Skill", gunnery)]
    } else {
        vec![(g_label, gunnery), (p_label, piloting)]
    };
    for (i, (name, val)) in rows.into_iter().enumerate() {
        let selected = i == sel;
        let marker = if selected { "▶ " } else { "  " };
        let style = if selected {
            Style::default()
                .fg(theme().on_accent)
                .bg(theme().accent)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        lines.push(Line::from(Span::styled(
            format!("{marker}{name:<10} {val}+"),
            style,
        )));
    }

    // This unit's skill-adjusted cost (and the baked base, when skill moved it).
    let base = if is_as {
        u32::from(m.as_stats.pv)
    } else {
        m.bv
    };
    let cost = if is_as {
        skill_adjusted_pv(base, gunnery)
    } else {
        skill_adjusted_bv(base, gunnery, piloting)
    };
    let mut cost_spans = vec![
        Span::styled("Cost      ", Style::default().fg(theme().dim)),
        Span::styled(
            format!("{unit} {cost}"),
            Style::default()
                .fg(theme().accent)
                .add_modifier(Modifier::BOLD),
        ),
    ];
    if u64::from(base) != cost {
        cost_spans.push(Span::styled(
            format!("   (base {base})"),
            Style::default().fg(theme().dim),
        ));
    }
    lines.push(Line::from(cost_spans));

    // Resulting force total vs. the budget (red when the add would bust it).
    let new_total = app.session.force_total() + cost;
    match app.session.limit {
        Some(limit) => {
            let over = new_total > limit;
            let color = if over { theme().danger } else { theme().good };
            let mut spans = vec![
                Span::styled("Force     ", Style::default().fg(theme().dim)),
                Span::styled(
                    format!("{unit} {new_total}/{limit}"),
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                ),
            ];
            if over {
                spans.push(Span::styled(
                    format!("   OVER by {}", new_total - limit),
                    Style::default()
                        .fg(theme().danger)
                        .add_modifier(Modifier::BOLD),
                ));
            } else {
                spans.push(Span::styled(
                    format!("   {} left", limit - new_total),
                    Style::default().fg(theme().dim),
                ));
            }
            lines.push(Line::from(spans));
        }
        None => lines.push(Line::from(vec![
            Span::styled("Force     ", Style::default().fg(theme().dim)),
            Span::styled(
                format!("{unit} {new_total}"),
                Style::default().fg(theme().accent),
            ),
        ])),
    }

    lines.push(Line::from(Span::styled(
        "lower is better   [↑↓] skill   [←→] adjust   [Enter] add   [Esc] cancel",
        Style::default().fg(theme().dim),
    )));

    // The same summary the Tab preview shows, so you don't have to bounce out to inspect the unit
    // while setting its skill. Kept below the editor so the interactive rows stay visible if a
    // short terminal clips the popup.
    lines.push(Line::from(Span::styled(
        "─".repeat(20),
        Style::default().fg(theme().dim),
    )));
    lines.extend(preview_lines(m));
    lines
}

/// Body lines for the movement editor: this turn's move mode + hexes, with the derived
/// attacker modifier and TMM shown live so the choice's effect is visible as you set it.
fn move_modal_lines(app: &App, sel: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let mut aero = false;
    if let Some(tm) = app.session.active_mech() {
        aero = tm.spec.is_aerospace();
        let rows = if aero {
            // Aerospace: velocity (hexes/turn) + altitude level; both persist across turns.
            [
                ("Velocity", format!("{} hexes", tm.velocity)),
                ("Altitude", format!("{} / 10", tm.altitude)),
            ]
        } else {
            [
                (
                    "Moved",
                    tm.move_mode
                        .label(tm.spec.is_vehicle(), tm.spec.is_infantry())
                        .to_string(),
                ),
                ("Hexes", format!("{} / {}", tm.hexes_moved, tm.max_hexes())),
            ]
        };
        for (i, (name, val)) in rows.into_iter().enumerate() {
            let selected = i == sel;
            let marker = if selected { "▶ " } else { "  " };
            let style = if selected {
                Style::default()
                    .fg(theme().on_accent)
                    .bg(theme().accent)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            lines.push(Line::from(Span::styled(
                format!("{marker}{name:<10} {val}"),
                style,
            )));
        }
        // Ground units derive their attack/TMM modifiers from the move; an aero's to-hit modifier
        // (airborne / velocity) isn't sourced from Mekbay, so it stays the player's to apply.
        if !aero {
            lines.push(Line::from(Span::styled(
                format!(
                    "own attacks {:+}   TMM {:+}",
                    tm.attack_move_modifier(),
                    tm.tmm()
                ),
                Style::default().fg(theme().accent),
            )));
        }
    }
    let footer = if aero {
        "persists across turns   [↑↓] select   [←→] adjust   [Esc] close"
    } else {
        "cleared on end turn   [↑↓] select   [←→] adjust   [Esc] close"
    };
    lines.push(Line::from(Span::styled(
        footer,
        Style::default().fg(theme().dim),
    )));
    lines
}

/// Body lines for the AS to-hit shot editor (§33 Phase 2). Four rows — attacker jumped, target TMM,
/// target jumped, target immobile — plus a live preview of the resulting per-range to-hit numbers.
fn shot_modal_lines(app: &App, sel: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let Some(tm) = app.session.active_mech() else {
        return lines;
    };
    let yn = |b: bool| if b { "[x]" } else { "[ ]" };
    let (tmm, tgt_jumped, tgt_immobile) = match tm.as_target {
        Some(t) => (
            t.tmm.to_string(),
            yn(t.jumped).to_string(),
            yn(t.immobile).to_string(),
        ),
        None => ("—".to_string(), "—".to_string(), "—".to_string()),
    };
    let rows = [
        ("Attacker jumped", yn(tm.as_attacker_jumped).to_string()),
        ("Target TMM", tmm),
        ("Target jumped", tgt_jumped),
        ("Target immobile", tgt_immobile),
    ];
    for (i, (name, val)) in rows.into_iter().enumerate() {
        let selected = i == sel;
        let marker = if selected { "▶ " } else { "  " };
        // Rows 2–3 only matter once a target exists; dim them when there's no target.
        let inactive = i >= 2 && tm.as_target.is_none();
        let style = if selected {
            Style::default()
                .fg(theme().on_accent)
                .bg(theme().accent)
                .add_modifier(Modifier::BOLD)
        } else if inactive {
            Style::default().fg(theme().dim)
        } else {
            Style::default()
        };
        lines.push(Line::from(Span::styled(
            format!("{marker}{name:<16} {val}"),
            style,
        )));
    }
    // Live preview of the resulting target numbers (mirrors the card's To-Hit row).
    let fc = tm.as_crit(AsCritKind::FireControl);
    let is_veh = tm.spec.is_vehicle();
    let tgt = tm.as_target.map(|t| (t.tmm, t.jumped, t.immobile));
    let n = |idx: usize| {
        as_to_hit_full(
            tm.gunnery,
            idx,
            tm.as_heat,
            fc,
            tm.crew_hits,
            is_veh,
            tm.as_attacker_jumped,
            tgt,
        )
    };
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        format!(
            "To-Hit   S {}+  M {}+  L {}+  E {}+",
            n(0),
            n(1),
            n(2),
            n(3)
        ),
        Style::default().fg(theme().warning),
    )));
    lines.push(Line::from(Span::styled(
        "[↑↓] select   [←→/space] adjust   [Esc] close",
        Style::default().fg(theme().dim),
    )));
    lines
}

/// Body lines for the Classic GATOR to-hit target editor (§24). Four rows — distance, target hexes
/// moved, target jumped, target immobile — plus a preview of the shot-level modifiers (gunnery,
/// attacker movement, target, and heat: the terms every weapon shares; range and equipment mods are
/// per-weapon and shown on the Equipment panel rows).
fn gator_modal_lines(app: &App, sel: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let Some(tm) = app.session.active_mech() else {
        return lines;
    };
    let yn = |b: bool| if b { "[x]" } else { "[ ]" };
    let (distance, moved, jumped, immobile) = match tm.ct_target {
        Some(t) => {
            let tmm = target_modifier(t.hexes_moved, t.jumped, t.immobile);
            (
                format!("{} hex", t.distance),
                if t.immobile {
                    format!("{} hex", t.hexes_moved)
                } else {
                    format!("{} hex  (TMM {tmm:+})", t.hexes_moved)
                },
                yn(t.jumped).to_string(),
                yn(t.immobile).to_string(),
            )
        }
        None => (
            "—".to_string(),
            "—".to_string(),
            "—".to_string(),
            "—".to_string(),
        ),
    };
    let rows = [
        ("Distance", distance),
        ("Target moved", moved),
        ("Target jumped", jumped),
        ("Target immobile", immobile),
    ];
    for (i, (name, val)) in rows.into_iter().enumerate() {
        let selected = i == sel;
        let marker = if selected { "▶ " } else { "  " };
        // Rows 1–3 only matter once a target exists; dim them when there's no target.
        let inactive = i >= 1 && tm.ct_target.is_none();
        let style = if selected {
            Style::default()
                .fg(theme().on_accent)
                .bg(theme().accent)
                .add_modifier(Modifier::BOLD)
        } else if inactive {
            Style::default().fg(theme().dim)
        } else {
            Style::default()
        };
        lines.push(Line::from(Span::styled(
            format!("{marker}{name:<16} {val}"),
            style,
        )));
    }
    lines.push(Line::from(""));
    // Preview the shot-level modifiers shared by every weapon: gunnery + attacker move + target +
    // heat. The per-weapon range bracket and equipment (TC/pulse) modifiers ride on the Equipment
    // rows, which switch to showing the assembled target number once a target is set.
    let mv = tm.attack_move_modifier();
    let mv_label = tm
        .move_mode
        .label(tm.spec.is_vehicle(), tm.spec.is_infantry());
    let heat = tm.heat_effects().to_hit_penalty;
    let tgt = tm
        .ct_target
        .map_or(0, |t| target_modifier(t.hexes_moved, t.jumped, t.immobile));
    let mut parts = vec![format!("G{}", tm.gunnery)];
    if mv != 0 {
        parts.push(format!("{mv_label} {mv:+}"));
    }
    if tgt != 0 {
        parts.push(format!("tgt {tgt:+}"));
    }
    if heat != 0 {
        parts.push(format!("heat +{heat}"));
    }
    let base = tm.gunnery as i32 + mv + tgt + heat as i32;
    lines.push(Line::from(Span::styled(
        format!("Base   {} = {base}   (+ range per weapon)", parts.join(" ")),
        Style::default().fg(theme().warning),
    )));
    lines.push(Line::from(Span::styled(
        "[↑↓] select   [←→/space] adjust   [Esc] close",
        Style::default().fg(theme().dim),
    )));
    lines
}

/// Body lines for the vehicle/aerospace crit popup: the rolled crit-result list (marked ones red),
/// followed for aerospace by a "Weapons" section of per-mount rows a weapon crit can destroy.
fn vehicle_crit_modal_lines(app: &App, sel: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    if let Some(tm) = app.session.active_mech() {
        let rows = tm.crit_rows();
        let mut weapons_header_done = false;
        for (i, row) in rows.iter().enumerate() {
            // Section header before the first weapon row (aerospace only).
            if matches!(row, CritRow::Weapon { .. }) && !weapons_header_done {
                weapons_header_done = true;
                lines.push(Line::from(Span::styled(
                    "  — Weapons —",
                    Style::default().fg(theme().accent),
                )));
            }
            let hit = row.marked();
            let selected = i == sel;
            let marker = if selected { "▶ " } else { "  " };
            let mut style = if hit {
                Style::default().fg(theme().danger)
            } else {
                Style::default()
            };
            if selected {
                style = Style::default()
                    .fg(theme().on_accent)
                    .bg(theme().accent)
                    .add_modifier(Modifier::BOLD);
            }
            // Graded aero systems show a "×N" tally; one-shot rows just an "×". A short effect hint
            // spells out what the marked hits do (the panels reflect it, this names it at a glance).
            let (tally, effect) = match row {
                CritRow::System {
                    label, hits, max, ..
                } if *hits > 0 => {
                    let tally = if *max > 1 {
                        format!("  ×{hits}")
                    } else {
                        "  ×".to_string()
                    };
                    (tally, aero_crit_effect(label, *hits, *max))
                }
                CritRow::Weapon {
                    destroyed: true, ..
                } => ("  ×".to_string(), String::new()),
                _ => (String::new(), String::new()),
            };
            let mut spans = vec![Span::styled(format!("{marker}{}", row.label()), style)];
            if !tally.is_empty() {
                spans.push(Span::styled(
                    tally,
                    Style::default()
                        .fg(theme().danger)
                        .add_modifier(Modifier::BOLD),
                ));
            }
            if !effect.is_empty() {
                spans.push(Span::styled(
                    format!("  {effect}"),
                    Style::default().fg(theme().dim),
                ));
            }
            lines.push(Line::from(spans));
        }
    }
    lines.push(Line::from(Span::styled(
        "[↑↓] select   [Spc] mark/clear   [Esc] close",
        Style::default().fg(theme().dim),
    )));
    lines
}

/// A one-line plain-language note of what an aerospace critical's current hits do (TW effects the
/// tracker applies). Empty for systems with no on-map combat effect (Landing Gear).
fn aero_crit_effect(name: &str, hits: u8, max: u8) -> String {
    let destroyed = hits >= max;
    match name {
        "Engine" if destroyed => "thrust 0 — destroyed".into(),
        "Engine" => format!("−{} thrust, +{} heat", 2 * hits, 2 * hits),
        "Sensors" if destroyed => "+5 to-hit".into(),
        "Sensors" => format!("+{hits} to-hit"),
        "FCS" if hits > 2 => "weapons offline".into(),
        "FCS" => format!("+{} to-hit", 2 * hits),
        "Avionics" if destroyed => "+5 control".into(),
        "Avionics" => format!("+{hits} control"),
        _ => String::new(), // Landing Gear: no on-map combat effect
    }
}

/// Body lines for the Motive System Damage popup: the four table results (each annotated with its
/// MP/steering effect), the running motive state, and the keys.
fn motive_modal_lines(app: &App, sel: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let effect = |lvl: MotiveLevel| match lvl {
        MotiveLevel::Minor => "+1 steer",
        MotiveLevel::Moderate => "−1 MP, +2 steer",
        MotiveLevel::Heavy => "half MP, +3 steer",
        MotiveLevel::Immobilized => "immobile",
    };
    for (i, lvl) in MotiveLevel::ALL.iter().enumerate() {
        let selected = i == sel;
        let marker = if selected { "▶ " } else { "  " };
        let style = if selected {
            Style::default()
                .fg(theme().on_accent)
                .bg(theme().accent)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        lines.push(Line::from(vec![
            Span::styled(format!("{marker}{:<12}", lvl.label()), style),
            Span::styled(
                format!("  {}", effect(*lvl)),
                Style::default().fg(theme().dim),
            ),
        ]));
    }
    // Running tally: current MP loss + steering + the results in roll order.
    if let Some(tm) = app.session.active_mech() {
        let dmg = &tm.motive_damage;
        lines.push(Line::from(Span::raw("")));
        if dmg.is_empty() {
            lines.push(Line::from(Span::styled(
                "  no motive damage",
                Style::default().fg(theme().good),
            )));
        } else {
            let cruise = tm.motive_cruise();
            let names: Vec<&str> = dmg.iter().map(|l| l.label()).collect();
            let tail = if tm.motive_immobilized() {
                "  ** IMMOBILIZED **".to_string()
            } else {
                format!("  cruise {cruise}  steer +{}", tm.motive_steering())
            };
            lines.push(Line::from(Span::styled(
                format!("  {}{tail}", names.join(" · ")),
                Style::default()
                    .fg(theme().danger)
                    .add_modifier(Modifier::BOLD),
            )));
        }
    }
    lines.push(Line::from(Span::styled(
        "[↑↓] select   [Spc] apply   [r] repair last   [Esc] close",
        Style::default().fg(theme().dim),
    )));
    lines
}

/// The keybinding reference for the Alpha Strike screen.
fn as_help_modal_lines() -> Vec<Line<'static>> {
    let header = |s: &'static str| {
        Line::from(Span::styled(
            s,
            Style::default()
                .fg(theme().accent)
                .add_modifier(Modifier::BOLD),
        ))
    };
    let row = |keys: &'static str, desc: &'static str| {
        Line::from(vec![
            Span::styled(format!("  {keys:<9}"), Style::default().fg(theme().accent)),
            Span::styled(desc, Style::default().fg(theme().dim)),
        ])
    };
    vec![
        header("Alpha Strike"),
        row("Space", "1 damage (armor then structure)"),
        row("u", "repair 1"),
        row("o / i", "heat up / down"),
        row("c", "critical hits"),
        row("t", "to-hit shot (attacker move + target TMM)"),
        row("L", "log snapshot (game log)"),
        row("g", "edit pilot Skill"),
        row("b", "set force point limit (PV)"),
        row("1", "toggle 1:1 ground (hex) scale"),
        row(", / .", "previous / next unit  ([ / ] also)"),
        row("< / >", "jump 4 units (one card row)"),
        header("General"),
        row("a / D", "add / delete unit"),
        row("S", "sessions browser"),
        row("z", "undo"),
        row("^t", "display picker (theme + layout)"),
        row("q", "quit"),
        Line::from(Span::styled(
            "  press any key to close",
            Style::default().fg(theme().dim),
        )),
    ]
}

/// The full keybinding reference for the Override mode `?` modal. Override is its own ruleset, so it
/// gets its own keymap (the Classic sheet lists keys — dice ref, prone, GATOR, motive — that don't
/// apply here).
fn ov_help_modal_lines() -> Vec<Line<'static>> {
    let header = |s: &'static str| {
        Line::from(Span::styled(
            s,
            Style::default()
                .fg(theme().accent)
                .add_modifier(Modifier::BOLD),
        ))
    };
    let row = |keys: &'static str, desc: &'static str| {
        Line::from(vec![
            Span::styled(format!("  {keys:<9}"), Style::default().fg(theme().accent)),
            Span::styled(desc, Style::default().fg(theme().dim)),
        ])
    };
    vec![
        header("Armor panel (Tab to focus)"),
        row("Space", "damage region (armor then structure)"),
        row("u", "repair region"),
        row("f", "toggle front / rear armor"),
        row("c", "critical hits (region table)"),
        header("Weapons panel (Tab to focus)"),
        row("Space", "fire TIC (banks its heat)"),
        row("u", "un-fire TIC (refund heat)"),
        header("General"),
        row("Tab", "switch panel (armor / weapons)"),
        row("↑↓ / kj", "move selection in panel"),
        row("o / i", "heat up / down (0–5 ladder)"),
        row("v", "movement this turn (mode + hexes)"),
        row("t", "to-hit shot (target movement / state)"),
        row("e", "end turn (dissipate heat, clear fired)"),
        row("x", "toggle shutdown / restart"),
        row("p / P", "pilot / crew hit / heal"),
        row("g", "edit gunnery / piloting skills"),
        row("L", "log snapshot (game log)"),
        row("z", "undo"),
        row(", / .", "previous / next unit  ([ / ] also)"),
        row("a / D", "add / delete unit"),
        row("S", "sessions browser"),
        row("^t", "display picker (theme + layout)"),
        row("q", "quit"),
        Line::from(Span::styled(
            "  Override included with permission from Death From Above Wargaming.",
            Style::default().fg(theme().dim),
        )),
        Line::from(Span::styled(
            "  Learn more about Override & DFA at https://dfawargaming.com",
            Style::default().fg(theme().dim),
        )),
        Line::from(Span::styled(
            "  press any key to close",
            Style::default().fg(theme().dim),
        )),
    ]
}

fn draw_sessions(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(f.area());

    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(" Sessions ", Style::default().add_modifier(Modifier::BOLD)),
            Span::styled(
                format!("— current: {}", app.current_name),
                Style::default().fg(theme().dim),
            ),
        ])),
        chunks[0],
    );

    let area = chunks[1];
    if app.sessions.is_empty() {
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "  (no saved sessions — press [n] to start one)",
                Style::default().fg(theme().dim),
            ))),
            area,
        );
    } else {
        let rows = area.height as usize;
        let offset = app.sessions_sel.saturating_sub(rows.saturating_sub(1));
        let mut lines = Vec::new();
        for (i, m) in app.sessions.iter().enumerate().skip(offset).take(rows) {
            let selected = i == app.sessions_sel;
            let is_current = m.name == app.current_name;
            let marker = if selected { "▶ " } else { "  " };
            let dot = if is_current { "● " } else { "  " };
            let (tag, tag_color) = match m.mode {
                GameMode::AlphaStrike => ("AS", theme().warning),
                GameMode::StrategicBattleForce => ("SB", theme().warning),
                GameMode::BattleForce => ("BF", theme().warning),
                GameMode::AbstractCombatSystem => ("AC", theme().warning),
                GameMode::Classic => ("CL", theme().dim),
                GameMode::Override => ("OV", theme().accent),
            };
            let base = if selected {
                Style::default()
                    .fg(theme().on_accent)
                    .bg(theme().accent)
                    .add_modifier(Modifier::BOLD)
            } else if is_current {
                Style::default().fg(theme().accent)
            } else {
                Style::default()
            };
            let tag_style = if selected {
                base
            } else {
                Style::default().fg(tag_color)
            };
            // Force point total in the session's own system (BV for Classic, PV for AS), with the
            // budget when one is set (`· BV 5420/6000`).
            let label = match m.mode {
                GameMode::AlphaStrike
                | GameMode::StrategicBattleForce
                | GameMode::BattleForce
                | GameMode::AbstractCombatSystem => "PV",
                GameMode::Classic | GameMode::Override => "BV",
            };
            let total = match (m.force_total, m.limit) {
                (0, None) => String::new(),
                (t, Some(l)) => format!("  · {label} {t}/{l}"),
                (t, None) => format!("  · {label} {t}"),
            };
            lines.push(Line::from(vec![
                Span::styled(format!("{marker}{dot}"), base),
                Span::styled(format!("{tag}  "), tag_style),
                Span::styled(
                    format!("{}  ({})  {}{total}", m.name, m.mech_count, m.summary),
                    base,
                ),
            ]));
        }
        f.render_widget(Paragraph::new(lines), area);
    }

    let mut help = " [↑↓] sel  [Enter] load  [n] new  [A] AS  [O] OV  [B] SBF  [F] BF  [C] ACS  [r] rename  [D] del  [Esc] back ".to_string();
    if !app.status.is_empty() {
        help = format!("{help}| {} ", app.status);
    }
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            help,
            Style::default().fg(theme().dim),
        ))),
        chunks[2],
    );
}

// ---------- helpers ----------

/// Format an integer with comma thousands separators (9626000 -> "9,626,000").
fn thousands(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn frac_color(rem: u16, max: u16) -> Color {
    theme().frac(rem, max)
}

fn bar(rem: u16, max: u16, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    if max == 0 {
        return " ".repeat(width);
    }
    let filled = ((rem as f32 / max as f32) * width as f32).round() as usize;
    let filled = filled.min(width);
    format!("{}{}", "█".repeat(filled), "░".repeat(width - filled))
}

// ---------- picker ----------

fn draw_picker(f: &mut Frame, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(f.area());

    let query = Paragraph::new(Line::from(vec![
        Span::raw(" Search: "),
        Span::styled(
            format!("{}\u{2588}", app.picker.query),
            Style::default().fg(theme().accent),
        ),
    ]))
    .block(Block::default().borders(Borders::ALL).title(format!(
        " Pick a unit ({}){}{} ",
        picker_count(app),
        budget_summary(app),
        filter_summary(app)
    )));
    f.render_widget(query, chunks[0]);

    let area = chunks[1];
    let rows = area.height as usize;
    app.picker.page = rows.max(1); // keep PageUp/PageDown in step with what's on screen
    let sel = app.picker.selected;
    let offset = sel.saturating_sub(rows.saturating_sub(1));
    // §35: when the availability lens (faction/era) is on, tint each row's name by rarity.
    let lens = app.filters.avail_context();
    let mut lines = Vec::new();
    for (vis, &idx) in app
        .picker
        .filtered
        .iter()
        .enumerate()
        .skip(offset)
        .take(rows)
    {
        let selected = vis == sel;
        let marker = if selected { "▶ " } else { "  " };
        let Some(m) = app.bundle.get(idx) else {
            continue;
        };
        let model = if m.model.is_empty() {
            String::new()
        } else {
            format!(" {}", m.model)
        };
        // Role (blue) and year (green, last) get their own colors; a selected row is fully
        // highlighted instead so it stays readable. With the §35 lens on, an unselected row's name
        // is tinted to its rarity tier (most-common bright → rare red → unknown/not-avail dim).
        let (base, role_style, year_style) = if selected {
            let sel = Style::default()
                .fg(theme().on_accent)
                .bg(theme().accent)
                .add_modifier(Modifier::BOLD);
            (sel, sel, sel)
        } else {
            let name_style = match lens {
                Some((era, fac)) => Style::default().fg(rarity_color(m.rarity(era, fac))),
                None => Style::default(),
            };
            (
                name_style,
                Style::default().fg(theme().accent_alt),
                Style::default().fg(theme().good),
            )
        };
        // Infantry are sized by squad/platoon strength, not tonnage; AS-only hand-entered units
        // (emplacements) have neither, so show their AS type code (e.g. "BD").
        let size = if m.is_infantry() {
            format!("sq {}", m.internal)
        } else if m.is_as_only() {
            m.as_stats.tp.clone()
        } else {
            format!("{}t", m.tonnage)
        };
        // Nerd-Font mode prefixes a per-unit-type glyph; Ascii mode returns "" (rows unchanged).
        let ut = icons::unit_type(m.unit_type);
        let ut = if ut.is_empty() {
            String::new()
        } else {
            format!("{ut} ")
        };
        let mut spans = vec![
            Span::styled(
                format!("{marker}{ut}{}{}  {}  ", m.chassis, model, size),
                base,
            ),
            Span::styled(m.role.clone(), role_style),
        ];
        if m.year > 0 {
            spans.push(Span::styled(format!("  {}", m.year), year_style));
        }
        if m.bv > 0 {
            let bv_style = if selected {
                base
            } else {
                Style::default().fg(theme().dim)
            };
            spans.push(Span::styled(format!("  BV {}", m.bv), bv_style));
        }
        lines.push(Line::from(spans));
    }
    f.render_widget(Paragraph::new(lines), area);

    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "[type] search  [^f] filter  [^g] gen  [^b] budget  [↑↓] select  [Tab] prev  [Enter] add  [Esc] back",
            Style::default().fg(theme().dim),
        ))),
        chunks[2],
    );

    if app.show_preview {
        draw_preview(f, app);
    }
}

/// Picker count for the title: `"8706"`, or `"N of 8706"` when a query/filters narrow the list.
fn picker_count(app: &App) -> String {
    let total = app.names.len();
    let shown = app.picker.filtered.len();
    if shown == total {
        format!("{total}")
    } else {
        format!("{shown} of {total}")
    }
}

/// The force point total / budget for the picker title: `"   BV 5420/6000"` (or just the total
/// when there's no limit, empty when the force is empty and unlimited).
fn budget_summary(app: &App) -> String {
    let unit = match app.session.mode {
        GameMode::AlphaStrike
        | GameMode::StrategicBattleForce
        | GameMode::BattleForce
        | GameMode::AbstractCombatSystem => "PV",
        GameMode::Classic | GameMode::Override => "BV",
    };
    let total = app.session.force_total();
    match app.session.limit {
        Some(limit) => {
            let flag = if total > limit { "  OVER" } else { "" };
            format!("   {unit} {total}/{limit}{flag}")
        }
        None if total > 0 => format!("   {unit} {total}"),
        None => String::new(),
    }
}

/// The active-filter summary for the title (empty when no facet is set).
fn filter_summary(app: &App) -> String {
    if app.filters.is_empty() {
        String::new()
    } else {
        format!("   filters: {}", app.filters.summary())
    }
}

/// A popup with the highlighted unit's stats + weapons (toggled by Tab in the picker).
/// The detail lines for a unit (size/tech/role/year, move, points, armor/structure, transport,
/// the grouped weapon + equipment lists). Shared by the Tab preview popup and the pre-add modal
/// so both show the same summary.
fn preview_lines(m: &Mech) -> Vec<Line<'static>> {
    let label = Style::default().fg(theme().dim);
    let mut lines: Vec<Line> = Vec::new();

    let size = if m.is_infantry() {
        format!("squad {}", m.internal)
    } else {
        format!("{}t", m.tonnage)
    };
    let mut head = vec![Span::styled(
        format!(
            "{}   {}   {}",
            size,
            m.tech_base,
            if m.role.is_empty() { "—" } else { &m.role }
        ),
        label,
    )];
    if m.year > 0 {
        head.push(Span::styled(
            format!("   intro {}", m.year),
            Style::default().fg(theme().dim),
        ));
    }
    lines.push(Line::from(head));
    lines.push(Line::from(""));

    // One aligned grid for every stat: an 11-wide label gutter, then the value, with any
    // parenthetical detail dim. `stat` right-aligns numbers in a shared 4-wide column.
    let row = |name: &str, value: String, extra: &str| {
        let mut spans = vec![Span::styled(format!("{name:<11}{value}"), label)];
        if !extra.is_empty() {
            spans.push(Span::styled(
                format!("  ({extra})"),
                Style::default().fg(theme().dim),
            ));
        }
        Line::from(spans)
    };
    let stat = |name: &str, total: u16, kind: &str| {
        Line::from(vec![
            Span::styled(format!("{name:<11}{total:>4}  "), label),
            Span::styled(
                format!("({})", if kind.is_empty() { "—" } else { kind }),
                Style::default().fg(theme().dim),
            ),
        ])
    };
    if m.is_vehicle() {
        lines.push(row(
            "Move",
            format!("Cruise {}  Flank {}", m.walk, m.run),
            m.motive.map_or("Vehicle", |mt| mt.label()),
        ));
    } else if m.is_infantry() {
        // Infantry ground MP (run == walk); the AS movement string carries the mode (j/f/m/w).
        lines.push(row(
            "Move",
            format!("Walk {}  Jump {}", m.walk, m.jump),
            &m.as_stats.movement,
        ));
    } else if m.is_aerospace() {
        // Aerospace flies on thrust — the AS movement string (e.g. "7a"), no Classic walk/run/jump.
        lines.push(row("Move", m.as_stats.movement.clone(), "thrust"));
    } else {
        lines.push(row(
            "Move",
            format!("Walk {}  Run {}  Jump {}", m.walk, m.run, m.jump),
            "",
        ));
    }
    // Point costs: BV (Classic) + PV (Alpha Strike), and the C-bill price tag.
    let mut points = Vec::new();
    if m.bv > 0 {
        points.push(format!("BV {}", m.bv));
    }
    if m.as_stats.pv > 0 {
        points.push(format!("PV {}", m.as_stats.pv));
    }
    if !points.is_empty() {
        lines.push(row("Points", points.join("   "), ""));
    }
    if m.cost > 0 {
        lines.push(row("Cost", format!("{} C-bills", thousands(m.cost)), ""));
    }
    if m.unit_type == UnitType::BattleArmor {
        // Per-trooper suit armor (the doll tracks each suit separately).
        lines.push(stat("Armor", m.total_armor(), &m.armor_type));
        lines.push(row("Troopers", format!("{}", m.internal), ""));
        lines.push(row("Type", "Battle Armor".into(), ""));
    } else if m.unit_type == UnitType::Infantry {
        // No armor — one troop-strength pool.
        lines.push(row("Strength", format!("{} troopers", m.internal), ""));
        lines.push(row("Type", "Conventional Infantry".into(), ""));
    } else if m.is_aerospace() {
        // AS-only: no Classic armor doll, so show the Alpha Strike armor/structure ratings.
        lines.push(row("Armor", format!("{}", m.as_stats.armor), "AS"));
        lines.push(row("Structure", format!("{}", m.as_stats.structure), "AS"));
        lines.push(row("Type", "Aerospace fighter".into(), ""));
    } else {
        lines.push(stat("Armor", m.total_armor(), &m.armor_type));
        lines.push(stat("Structure", m.total_internal(), &m.structure_type));
        if m.is_vehicle() {
            lines.push(row("Type", "Combat vehicle".into(), ""));
        } else {
            lines.push(stat("Heat sinks", m.heat_sinks, m.heat_sink_type.label()));
        }
    }
    // Transport / storage bays (Infantry Compartment, Cargo, …), if any.
    if !m.transport.is_empty() {
        lines.push(row("Transport", m.transport.join(", "), ""));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Weapons",
        Style::default()
            .fg(theme().accent)
            .add_modifier(Modifier::BOLD),
    )));

    // Group identical weapons by name (record sheets expand qty into separate mounts). Infantry
    // weapons are one mount each with `count` = troopers wielding them, and `damage` is per trooper,
    // so use that count and show the group's damage (count × per, rounded) instead of "0.35".
    let infantry = m.unit_type == UnitType::Infantry; // conventional only; BA groups normally
    let mut group_dmg: Vec<String> = Vec::new();
    let mut groups: Vec<(&str, usize, u8, &str, &str)> = Vec::new();
    for w in &m.weapons {
        if !infantry {
            if let Some(g) = groups.iter_mut().find(|g| g.0 == w.name) {
                g.1 += 1;
                continue;
            }
            groups.push((&w.name, 1, w.heat, &w.damage, &w.range));
        } else {
            let dmg = w
                .damage
                .parse::<f32>()
                .map(|per| ((per * f32::from(w.count)).round() as u32).to_string())
                .unwrap_or_else(|_| w.damage.clone());
            group_dmg.push(dmg);
            groups.push((&w.name, w.count as usize, w.heat, "", &w.range));
        }
    }
    // For infantry, point each group's damage slot at the rounded group total computed above.
    if infantry {
        for (g, d) in groups.iter_mut().zip(group_dmg.iter()) {
            g.3 = d.as_str();
        }
    }
    if groups.is_empty() {
        lines.push(Line::from(Span::styled(
            "  (none)",
            Style::default().fg(theme().dim),
        )));
    }
    const MAX_ROWS: usize = 12;
    // Only 'Mechs track weapon heat; vehicles and infantry drop the column.
    let show_heat = !m.is_vehicle() && !m.is_infantry();
    for &(name, count, heat, dmg, range) in groups.iter().take(MAX_ROWS) {
        let mut spans = vec![Span::styled(
            format!("  {count:>2}× {:<18}", truncate(name, 18)),
            label,
        )];
        // Combat vehicles don't track heat — drop the column instead of leaving a gap.
        if show_heat {
            let heat = if heat == 0 {
                "—".to_string()
            } else {
                format!("H{heat}")
            };
            spans.push(Span::styled(
                format!("{heat:<3}    "),
                Style::default().fg(theme().danger),
            ));
        }
        spans.push(Span::styled(format!("{dmg:<6}"), label));
        spans.push(Span::styled(
            format!("  {range}"),
            Style::default().fg(theme().dim),
        ));
        lines.push(Line::from(spans));
    }
    if groups.len() > MAX_ROWS {
        lines.push(Line::from(Span::styled(
            format!("  … +{} more", groups.len() - MAX_ROWS),
            Style::default().fg(theme().dim),
        )));
    }

    // Equipment (gear only — heat sinks are summarized above). Group identical names, collecting
    // the locations they're mounted in.
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Equipment",
        Style::default()
            .fg(theme().accent)
            .add_modifier(Modifier::BOLD),
    )));
    let mut eq: Vec<(&str, usize, Vec<&str>)> = Vec::new();
    for e in &m.equipment {
        let code = e.location.code();
        if let Some(g) = eq.iter_mut().find(|g| g.0 == e.name) {
            g.1 += 1;
            if !g.2.contains(&code) {
                g.2.push(code);
            }
        } else {
            eq.push((&e.name, 1, vec![code]));
        }
    }
    if eq.is_empty() {
        lines.push(Line::from(Span::styled(
            "  (none)",
            Style::default().fg(theme().dim),
        )));
    }
    for (name, count, locs) in eq.iter().take(MAX_ROWS) {
        lines.push(Line::from(vec![
            Span::styled(format!("  {count:>2}× {:<20}  ", truncate(name, 20)), label),
            Span::styled(locs.join(" "), Style::default().fg(theme().dim)),
        ]));
    }
    if eq.len() > MAX_ROWS {
        lines.push(Line::from(Span::styled(
            format!("  … +{} more", eq.len() - MAX_ROWS),
            Style::default().fg(theme().dim),
        )));
    }

    lines
}

/// The Tab preview popup for the highlighted unit (the shared summary, in a bordered box).
fn draw_preview(f: &mut Frame, app: &App) {
    let Some(m) = app.picker.current().and_then(|idx| app.bundle.get(idx)) else {
        return;
    };
    let lines = preview_lines(m);
    let area = centered_rect(66, (lines.len() as u16 + 2).min(f.area().height), f.area());
    f.render_widget(Clear, area);
    f.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Double)
                .border_style(Style::default().fg(theme().accent))
                .title(format!(" {} ", m.display_name())),
        ),
        area,
    );
}

// ---------- tracker ----------

fn draw_tracker(f: &mut Frame, area: Rect, sidebar: bool, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            // The sidebar already lists the whole force, so drop the top tabs when it's shown.
            Constraint::Length(u16::from(!sidebar)),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(area);

    draw_roster(f, chunks[0], app);

    let Some(tm) = app.session.active_mech() else {
        f.render_widget(
            Paragraph::new("No mech loaded. Press [a] to add one.").alignment(Alignment::Center),
            chunks[1],
        );
        draw_status(f, chunks[2], app);
        return;
    };

    let main = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(chunks[1]);

    draw_doll(f, main[0], app, tm);

    let right = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(8),
            Constraint::Length(4),
            Constraint::Length(5),
            Constraint::Min(1),
        ])
        .split(main[1]);
    if tm.spec.is_vehicle() {
        // Vehicles have no heat: crits panel, crew (instead of pilot), movement.
        draw_vehicle_crits(f, right[0], tm);
        draw_crew(f, right[1], tm);
    } else if tm.spec.is_infantry() {
        // Infantry have no heat or pilot: a troop-strength panel + the squad's skills.
        draw_troops(f, right[0], tm);
        draw_infantry_skills(f, right[1], tm);
    } else {
        draw_heat(f, right[0], tm);
        draw_pilot(f, right[1], tm);
    }
    draw_move(f, right[2], tm);
    draw_equip(f, right[3], app, tm);

    draw_status(f, chunks[2], app);
}

// ---------- Alpha Strike card ----------

fn draw_alpha_strike(f: &mut Frame, area: Rect, sidebar: bool, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(u16::from(!sidebar)),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(area);
    draw_roster(f, chunks[0], app);

    let as_help = " Spc:dmg  u:repair  o/i:heat  c:crits  t:to-hit  1:scale  ,/.:unit  a:add  D:del  S:sess  ?:help ";
    if app.session.mechs.is_empty() {
        f.render_widget(
            Paragraph::new("No unit. Press [a] to add one.").alignment(Alignment::Center),
            chunks[1],
        );
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                as_help,
                Style::default().fg(theme().dim),
            ))),
            chunks[2],
        );
        return;
    }

    // A grid of cards. Pi shows a fixed 2×2; Modern fits as many as the area affords (2–4 per axis,
    // up to a 4×4 = 16-card company). The active unit's page is shown; its card is highlighted.
    let active = app.session.active;
    const CARD_MIN_W: usize = 38;
    const CARD_MIN_H: usize = 14;
    let (cols, rows_n) = if profile() == DisplayProfile::Modern {
        let c = (chunks[1].width as usize / CARD_MIN_W).clamp(2, 4);
        let r = (chunks[1].height as usize / CARD_MIN_H).clamp(2, 4);
        (c, r)
    } else {
        (2, 2)
    };
    let per_page = cols * rows_n;
    let start = (active / per_page) * per_page;
    let row_rects = Layout::default()
        .direction(Direction::Vertical)
        .constraints(vec![Constraint::Ratio(1, rows_n as u32); rows_n])
        .split(chunks[1]);
    let ground_scale = app.session.as_ground_scale;
    for r in 0..rows_n {
        let col_rects = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(vec![Constraint::Ratio(1, cols as u32); cols])
            .split(row_rects[r]);
        for c in 0..cols {
            let idx = start + r * cols + c;
            if let Some(tm) = app.session.mechs.get(idx) {
                draw_as_card(f, col_rects[c], tm, idx == active, ground_scale);
            }
        }
    }

    // In 1:1 ground scale, show the scale + hex range brackets; the per-card MV is in hexes too.
    let base = if ground_scale {
        format!(
            " 1:1 hex scale — Rng {}  [1] inches  [?] keys ",
            range_brackets_hexes()
        )
    } else {
        String::from(as_help)
    };
    // With a variable page size, show which page of the roster is in view — prefixed so it stays
    // visible even when the help text runs wider than the screen.
    let total_pages = app.session.mechs.len().div_ceil(per_page);
    let mut help = if total_pages > 1 {
        format!(" page {}/{} ·{base}", start / per_page + 1, total_pages)
    } else {
        base
    };
    if !app.status.is_empty() {
        help = format!(" {} | {}", app.status, help.trim_start());
    }
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            help,
            Style::default().fg(theme().dim),
        ))),
        chunks[2],
    );
}

/// One Alpha Strike card in the 2×2 grid. The active unit gets a bold cyan double border.
fn draw_as_card(f: &mut Frame, area: Rect, tm: &TrackedMech, active: bool, ground_scale: bool) {
    let border_style = if tm.as_destroyed() {
        Style::default()
            .fg(theme().danger)
            .add_modifier(Modifier::BOLD)
    } else if active {
        Style::default()
            .fg(theme().accent)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme().dim)
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(if active {
            BorderType::Double
        } else {
            BorderType::Plain
        })
        .border_style(border_style)
        .title(format!(" {} ", tm.spec.display_name()));
    f.render_widget(
        Paragraph::new(as_card_lines(tm, ground_scale)).block(block),
        area,
    );
}

/// The compact card content (unit name is in the border title). At 1:1 `ground_scale`, movement
/// is shown in hexes (halved, rounded up) instead of inches.
fn as_card_lines(tm: &TrackedMech, ground_scale: bool) -> Vec<Line<'static>> {
    let a = &tm.spec.as_stats;
    let gray = Style::default().fg(theme().dim);
    let mut lines: Vec<Line> = Vec::new();

    let mv = if ground_scale {
        movement_hexes(&a.movement)
    } else {
        a.movement.clone()
    };
    // Size 0 isn't a real AS size (e.g. gun emplacements) — print it as "-" like the card.
    let sz = if a.size == 0 {
        "-".to_string()
    } else {
        a.size.to_string()
    };
    // Aerospace carry an armor Threshold (TH) instead of relying on TMM alone; show it when present.
    let th = if a.threshold > 0 {
        format!("   TH {}", a.threshold)
    } else {
        String::new()
    };
    lines.push(Line::from(Span::styled(
        format!(
            "PV {}   SZ {}   {}   MV {}   TMM {}{}",
            a.pv, sz, a.tp, mv, a.tmm, th
        ),
        Style::default().fg(theme().dim),
    )));
    // Skill + skill-adjusted PV (the single Alpha Strike Skill is the gunnery field; edit with `g`).
    let adj_pv = skill_adjusted_pv(u32::from(a.pv), tm.gunnery);
    lines.push(Line::from(vec![
        Span::styled("Skill ", Style::default().fg(theme().dim)),
        Span::styled(
            format!("{}+", tm.gunnery),
            Style::default()
                .fg(theme().accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("   PV ", Style::default().fg(theme().dim)),
        Span::styled(
            format!("{adj_pv}"),
            Style::default()
                .fg(theme().accent)
                .add_modifier(Modifier::BOLD),
        ),
    ]));
    if tm.as_destroyed() {
        lines.push(Line::from(Span::styled(
            "*** DESTROYED ***",
            Style::default()
                .fg(theme().danger)
                .add_modifier(Modifier::BOLD | Modifier::REVERSED),
        )));
    } else if tm.as_shutdown() {
        lines.push(Line::from(Span::styled(
            "*** SHUTDOWN (heat) ***",
            Style::default()
                .fg(theme().warning)
                .add_modifier(Modifier::BOLD | Modifier::REVERSED),
        )));
    }
    lines.push(Line::from(""));

    let pip_row = |label: &str, rem: u8, max: u8| {
        Line::from(vec![
            Span::styled(format!("{label:<10}"), gray),
            Span::styled(
                "█".repeat(rem as usize),
                Style::default().fg(frac_color(rem as u16, max as u16)),
            ),
            Span::styled(
                "░".repeat(max.saturating_sub(rem) as usize),
                Style::default().fg(theme().dim),
            ),
            Span::styled(format!("  {rem}/{max}"), Style::default().fg(theme().dim)),
        ])
    };
    lines.push(pip_row("Armor", tm.as_armor_remaining(), a.armor));
    lines.push(pip_row("Structure", tm.as_struct_remaining(), a.structure));
    lines.push(Line::from(""));

    let mut heat = vec![Span::styled(format!("{:<10}", "Heat"), gray)];
    // The Alpha Strike heat scale is 1 2 3 S — the 4th box (heat 4) is Shutdown.
    for h in 0..=4u8 {
        let label = if h == 4 {
            "S".to_string()
        } else {
            h.to_string()
        };
        if h == tm.as_heat {
            heat.push(Span::styled(
                format!("[{label}]"),
                Style::default()
                    .fg(theme().danger)
                    .add_modifier(Modifier::BOLD | Modifier::REVERSED),
            ));
        } else {
            heat.push(Span::styled(
                format!(" {label} "),
                Style::default().fg(theme().dim),
            ));
        }
    }
    if a.overheat > 0 {
        heat.push(Span::styled(
            format!("  OV {}", a.overheat),
            Style::default().fg(theme().warning),
        ));
    }
    lines.push(Line::from(heat));

    let dmg = |label: &str, v: &str| {
        let style = if v == "0" {
            Style::default().fg(theme().dim)
        } else {
            gray
        };
        Span::styled(format!("{label}{v:<4}"), style)
    };
    lines.push(Line::from(vec![
        Span::styled(format!("{:<10}", "Damage"), gray),
        dmg("S ", &a.dmg_s),
        dmg("M ", &a.dmg_m),
        dmg("L ", &a.dmg_l),
        dmg("E ", &a.dmg_e),
    ]));

    // §33 to-hit target per range bracket. Phase 1 is the "self" number (skill + range + heat +
    // FC×2 [+ crew×2]); Phase 2 folds in attacker jump (+2) and the hand-entered target's TMM when a
    // shot context is set. Brackets where the unit does 0 damage show "—". Skipped entirely when
    // destroyed/shutdown — no shot to make, and that frees the line the banner consumes on the tight
    // 2×2 card.
    let to_hit_shown = !tm.as_destroyed() && !tm.as_shutdown();
    if to_hit_shown {
        let fc = tm.as_crit(AsCritKind::FireControl);
        let is_veh = tm.spec.is_vehicle();
        let tgt = tm.as_target.map(|t| (t.tmm, t.jumped, t.immobile));
        let to_hit = |label: &str, idx: usize, dv: &str| {
            if dv == "0" {
                return Span::styled(
                    format!("{label}{:<4}", "—"),
                    Style::default().fg(theme().dim),
                );
            }
            let n = as_to_hit_full(
                tm.gunnery,
                idx,
                tm.as_heat,
                fc,
                tm.crew_hits,
                is_veh,
                tm.as_attacker_jumped,
                tgt,
            );
            Span::styled(format!("{label}{:<4}", format!("{n}+")), gray)
        };
        lines.push(Line::from(vec![
            Span::styled(format!("{:<10}", "To-Hit"), gray),
            to_hit("S ", 0, &a.dmg_s),
            to_hit("M ", 1, &a.dmg_m),
            to_hit("L ", 2, &a.dmg_l),
            to_hit("E ", 3, &a.dmg_e),
        ]));
    }

    let mut crits = vec![Span::styled(format!("{:<10}", "Crits"), gray)];
    for &kind in tm.as_crit_kinds() {
        let n = tm.as_crit(kind);
        let abbr = match kind {
            AsCritKind::Engine => "Eng",
            AsCritKind::FireControl => "FC",
            AsCritKind::Mp => "MP",
            AsCritKind::Weapon => "Wpn",
            AsCritKind::Motive => "Mot",
        };
        let style = if n > 0 {
            Style::default().fg(theme().danger)
        } else {
            Style::default().fg(theme().dim)
        };
        crits.push(Span::styled(format!("{abbr}{n}  "), style));
    }
    lines.push(Line::from(crits));
    // When a shot context is set (and the To-Hit row is shown), this line summarises the inputs the
    // To-Hit row folds in; it reuses the otherwise-blank spacer so the card height is unchanged.
    if to_hit_shown && tm.as_shot_active() {
        let mut parts = Vec::new();
        if tm.as_attacker_jumped {
            parts.push("atk jump".to_string());
        }
        if let Some(t) = tm.as_target {
            parts.push(if t.immobile {
                "tgt immobile".to_string()
            } else if t.jumped {
                format!("tgt TMM{} jumped", t.tmm)
            } else {
                format!("tgt TMM{}", t.tmm)
            });
        }
        lines.push(Line::from(vec![
            Span::styled(format!("{:<10}", "Shot"), gray),
            Span::styled(parts.join("   "), Style::default().fg(theme().warning)),
        ]));
    } else {
        lines.push(Line::from(""));
    }

    let specials = if a.specials.is_empty() {
        "—".to_string()
    } else {
        a.specials.join("  ")
    };
    lines.push(Line::from(vec![
        Span::styled(format!("{:<10}", "Specials"), gray),
        Span::styled(specials, Style::default().fg(theme().good)),
    ]));
    lines
}

// ---------- Override mode (live tracker) ----------

const OVERRIDE_HELP: &str =
    " Spc:dmg/fire  u:rep/unfire  o/i:heat  c:crit  v:move  t:to-hit  f:face  Tab:panel  e:end  p:pilot  ?:help ";

/// The Override condition-monitor consciousness target numbers, in order (the printed card's track).
const OV_CONDITION: [&str; 6] = ["3+", "5+", "7+", "9+", "11+", "KIA"];

fn draw_override(f: &mut Frame, area: Rect, sidebar: bool, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(u16::from(!sidebar)),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(area);
    draw_roster(f, chunks[0], app);

    let Some(tm) = app.session.active_mech() else {
        f.render_widget(
            Paragraph::new("No unit. Press [a] to add one.").alignment(Alignment::Center),
            chunks[1],
        );
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                OVERRIDE_HELP,
                Style::default().fg(theme().dim),
            ))),
            chunks[2],
        );
        return;
    };

    let card = tm.ov_card();
    let vehicle = tm.spec.is_vehicle();

    // The armor diagram is the primary spatial doll (left, like the Classic record sheet — the
    // Override 'Mech doll is the same minus side torsos). The weapons/heat/condition panel sits to
    // the right.
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(chunks[1]);
    draw_override_doll(f, cols[0], app, tm, &card);
    draw_override_panel(f, cols[1], app, tm, &card, vehicle);

    // Status leads; help fills the rest of the footer.
    let mut spans = vec![Span::raw(" ")];
    if !app.status.is_empty() {
        spans.push(Span::styled(
            format!("{} ", app.status),
            Style::default()
                .fg(theme().accent)
                .add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled("| ", Style::default().fg(theme().dim)));
    }
    spans.push(Span::styled(
        OVERRIDE_HELP.trim_start(),
        Style::default().fg(theme().dim),
    ));
    f.render_widget(Paragraph::new(Line::from(spans)), chunks[2]);
}

/// The Override armor diagram as a spatial paper doll (mirrors the Classic [`draw_doll`]). Regions
/// are placed by their shared `grid_pos`; the three 'Mech torsos are one merged `CenterTorso` box,
/// so the side-torso slots simply stay empty.
fn draw_override_doll(f: &mut Frame, area: Rect, app: &App, tm: &TrackedMech, card: &OverrideCard) {
    let vehicle = tm.spec.is_vehicle();
    let row_c: &[Constraint] = if vehicle {
        &[Constraint::Ratio(1, 4); 4]
    } else {
        &[
            Constraint::Percentage(30),
            Constraint::Percentage(40),
            Constraint::Percentage(30),
        ]
    };
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints(row_c)
        .split(area);
    let cols = |r: Rect| {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Ratio(1, 5); 5])
            .split(r)
    };
    let grid: Vec<_> = rows.iter().map(|r| cols(*r)).collect();

    let destroyed = tm.ov_destroyed_reason();
    if let Some(reason) = destroyed {
        // Collapse the middle row's centre cells into a DESTROYED banner (as the Classic doll does).
        let banner = Rect {
            x: grid[1][1].x,
            y: grid[1][1].y,
            width: (grid[1][3].x + grid[1][3].width).saturating_sub(grid[1][1].x),
            height: grid[1][1].height,
        };
        draw_destroyed_banner(f, banner, reason);
    }

    for r in &card.armor {
        let (row, col) = super::app::grid_pos(r.loc);
        if destroyed.is_some() && row == 1 && (1..=3).contains(&col) {
            continue;
        }
        if let Some(cell) = grid.get(row as usize).and_then(|g| g.get(col as usize)) {
            draw_override_box(f, *cell, app, tm, r);
        }
    }
}

/// One Override armor-diagram region box: armor / rear / structure pip bars (remaining/max), the
/// fixed 2d6 hit-location number in the title, a `*` when the region has marked crits, and the
/// Classic doll's cyan focus border + front/rear facing indicator.
fn draw_override_box(f: &mut Frame, area: Rect, app: &App, tm: &TrackedMech, r: &ArmorRegion) {
    let focused = app.focus == Focus::Doll && app.cursor == r.loc;
    let a_rem = tm.ov_armor_remaining(r.loc);
    let s_rem = tm.ov_struct_remaining(r.loc);
    let (rr_rem, rr_max) = match r.rear {
        Some(m) => (tm.ov_rear_remaining(r.loc), m),
        None => (0, 0),
    };
    let destroyed = tm.ov_loc_destroyed(r.loc);
    let has_crit = tm.ov_crits.get(&r.loc).is_some_and(|v| !v.is_empty());

    let border_style = if focused {
        Style::default()
            .fg(theme().accent)
            .add_modifier(Modifier::BOLD)
    } else if destroyed {
        Style::default().fg(theme().dim)
    } else {
        Style::default().fg(frac_color(
            a_rem + s_rem + rr_rem,
            r.armor + r.structure + rr_max,
        ))
    };
    let code = r.loc.code();
    let mut title = if has_crit {
        format!("*{code}")
    } else {
        code.to_string()
    };
    if focused && r.rear.is_some() {
        title.push_str(if app.facing == Facing::Rear {
            " ▸R"
        } else {
            " ▸F"
        });
    } else if let Some(n) = hit_location(r.loc) {
        title.push_str(&format!(" {n}"));
    }
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(if focused {
            BorderType::Double
        } else {
            BorderType::Plain
        })
        .border_style(border_style)
        .title(title);

    let inner_w = area.width.saturating_sub(2) as usize;
    let mut lines = Vec::new();
    let push_stat = |lines: &mut Vec<Line>, label: &str, rem: u16, max: u16, hi: bool| {
        let col = frac_color(rem, max);
        let mut s = Style::default().fg(col);
        if hi {
            s = s.add_modifier(Modifier::BOLD | Modifier::REVERSED);
        }
        lines.push(Line::from(Span::styled(format!("{label} {rem}/{max}"), s)));
        lines.push(Line::from(Span::styled(
            bar(rem, max, inner_w),
            Style::default().fg(col),
        )));
    };
    let front_hi = focused && app.facing == Facing::Front;
    if r.armor > 0 {
        push_stat(&mut lines, "A", a_rem, r.armor, front_hi);
    }
    if let Some(m) = r.rear {
        let rear_hi = focused && app.facing == Facing::Rear;
        push_stat(&mut lines, "R", rr_rem, m, rear_hi);
    }
    if r.structure > 0 {
        push_stat(&mut lines, "S", s_rem, r.structure, false);
    }
    f.render_widget(Paragraph::new(lines).block(block), area);
}

/// Right panel: unit data, the live weapons (TIC) list, the selected weapon's range/to-hit detail,
/// the condition monitor + crit-effects summary, and the 0–5 heat ladder (mech/aero only).
fn draw_override_panel(
    f: &mut Frame,
    area: Rect,
    app: &App,
    tm: &TrackedMech,
    card: &OverrideCard,
    vehicle: bool,
) {
    // Reserve a fixed heat-ladder block at the bottom for mech/aero; vehicles use the whole height.
    let split = if card.unit.heat_scale {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(8)])
            .split(area)
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1)])
            .split(area)
    };

    let u = &card.unit;
    let gray = Style::default().fg(theme().dim);
    let dim = Style::default().fg(theme().dim);
    let mut lines: Vec<Line> = Vec::new();

    lines.push(Line::from(vec![
        Span::styled("Type ", dim),
        Span::styled(format!("{}  ", u.type_label), gray),
        Span::styled("Mass ", dim),
        Span::styled(format!("{}t", u.mass), gray),
    ]));
    let mv_label = if u.aero { "Thrust" } else { "Move" };
    let mut mv = vec![
        Span::styled(format!("{mv_label} "), dim),
        Span::styled(format!("{:<10}", u.move_line), gray),
        Span::styled("TMM ", dim),
        Span::styled(u.tmm_line.clone(), gray),
    ];
    if let Some(d) = u.d_thr {
        mv.push(Span::styled(format!("  DThr {d}"), dim));
    }
    lines.push(Line::from(mv));
    if u.heat_scale {
        let mut heat = vec![
            Span::styled("Heat ", dim),
            Span::styled(
                format!("{}/{}", tm.ov_heat, OV_HEAT_MAX),
                Style::default()
                    .fg(ov_heat_color(tm.ov_heat))
                    .add_modifier(Modifier::BOLD),
            ),
        ];
        if let Some(s) = u.sinks {
            heat.push(Span::styled(format!("  Sinks {s}"), dim));
        }
        if tm.ov_shutdown() {
            heat.push(Span::styled(
                "  SHUTDOWN",
                Style::default().fg(theme().warning),
            ));
        }
        lines.push(Line::from(heat));
    }
    // This-phase movement (set with `v`): mode + the attacker to-hit modifier it implies, plus any
    // heat/crit move-TMM penalty.
    let (mv_label, atk_mod) = match tm.move_mode {
        neurohelmet_core::engine::MoveMode::Stationary => ("standstill", -1),
        neurohelmet_core::engine::MoveMode::Jumped => ("jumped", 2),
        neurohelmet_core::engine::MoveMode::Ran => ("ran", 0),
        neurohelmet_core::engine::MoveMode::Walked => ("walked", 0),
    };
    let mut move_spans = vec![
        Span::styled("Moved ", dim),
        Span::styled(format!("{mv_label} ({atk_mod:+} hit)"), gray),
    ];
    let (mp, tp) = tm.ov_move_penalty();
    if mp != 0 {
        move_spans.push(Span::styled(
            format!("  -{mp} move/-{tp} TMM"),
            Style::default().fg(theme().danger),
        ));
    }
    lines.push(Line::from(move_spans));
    lines.push(Line::from(""));

    // Weapons (TIC) list — compact (name / Dmg / Ht); per-bracket range + to-hit is shown below for
    // the selected weapon.
    let ht_hdr = if vehicle {
        String::new()
    } else {
        "Ht".to_string()
    };
    lines.push(Line::from(Span::styled(
        format!("  {:<15}{:<5}{ht_hdr}", "Weapons", "Dmg"),
        Style::default().fg(theme().accent),
    )));
    if card.tics.is_empty() {
        lines.push(Line::from(Span::styled("  (no weapons)", dim)));
    }
    for (i, t) in card.tics.iter().enumerate() {
        let fired = tm.ov_fired.contains(&i);
        let selected = app.focus == Focus::Equipment && i == app.ov_tic;
        let cur = if i == app.ov_tic { "▶" } else { " " };
        let mark = if fired { "✓" } else { " " };
        let ht = if vehicle {
            String::new()
        } else {
            t.heat.to_string()
        };
        let body = format!(
            "{cur}{mark}{:<15}{:<5}{ht}",
            trunc(&t.name, 14),
            trunc(&t.damage, 5)
        );
        let mut style = if fired { dim } else { gray };
        if selected {
            style = Style::default()
                .fg(theme().on_accent)
                .bg(theme().accent)
                .add_modifier(Modifier::BOLD);
        }
        lines.push(Line::from(Span::styled(body, style)));
    }
    lines.push(Line::from(""));

    // Selected-weapon detail: the printed range-bracket modifiers + the live per-bracket To-Hit.
    if let Some(t) = card.tics.get(app.ov_tic) {
        lines.push(Line::from(Span::styled(
            format!("▶ {} ({})", trunc(&t.name, 24), t.location),
            gray,
        )));
        lines.push(Line::from(Span::styled(
            format!("  Rng  {:<4}{:<4}{:<4}{:<4}{:<4}", "PB", "S", "M", "L", "X"),
            dim,
        )));
        lines.push(Line::from(Span::styled(
            format!("  mod  {:<4}{:<4}{:<4}{:<4}{:<4}", t.pb, t.s, t.m, t.l, t.x),
            gray,
        )));
        let cell = |b: &str| match override_conv::parse_bracket(b) {
            Some(m) => format!("{}+", tm.ov_to_hit(m)),
            None => "—".to_string(),
        };
        lines.push(Line::from(Span::styled(
            format!(
                "  hit  {:<4}{:<4}{:<4}{:<4}{:<4}",
                cell(&t.pb),
                cell(&t.s),
                cell(&t.m),
                cell(&t.l),
                cell(&t.x)
            ),
            Style::default().fg(theme().warning),
        )));
    }
    if tm.ov_shot_active() {
        lines.push(Line::from(vec![
            Span::styled("Shot ", dim),
            Span::styled(ov_shot_summary(tm), Style::default().fg(theme().warning)),
        ]));
    }
    // Physical-attack damage (mech only): Punch ⌈Mass/30⌉ / Kick ⌈Mass/15⌉.
    if let Some((punch, kick)) = override_conv::override_physicals(&tm.spec) {
        lines.push(Line::from(vec![
            Span::styled("Punch/Kick ", dim),
            Span::styled(format!("{punch} / {kick}"), gray),
        ]));
    }
    lines.push(Line::from(""));

    if !card.equipment.is_empty() {
        lines.push(Line::from(vec![
            Span::styled("Equip ", dim),
            Span::styled(trunc(&card.equipment.join(", "), 32), gray),
        ]));
    }
    lines.push(ov_condition_line(tm, vehicle));
    if let Some(summary) = ov_crit_summary(tm) {
        lines.push(Line::from(vec![
            Span::styled("Crits ", dim),
            Span::styled(summary, Style::default().fg(theme().danger)),
        ]));
    }
    // PSR / morale prompts (end-of-phase reminders) when owed.
    let warn = Style::default()
        .fg(theme().danger)
        .add_modifier(Modifier::BOLD);
    if let Some(reason) = tm.ov_psr_auto_fail() {
        lines.push(Line::from(Span::styled(
            format!("⚠ PSR auto-fail: {reason}"),
            warn,
        )));
    } else {
        let sits = tm.ov_psr_situations();
        if !sits.is_empty() {
            lines.push(Line::from(Span::styled(
                format!("⚠ PSR {}+: {}", tm.ov_psr_target(), sits.join(", ")),
                warn,
            )));
        }
    }
    if tm.ov_crippled() {
        lines.push(Line::from(Span::styled(
            format!("⚠ Morale {}+ (crippled)", tm.ov_morale_target()),
            Style::default()
                .fg(theme().warning)
                .add_modifier(Modifier::BOLD),
        )));
    }

    let panel_focused = app.focus == Focus::Equipment;
    let border = if panel_focused {
        Style::default()
            .fg(theme().accent)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme().dim)
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(if panel_focused {
            BorderType::Double
        } else {
            BorderType::Plain
        })
        .border_style(border)
        .title(format!(" {} ", tm.spec.display_name()));
    f.render_widget(Paragraph::new(lines).block(block), split[0]);

    if card.unit.heat_scale {
        draw_override_heat_ladder(f, split[1], tm);
    }
}

/// The condition-monitor line: the fixed consciousness track with the taken hits marked off.
fn ov_condition_line(tm: &TrackedMech, vehicle: bool) -> Line<'static> {
    let (label, hits) = if vehicle {
        ("Crew", tm.crew_hits)
    } else {
        ("Pilot", tm.pilot_hits)
    };
    let mut spans = vec![Span::styled(
        format!("{label:<6}"),
        Style::default().fg(theme().dim),
    )];
    for (i, tn) in OV_CONDITION.iter().enumerate() {
        let taken = (i as u8) < hits;
        let style = if taken {
            Style::default()
                .fg(theme().danger)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme().dim)
        };
        let cell = if taken {
            format!("[{tn}]")
        } else {
            format!(" {tn} ")
        };
        spans.push(Span::styled(cell, style));
    }
    Line::from(spans)
}

/// One-line summary of the marked crit effects (the applied move/TMM/heat penalties plus the
/// player-resolved results), or `None` when no crits are marked.
fn ov_crit_summary(tm: &TrackedMech) -> Option<String> {
    let fx = tm.ov_crit_effects();
    if !fx.any() {
        return None;
    }
    let mut parts: Vec<String> = Vec::new();
    if fx.move_penalty != 0 {
        parts.push(format!("-{} move/-{} TMM", fx.move_penalty, fx.tmm_penalty));
    }
    if fx.engine_hits > 0 {
        parts.push(format!("+{} heat/turn", fx.engine_hits));
    }
    if fx.gyro_hits == 1 {
        parts.push("gyro +2 PSR".into());
    } else if fx.gyro_hits >= 2 {
        parts.push("gyro: fall".into());
    }
    if fx.weapon_hits > 0 {
        parts.push(format!("{}× weapon", fx.weapon_hits));
    }
    if fx.crew_pilot_hits > 0 {
        parts.push(format!("{}× crew hit", fx.crew_pilot_hits));
    }
    if fx.stunned {
        parts.push("stunned".into());
    }
    if fx.avionics {
        parts.push("avionics +2 PSR".into());
    }
    if fx.ammo_marked {
        // A live ammo crit detonates; a spent/no-ammo one is a dud (becomes a weapon result).
        parts.push(if tm.ov_ammo_exploded() {
            "ammo EXPLOSION".into()
        } else {
            "ammo (dud)".into()
        });
    }
    if fx.fuel_marked {
        parts.push("fuel".into());
    }
    Some(parts.join(", "))
}

/// Short summary of the current Override shot context (attacker move + target), for the panel.
fn ov_shot_summary(tm: &TrackedMech) -> String {
    let s = &tm.ov_shot;
    let mut parts: Vec<String> = Vec::new();
    if s.target_immobile {
        parts.push("tgt immobile".into());
    } else if s.target_tmm > 0 || s.target_jumped {
        parts.push(format!(
            "tgt TMM{}{}",
            s.target_tmm,
            if s.target_jumped { "+j" } else { "" }
        ));
    }
    if s.secondary {
        parts.push("secondary".into());
    }
    if s.rear {
        parts.push("rear".into());
    }
    if parts.is_empty() {
        "—".into()
    } else {
        parts.join(", ")
    }
}

/// The 0–5 Override heat ladder (mech/aero), with the current level highlighted.
fn draw_override_heat_ladder(f: &mut Frame, area: Rect, tm: &TrackedMech) {
    let ladder = [
        (5u8, "Automatic Shutdown", theme().danger),
        (4, "Ammo Explosion (8+)", theme().danger),
        (3, "Shutdown (avoid 8+)", theme().warning),
        (2, "+1 Ranged Attack", theme().warning),
        (1, "-2 Move / -1 TMM", theme().good),
        (0, "No Effects", theme().dim),
    ];
    let lines: Vec<Line> = ladder
        .iter()
        .map(|(n, label, col)| {
            let here = *n == tm.ov_heat;
            let num = if here {
                Span::styled(
                    format!("[{n}]"),
                    Style::default()
                        .fg(theme().on_accent)
                        .bg(*col)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                Span::styled(format!(" {n} "), Style::default().fg(*col))
            };
            let label_style = if here {
                Style::default()
                    .fg(theme().fg_strong)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme().dim)
            };
            Line::from(vec![num, Span::styled(format!(" {label}"), label_style)])
        })
        .collect();
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme().dim))
        .title(" Heat ");
    f.render_widget(Paragraph::new(lines).block(block), area);
}

/// Heat-readout color for the 0–5 Override ladder (green low, yellow mid, red at shutdown).
fn ov_heat_color(heat: u8) -> Color {
    theme().heat_ov(heat)
}

/// Body lines for the Override per-region crit popup: the region's crit table with each result's
/// roll + effect, marked results flagged, the selected row highlighted, and a footer hint.
fn ov_crit_modal_lines(app: &App, loc: Location, sel: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let Some(tm) = app.session.active_mech() else {
        return lines;
    };
    let Some(table) = override_conv::crit_table(&tm.spec, loc) else {
        return lines;
    };
    lines.push(Line::from(Span::styled(
        format!("{} — roll 2d6 (8+ confirms)", loc.label()),
        Style::default()
            .fg(theme().accent)
            .add_modifier(Modifier::BOLD),
    )));
    for (i, r) in table.iter().enumerate() {
        let count = tm.ov_crit_count(loc, i as u8);
        let selected = i == sel;
        let marker = if selected { "▶ " } else { "  " };
        // Show the recorded hit count (results stack): "✗2" for two, "✗ " for one, blank for none.
        let hit = match count {
            0 => "  ".to_string(),
            1 => "✗ ".to_string(),
            n => format!("✗{n}"),
        };
        let mut style = if count > 0 {
            Style::default().fg(theme().danger)
        } else {
            Style::default()
        };
        if selected {
            style = Style::default()
                .fg(theme().on_accent)
                .bg(theme().accent)
                .add_modifier(Modifier::BOLD);
        }
        lines.push(Line::from(Span::styled(
            format!("{marker}{hit}{:<5} {}", r.roll, r.effect),
            style,
        )));
    }
    // Ammo status (Override doesn't count shots): a live bin here detonates on an ammo crit;
    // marking it spent makes that crit a dud. Only shown for ammo-bearing regions.
    if tm.ov_region_has_ammo(loc) {
        let live = tm.ov_ammo_live(loc);
        let (label, col) = if live {
            ("live", theme().warning)
        } else {
            ("spent", theme().dim)
        };
        let mut spans = vec![
            Span::styled("Ammo here: ", Style::default().fg(theme().dim)),
            Span::styled(label, Style::default().fg(col).add_modifier(Modifier::BOLD)),
            Span::styled("  (a: toggle)", Style::default().fg(theme().dim)),
        ];
        if tm.ov_ammo_exploded() {
            spans.push(Span::styled(
                "  → EXPLOSION: destroyed +2 pilot",
                Style::default()
                    .fg(theme().danger)
                    .add_modifier(Modifier::BOLD),
            ));
        }
        lines.push(Line::from(spans));
    }
    lines.push(Line::from(Span::styled(
        "  Space +hit   Bksp −hit   ↑↓ move   Esc close",
        Style::default().fg(theme().dim),
    )));
    lines
}

/// Body lines for the Override to-hit shot editor: attacker movement, the hand-entered target's
/// movement/state, and the arc, with a live per-bracket To-Hit preview for the selected TIC.
fn ov_shot_modal_lines(app: &App, sel: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let Some(tm) = app.session.active_mech() else {
        return lines;
    };
    let s = &tm.ov_shot;
    let yn = |b: bool| if b { "[x]" } else { "[ ]" };
    let rows = [
        ("Target TMM", s.target_tmm.to_string()),
        ("Target jumped (+1)", yn(s.target_jumped).into()),
        ("Target immobile (-2)", yn(s.target_immobile).into()),
        ("Secondary target (+1)", yn(s.secondary).into()),
        ("Rear arc (+1)", yn(s.rear).into()),
    ];
    for (i, (name, val)) in rows.into_iter().enumerate() {
        let selected = i == sel;
        let marker = if selected { "▶ " } else { "  " };
        let style = if selected {
            Style::default()
                .fg(theme().on_accent)
                .bg(theme().accent)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        lines.push(Line::from(Span::styled(
            format!("{marker}{name:<22} {val}"),
            style,
        )));
    }
    // Live To-Hit preview for the selected TIC (mirrors the card's per-TIC To-Hit row).
    let card = tm.ov_card();
    lines.push(Line::from(""));
    if let Some(t) = card.tics.get(app.ov_tic) {
        let cell = |b: &str| match override_conv::parse_bracket(b) {
            Some(m) => format!("{}+", tm.ov_to_hit(m)),
            None => "—".to_string(),
        };
        lines.push(Line::from(Span::styled(
            format!(
                "{}:  PB {}  S {}  M {}  L {}  X {}",
                trunc(&t.name, 10),
                cell(&t.pb),
                cell(&t.s),
                cell(&t.m),
                cell(&t.l),
                cell(&t.x)
            ),
            Style::default().fg(theme().warning),
        )));
    }
    lines.push(Line::from(Span::styled(
        "  attacker move: set with [v]   [Esc] close",
        Style::default().fg(theme().dim),
    )));
    lines
}

/// The fixed 2d6 hit-location number(s) printed beside an Override diagram region. `None` for
/// regions the card leaves unnumbered (torso/body rear, rotor, aerospace SI).
fn hit_location(loc: Location) -> Option<&'static str> {
    use Location::*;
    Some(match loc {
        // 'Mech (quad legs share the biped arm/leg numbers).
        Head => "12",
        LeftArm | FrontLeftLeg => "10,11",
        RightArm | FrontRightLeg => "3,4",
        CenterTorso => "6,7,8",
        LeftLeg | RearLeftLeg => "9",
        RightLeg | RearRightLeg => "5",
        // Combat vehicle.
        Front => "6,7,8",
        LeftSide | FrontLeftSide => "10,11",
        RightSide | FrontRightSide => "3,4",
        Turret | FrontTurret => "5,9",
        // Aerospace arcs.
        Nose => "6,7,8",
        LeftWing => "9,10,11",
        RightWing => "3,4,5",
        Aft => "2,12",
        _ => return None,
    })
}

/// Truncate a string to `n` display chars, appending `…` when clipped (the caller's format width
/// handles padding).
fn trunc(s: &str, n: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= n {
        s.to_string()
    } else {
        format!(
            "{}…",
            chars[..n.saturating_sub(1)].iter().collect::<String>()
        )
    }
}

// ---------- Strategic BattleForce mode (spec Phase 5) ----------

const SBF_HELP: &str =
    " Spc:dmg  u:rep  c:crit  t:to-hit  m:morale  n:round  e:done  g:group  r:rename  ,/.:form  ?:help ";

/// Format an SBF damage band: whole numbers plain, halves (minimal damage) as `n.5`.
fn sbf_dmg(v: f32) -> String {
    if v.fract() == 0.0 {
        format!("{}", v as i64)
    } else {
        format!("{v:.1}")
    }
}

fn morale_color(m: MoraleStatus) -> Color {
    match m {
        MoraleStatus::Normal => theme().dim,
        MoraleStatus::Shaken => theme().warning,
        MoraleStatus::Broken => theme().warning,
        MoraleStatus::Routed => theme().danger,
    }
}

fn sbf_range_label(r: SbfRange) -> &'static str {
    match r {
        SbfRange::Short => "Short",
        SbfRange::Medium => "Medium",
        SbfRange::Long => "Long",
        SbfRange::Extreme => "Extreme",
    }
}

/// The SBF screen: formations (left) → units of the active formation (middle) → active-unit
/// detail + to-hit (right). Single-force; the target is hand-entered via `t` (spec §5.2).
/// No force sidebar — see the `draw` dispatch note.
fn draw_sbf(f: &mut Frame, area: Rect, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(area);

    if app.session.sbf.formations.is_empty() {
        let pool = app.session.mechs.len();
        let msg = if pool == 0 {
            "No formations. Press [a] to add elements, then [g] to group them.".to_string()
        } else {
            format!("No formations. Press [g] to group the {pool} pool element(s).")
        };
        f.render_widget(Paragraph::new(msg).alignment(Alignment::Center), chunks[0]);
    } else {
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(34),
                Constraint::Min(30),
                Constraint::Length(34),
            ])
            .split(chunks[0]);
        draw_sbf_formations(f, cols[0], app);
        draw_sbf_units(f, cols[1], app);
        draw_sbf_detail(f, cols[2], app);
    }

    // Status leads; help fills the rest of the footer (the Override footer pattern).
    let mut spans = vec![Span::raw(" ")];
    if !app.status.is_empty() {
        spans.push(Span::styled(
            format!("{} ", app.status),
            Style::default()
                .fg(theme().accent)
                .add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled("| ", Style::default().fg(theme().dim)));
    }
    spans.push(Span::styled(
        SBF_HELP.trim_start(),
        Style::default().fg(theme().dim),
    ));
    f.render_widget(Paragraph::new(Line::from(spans)), chunks[1]);
}

/// Left pane: the formation list — name + activation flag, then a derived stat/morale line.
fn draw_sbf_formations(f: &mut Frame, area: Rect, app: &App) {
    let mut lines: Vec<Line> = Vec::new();
    for (i, fs) in app.session.sbf.formations.iter().enumerate() {
        let selected = i == app.session.sbf.active_formation;
        let marker = if selected { "▶ " } else { "  " };
        let name_style = if selected {
            Style::default()
                .fg(theme().on_accent)
                .bg(theme().accent)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        // An empty formation is a valid workspace, not a casualty — placeholder, no stats.
        if fs.units.is_empty() {
            lines.push(Line::from(Span::styled(
                format!("{marker}{}", fs.name),
                name_style,
            )));
            lines.push(Line::from(Span::styled(
                "    (no units — [g] to assign)",
                Style::default().fg(theme().dim),
            )));
            continue;
        }
        let derived = app.session.sbf_formation(fs);
        let mut head = vec![Span::styled(format!("{marker}{}", fs.name), name_style)];
        if fs.units.iter().any(|u| u.is_commander) {
            // The Force Commander's COM is inherited by its parent formation (IO:BF p.165).
            head.push(Span::styled(
                " COM",
                Style::default()
                    .fg(theme().accent)
                    .add_modifier(Modifier::BOLD),
            ));
        }
        if fs.is_done {
            head.push(Span::styled(" ✓", Style::default().fg(theme().good)));
        }
        if app.session.sbf_formation_eliminated(fs) {
            head.push(Span::styled(
                format!(" {}", icons::cond_destroyed()),
                Style::default().fg(theme().danger),
            ));
        } else if app.session.sbf_would_withdraw(fs) {
            head.push(Span::styled(
                " ⚠withdraw",
                Style::default().fg(theme().danger),
            ));
        }
        if !app.session.sbf_can_convert(fs) {
            head.push(Span::styled(
                " !invalid",
                Style::default().fg(theme().warning),
            ));
        }
        lines.push(Line::from(head));
        // PV lives in the detail pane; keep this line inside the 32-col pane. Aero formations
        // hide TMM (Open Q 1, MOOT): aerospace targets never use it — air-to-air applies no
        // movement modifier and ground-to-air replaces it with the flat +2 airborne row.
        let tmm = if derived.is_aerospace() {
            String::new()
        } else {
            format!(" TMM{}", derived.tmm)
        };
        lines.push(Line::from(vec![
            Span::styled(
                format!(
                    "    SZ{}{tmm} MV{}{} SK{}  ",
                    derived.size,
                    derived.movement,
                    derived.move_mode.code(),
                    derived.skill
                ),
                Style::default().fg(theme().dim),
            ),
            Span::styled(
                fs.morale.label(),
                Style::default().fg(morale_color(fs.morale)),
            ),
        ]));
    }
    // Pool elements not yet in any formation (added after the last [g]) — surface the count.
    let grouped: usize = app
        .session
        .sbf
        .formations
        .iter()
        .flat_map(|f| f.units.iter())
        .map(|u| u.elements.len())
        .sum();
    let pool = app.session.mechs.len();
    if pool > grouped {
        lines.push(Line::from(Span::styled(
            format!("  +{} ungrouped — [g] to assign", pool - grouped),
            Style::default().fg(theme().warning),
        )));
    }
    let title = format!(
        " FORMATIONS · Round {} · PV {} ",
        app.session.sbf.round,
        app.session.sbf_force_pv()
    );
    f.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme().dim))
                .title(title),
        ),
        area,
    );
}

/// Middle pane: the active formation's units — name + destroyed flag, armor track, current
/// (post-crit) damage bands, crit counters.
fn draw_sbf_units(f: &mut Frame, area: Rect, app: &App) {
    let Some(fs) = app
        .session
        .sbf
        .formations
        .get(app.session.sbf.active_formation)
    else {
        return;
    };
    let mut lines: Vec<Line> = Vec::new();
    if fs.units.is_empty() {
        lines.push(Line::from(Span::styled(
            "(no units — [g] to assign)",
            Style::default().fg(theme().dim),
        )));
    }
    for (i, u) in fs.units.iter().enumerate() {
        let selected = i == app.session.sbf.active_unit;
        let marker = if selected { "▶ " } else { "  " };
        let name_style = if selected {
            Style::default()
                .fg(theme().on_accent)
                .bg(theme().accent)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        // Mid-edit a unit can be empty (kept as a move target); placeholder, never "destroyed".
        if u.elements.is_empty() {
            lines.push(Line::from(vec![
                Span::styled(format!("{marker}{}", u.name), name_style),
                Span::styled("  (empty)", Style::default().fg(theme().dim)),
            ]));
            lines.push(Line::from(""));
            continue;
        }
        let derived = app.session.sbf_unit(u);
        let mut head = vec![Span::styled(
            format!("{marker}{} ×{}", u.name, u.elements.len()),
            name_style,
        )];
        if u.is_commander {
            head.push(Span::styled(
                " COM",
                Style::default()
                    .fg(theme().accent)
                    .add_modifier(Modifier::BOLD),
            ));
        }
        if u.is_leader {
            head.push(Span::styled(" LEAD", Style::default().fg(theme().accent)));
        }
        if u.is_destroyed(&derived) {
            head.push(Span::styled(
                format!(" {} DESTROYED", icons::cond_destroyed()),
                Style::default()
                    .fg(theme().danger)
                    .add_modifier(Modifier::BOLD),
            ));
        } else if matches!(derived.sbf_type, SbfElementType::As | SbfElementType::La)
            && u.current_movement(&derived) == 0
        {
            // An airborne aero unit at 0 Thrust is not immobile — it has crashed and comes off
            // in the End Phase (IO:BF p.178/p.181 Thrust Loss); the removal mark stays manual.
            head.push(Span::styled(
                " CRASHES (End Phase)",
                Style::default()
                    .fg(theme().danger)
                    .add_modifier(Modifier::BOLD),
            ));
        } else if u.crit_check_due(&derived) {
            head.push(Span::styled(
                " ⚠crit due",
                Style::default().fg(theme().warning),
            ));
        }
        lines.push(Line::from(head));

        let rem = u.armor_remaining(&derived) as u16;
        let max = derived.armor.max(0) as u16;
        let cur = u.current_damage(&derived);
        lines.push(Line::from(vec![
            Span::styled("    A ", Style::default().fg(theme().dim)),
            Span::styled(bar(rem, max, 8), Style::default().fg(frac_color(rem, max))),
            Span::styled(
                format!(" {rem}/{max}"),
                Style::default().fg(frac_color(rem, max)),
            ),
            Span::styled(
                format!(
                    " D{}/{}/{}",
                    sbf_dmg(cur.s),
                    sbf_dmg(cur.m),
                    sbf_dmg(cur.l.unwrap_or(0.0))
                ),
                Style::default().fg(if u.damage_crits > 0 {
                    theme().danger
                } else {
                    theme().fg
                }),
            ),
        ]));
        let crit_style = |n: u8| {
            if n > 0 {
                Style::default().fg(theme().danger)
            } else {
                Style::default().fg(theme().dim)
            }
        };
        lines.push(Line::from(vec![
            Span::styled("    crits ", Style::default().fg(theme().dim)),
            Span::styled(
                format!("DMG {}", u.damage_crits),
                crit_style(u.damage_crits),
            ),
            Span::styled("  ", Style::default()),
            Span::styled(
                format!("TGT {}", u.targeting_crits),
                crit_style(u.targeting_crits),
            ),
            Span::styled("  ", Style::default()),
            Span::styled(format!("MP {}", u.mp_crits), crit_style(u.mp_crits)),
            Span::styled(
                format!(
                    "  MV {}{}",
                    u.current_movement(&derived),
                    derived.move_mode.code()
                ),
                Style::default().fg(if u.mp_crits > 0 {
                    theme().warning
                } else {
                    theme().dim
                }),
            ),
        ]));
        lines.push(Line::from(""));
    }
    f.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Double)
                .border_style(Style::default().fg(theme().accent))
                .title(format!(" {} ", fs.name)),
        ),
        area,
    );
}

/// Right pane: the active unit's derived stat line + specials, the formation's live state, and
/// the current hand-entered to-hit number.
fn draw_sbf_detail(f: &mut Frame, area: Rect, app: &App) {
    let Some(fs) = app
        .session
        .sbf
        .formations
        .get(app.session.sbf.active_formation)
    else {
        return;
    };
    let empty_block = |f: &mut Frame, title: &str| {
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "no unit selected",
                Style::default().fg(theme().dim),
            )))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme().dim))
                    .title(format!(" {title} ")),
            ),
            area,
        );
    };
    let Some(u) = fs.units.get(app.session.sbf.active_unit) else {
        empty_block(f, &fs.name);
        return;
    };
    if u.elements.is_empty() {
        empty_block(f, &u.name);
        return;
    }
    let derived = app.session.sbf_unit(u);
    let formation = app.session.sbf_formation(fs);
    let mut lines: Vec<Line> = Vec::new();

    let jump = if derived.jump_move > 0 {
        format!("  J{}", derived.jump_move)
    } else {
        String::new()
    };
    // Aero units hide TMM like the formation line (Open Q 1, MOOT — never used vs aerospace).
    let tmm = if matches!(derived.sbf_type, SbfElementType::As | SbfElementType::La) {
        String::new()
    } else {
        format!("  TMM {}", derived.tmm)
    };
    lines.push(Line::from(Span::styled(
        format!(
            "{} SZ{}  MV {}{}{jump}{tmm}",
            format!("{:?}", derived.sbf_type).to_uppercase(),
            derived.size,
            derived.movement,
            derived.move_mode.code(),
        ),
        Style::default().fg(theme().dim),
    )));
    lines.push(Line::from(vec![
        Span::styled("Skill ", Style::default().fg(theme().dim)),
        Span::styled(
            format!("{}", u.base_gunnery(&derived)),
            Style::default()
                .fg(theme().accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("  PV {}", derived.point_value),
            Style::default().fg(theme().dim),
        ),
    ]));
    lines.push(Line::from(vec![
        Span::styled("Dmg  ", Style::default().fg(theme().dim)),
        Span::styled(
            format!(
                "S{} M{} L{} E{}",
                sbf_dmg(derived.damage.s),
                sbf_dmg(derived.damage.m),
                sbf_dmg(derived.damage.l.unwrap_or(0.0)),
                sbf_dmg(derived.damage.band(SbfRange::Extreme))
            ),
            Style::default(),
        ),
    ]));
    if !derived.suas.is_empty() {
        let specials: Vec<String> = derived.suas.keys().cloned().collect();
        lines.push(Line::from(Span::styled(
            specials.join(" "),
            Style::default().fg(theme().good),
        )));
    }

    // The elements composing this unit, with their pool numbers (the group editor's indices).
    // Headed like the official Formation Record Sheet's per-unit block.
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Alpha Strike Elements",
        Style::default()
            .fg(theme().dim)
            .add_modifier(Modifier::BOLD),
    )));
    for &i in &u.elements {
        if let Some(tm) = app.session.mechs.get(i) {
            lines.push(Line::from(Span::styled(
                format!(
                    "{:>2} {:<24} SK{}",
                    i + 1,
                    trunc(&tm.spec.display_name(), 24),
                    tm.gunnery
                ),
                Style::default().fg(theme().dim),
            )));
        }
    }

    // The hand-entered shot: range + target legs (App state) → live target number.
    lines.push(Line::from(""));
    if let Some(ctx) = app.sbf_to_hit_ctx() {
        let n = sbf_engine::sbf_to_hit(&ctx);
        // Under an aero shot the summary names the attack kind — "vs TMM" would misread when
        // the kind suppresses target movement entirely.
        let vs = match ctx.aero {
            None => format!("vs TMM{}", ctx.target_tmm),
            Some(a) => sbf_aero_kind_label(a.kind).to_string(),
        };
        lines.push(Line::from(vec![
            Span::styled("To-Hit ", Style::default().fg(theme().dim)),
            Span::styled(
                format!("{n}+"),
                Style::default()
                    .fg(theme().warning)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(
                    "  ({} {vs}{})",
                    sbf_range_label(ctx.range),
                    if ctx.secondary { ", 2nd tgt" } else { "" }
                ),
                Style::default().fg(theme().dim),
            ),
        ]));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("Formation  ", Style::default().fg(theme().dim)),
        Span::styled(
            fs.morale.label(),
            Style::default().fg(morale_color(fs.morale)),
        ),
    ]));
    lines.push(Line::from(Span::styled(
        format!(
            "TAC {}  MOR {}  PV {}",
            formation.tactics, formation.morale_rating, formation.point_value
        ),
        Style::default().fg(theme().dim),
    )));
    if !formation.suas.is_empty() {
        // The record sheet's "Formation Specials" column.
        let specials: Vec<String> = formation.suas.keys().cloned().collect();
        lines.push(Line::from(Span::styled(
            specials.join(" "),
            Style::default().fg(theme().good),
        )));
    }
    if formation.suas.contains_key("BOMB") {
        // Carrying bombs costs 1 Thrust each, min 1 (p.178); load-out is scenario setup the
        // sheet doesn't track (Open Q 26) — a note, not state.
        lines.push(Line::from(Span::styled(
            "bombs: −1 Thrust each (min 1)",
            Style::default().fg(theme().warning),
        )));
    }
    if fs.jump_used_this_turn > 0 {
        lines.push(Line::from(Span::styled(
            format!("jumped {} this turn", fs.jump_used_this_turn),
            Style::default().fg(theme().warning),
        )));
    }
    if app.session.sbf_has_com_or_lead(fs) {
        // Step 5b (p.172): a formation holding COM or LEAD defends allocation at +2 Tactics.
        lines.push(Line::from(Span::styled(
            "defender +2 Tactics (COM/LEAD)",
            Style::default().fg(theme().good),
        )));
    }
    if app.session.sbf_is_crippled(fs) {
        lines.push(Line::from(Span::styled(
            "⚠ CRIPPLED",
            Style::default()
                .fg(theme().danger)
                .add_modifier(Modifier::BOLD),
        )));
    }

    f.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme().dim))
                .title(format!(" {} ", u.name)),
        ),
        area,
    );
}

/// Body lines for the grouping editor (the manual-first flow): every pool element with its
/// current formation · unit assignment, plus the move/split/new-formation verbs.
fn sbf_group_modal_lines(app: &App, sel: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for (i, tm) in app.session.mechs.iter().enumerate() {
        let selected = i == sel;
        let marker = if selected { "▶ " } else { "  " };
        let name_style = if selected {
            Style::default()
                .fg(theme().on_accent)
                .bg(theme().accent)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        let name = trunc(&tm.spec.display_name(), 30);
        let (tag, tag_style) = match app.session.sbf_element_assignment(i) {
            Some((fi, ui)) => {
                let f = &app.session.sbf.formations[fi];
                (
                    trunc(&format!("{} · {}", f.name, f.units[ui].name), 26),
                    Style::default().fg(theme().dim),
                )
            }
            None => ("— unassigned".into(), Style::default().fg(theme().warning)),
        };
        lines.push(Line::from(vec![
            Span::styled(
                format!("{marker}{:>2} {name:<30} SK{:<2} ", i + 1, tm.gunnery),
                name_style,
            ),
            Span::styled(tag, tag_style),
        ]));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "[↑↓] element   [←→] move between units   [n] split to new unit",
        Style::default().fg(theme().dim),
    )));
    lines.push(Line::from(Span::styled(
        "[f] new formation   [u] unassign   [s/S] skill ±   [x] remove",
        Style::default().fg(theme().dim),
    )));
    lines.push(Line::from(Span::styled(
        "[a] auto-group…   [Esc] done",
        Style::default().fg(theme().dim),
    )));
    lines
}

/// Body lines for the doctrine picker: the three IO:BF p.165 organization schemes.
fn sbf_doctrine_modal_lines(sel: usize) -> Vec<Line<'static>> {
    let rows = [
        (
            "Inner Sphere",
            "Lances of 4 → Companies; Flights of 2 → Squadrons",
        ),
        (
            "Clan",
            "Stars of 5 → Binary / Trinary; aero Flights → Squadrons",
        ),
        ("ComStar", "Level IIs of 6 → Level III"),
    ];
    let mut lines = Vec::new();
    for (i, (name, desc)) in rows.into_iter().enumerate() {
        let selected = i == sel;
        let marker = if selected { "▶ " } else { "  " };
        let style = if selected {
            Style::default()
                .fg(theme().on_accent)
                .bg(theme().accent)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        lines.push(Line::from(vec![
            Span::styled(format!("{marker}{name:<14}"), style),
            Span::styled(desc, Style::default().fg(theme().dim)),
        ]));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "rebuilds ALL formations — custom names, marks and COM/LEAD are lost",
        Style::default().fg(theme().warning),
    )));
    lines.push(Line::from(Span::styled(
        "z undoes · ground and aero group separately   [↑↓] select   [Enter] apply   [Esc] back",
        Style::default().fg(theme().dim),
    )));
    lines
}

/// Body lines for the SBF crit-counter popup: three counters + the single §4.2 crit table as a
/// dim reference (the player rolls the 2d6 by hand).
fn sbf_crit_modal_lines(app: &App, sel: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let sbf = &app.session.sbf;
    let Some(u) = sbf
        .formations
        .get(sbf.active_formation)
        .and_then(|f| f.units.get(sbf.active_unit))
    else {
        return lines;
    };
    let derived = app.session.sbf_unit(u);
    let due = if u.crit_check_due(&derived) {
        "⚠ below half armor — roll 2d6"
    } else if u.is_destroyed(&derived) {
        "unit destroyed — no roll"
    } else {
        "at or above half armor — no roll owed"
    };
    lines.push(Line::from(Span::styled(
        format!("{} — {}", u.name, due),
        Style::default()
            .fg(theme().accent)
            .add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(""));
    // Aero at 0 Thrust crashes in the End Phase rather than sitting immobile (p.178/p.181).
    let mp_effect = if matches!(derived.sbf_type, SbfElementType::As | SbfElementType::La) {
        "−1 Thrust each (0 = crashes, End Phase)"
    } else {
        "−1 MP each (0 = immobile)"
    };
    let rows = [
        ("Damage crits", u.damage_crits, "−1 damage every band"),
        ("Targeting crits", u.targeting_crits, "+1 to-hit each"),
        ("MP crits", u.mp_crits, mp_effect),
    ];
    for (i, (name, n, effect)) in rows.into_iter().enumerate() {
        let selected = i == sel;
        let marker = if selected { "▶ " } else { "  " };
        let style = if selected {
            Style::default()
                .fg(theme().on_accent)
                .bg(theme().accent)
                .add_modifier(Modifier::BOLD)
        } else if n > 0 {
            Style::default().fg(theme().danger)
        } else {
            Style::default()
        };
        lines.push(Line::from(vec![
            Span::styled(format!("{marker}{name:<16} {n}   "), style),
            Span::styled(effect, Style::default().fg(theme().dim)),
        ]));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "2d6:  2-4 none   5-7 targeting   8-9 damage",
        Style::default().fg(theme().dim),
    )));
    lines.push(Line::from(Span::styled(
        "      10-11 both   12 unit destroyed",
        Style::default().fg(theme().dim),
    )));
    // Large craft with more than one weapon class in an arc: a Weapon-Damage result hits only ONE
    // class — the player rolls 1D6 on the Random Weapon Class table and applies the −1 to that
    // class at the table (table-side; the arc card carries raw per-class damage).
    if derived.arcs.as_ref().is_some_and(|c| {
        large_craft::Arc::ALL
            .iter()
            .any(|&a| large_craft::arc_lines(c, a).len() > 1)
    }) {
        lines.push(Line::from(Span::styled(
            "large craft: on a Damage result roll 1D6 (Random Weapon Class):",
            Style::default().fg(theme().warning),
        )));
        lines.push(Line::from(Span::styled(
            "  1-2 STD   3-4 CAP (non-missile)   5-6 MSL — apply −1 at table (p.190)",
            Style::default().fg(theme().dim),
        )));
    }
    lines.push(Line::from(Span::styled(
        "[↑↓] select   [←→/space] adjust   [Esc] close",
        Style::default().fg(theme().dim),
    )));
    lines
}

/// Short label for an engine aero attack kind (the shot modal + detail-pane summary).
fn sbf_aero_kind_label(k: SbfAeroKind) -> &'static str {
    match k {
        SbfAeroKind::AirToAir => "air-to-air",
        SbfAeroKind::GroundToAir => "ground-to-air",
        SbfAeroKind::A2G(SbfA2G::AltitudeBombing { .. }) => "alt bombing",
        SbfAeroKind::A2G(SbfA2G::DiveBombing { .. }) => "dive bombing",
        SbfAeroKind::A2G(SbfA2G::Strafing) => "strafing",
        SbfAeroKind::A2G(SbfA2G::Striking) => "striking",
    }
}

/// Short label for the p.179 target-type rows.
fn sbf_aero_target_label(t: SbfAeroTarget) -> &'static str {
    match t {
        SbfAeroTarget::AirborneAero => "airborne aero",
        SbfAeroTarget::AirborneDropship => "airborne DropShip",
        SbfAeroTarget::AirborneVtolWige => "airborne VTOL/WiGE",
        SbfAeroTarget::SmallCraft => "small craft",
        SbfAeroTarget::GroundedSquadron => "grounded squadron",
        SbfAeroTarget::GroundFormation => "ground formation",
    }
}

/// Body lines for the SBF to-hit editor — the printed p.172 To-Hit Modifiers Table plus the
/// Strategic Aerospace rows (the p.179 table), with the live target number below (morale is
/// manual and deliberately not a term, §4.3).
fn sbf_shot_modal_lines(app: &App, sel: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let Some(ctx) = app.sbf_to_hit_ctx() else {
        return lines;
    };
    let yn = |b: bool| if b { "[x]" } else { "[ ]" };
    let aero = ctx.aero;
    // Air-to-air / ground-to-air / bombing shots apply no target-movement or terrain modifiers
    // (pp.179–181) — those rows render dim while an aero kind suppresses them.
    let suppressed = aero.is_some_and(|a| a.kind.suppresses_target_movement());
    let (kind_val, target_val) = match aero {
        None => ("off".to_string(), "—".to_string()),
        Some(a) => (
            format!(
                "{} ({:+})",
                sbf_aero_kind_label(a.kind),
                a.kind.attack_mod()
            ),
            format!("{} ({:+})", sbf_aero_target_label(a.target), a.target_mod()),
        ),
    };
    let s = app.sbf_shot;
    // Capital-scale (p.191) rows for a Large-Aerospace firing unit. They stay visible (so the
    // player can pre-set them) but are only active — and only priced — once an aero kind is on,
    // which is when the `capital` leg is built.
    let large = app.sbf_firing_unit_is_large_craft();
    let cap = aero.and_then(|a| a.capital);
    let cap_active = cap.is_some();
    let mut rows: Vec<(&str, String, bool)> = vec![
        (
            "Range",
            format!(
                "{} (+{})",
                sbf_range_label(ctx.range),
                sbf_engine::sbf_range_mod(ctx.range)
            ),
            true,
        ),
        ("Indirect fire", yn(ctx.indirect_fire).to_string(), true),
        ("Formation JUMP used", ctx.attacker_jump.to_string(), true),
        ("Units withholding", ctx.withheld_units.to_string(), true),
        ("Spotting for IF", yn(ctx.spotting).to_string(), true),
        ("Secondary target", yn(ctx.secondary).to_string(), true),
        ("Target TMM", ctx.target_tmm.to_string(), !suppressed),
        ("Target JUMP used", ctx.target_jump.to_string(), !suppressed),
        (
            "Target evaded",
            yn(ctx.target_evaded).to_string(),
            !suppressed,
        ),
        ("Terrain", format!("+{}", ctx.terrain), !suppressed),
        ("Aero attack", kind_val, true),
        ("Aero target", target_val, aero.is_some()),
        (
            "Behind target",
            yn(s.behind_target).to_string(),
            aero.is_some(),
        ),
        (
            "Cluster bombs (−1)",
            yn(s.cluster).to_string(),
            s.aero_kind.is_bombing(),
        ),
    ];
    if large {
        let pd = match s.point_defense {
            0 => "0".to_string(),
            1 => "1 (+1)".to_string(),
            _ => "2+ (auto-fail)".to_string(),
        };
        let acm = match s.acm {
            sbf_engine::SbfAcm::Off => "off",
            sbf_engine::SbfAcm::SameSector => "same sector (+0)",
            sbf_engine::SbfAcm::AdjacentSector => "adjacent (+2)",
        };
        rows.extend([
            ("Firing arc", s.firing_arc.label().to_string(), cap_active),
            (
                "Weapon class",
                s.weapon_class.label().to_string(),
                cap_active,
            ),
            (
                "Target is large craft (waive)",
                yn(s.target_large_craft).to_string(),
                cap_active,
            ),
            (
                "High-speed attack (+8)",
                yn(s.high_speed).to_string(),
                cap_active,
            ),
            (
                "Atmospheric (+2)",
                yn(s.atmospheric).to_string(),
                cap_active,
            ),
            ("Point defense (vs MSL)", pd, cap_active),
            (
                "Screen launchers (SCR)",
                format!("+{}", (s.screen).min(4)),
                cap_active,
            ),
            ("Naval C3 (−1)", yn(s.naval_c3).to_string(), cap_active),
            (
                "Teleoperated MSL (−1)",
                yn(s.teleoperated).to_string(),
                cap_active,
            ),
            (
                "Target crippled (−2)",
                yn(s.crippled).to_string(),
                cap_active,
            ),
            (
                "Target grappled (−4)",
                yn(s.grappled).to_string(),
                cap_active,
            ),
            ("Adv capital missile", acm.to_string(), cap_active),
        ]);
    }
    // A large craft's row list is long; render the To-Hit + capital damage/limit summary at the
    // TOP so it stays on-screen above the editor (mirrors the BF large-craft modal).
    if large {
        let n = sbf_engine::sbf_to_hit(&ctx);
        lines.push(Line::from(Span::styled(
            format!("To-Hit   {n}+   (2d6 ≥ {n} per attack)"),
            Style::default().fg(theme().warning),
        )));
        if let Some(c) = cap {
            let sbf = &app.session.sbf;
            if let Some(u) = sbf
                .formations
                .get(sbf.active_formation)
                .and_then(|f| f.units.get(sbf.active_unit))
            {
                if let Some(card) = app.session.sbf_unit(u).arcs {
                    let eff = sbf_engine::capital_range(ctx.range, c.weapon_class);
                    let dmg =
                        large_craft::arc_damage(&card, s.firing_arc, c.weapon_class).band(eff);
                    let dmg_str = if dmg == 0.5 {
                        "0*".to_string()
                    } else {
                        format!("{}", dmg as u32)
                    };
                    let mut line = format!(
                        "{} {} @ {}:  damage {dmg_str}",
                        s.firing_arc.label(),
                        s.weapon_class.label(),
                        sbf_range_label(eff)
                    );
                    if let Some(lim) = u
                        .elements
                        .first()
                        .map(|&i| app.session.sbf_element(i))
                        .and_then(|e| sbf_engine::large_aero_attack_limit(&e.as_type))
                    {
                        line.push_str(&format!("   ·   attack limit {lim}/turn per Flight"));
                    }
                    lines.push(Line::from(Span::styled(
                        line,
                        Style::default()
                            .fg(theme().accent)
                            .add_modifier(Modifier::BOLD),
                    )));
                }
            }
            if c.auto_fail() {
                lines.push(Line::from(Span::styled(
                    "point defense (2+) eliminates this capital-missile attack — AUTO-FAIL (p.191)",
                    Style::default().fg(theme().warning),
                )));
            }
        } else {
            lines.push(Line::from(Span::styled(
                "select an Aero attack to fire capital weapons (p.191)",
                Style::default().fg(theme().dim),
            )));
        }
        lines.push(Line::from(""));
    }
    for (i, (name, val, active)) in rows.into_iter().enumerate() {
        let selected = i == sel;
        let marker = if selected { "▶ " } else { "  " };
        let style = if selected {
            Style::default()
                .fg(theme().on_accent)
                .bg(theme().accent)
                .add_modifier(Modifier::BOLD)
        } else if !active {
            Style::default().fg(theme().dim)
        } else {
            Style::default()
        };
        lines.push(Line::from(Span::styled(
            format!("{marker}{name:<20} {val}"),
            style,
        )));
    }
    if suppressed {
        lines.push(Line::from(Span::styled(
            "  (no TMM vs airborne — target movement/terrain suppressed)",
            Style::default().fg(theme().dim),
        )));
    }
    // Formation specials fold in automatically — say so when they do. Under an aero shot the
    // fire-control rows come from the p.179 SV ladder instead of the ground BFC row.
    let mut autos: Vec<String> = Vec::new();
    match aero {
        None => {
            if ctx.bfc {
                autos.push("BFC +1".into());
            }
        }
        Some(a) => match a.sv_fire_control {
            sbf_engine::SbfSvFireControl::Afc => {}
            sbf_engine::SbfSvFireControl::Bfc => autos.push("BFC +1".into()),
            sbf_engine::SbfSvFireControl::None => autos.push("SV no fire control +2".into()),
        },
    }
    if ctx.drone {
        autos.push("DRO +1".into());
    }
    if aero.is_some() && ctx.firing_unit_targeting_crits > 0 {
        // The p.179 table prices targeting crits at +2 each (vs the ground +1).
        autos.push(format!(
            "TGT crits ×{} +2 each",
            ctx.firing_unit_targeting_crits
        ));
    }
    if !autos.is_empty() {
        lines.push(Line::from(Span::styled(
            format!("  formation specials: {}", autos.join("  ")),
            Style::default().fg(theme().good),
        )));
    }
    lines.push(Line::from(""));
    // Non-large-craft shots print the To-Hit here; a large craft has it (with its capital damage +
    // attack-limit summary) at the TOP so it survives the long capital row list.
    if !large {
        let n = sbf_engine::sbf_to_hit(&ctx);
        lines.push(Line::from(Span::styled(
            format!("To-Hit   {n}+   (each firing unit rolls 2d6 ≥ {n})"),
            Style::default().fg(theme().warning),
        )));
    }
    lines.push(Line::from(Span::styled(
        "target morale is manual — not a to-hit term (§4.3)",
        Style::default().fg(theme().dim),
    )));
    if let Some(a) = aero {
        if let SbfAeroKind::A2G(_) = a.kind {
            // The p.180 damage math for the active unit's current (post-crit) Short value.
            let sbf = &app.session.sbf;
            if let Some(u) = sbf
                .formations
                .get(sbf.active_formation)
                .and_then(|f| f.units.get(sbf.active_unit))
            {
                let s = u.current_damage(&app.session.sbf_unit(u)).s;
                lines.push(Line::from(Span::styled(
                    format!(
                        "A2G dmg: strafe ⌈S/4⌉={}  strike S={}  HE bomb {}  cluster {} (p.180)",
                        sbf_engine::sbf_strafe_damage(s),
                        sbf_dmg(s),
                        sbf_engine::SBF_BOMB_HE_DAMAGE,
                        sbf_engine::SBF_BOMB_CLUSTER_DAMAGE,
                    ),
                    Style::default().fg(theme().dim),
                )));
            }
        }
        // Engagement control is a two-player radar-map procedure — reference only.
        lines.push(Line::from(Span::styled(
            "engagement ref: +2 atmosphere · +2 tailed · −2 tailing (p.179)",
            Style::default().fg(theme().dim),
        )));
    }
    lines.push(Line::from(Span::styled(
        "[↑↓] select   [←→/space] adjust   [Esc] close",
        Style::default().fg(theme().dim),
    )));
    lines
}

fn sbf_help_modal_lines() -> Vec<Line<'static>> {
    let header = |s: &'static str| {
        Line::from(Span::styled(
            s,
            Style::default()
                .fg(theme().accent)
                .add_modifier(Modifier::BOLD),
        ))
    };
    let row = |keys: &'static str, desc: &'static str| {
        Line::from(vec![
            Span::styled(format!("  {keys:<9}"), Style::default().fg(theme().accent)),
            Span::styled(desc, Style::default().fg(theme().dim)),
        ])
    };
    vec![
        header("Strategic BattleForce"),
        row("Space", "1 damage to unit (overflow spills over)"),
        row("u", "repair 1"),
        row("c", "crit counters (roll 2d6, mark the result)"),
        row("t", "to-hit (range + hand-entered target)"),
        row("m", "cycle formation morale rung (manual)"),
        row("n", "begin round (clears done, resets jump)"),
        row("e", "formation done this turn"),
        row("g", "group editor (move / split / skill / remove)"),
        row("r / R", "rename formation / unit"),
        row("C / l", "mark Force Commander / Formation Leader"),
        header("Selection"),
        row(", / .", "previous / next formation  ([ / ] also)"),
        row("↑↓ / kj", "previous / next unit"),
        header("General"),
        row("b", "set force PV limit"),
        row("z", "undo"),
        row("a / D", "add elements / delete formation"),
        row("S", "sessions browser"),
        row("P", "export record-sheet PDF"),
        row("^t", "display picker (theme + layout)"),
        row("q", "quit"),
        Line::from(Span::styled(
            "  press any key to close",
            Style::default().fg(theme().dim),
        )),
    ]
}

// ---------- Abstract Combat System mode (spec Phase 4) ----------

const ACS_HELP: &str =
    " Spc:dmg  u:rep  m/M:CU/form morale  f/F:fatigue  n:round  e:done  a:add  g:group  [/]:range  +/-:tmm  S:sess  ?:help ";

fn acs_morale_color(m: AcsMorale) -> Color {
    match m {
        AcsMorale::Normal => theme().dim,
        AcsMorale::Shaken | AcsMorale::Unsteady => theme().warning,
        AcsMorale::Broken | AcsMorale::Retreating => theme().warning,
        AcsMorale::Routed | AcsMorale::Surrender => theme().danger,
    }
}

fn acs_range_label(r: AcsRange) -> &'static str {
    match r {
        AcsRange::Short => "Short",
        AcsRange::Medium => "Medium",
        AcsRange::Long => "Long",
    }
}

fn draw_acs(f: &mut Frame, area: Rect, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(area);

    if app.session.acs.formations.is_empty() {
        let pool = app.session.mechs.len();
        let msg = if pool == 0 {
            "No formations. Press [a] to add elements, then [g] to group them.".to_string()
        } else {
            format!("No formations. Press [g] to group the {pool} pool element(s).")
        };
        f.render_widget(Paragraph::new(msg).alignment(Alignment::Center), chunks[0]);
    } else {
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(34),
                Constraint::Min(30),
                Constraint::Length(38),
            ])
            .split(chunks[0]);
        draw_acs_formations(f, cols[0], app);
        draw_acs_units(f, cols[1], app);
        draw_acs_detail(f, cols[2], app);
    }

    let mut spans = vec![Span::raw(" ")];
    if !app.status.is_empty() {
        spans.push(Span::styled(
            format!("{} ", app.status),
            Style::default()
                .fg(theme().accent)
                .add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled("| ", Style::default().fg(theme().dim)));
    }
    spans.push(Span::styled(
        ACS_HELP.trim_start(),
        Style::default().fg(theme().dim),
    ));
    f.render_widget(Paragraph::new(Line::from(spans)), chunks[1]);
}

/// Left pane: the Formation list — name + COM/done flags, then a derived Move/Tactics/Morale line.
fn draw_acs_formations(f: &mut Frame, area: Rect, app: &App) {
    let mut lines: Vec<Line> = Vec::new();
    for (i, fs) in app.session.acs.formations.iter().enumerate() {
        let selected = i == app.session.acs.active_formation;
        let marker = if selected { "▶ " } else { "  " };
        let name_style = if selected {
            Style::default()
                .fg(theme().on_accent)
                .bg(theme().accent)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        if fs.units.is_empty() {
            lines.push(Line::from(Span::styled(
                format!("{marker}{}", fs.name),
                name_style,
            )));
            lines.push(Line::from(Span::styled(
                "    (no units — [g] to group)",
                Style::default().fg(theme().dim),
            )));
            continue;
        }
        let derived = app.session.acs_formation(fs);
        let mut head = vec![Span::styled(format!("{marker}{}", fs.name), name_style)];
        if fs.units.iter().any(|u| u.is_commander) {
            head.push(Span::styled(
                " COM",
                Style::default()
                    .fg(theme().accent)
                    .add_modifier(Modifier::BOLD),
            ));
        }
        if fs.is_done {
            head.push(Span::styled(" ✓", Style::default().fg(theme().good)));
        }
        if derived.is_aerospace() {
            // Abstract Combat Aerospace is a v1 non-goal; the ground converter's numbers for an
            // aero Formation are not valid. Flag it (full explanation in the detail pane).
            head.push(Span::styled(" ⚠aero", Style::default().fg(theme().warning)));
        }
        lines.push(Line::from(head));
        lines.push(Line::from(vec![
            Span::styled(
                format!(
                    "    MV{} TAC{} SK{}  ",
                    derived.movement, derived.tactics, derived.skill
                ),
                Style::default().fg(theme().dim),
            ),
            Span::styled(
                fs.morale.label(),
                Style::default().fg(acs_morale_color(fs.morale)),
            ),
        ]));
    }
    let grouped: usize = app
        .session
        .acs
        .formations
        .iter()
        .flat_map(|f| f.units.iter())
        .flat_map(|cu| cu.teams.iter())
        .flat_map(|t| t.units.iter())
        .map(|u| u.elements.len())
        .sum();
    let pool = app.session.mechs.len();
    if pool > grouped {
        lines.push(Line::from(Span::styled(
            format!("  +{} ungrouped — [g] to group", pool - grouped),
            Style::default().fg(theme().warning),
        )));
    }
    let title = format!(
        " FORMATIONS · Round {} · PV {} ",
        app.session.acs.round,
        app.session.acs_force_pv()
    );
    f.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme().dim))
                .title(title),
        ),
        area,
    );
}

/// Middle pane: the active Formation's Combat Units — name + destroyed flag, armor bar with the
/// three threshold marks, damage line, fatigue band, morale.
fn draw_acs_units(f: &mut Frame, area: Rect, app: &App) {
    let Some(fs) = app
        .session
        .acs
        .formations
        .get(app.session.acs.active_formation)
    else {
        return;
    };
    let mut lines: Vec<Line> = Vec::new();
    if fs.units.is_empty() {
        lines.push(Line::from(Span::styled(
            "(no Combat Units — [g] to group)",
            Style::default().fg(theme().dim),
        )));
    }
    for (i, cu) in fs.units.iter().enumerate() {
        let selected = i == app.session.acs.active_unit;
        let marker = if selected { "▶ " } else { "  " };
        let name_style = if selected {
            Style::default()
                .fg(theme().on_accent)
                .bg(theme().accent)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        let derived = app.session.acs_combat_unit(cu);
        let mut head = vec![Span::styled(format!("{marker}{}", cu.name), name_style)];
        if cu.is_commander {
            head.push(Span::styled(
                " COM",
                Style::default()
                    .fg(theme().accent)
                    .add_modifier(Modifier::BOLD),
            ));
        }
        if cu.is_leader {
            head.push(Span::styled(" LEAD", Style::default().fg(theme().accent)));
        }
        if cu.is_destroyed(&derived) {
            head.push(Span::styled(
                format!(" {} DESTROYED", icons::cond_destroyed()),
                Style::default()
                    .fg(theme().danger)
                    .add_modifier(Modifier::BOLD),
            ));
        }
        lines.push(Line::from(head));

        let rem = cu.armor_remaining(&derived) as u16;
        let max = derived.armor.max(0) as u16;
        lines.push(Line::from(vec![
            Span::styled("    A ", Style::default().fg(theme().dim)),
            Span::styled(bar(rem, max, 10), Style::default().fg(frac_color(rem, max))),
            Span::styled(
                format!(" {rem}/{max}"),
                Style::default().fg(frac_color(rem, max)),
            ),
        ]));
        // Threshold marks + current fatigue band + morale.
        let band = acs_fatigue_band(
            cu.fatigue_points(),
            AcsExperience::from_skill(derived.skill),
        );
        lines.push(Line::from(vec![
            Span::styled(
                format!(
                    "    thr {}/{}/{}",
                    derived.damage_thresholds[0],
                    derived.damage_thresholds[1],
                    derived.damage_thresholds[2]
                ),
                Style::default().fg(theme().dim),
            ),
            Span::styled(
                format!(
                    "  D{}/{}/{}",
                    sbf_dmg(derived.damage.s),
                    sbf_dmg(derived.damage.m),
                    sbf_dmg(derived.damage.l.unwrap_or(0.0))
                ),
                Style::default(),
            ),
        ]));
        lines.push(Line::from(vec![
            Span::styled(
                format!("    {:?}", band),
                Style::default().fg(if matches!(band, AcsFatigueBand::Rested) {
                    theme().dim
                } else {
                    theme().warning
                }),
            ),
            Span::styled(
                format!("  {:.1}FP  ", cu.fatigue_points()),
                Style::default().fg(theme().dim),
            ),
            Span::styled(
                cu.morale.label(),
                Style::default().fg(acs_morale_color(cu.morale)),
            ),
        ]));
        lines.push(Line::from(""));
    }
    f.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Double)
                .border_style(Style::default().fg(theme().accent))
                .title(format!(" {} ", fs.name)),
        ),
        area,
    );
}

/// Right pane: the active Combat Unit's derived stat line, the to-hit / damage / morale readouts
/// (Phase 3 calculators), and the Combat-Team derivation fold.
/// The ACS aerospace to-hit / damage / Ground-Support readout (IO:BF pp.250/241/251-252), shown for
/// an aero Formation in place of the ground p.248 calc. Large craft resolve per-arc capital damage
/// off the arc card; a plain aero Formation uses its aggregated band.
fn acs_aero_readout(app: &App, derived: &AcsCombatUnit, lines: &mut Vec<Line<'static>>) {
    use super::app::AcsAeroMission;
    let Some(ctx) = app.acs_aero_to_hit_ctx() else {
        return;
    };
    let s = app.acs_shot;
    let tn = acs_aero_to_hit(&ctx);
    // The capital range reduction feeds the TN and the damage lookup — label the effective bracket.
    let eff = acs_aero_range(ctx.range, ctx.capital.as_ref());
    lines.push(Line::from(vec![
        Span::styled("To-Hit ", Style::default().fg(theme().dim)),
        Span::styled(
            format!("{tn}+"),
            Style::default()
                .fg(theme().warning)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(
                "  (aero {} vs TMM{}{})",
                sbf_range_label(eff),
                ctx.target_tmm,
                if ctx.secondary_target { ", 2nd" } else { "" },
            ),
            Style::default().fg(theme().dim),
        ),
    ]));
    // Per-arc capital damage (large craft) or the aggregated band (plain aero), run through the ACS
    // fractional-damage formula.
    let band = match &derived.arcs {
        Some(card) => large_craft::arc_damage(card, s.firing_arc, s.weapon_class).band(eff),
        None => derived.damage.band(eff),
    };
    let dmg = acs_damage(
        band,
        &AcsDamageCtx {
            secondary_target: ctx.secondary_target,
            attacker_fatigue: ctx.fatigue,
            attacker_morale: ctx.own_morale,
            ..Default::default()
        },
    );
    let src = if derived.arcs.is_some() {
        format!(
            "{} {} @ {}",
            s.firing_arc.label(),
            s.weapon_class.label(),
            sbf_range_label(eff)
        )
    } else {
        format!("band {}", sbf_range_label(eff))
    };
    lines.push(Line::from(vec![
        Span::styled("Damage ", Style::default().fg(theme().dim)),
        Span::styled(
            format!("{dmg}"),
            Style::default()
                .fg(theme().accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!("  ({src})"), Style::default().fg(theme().dim)),
    ]));
    lines.push(Line::from(Span::styled(
        format!(
            "matchup: {} ({:+}){}   [w] class · [v] arc · [x] matchup · [L] large-tgt · [y] mission",
            s.matchup.label(),
            s.matchup.to_hit_mod(),
            if s.target_large_craft { " · large-craft tgt (class waived)" } else { "" },
        ),
        Style::default().fg(theme().dim),
    )));
    // Ground-Support mission readout (p.251-252) for the selected mission. Large craft carry ~0
    // aggregated damage (their weapons live on the arc card), so the mission damage reads the chosen
    // arc/class off the card, matching the space-combat line above.
    let cu_dmg = |r: SbfRange| -> f32 {
        match &derived.arcs {
            Some(card) => large_craft::arc_damage(card, s.firing_arc, s.weapon_class).band(r),
            None => derived.damage.band(r),
        }
    };
    let short = cu_dmg(SbfRange::Short);
    let bomb = derived
        .suas
        .get("BOMB")
        .map(neurohelmet_core::engine::sbf::suaval_num)
        .unwrap_or(0.0) as i64;
    let mission_line = match s.aero_mission {
        AcsAeroMission::SpaceCombat => None,
        AcsAeroMission::Cap => {
            Some(format!("CAP: {ACS_CAP_ENGAGEMENT_MOD} Engagement Control (p.251)"))
        }
        AcsAeroMission::GroundStrike => Some(format!(
            "Ground Strike (TN Skill+{ACS_GROUND_STRIKE_TOHIT}): Strike ½S={} · Bomb {} cluster(s) of 5 (p.251)",
            acs_ground_strike_damage(short),
            acs_bomb_clusters(bomb),
        )),
        AcsAeroMission::AerialRecon => {
            Some(format!("Aerial Recon: {ACS_AERIAL_RECON_MOD} Recon (−3 if engaged, +2 in air-air) (p.251)"))
        }
        AcsAeroMission::OrbitToSurface => {
            let p = acs_orbit_to_surface_primary(short.max(cu_dmg(SbfRange::Medium)));
            Some(format!(
                "Orbit-to-Surface: primary ¼+1={p} (min 1) · secondary {} · scatter 5-6 (p.251)",
                acs_orbit_to_surface_secondary(p),
            ))
        }
        AcsAeroMission::CombatDrop => {
            let r = acs_combat_drop_result(0);
            Some(format!(
                "Combat Drop (TN 6): MoS 0 → drop {:+}, {} (fail: {}% armor) — MoS-driven (p.251)",
                r.drop_value, r.result, r.drop_damage_pct,
            ))
        }
    };
    if let Some(m) = mission_line {
        lines.push(Line::from(Span::styled(
            m,
            Style::default().fg(theme().good),
        )));
    }
}

fn draw_acs_detail(f: &mut Frame, area: Rect, app: &App) {
    let Some(fs) = app
        .session
        .acs
        .formations
        .get(app.session.acs.active_formation)
    else {
        return;
    };
    let empty = |f: &mut Frame, title: &str| {
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "no Combat Unit selected",
                Style::default().fg(theme().dim),
            )))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme().dim))
                    .title(format!(" {title} ")),
            ),
            area,
        );
    };
    let Some(cu) = fs.units.get(app.session.acs.active_unit) else {
        empty(f, &fs.name);
        return;
    };
    let derived = app.session.acs_combat_unit(cu);
    let mut lines: Vec<Line> = Vec::new();

    let aero = app.session.acs_formation_is_aerospace(fs);

    lines.push(Line::from(Span::styled(
        format!(
            "{} SZ{} MV{}{} TMM{}",
            format!("{:?}", derived.acs_type).to_uppercase(),
            derived.size,
            derived.movement,
            derived.move_mode.code(),
            derived.tmm,
        ),
        Style::default().fg(theme().dim),
    )));
    lines.push(Line::from(vec![
        Span::styled("Skill ", Style::default().fg(theme().dim)),
        Span::styled(
            format!("{}", derived.skill),
            Style::default()
                .fg(theme().accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(
                "  Mor {}  PV {}",
                derived.morale_rating, derived.point_value
            ),
            Style::default().fg(theme().dim),
        ),
    ]));
    lines.push(Line::from(vec![
        Span::styled("Dmg  ", Style::default().fg(theme().dim)),
        Span::styled(
            format!(
                "S{} M{} L{}",
                sbf_dmg(derived.damage.s),
                sbf_dmg(derived.damage.m),
                sbf_dmg(derived.damage.l.unwrap_or(0.0))
            ),
            Style::default(),
        ),
    ]));

    // The to-hit / damage readout (hand-set range + target TMM).
    lines.push(Line::from(""));
    if aero {
        acs_aero_readout(app, &derived, &mut lines);
    } else if let Some(ctx) = app.acs_to_hit_ctx() {
        let tn = acs_to_hit(&ctx);
        let band = ctx.range.band(&derived.damage);
        let dmg = acs_damage(
            band,
            &AcsDamageCtx {
                secondary_target: ctx.secondary_target,
                attacker_fatigue: ctx.fatigue,
                attacker_morale: ctx.own_morale,
                ..Default::default()
            },
        );
        lines.push(Line::from(vec![
            Span::styled("To-Hit ", Style::default().fg(theme().dim)),
            Span::styled(
                format!("{tn}+"),
                Style::default()
                    .fg(theme().warning)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(
                    "  ({} vs TMM{}{})",
                    acs_range_label(ctx.range),
                    ctx.target_tmm,
                    if ctx.secondary_target { ", 2nd" } else { "" }
                ),
                Style::default().fg(theme().dim),
            ),
        ]));
        lines.push(Line::from(vec![
            Span::styled("Damage ", Style::default().fg(theme().dim)),
            Span::styled(
                format!("{dmg}"),
                Style::default()
                    .fg(theme().accent)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "  ([/] range · +/- TMM · s 2nd)",
                Style::default().fg(theme().dim),
            ),
        ]));
    }

    // Morale-check readout for the current state.
    lines.push(Line::from(""));
    let cu_band = acs_damage_band(
        cu.armor_remaining(&derived),
        derived.armor,
        derived.damage_thresholds,
    );
    let morale_tn = acs_morale_tn(&AcsMoraleCtx {
        morale_value: derived.morale_rating,
        experience: AcsExperience::from_skill(derived.skill),
        fatigue: acs_fatigue_band(
            cu.fatigue_points(),
            AcsExperience::from_skill(derived.skill),
        ),
        third_threshold: matches!(cu_band, AcsDamageBand::Pct25),
        ..Default::default()
    });
    lines.push(Line::from(vec![
        Span::styled("Morale ", Style::default().fg(theme().dim)),
        Span::styled(
            cu.morale.label(),
            Style::default().fg(acs_morale_color(cu.morale)),
        ),
        Span::styled(
            format!("  check {morale_tn}+ (2d6)"),
            Style::default().fg(theme().dim),
        ),
    ]));

    // The Combat-Team derivation fold.
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Combat Teams",
        Style::default()
            .fg(theme().dim)
            .add_modifier(Modifier::BOLD),
    )));
    for t in &cu.teams {
        let n: usize = t.units.iter().map(|u| u.elements.len()).sum();
        lines.push(Line::from(Span::styled(
            format!("  {} — {} elements", trunc(&t.name, 22), n),
            Style::default().fg(theme().dim),
        )));
    }

    f.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme().dim))
                .title(format!(" {} ", cu.name)),
        ),
        area,
    );
}

/// Body lines for the ACS grouping editor: every pool element with its current Formation · Combat
/// Unit · Team · SBF-Unit path, plus the four-tier split verbs.
fn acs_group_modal_lines(app: &App, sel: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for (i, tm) in app.session.mechs.iter().enumerate() {
        let selected = i == sel;
        let marker = if selected { "▶ " } else { "  " };
        let name_style = if selected {
            Style::default()
                .fg(theme().on_accent)
                .bg(theme().accent)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        let name = trunc(&tm.spec.display_name(), 26);
        let (tag, tag_style) = match app.session.acs_element_assignment(i) {
            Some((fi, cui, ti, ui)) => {
                let f = &app.session.acs.formations[fi];
                let cu = &f.units[cui];
                let u = &cu.teams[ti].units[ui];
                (
                    trunc(&format!("{} · {} · {}", f.name, cu.name, u.name), 30),
                    Style::default().fg(theme().dim),
                )
            }
            None => ("— unassigned".into(), Style::default().fg(theme().warning)),
        };
        lines.push(Line::from(vec![
            Span::styled(
                format!("{marker}{:>2} {name:<26} SK{:<2} ", i + 1, tm.gunnery),
                name_style,
            ),
            Span::styled(tag, tag_style),
        ]));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "[↑↓] element   [←→] move between SBF Units",
        Style::default().fg(theme().dim),
    )));
    lines.push(Line::from(Span::styled(
        "[n/t/c/F] split → new Unit / Team / Combat Unit / Formation",
        Style::default().fg(theme().dim),
    )));
    lines.push(Line::from(Span::styled(
        "[u] unassign   [a] auto-group…   [Esc] done",
        Style::default().fg(theme().dim),
    )));
    lines
}

fn acs_help_modal_lines() -> Vec<Line<'static>> {
    let header = |s: &'static str| {
        Line::from(Span::styled(
            s,
            Style::default()
                .fg(theme().accent)
                .add_modifier(Modifier::BOLD),
        ))
    };
    let row = |keys: &'static str, desc: &'static str| {
        Line::from(vec![
            Span::styled(format!("  {keys:<9}"), Style::default().fg(theme().accent)),
            Span::styled(desc, Style::default().fg(theme().dim)),
        ])
    };
    vec![
        header("Abstract Combat System"),
        row("Space", "apply damage to Combat Unit (typed amount)"),
        row("u", "repair 1 armor"),
        row("m / M", "cycle Combat Unit / Formation morale rung"),
        row("f / F", "accrue fatigue (fought) / rest (−1 FP)"),
        row("n / e", "begin round / Formation done this turn"),
        row("g", "group the pool into a Formation"),
        row("r / D", "rename / delete Formation"),
        row("C / l", "mark Force Commander / Formation Leader"),
        header("Readout"),
        row("[ / ]", "cycle range (Short/Medium/Long)"),
        row("+ / -", "target TMM up / down"),
        row("s", "toggle secondary target"),
        header("Selection"),
        row(", / .", "previous / next Formation"),
        row("↑↓ / kj", "previous / next Combat Unit"),
        header("General"),
        row("a / z", "add elements / undo"),
        row("P", "export record-sheet PDF"),
        row("S / q", "sessions browser / quit"),
        Line::from(Span::styled(
            "  press any key to close",
            Style::default().fg(theme().dim),
        )),
    ]
}

// ---------- Standard BattleForce mode (spec Phase 3) ----------

const BF_HELP: &str =
    " Spc:dmg  u:rep  o/i:heat  c:crit  t:to-hit  g:group  m:morale  n:round  r:rename  ,/.:unit  ?:help ";

/// Card height in the BF grid (borders included). One line tighter than the AS card — the spacer
/// goes to the hex-native range-bracket footer, and a Unit header row rides above each group.
const BF_CARD_H: u16 = 12;

fn bf_morale_color(m: BfMorale) -> Color {
    match m {
        BfMorale::Normal => theme().dim,
        BfMorale::Broken => theme().warning,
        BfMorale::Routed => theme().danger,
    }
}

/// Format a BF damage cell: `None` = no attack (dash), 0.5 = the `0*` minimal band, else whole.
fn bf_dmg_cell(v: Option<f32>) -> String {
    match v {
        None => "—".into(),
        Some(x) if (x - 0.5).abs() < f32::EPSILON => "0*".into(),
        Some(x) if x.fract() == 0.0 => format!("{}", x as i64),
        Some(x) => format!("{x:.1}"),
    }
}

/// One vertical slice of the BF sheet: a Unit header row, an empty-unit placeholder, or a row of
/// element cards (chunked to the grid width).
enum BfBlock {
    /// `Some(ui)` = a Unit's header; `None` = the implicit "Unassigned" section header.
    Header(Option<usize>),
    /// The placeholder under an element-less Unit's header.
    Empty,
    Cards(Vec<usize>),
}

/// The Standard BF sheet: the AS adaptive card grid grouped under Unit (lance) header rows
/// (spec §3.2), paged vertically so the active element's card is always in view.
fn draw_battleforce(f: &mut Frame, area: Rect, sidebar: bool, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(u16::from(!sidebar)),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(area);
    draw_roster(f, chunks[0], app);

    if app.session.mechs.is_empty() && app.session.bf.units.is_empty() {
        f.render_widget(
            Paragraph::new("No unit. Press [a] to add one.").alignment(Alignment::Center),
            chunks[1],
        );
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                BF_HELP,
                Style::default().fg(theme().dim),
            ))),
            chunks[2],
        );
        return;
    }

    // Grid width follows the AS card grid: Pi shows 2 columns, Modern fits 2–4.
    const CARD_MIN_W: usize = 38;
    let cols = if profile() == DisplayProfile::Modern {
        (chunks[1].width as usize / CARD_MIN_W).clamp(2, 4)
    } else {
        2
    };

    // Units in sheet order, then the implicit Unassigned section (spec §2.3).
    let mut blocks: Vec<(BfBlock, u16)> = Vec::new();
    for (ui, u) in app.session.bf.units.iter().enumerate() {
        blocks.push((BfBlock::Header(Some(ui)), 1));
        if u.elements.is_empty() {
            blocks.push((BfBlock::Empty, 1));
        } else {
            for chunk in u.elements.chunks(cols) {
                blocks.push((BfBlock::Cards(chunk.to_vec()), BF_CARD_H));
            }
        }
    }
    let unassigned: Vec<usize> = (0..app.session.mechs.len())
        .filter(|&i| app.session.bf_element_assignment(i).is_none())
        .collect();
    if !unassigned.is_empty() {
        blocks.push((BfBlock::Header(None), 1));
        for chunk in unassigned.chunks(cols) {
            blocks.push((BfBlock::Cards(chunk.to_vec()), BF_CARD_H));
        }
    }

    // Greedy vertical pagination; the page holding the active element's card is shown. A header
    // must fit together with the block under it, so a Unit never orphans its title at a page foot.
    let avail = chunks[1].height;
    let mut pages: Vec<Vec<usize>> = Vec::new();
    let mut cur: Vec<usize> = Vec::new();
    let mut used: u16 = 0;
    for (bi, (block, bh)) in blocks.iter().enumerate() {
        let needed = match block {
            BfBlock::Header(_) => bh + blocks.get(bi + 1).map_or(0, |(_, h)| *h),
            _ => *bh,
        };
        if !cur.is_empty() && used + needed > avail {
            pages.push(std::mem::take(&mut cur));
            used = 0;
        }
        cur.push(bi);
        used += bh;
    }
    if !cur.is_empty() {
        pages.push(cur);
    }
    let active = app.session.active;
    let active_block = blocks
        .iter()
        .position(|(b, _)| matches!(b, BfBlock::Cards(v) if v.contains(&active)))
        .unwrap_or(0);
    let page_idx = pages
        .iter()
        .position(|p| p.contains(&active_block))
        .unwrap_or(0);

    let mut y = chunks[1].y;
    for &bi in &pages[page_idx] {
        let (block, bh) = &blocks[bi];
        let h = (*bh).min((chunks[1].y + chunks[1].height).saturating_sub(y));
        if h == 0 {
            break;
        }
        let rect = Rect {
            x: chunks[1].x,
            y,
            width: chunks[1].width,
            height: h,
        };
        match block {
            BfBlock::Header(ui) => {
                f.render_widget(Paragraph::new(bf_unit_header_line(app, *ui)), rect);
            }
            BfBlock::Empty => f.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    "    (no elements — [g] assigns, [a] adds)",
                    Style::default().fg(theme().dim),
                ))),
                rect,
            ),
            BfBlock::Cards(idxs) => {
                let col_rects = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints(vec![Constraint::Ratio(1, cols as u32); cols])
                    .split(rect);
                for (c, &i) in idxs.iter().enumerate() {
                    draw_bf_card(f, col_rects[c], app, i, i == active);
                }
            }
        }
        y += h;
    }

    // Footer: page indicator (when paged) + status + keys, the AS pattern.
    let base = if pages.len() > 1 {
        format!(" page {}/{} ·{BF_HELP}", page_idx + 1, pages.len())
    } else {
        String::from(BF_HELP)
    };
    let help = if app.status.is_empty() {
        base
    } else {
        format!(" {} | {}", app.status, base.trim_start())
    };
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            help,
            Style::default().fg(theme().dim),
        ))),
        chunks[2],
    );
}

/// A Unit header row: `▸ Fire Lance   MV 3 (j3)  SZ 2  PV 187  Broken` — live Unit MV
/// ([`battleforce::bf_unit_mv`] over the members' current MP), static Size (p.53), summed
/// skill-adjusted PV, the manual morale rung, and the shutdown-pin badge (p.49).
fn bf_unit_header_line(app: &App, ui: Option<usize>) -> Line<'static> {
    let active_assignment = app.session.bf_element_assignment(app.session.active);
    let (name, is_active) = match ui {
        Some(ui) => (
            app.session.bf.units[ui].name.clone(),
            active_assignment == Some(ui),
        ),
        None => ("Unassigned".to_string(), active_assignment.is_none()),
    };
    let name_style = if is_active {
        Style::default()
            .fg(theme().accent)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(theme().dim)
            .add_modifier(Modifier::BOLD)
    };
    let mut spans = vec![Span::styled(format!("▸ {name}"), name_style)];
    let Some(ui) = ui else {
        return Line::from(spans);
    };
    let u = &app.session.bf.units[ui];
    if u.elements.is_empty() {
        return Line::from(spans);
    }
    let members = app.bf_member_stats(&u.elements);
    let (mv, jump) = battleforce::bf_unit_mv(&members);
    let jump_str = match jump {
        Some(j) => format!(" (j{j})"),
        None => String::new(),
    };
    let pv: u64 = u
        .elements
        .iter()
        .filter_map(|&i| app.session.mechs.get(i))
        .map(|tm| tm.point_cost(GameMode::BattleForce))
        .sum();
    spans.push(Span::styled(
        format!("   MV {mv}{jump_str}  SZ {}  PV {pv}  ", u.size),
        Style::default().fg(theme().dim),
    ));
    spans.push(Span::styled(
        u.morale.label(),
        Style::default().fg(bf_morale_color(u.morale)),
    ));
    if u.elements
        .iter()
        .any(|&i| app.session.bf_shutdown(i) && !app.session.mechs[i].bf_destroyed())
    {
        // "Ground Units containing a shutdown Element cannot move" (p.49) — a badge, not an MP
        // change. Destroyed members never pin (the p.52 Unit-MP rule is over SURVIVING
        // elements; a dead element can't cool down and would pin forever).
        spans.push(Span::styled(
            "  CANNOT MOVE (shutdown)",
            Style::default()
                .fg(theme().danger)
                .add_modifier(Modifier::BOLD),
        ));
    }
    Line::from(spans)
}

/// One BF element card in the grid — the AS card with the spec §3.2 deltas (hex-native live MV,
/// live TMM, post-crit damage + derived ground E, the BF crit vocabulary, per-bracket To-Hit via
/// the persisted shot context, and the hex range-bracket footer).
fn draw_bf_card(f: &mut Frame, area: Rect, app: &App, i: usize, active: bool) {
    let Some(tm) = app.session.mechs.get(i) else {
        return;
    };
    let border_style = if tm.bf_destroyed() {
        Style::default()
            .fg(theme().danger)
            .add_modifier(Modifier::BOLD)
    } else if active {
        Style::default()
            .fg(theme().accent)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme().dim)
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(if active {
            BorderType::Double
        } else {
            BorderType::Plain
        })
        .border_style(border_style)
        .title(format!(" {} ", tm.spec.display_name()));
    f.render_widget(Paragraph::new(bf_card_lines(app, i)).block(block), area);
}

fn bf_card_lines(app: &App, i: usize) -> Vec<Line<'static>> {
    let tm = &app.session.mechs[i];
    let a = &tm.spec.as_stats;
    let el = bf_element_of(tm);
    let aero = battleforce::bf_is_aero(&el);
    let large = a.arcs.is_some();
    let gray = Style::default().fg(theme().dim);
    let mut lines: Vec<Line> = Vec::new();

    // Type / size / hex-native MV with live degradation / TMM (live) — or the aero TH row.
    let sz = if a.size == 0 {
        "-".to_string()
    } else {
        a.size.to_string()
    };
    let base_mp = if aero {
        el.primary_move
    } else {
        inches_to_hexes(el.primary_move)
    };
    let cur_mp = app.session.bf_current_mp(i);
    let mv_base = movement_hexes(&a.movement);
    let mut mv = format!("MV {mv_base}");
    if cur_mp != base_mp {
        let mut why = Vec::new();
        if el.has_sua("TSM") && tm.as_heat >= 1 {
            why.push("TSM".to_string());
        }
        if tm.as_heat > 0 {
            why.push(format!("heat {}", tm.as_heat));
        }
        if tm.bf.mp_lost > 0 {
            why.push("crit".to_string());
        }
        if tm.bf.engine > 0
            && matches!(
                battleforce::bf_crit_col(&el),
                Some(battleforce::BfCritCol::Vehicle | battleforce::BfCritCol::Aerospace)
            )
        {
            why.push("engine".to_string());
        }
        if tm.bf.motive.any() {
            why.push("motive".to_string());
        }
        mv = format!("MV {mv_base}→{cur_mp} ({})", why.join(", "));
    }
    let tail = if aero {
        // Aerospace: no ground TMM bracket (p.86 fn1); the TH threshold drives crit chances.
        format!("TH {t} (crit if hit > {t})", t = a.threshold)
    } else {
        let live = app.session.bf_live_tmm(i);
        if live == i32::from(a.tmm) {
            format!("TMM {}", a.tmm)
        } else {
            format!("TMM {} (live {live})", a.tmm)
        }
    };
    lines.push(Line::from(Span::styled(
        format!("{}   SZ {sz}   {mv}   {tail}", a.tp),
        gray,
    )));

    // Skill + skill-adjusted PV (the BF Skill PV table IS the AS one, p.50).
    let adj_pv = skill_adjusted_pv(u32::from(a.pv), tm.gunnery);
    lines.push(Line::from(vec![
        Span::styled("Skill ", gray),
        Span::styled(
            format!("{}+", tm.gunnery),
            Style::default()
                .fg(theme().accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("   PV ", gray),
        Span::styled(
            format!("{adj_pv}"),
            Style::default()
                .fg(theme().accent)
                .add_modifier(Modifier::BOLD),
        ),
    ]));

    let pip_row = |label: &str, rem: u8, max: u8| {
        Line::from(vec![
            Span::styled(format!("{label:<10}"), gray),
            Span::styled(
                "█".repeat(rem as usize),
                Style::default().fg(frac_color(rem.into(), max.into())),
            ),
            Span::styled("░".repeat(max.saturating_sub(rem) as usize), gray),
            Span::styled(format!("  {rem}/{max}"), gray),
        ])
    };
    lines.push(pip_row("Armor", tm.as_armor_remaining(), a.armor));
    lines.push(pip_row("Structure", tm.as_struct_remaining(), a.structure));

    // Heat: the shared 1 2 3 S ladder (p.26) + the OV rating. Large craft omit it — they don't
    // overheat at BF scale, and dropping it keeps the taller multi-arc card within the card height.
    if !large {
        let mut heat = vec![Span::styled(format!("{:<10}", "Heat"), gray)];
        for h in 0..=4u8 {
            let label = if h == 4 {
                "S".to_string()
            } else {
                h.to_string()
            };
            if h == tm.as_heat {
                heat.push(Span::styled(
                    format!("[{label}]"),
                    Style::default()
                        .fg(theme().danger)
                        .add_modifier(Modifier::BOLD | Modifier::REVERSED),
                ));
            } else {
                heat.push(Span::styled(format!(" {label} "), gray));
            }
        }
        if a.overheat > 0 {
            heat.push(Span::styled(
                format!("  OV {}", a.overheat),
                Style::default().fg(theme().warning),
            ));
        }
        lines.push(Line::from(heat));
    }

    // Damage: large craft show the multi-arc card (front/left/right/rear × weapon class); every
    // other unit shows the single S/M/L/E line (post-crit; ground E derives as L−1, spec §1.1).
    if let Some(card) = &a.arcs {
        // Per-arc damage (S/M/L/E per weapon class) replaces the single damage line + range footer.
        for arc in large_craft::Arc::ALL {
            let disp = large_craft::arc_display_lines(card, arc);
            if disp.is_empty() {
                continue;
            }
            let mut spans = vec![Span::styled(format!("  {:<6}", arc.label()), gray)];
            for (cls, s) in disp {
                spans.push(Span::styled(
                    format!("{} {}  ", cls.label(), s),
                    Style::default(),
                ));
            }
            let arc_specials = &arc.of(card).specials;
            if !arc_specials.is_empty() {
                spans.push(Span::styled(
                    format!("⟨{}⟩", arc_specials.join(" ")),
                    Style::default().fg(theme().good),
                ));
            }
            lines.push(Line::from(spans));
        }
    } else {
        // Post-crit damage per bracket; ground E derives as L−1 at attack time (p.84, spec §1.1).
        let dmg_cell = |label: &str, r: BfRange| {
            let cell = bf_dmg_cell(app.session.bf_current_damage(i, r));
            let style = if cell == "—" {
                gray
            } else {
                Style::default()
            };
            Span::styled(format!("{label}{cell:<4}"), style)
        };
        lines.push(Line::from(vec![
            Span::styled(format!("{:<10}", "Damage"), gray),
            dmg_cell("S ", BfRange::Short),
            dmg_cell("M ", BfRange::Medium),
            dmg_cell("L ", BfRange::Long),
            dmg_cell("E ", BfRange::Extreme),
        ]));
    }

    // To-Hit per bracket via the persisted shot context (spec §3.2) — or the status banner.
    if tm.bf_destroyed() {
        lines.push(Line::from(Span::styled(
            "*** DESTROYED ***",
            Style::default()
                .fg(theme().danger)
                .add_modifier(Modifier::BOLD | Modifier::REVERSED),
        )));
    } else if tm.as_shutdown() {
        lines.push(Line::from(Span::styled(
            "*** SHUTDOWN (heat) ***",
            Style::default()
                .fg(theme().warning)
                .add_modifier(Modifier::BOLD | Modifier::REVERSED),
        )));
    } else if aero && tm.bf.engine >= battleforce::BF_ENGINE_HITS_DESTROY {
        // The aerospace 2nd Engine hit: TP 0 + shutdown, not destruction (spec §1.4).
        lines.push(Line::from(Span::styled(
            "*** TP 0 — SHUTDOWN (engine) ***",
            Style::default()
                .fg(theme().warning)
                .add_modifier(Modifier::BOLD | Modifier::REVERSED),
        )));
    } else if a.arcs.is_some() {
        // Large craft resolve to-hit per firing arc + weapon class (the arc block above), not the
        // single-vector brackets, so no To-Hit row here. Per-arc shot builder: next increment.
    } else {
        let shot = app.bf_shot_for(i);
        let to_hit = |label: &str, r: BfRange| {
            if app.session.bf_current_damage(i, r).is_none() {
                return Span::styled(format!("{label}{:<4}", "—"), gray);
            }
            let mut s = shot;
            s.range = r;
            let n = battleforce::bf_to_hit(&el, tm.gunnery, tm.as_heat, tm.bf.fire_control, &s);
            Span::styled(format!("{label}{:<4}", format!("{n}+")), gray)
        };
        // `*` = a hand-entered shot context is folded in (edit with t).
        let tag = if app.bf_shot == BfShotUi::default() {
            "To-Hit"
        } else {
            "To-Hit*"
        };
        lines.push(Line::from(vec![
            Span::styled(format!("{tag:<10}"), gray),
            to_hit("S ", BfRange::Short),
            to_hit("M ", BfRange::Medium),
            to_hit("L ", BfRange::Long),
            to_hit("E ", BfRange::Extreme),
        ]));
    }

    // The BF crit vocabulary (spec §2.2): counters, the accumulated MP loss, and the flags.
    let mut crits = vec![Span::styled(format!("{:<10}", "Crits"), gray)];
    let mark = |label: String, hot: bool| {
        Span::styled(
            label,
            if hot {
                Style::default().fg(theme().danger)
            } else {
                gray
            },
        )
    };
    crits.push(mark(format!("Eng{}  ", tm.bf.engine), tm.bf.engine > 0));
    crits.push(mark(
        format!("FC{}  ", tm.bf.fire_control),
        tm.bf.fire_control > 0,
    ));
    crits.push(mark(format!("MP−{}  ", tm.bf.mp_lost), tm.bf.mp_lost > 0));
    crits.push(mark(format!("Wpn{}  ", tm.bf.weapon), tm.bf.weapon > 0));
    if tm.bf.crew_stunned {
        crits.push(mark("STUN  ".into(), true));
    }
    // Motive spent-flags render independently — they stack (p.43, §1.2).
    if tm.bf.motive.minus_one {
        crits.push(mark("MOT−1  ".into(), true));
    }
    if tm.bf.motive.half {
        crits.push(mark("MOT½  ".into(), true));
    }
    if tm.bf.motive.immobile {
        crits.push(mark("MOT0  ".into(), true));
    }
    if tm.bf.arm_spent {
        crits.push(Span::styled(
            "ARM✗  ".to_string(),
            Style::default().fg(theme().warning),
        ));
    }
    lines.push(Line::from(crits));

    let specials = if a.specials.is_empty() {
        "—".to_string()
    } else {
        a.specials.join("  ")
    };
    lines.push(Line::from(vec![
        Span::styled(format!("{:<10}", "Specials"), gray),
        Span::styled(specials, Style::default().fg(theme().good)),
    ]));

    // Hex-native range brackets (p.38) — NOT the AS inches÷2 labels (spec §1.1). Large craft
    // resolve per arc, so they omit the single range-bracket footer to keep the card compact.
    if !large {
        lines.push(Line::from(Span::styled(
            format!("Rng {}", battleforce::bf_range_label(aero)),
            gray,
        )));
    }
    lines
}

/// Body lines for the BF crit modal (spec §3.3): the element's column of the p.42 table as the
/// pick list (2D6 rows 2..=12), the defender's crit-roll modifiers as a dim hint, the live crit
/// state, and — for vehicles — the once-per-game motive rungs with the p.44 table reference.
fn bf_crit_modal_lines(app: &App, sel: usize) -> Vec<Line<'static>> {
    use battleforce::{BfCrit, BfCritCol};
    let mut lines = Vec::new();
    let Some(tm) = app.session.active_mech() else {
        return lines;
    };
    let el = bf_element_of(tm);
    let Some(col) = battleforce::bf_crit_col(&el) else {
        lines.push(Line::from(Span::styled(
            "Infantry and BA never take critical hits (p.42)",
            Style::default().fg(theme().dim),
        )));
        return lines;
    };
    let col_name = match col {
        BfCritCol::Mech => "'Mech",
        BfCritCol::ProtoMech => "ProtoMech",
        BfCritCol::Vehicle => "Vehicle",
        BfCritCol::Aerospace => "Aerospace",
        BfCritCol::DropShip => "DropShip",
        BfCritCol::JumpShip => "JumpShip",
    };
    let rolls = battleforce::bf_crit_rolls(&el);
    lines.push(Line::from(Span::styled(
        format!(
            "{} column — pick the 2D6 result{}",
            col_name,
            if rolls > 1 {
                "  (IndustrialMech: roll TWICE, apply both)"
            } else {
                ""
            }
        ),
        Style::default()
            .fg(theme().accent)
            .add_modifier(Modifier::BOLD),
    )));
    // Defender-side crit-roll modifiers (CR −2 / IRA +1 / RFA +2) + the ARM spent-checkbox.
    let m = battleforce::bf_crit_roll_mod(&el);
    let mut hints = Vec::new();
    if m != 0 {
        hints.push(format!("crit roll {m:+} (apply before picking the row)"));
    }
    if el.has_sua("ARM") {
        hints.push(if tm.bf.arm_spent {
            "ARM spent".to_string()
        } else {
            "ARM: first crit chance ignored — [a] marks it spent".to_string()
        });
    }
    if !hints.is_empty() {
        lines.push(Line::from(Span::styled(
            hints.join("   "),
            Style::default().fg(theme().warning),
        )));
    }
    // Large-craft crit ladders, shown only when they carry state (spec §10 table-side effects).
    let large = {
        let mut s = String::new();
        if tm.bf.crew_hit > 0 {
            s.push_str(&format!(" Crew{}", tm.bf.crew_hit));
        }
        if tm.bf.kf_drive > 0 {
            s.push_str(&format!(" KF−{}", 2 * tm.bf.kf_drive));
        }
        if tm.bf.dock_hits > 0 {
            s.push_str(&format!(" Dock−{}", tm.bf.dock_hits));
        }
        if tm.bf.door_hits > 0 {
            s.push_str(&format!(" Door−{}", tm.bf.door_hits));
        }
        if tm.bf.kf_boom {
            s.push_str(" KFBoom");
        }
        if tm.bf.docking_collar {
            s.push_str(" NoDock");
        }
        if tm.bf.thruster {
            s.push_str(" Thr");
        }
        s
    };
    lines.push(Line::from(Span::styled(
        format!(
            "live: Eng{} FC{} MP−{} Wpn{}{}{}{}{}{}",
            tm.bf.engine,
            tm.bf.fire_control,
            tm.bf.mp_lost,
            tm.bf.weapon,
            if tm.bf.crew_stunned { " STUN" } else { "" },
            if tm.bf.motive.minus_one {
                " MOT−1"
            } else {
                ""
            },
            if tm.bf.motive.half { " MOT½" } else { "" },
            if tm.bf.motive.immobile { " MOT0" } else { "" },
            large,
        ),
        Style::default().fg(theme().dim),
    )));
    lines.push(Line::from(""));

    let crit_label = |c: BfCrit| match c {
        BfCrit::NoCrit => "No Critical Hit",
        BfCrit::Ammo => "Ammo Hit",
        BfCrit::Engine => "Engine Hit",
        BfCrit::FireControl => "Fire Control Hit",
        BfCrit::Mp => "MP Hit",
        BfCrit::Weapon => "Weapon Hit",
        BfCrit::CrewStunned => "Crew Stunned",
        BfCrit::CrewKilled => "Crew Killed",
        BfCrit::Fuel => "Fuel Hit",
        BfCrit::HeadBlownOff => "Head Blown Off",
        BfCrit::ProtoDestroyed => "Unit Destroyed",
        BfCrit::KfBoom => "KF Boom",
        BfCrit::DockingCollar => "Docking Collar",
        BfCrit::Thruster => "Thruster",
        BfCrit::Door => "Door",
        BfCrit::CrewHit => "Crew Hit",
        BfCrit::KfDrive => "K-F Drive",
        BfCrit::Dock => "Dock",
    };
    // BD gun emplacements: weapons-only crit vocabulary (spec §Data-fidelity 8) — every other
    // effect row "does not apply" and is +1 damage instead (p.42); render those dim with the
    // substitute effect, and suppress the motive rows entirely (an emplacement has no drive).
    let bd = el.as_type == "BD";
    for row in 0..11usize {
        let roll = row as i32 + 2;
        let result = battleforce::bf_crit(roll, col);
        let inapplicable = bd && !matches!(result, BfCrit::NoCrit | BfCrit::Weapon);
        let selected = row == sel;
        let marker = if selected { "▶ " } else { "  " };
        let style = if selected {
            Style::default()
                .fg(theme().on_accent)
                .bg(theme().accent)
                .add_modifier(Modifier::BOLD)
        } else if result == BfCrit::NoCrit || inapplicable {
            Style::default().fg(theme().dim)
        } else {
            Style::default()
        };
        let suffix = if inapplicable {
            "  (+1 damage instead, p.42)"
        } else {
            ""
        };
        lines.push(Line::from(Span::styled(
            format!("{marker}{roll:>2}  {}{suffix}", crit_label(result)),
            style,
        )));
    }

    if col == BfCritCol::Vehicle && !bd {
        lines.push(Line::from(Span::styled(
            "Motive damage (p.44) — once per game each:",
            Style::default().fg(theme().accent),
        )));
        let rungs = [
            ("−1 MV", tm.bf.motive.minus_one),
            ("½ MV (round down)", tm.bf.motive.half),
            ("Immobilized", tm.bf.motive.immobile),
        ];
        for (k, (label, marked)) in rungs.into_iter().enumerate() {
            let row = 11 + k;
            let selected = row == sel;
            let marker = if selected { "▶ " } else { "  " };
            let style = if selected {
                Style::default()
                    .fg(theme().on_accent)
                    .bg(theme().accent)
                    .add_modifier(Modifier::BOLD)
            } else if marked {
                Style::default().fg(theme().dim)
            } else {
                Style::default()
            };
            let spent = if marked { "  ✓ spent" } else { "" };
            lines.push(Line::from(Span::styled(
                format!("{marker}    {label}{spent}"),
                style,
            )));
        }
        lines.push(Line::from(Span::styled(
            "1D6 5-6 then 2D6: 8-9 −1MV · 10-11 ½MV · 12 immob.  (Wheeled +2, Hover +3, VTOL/WiGE +4, rear +1)",
            Style::default().fg(theme().dim),
        )));
        if matches!(el.primary_mode.as_str(), "v" | "g") {
            lines.push(Line::from(Span::styled(
                "VTOL/WiGE at 0 MV while airborne crashes: 1 damage + immobile (p.43)",
                Style::default().fg(theme().dim),
            )));
        }
    }
    lines.push(Line::from(Span::styled(
        "[↑↓] select   [Enter/Spc] apply   [a] ARM spent   [Esc] close",
        Style::default().fg(theme().dim),
    )));
    lines
}

/// Body lines for the BF to-hit shot editor — the p.39 To-Hit Modifiers Table as editable rows
/// (attacker legs derive from the element and fold in silently), with the live TN + damage
/// preview below (spec §3.3).
fn bf_shot_modal_lines(app: &App, sel: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let Some(tm) = app.session.active_mech() else {
        return lines;
    };
    let el = bf_element_of(tm);
    let aero = battleforce::bf_is_aero(&el);
    let large = el.arcs.is_some();
    let s = app.bf_shot;
    let yn = |b: bool| if b { "[x]" } else { "[ ]" };

    // Large craft: the per-arc TN + damage preview renders at the TOP so it stays visible above the
    // (long) row editor. Every weapon class resolves through the standard BF to-hit (IO:BF p.83
    // Advanced Combat Modifiers Table): base `bf_to_hit` (range +0/+2/+4/+6) + the capital /
    // sub-capital "vs Small Target" modifier (CAP +5 / SCAP +3, applied only vs a small airborne
    // aerospace target) + any live Crew-Hit penalty. There is no capital bracket-reduction and no
    // 8/6/4 attack cap in standard BF — those are the Strategic-Aerospace (SBF) subsystem.
    if let Some(card) = &tm.spec.as_stats.arcs {
        let cls = s.weapon_class;
        let vec = large_craft::arc_damage(card, s.firing_arc, cls);
        let band = match s.range {
            BfRange::Short => vec.s,
            BfRange::Medium => vec.m,
            BfRange::Long => vec.l.unwrap_or(0.0),
            BfRange::Extreme => vec.e.unwrap_or(0.0),
        };
        let dmg_str = if band == 0.5 {
            "0*".to_string()
        } else {
            format!("{}", band as u32)
        };
        let rl = match s.range {
            BfRange::Short => "S",
            BfRange::Medium => "M",
            BfRange::Long => "L",
            BfRange::Extreme => "E",
        };
        let shot = app.bf_shot_for(app.session.active);
        // "Small target" (p.83 footnote 28) = an airborne aerospace unit (fighter / Small Craft /
        // Satellite); the capital-vs-small modifier is waived vs large-craft and ground targets.
        let small_aero = matches!(shot.target_kind, battleforce::BfTargetKind::AirborneAero(_));
        let cap_mod = cls.bf_vs_small_mod(small_aero);
        let crew_mod = 2 * i32::from(tm.bf.crew_hit);
        let n = battleforce::bf_to_hit(&el, tm.gunnery, tm.as_heat, tm.bf.fire_control, &shot)
            + cap_mod
            + crew_mod;
        lines.push(Line::from(Span::styled(
            format!(
                "{} {} @ {rl}:  TN {n}+   damage {dmg_str}",
                s.firing_arc.label(),
                cls.label()
            ),
            Style::default()
                .fg(theme().accent)
                .add_modifier(Modifier::BOLD),
        )));
        if cap_mod != 0 {
            lines.push(Line::from(Span::styled(
                format!(
                    "  {} vs small target: +{cap_mod} to-hit (p.83)",
                    cls.label()
                ),
                Style::default().fg(theme().dim),
            )));
        }
        lines.push(Line::from("")); // spacer before the row editor
    }

    let move_label = match s.attacker_move {
        BfMove::StoodStill => "stood still (−1)",
        BfMove::Moved => "moved (+0)",
        BfMove::Jumped => "jumped (+2)",
    };
    let range_label = match s.range {
        BfRange::Short => "Short (+0)",
        BfRange::Medium => "Medium (+2)",
        BfRange::Long => "Long (+4)",
        BfRange::Extreme => "Extreme (+6)",
    };
    let (spotter_attacked, spotter_remote) = match s.kind {
        BfAttackKind::Indirect {
            spotter_also_attacked,
            spotter_is_remote_sensor,
        } => (spotter_also_attacked, spotter_is_remote_sensor),
        _ => (false, false),
    };
    let max_ov = battleforce::bf_max_ov_commit(&el, tm.as_heat);
    let tmove_label = match s.target_move {
        BfTargetMove::StoodStill => "stood still",
        BfTargetMove::Ground => "ground (+TMM)",
        BfTargetMove::Jumped => "jumped (+TMM+1)",
        BfTargetMove::Submersible => "submersible (+TMM+1)",
        BfTargetMove::Dropped => "dropped (+3)",
    };
    let tkind_label = match s.target_kind {
        BfTargetKind::None => "none",
        BfTargetKind::BattleArmor => "Battle Armor (+1)",
        BfTargetKind::ProtoMech => "ProtoMech (+1)",
        BfTargetKind::Large => "Large LG/SLG/VLG (−1)",
        BfTargetKind::AirborneAero(BfAeroAngle::Nose) => "airborne aero, nose (+1)",
        BfTargetKind::AirborneAero(BfAeroAngle::Side) => "airborne aero, side (+2)",
        BfTargetKind::AirborneAero(BfAeroAngle::Aft) => "airborne aero, aft (+0)",
        BfTargetKind::AirborneDropship => "airborne DropShip (−2)",
        BfTargetKind::AirborneVtolWige => "airborne VTOL/WiGE (+1)",
    };
    let mas_label = match s.target_mas {
        0 => "none",
        1 => "MAS (+3 if still/immobile)",
        _ => "LMAS (+2 if still/immobile)",
    };
    let indirect = matches!(s.kind, BfAttackKind::Indirect { .. });
    let physical = matches!(s.kind, BfAttackKind::Physical(_));
    let jumpish = matches!(
        s.target_move,
        BfTargetMove::Jumped | BfTargetMove::Submersible
    );
    // MAS/LMAS bites weapon attacks against a target that stood still OR is immobile (p.148).
    let mas_active = !physical && (s.target_immobile || s.target_move == BfTargetMove::StoodStill);
    // The strafing/striking rear +1 (p.41); bombing never strikes the rear (p.48).
    let rear_active = matches!(
        s.kind,
        BfAttackKind::AirToGround(BfA2G::Strafing | BfA2G::Striking)
    );

    // (label, value, active) — inactive rows render dim (the AS shot-modal pattern).
    let mut rows: Vec<(&str, String, bool)> = vec![
        ("Attacker move", move_label.into(), true),
        ("Range", range_label.into(), true),
        ("Attack kind", bf_kind_label(s.kind).into(), true),
        (
            "  spotter also attacked",
            yn(spotter_attacked).into(),
            indirect,
        ),
        (
            "  remote-sensor spotter",
            yn(spotter_remote).into(),
            indirect,
        ),
        ("OV commit", format!("{} / max {max_ov}", s.ov), max_ov > 0),
        ("Area-effect (+1)", yn(s.area_effect).into(), true),
        ("Secondary target (+1)", yn(s.secondary).into(), true),
        ("Also spotting (+1)", yn(s.also_spotting).into(), true),
        ("Grounded aero (p.46)", yn(s.grounded).into(), aero),
        ("Target TMM", s.target_tmm.to_string(), true),
        ("Target move", tmove_label.into(), true),
        (
            "  ±JMPS/JMPW/SUBS/SUBW",
            format!("{:+}", s.target_move_adj),
            jumpish,
        ),
        ("Target immobile (−4)", yn(s.target_immobile).into(), true),
        ("Target type", tkind_label.into(), true),
        ("Target MAS/LMAS", mas_label.into(), mas_active),
        ("Woods (+1)", yn(s.target_woods).into(), true),
        (
            "Partial cover (+1)",
            yn(s.target_partial_cover).into(),
            true,
        ),
        (
            "Underwater (+1, atk submerged)",
            yn(s.target_underwater).into(),
            true,
        ),
        ("Target STL active", yn(s.target_stealth).into(), !physical),
        (
            "Target carrying BA (+3)",
            yn(s.target_carrying_ba).into(),
            physical,
        ),
        (
            "Strikes rear (+1 dmg)",
            yn(s.strike_rear).into(),
            rear_active,
        ),
    ];
    // Large craft add a firing-arc + weapon-class picker; ground units never see these rows.
    if large {
        rows.push(("Firing arc", s.firing_arc.label().into(), true));
        rows.push(("Weapon class", s.weapon_class.label().into(), true));
    }
    for (i, (name, val, active)) in rows.into_iter().enumerate() {
        let selected = i == sel;
        let marker = if selected { "▶ " } else { "  " };
        let style = if selected {
            Style::default()
                .fg(theme().on_accent)
                .bg(theme().accent)
                .add_modifier(Modifier::BOLD)
        } else if !active {
            Style::default().fg(theme().dim)
        } else {
            Style::default()
        };
        lines.push(Line::from(Span::styled(
            format!("{marker}{name:<30} {val}"),
            style,
        )));
    }

    // Live TN + damage preview (non-large; large craft render their per-arc preview at the TOP of
    // the modal, above the row editor, so it stays visible even when the row list is long).
    if large {
        return lines;
    }
    let i = app.session.active;
    let shot = app.bf_shot_for(i);

    let n = battleforce::bf_to_hit(&el, tm.gunnery, tm.as_heat, tm.bf.fire_control, &shot);
    let dmg = match shot.kind {
        BfAttackKind::Standard => format!(
            "damage {}",
            bf_dmg_cell(battleforce::bf_shot_damage(
                &el,
                shot.range,
                tm.bf.weapon,
                s.ov,
                tm.bf.engine
            ))
        ),
        BfAttackKind::Indirect { .. } => format!(
            "IF damage {} (never OV-boosted)",
            bf_dmg_cell(battleforce::bf_indirect_damage(
                &el,
                tm.bf.weapon,
                tm.bf.engine
            ))
        ),
        BfAttackKind::RearWeapons => format!(
            "REAR damage {}",
            bf_dmg_cell(battleforce::bf_rear_damage(
                &el,
                shot.range,
                tm.bf.weapon,
                tm.bf.engine
            ))
        ),
        BfAttackKind::Physical(p) => {
            // Charge spends ground MP; DFA spends jump MP (spec §1.5).
            let mp = if p == BfPhysical::Dfa {
                battleforce::bf_current_mp(
                    inches_to_hexes(el.jump_move),
                    tm.as_heat,
                    tm.bf.mp_lost,
                    BfMotive::default(),
                    false,
                    0,
                    None,
                )
            } else {
                app.session.bf_current_mp(i)
            };
            let v = battleforce::bf_physical_damage(p, &el, mp, tm.as_heat);
            match p {
                BfPhysical::Charge => format!("charge damage {} (self: target Size)", sbf_dmg(v)),
                BfPhysical::Dfa => {
                    format!("DFA damage {} (self: own Size, +1 on a miss)", sbf_dmg(v))
                }
                BfPhysical::AntiMech => format!("damage {} + crit roll on success", sbf_dmg(v)),
                _ => format!("physical damage {}", sbf_dmg(v)),
            }
        }
        BfAttackKind::AirToGround(a2g) => match a2g {
            BfA2G::AltitudeBombing => {
                // One bomb per hex, one hex per bomb (p.47) — the per-hex damage is a flat
                // 2 (p.48); only dive bombing concentrates the load in one hex.
                let bombs = el.sua_num("BOMB") as u32;
                format!(
                    "altitude bombing: {} to every element in the hex — one hex per bomb, {bombs} hex(es)",
                    sbf_dmg(battleforce::bf_bomb_damage(1))
                )
            }
            BfA2G::DiveBombing => {
                let bombs = el.sua_num("BOMB") as u32;
                format!(
                    "dive bombing: {} to every element in the hex ({bombs} bomb(s) × 2)",
                    sbf_dmg(battleforce::bf_bomb_damage(bombs))
                )
            }
            BfA2G::Strafing => format!(
                "strafing damage {} per element in the strafed hexes",
                sbf_dmg(battleforce::bf_strafe_damage(
                    &el,
                    tm.bf.weapon,
                    s.ov,
                    s.strike_rear
                ))
            ),
            BfA2G::Striking => format!(
                "striking damage {}",
                sbf_dmg(battleforce::bf_strike_damage(
                    &el,
                    tm.bf.weapon,
                    s.ov,
                    s.strike_rear
                ))
            ),
        },
    };
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        format!("To-Hit   {n}+   {dmg}"),
        Style::default().fg(theme().warning),
    )));
    if matches!(shot.kind, BfAttackKind::RearWeapons) {
        // §1.6: same-turn REAR + forward fire reduces the forward damage 1-for-1 per point of
        // REAR damage dealt, applied BEFORE overheat (p.152) — the composed forward line.
        let rear =
            battleforce::bf_rear_damage(&el, shot.range, tm.bf.weapon, tm.bf.engine).unwrap_or(0.0);
        let fwd = app
            .session
            .bf_current_damage(i, shot.range)
            .map(|d| (d - rear).max(0.0))
            .filter(|d| *d > 0.0)
            .map(|d| {
                if s.ov > 0 && battleforce::bf_ov_applies(&el, shot.range) {
                    d + f32::from(s.ov)
                } else {
                    d
                }
            });
        lines.push(Line::from(Span::styled(
            format!(
                "fwd after REAR: {} (fwd fire −1 per REAR pt dealt, before OV — p.152)",
                bf_dmg_cell(fwd)
            ),
            Style::default().fg(theme().dim),
        )));
    }
    if !bf_kind_eligible(&el, s.kind) {
        lines.push(Line::from(Span::styled(
            "this element can't declare that kind — card shows Standard",
            Style::default().fg(theme().warning),
        )));
    }
    if matches!(s.kind, BfAttackKind::Physical(BfPhysical::Dfa))
        && matches!(
            s.target_kind,
            BfTargetKind::AirborneAero(_) | BfTargetKind::AirborneDropship
        )
    {
        lines.push(Line::from(Span::styled(
            "DFA may not target airborne aerospace (p.45) — card shows Standard",
            Style::default().fg(theme().warning),
        )));
    }
    let target_airborne = matches!(
        s.target_kind,
        BfTargetKind::AirborneAero(_)
            | BfTargetKind::AirborneDropship
            | BfTargetKind::AirborneVtolWige
    );
    if el.has_sua("FLK") && target_airborne {
        // The −2 folds in only for ground-to-air Standard weapon attacks (p.86 fn6; never on
        // REAR, p.152) — mirror bf_to_hit's derived gate: a non-aero attacker is always
        // ground-based, an aero attacker only when grounded (p.46).
        let folded = matches!(shot.kind, BfAttackKind::Standard) && (!aero || shot.grounded);
        lines.push(Line::from(Span::styled(
            if folded {
                "FLK: −2 vs airborne folded in; a miss by ≤2 still deals the FLK damage (p.148)"
            } else {
                "FLK: −2 not folded in — ground-to-air Standard attacks only (p.86; never REAR, p.152)"
            },
            Style::default().fg(theme().dim),
        )));
    }
    if el.has_sua("HT") {
        lines.push(Line::from(Span::styled(
            "HT: vs a no-heat-scale target the heat converts to damage (p.148)",
            Style::default().fg(theme().dim),
        )));
    }
    lines.push(Line::from(Span::styled(
        "[↑↓] select   [←→/space] adjust   [Esc] close",
        Style::default().fg(theme().dim),
    )));
    lines
}

/// Body lines for the BF grouping editor: every pool element with its current Unit assignment,
/// plus the move/split/skill verbs (the SbfGroup body one tier flatter).
fn bf_group_modal_lines(app: &App, sel: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for (i, tm) in app.session.mechs.iter().enumerate() {
        let selected = i == sel;
        let marker = if selected { "▶ " } else { "  " };
        let name_style = if selected {
            Style::default()
                .fg(theme().on_accent)
                .bg(theme().accent)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        let name = trunc(&tm.spec.display_name(), 30);
        let (tag, tag_style) = match app.session.bf_element_assignment(i) {
            Some(ui) => (
                trunc(&app.session.bf.units[ui].name, 26),
                Style::default().fg(theme().dim),
            ),
            None => ("— unassigned".into(), Style::default().fg(theme().warning)),
        };
        lines.push(Line::from(vec![
            Span::styled(
                format!("{marker}{:>2} {name:<30} SK{:<2} ", i + 1, tm.gunnery),
                name_style,
            ),
            Span::styled(tag, tag_style),
        ]));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "[↑↓] element   [←→] move between units   [n] split to new unit",
        Style::default().fg(theme().dim),
    )));
    lines.push(Line::from(Span::styled(
        "[u] unassign   [s/S] skill ±   [x] remove   [a] auto-group…   [Esc] done",
        Style::default().fg(theme().dim),
    )));
    lines
}

/// Body lines for the BF doctrine picker: ground Units per doctrine, aero pairs of 2 (spec §1.7).
fn bf_doctrine_modal_lines(sel: usize) -> Vec<Line<'static>> {
    let rows = [
        ("Inner Sphere", "Lances of 4 · Air Lances of 2"),
        ("Clan", "Stars of 5 · aero Points of 2"),
        ("ComStar", "Level IIs of 6 · Air Lances of 2"),
    ];
    let mut lines = Vec::new();
    for (i, (name, desc)) in rows.into_iter().enumerate() {
        let selected = i == sel;
        let marker = if selected { "▶ " } else { "  " };
        let style = if selected {
            Style::default()
                .fg(theme().on_accent)
                .bg(theme().accent)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        lines.push(Line::from(vec![
            Span::styled(format!("{marker}{name:<14}"), style),
            Span::styled(desc, Style::default().fg(theme().dim)),
        ]));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "rebuilds ALL units — custom names, morale rungs and notes are lost",
        Style::default().fg(theme().warning),
    )));
    lines.push(Line::from(Span::styled(
        "element damage/heat/crits stay on the cards · z undoes · aero never mixes with ground",
        Style::default().fg(theme().dim),
    )));
    lines.push(Line::from(Span::styled(
        "[↑↓] select   [Enter] apply   [Esc] back",
        Style::default().fg(theme().dim),
    )));
    lines
}

/// The full keybinding reference for the Standard BF `?` modal (spec §3.3 — the three-place-sync
/// source of truth alongside the footer and the cheatsheet).
fn bf_help_modal_lines() -> Vec<Line<'static>> {
    let header = |s: &'static str| {
        Line::from(Span::styled(
            s,
            Style::default()
                .fg(theme().accent)
                .add_modifier(Modifier::BOLD),
        ))
    };
    let row = |keys: &'static str, desc: &'static str| {
        Line::from(vec![
            Span::styled(format!("  {keys:<9}"), Style::default().fg(theme().accent)),
            Span::styled(desc, Style::default().fg(theme().dim)),
        ])
    };
    vec![
        header("Standard BattleForce"),
        row("Space", "1 damage (armor then structure)"),
        row("u", "repair 1"),
        row("o / i", "heat up / down (manual cooldown, p.49)"),
        row("c", "criticals (p.42 column; a = ARM spent)"),
        row("t", "to-hit shot (p.39 table + damage preview)"),
        row("g", "grouping editor (a = doctrine auto-group)"),
        row("m", "cycle Unit morale rung (manual)"),
        row("n", "new round (clears crew-stunned)"),
        row("r", "rename unit"),
        row("s", "edit element Skill  (g is grouping here)"),
        row("L", "log snapshot (game log)"),
        header("Selection"),
        row(", / .", "previous / next element  ([ / ] also)"),
        row("< / >", "jump 4 elements (one card row)"),
        header("General"),
        row("b", "set force point limit (PV)"),
        row("z", "undo"),
        row("a / D", "add / delete element"),
        row("P", "export record-sheet PDF"),
        row("S", "sessions browser"),
        row("^t", "display picker (theme + layout)"),
        row("q", "quit"),
        Line::from(Span::styled(
            "  press any key to close",
            Style::default().fg(theme().dim),
        )),
    ]
}

/// Condition glyph + color for a roster unit, in the session's game system: `✖` out of action,
/// `◐` damaged/stressed (lost a section, pilot hit, dangerous heat, or any AS damage), else `●` ok.
fn unit_condition(tm: &TrackedMech, mode: GameMode) -> (&'static str, Color) {
    // SBF tracks damage at unit (lance) scale, not per element — a pool element is always "ok".
    if mode == GameMode::StrategicBattleForce {
        return (icons::cond_ok(), theme().good);
    }
    let dead = match mode {
        GameMode::AlphaStrike => tm.as_destroyed(),
        // BF: structure gone, a kill-crit marked, or the 2-engine rule (spec §2.2).
        GameMode::BattleForce => tm.bf_destroyed(),
        _ => tm.destroyed_reason().is_some(),
    };
    if dead {
        return (icons::cond_destroyed(), theme().danger);
    }
    let damaged = match mode {
        GameMode::AlphaStrike => tm.as_armor_hits > 0 || tm.as_struct_hits > 0,
        // The AS shape plus the BF live-crit block (any marked crit is card damage).
        GameMode::BattleForce => {
            tm.as_armor_hits > 0 || tm.as_struct_hits > 0 || tm.bf != BfLive::default()
        }
        _ => {
            tm.pilot_hits > 0
                || tm.heat >= 14
                || tm.spec.locations().iter().any(|&l| tm.is_destroyed(l))
        }
    };
    if damaged {
        (icons::cond_damaged(), theme().warning)
    } else {
        (icons::cond_ok(), theme().good)
    }
}

/// §36 Modern force sidebar (play screens only): one row per roster unit — index, name, and a
/// condition glyph — with the active unit highlighted and a force point-total footer. A whole-lance
/// status view the terse top tabs can't give. Drawn by [`play_content`] when the profile + width
/// allow; the top roster tabs still render inside the (now narrower) main area.
fn draw_sidebar(f: &mut Frame, area: Rect, app: &App) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme().dim))
        .title(" Force ");
    let inner = block.inner(area);
    f.render_widget(block, area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let mode = app.session.mode;
    let active = app.session.active;
    let mechs = &app.session.mechs;
    let width = inner.width as usize;

    // List fills the inner area above a 1-line footer (force total).
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(inner);
    let list_h = rows[0].height as usize;
    // Window around the active unit so it stays visible when the roster is taller than the panel.
    let shown = mechs.len().min(list_h);
    let start = active
        .saturating_sub(list_h.saturating_sub(1))
        .min(mechs.len().saturating_sub(shown));

    let mut lines: Vec<Line> = Vec::new();
    for (i, m) in mechs.iter().enumerate().skip(start).take(list_h) {
        let (glyph, gcol) = unit_condition(m, mode);
        let name = if m.spec.model.is_empty() {
            m.spec.chassis.clone()
        } else {
            format!("{} {}", m.spec.chassis, m.spec.model)
        };
        // " NN name … " padded to leave the last 2 cols for the glyph.
        let prefix = format!(" {:>2} ", i + 1);
        let name_w = width.saturating_sub(prefix.chars().count() + 2).max(1);
        let body = format!(
            "{prefix}{:<width$}",
            truncate(&name, name_w),
            width = name_w
        );
        let row_style = if i == active {
            Style::default()
                .fg(theme().on_accent)
                .bg(theme().accent)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme().dim)
        };
        lines.push(Line::from(vec![
            Span::styled(body, row_style),
            Span::styled(format!("{glyph} "), Style::default().fg(gcol)),
        ]));
    }
    f.render_widget(Paragraph::new(lines), rows[0]);

    // Footer: force total in the session's own system (BV Classic/Override, PV AS/SBF/BF).
    let label = match mode {
        GameMode::AlphaStrike
        | GameMode::StrategicBattleForce
        | GameMode::BattleForce
        | GameMode::AbstractCombatSystem => "PV",
        GameMode::Classic | GameMode::Override => "BV",
    };
    let total = app.session.force_total();
    let unit_word = if mechs.len() == 1 { "unit" } else { "units" };
    let mut foot = vec![Span::styled(
        format!(
            " {} {unit_word} · {label} {}",
            mechs.len(),
            thousands(total)
        ),
        Style::default().fg(theme().dim),
    )];
    if let Some(limit) = app.session.limit {
        let col = if total > limit {
            theme().danger
        } else {
            theme().good
        };
        foot.push(Span::styled(
            format!("/{}", thousands(limit)),
            Style::default().fg(col),
        ));
    }
    // The game-log indicator the top tabs used to show (the tabs are dropped under the sidebar).
    if app.session.turn > 0 {
        foot.push(Span::styled(
            format!(" · log {}", app.session.turn),
            Style::default().fg(theme().dim),
        ));
    }
    f.render_widget(Paragraph::new(Line::from(foot)), rows[1]);
}

fn draw_roster(f: &mut Frame, area: Rect, app: &App) {
    if area.height == 0 {
        return; // collapsed: the Modern sidebar lists the force instead (see `play_content`).
    }
    let total = app.session.mechs.len();
    let active = app.session.active;
    let tab_style = |selected: bool| {
        if selected {
            Style::default()
                .fg(theme().on_accent)
                .bg(theme().accent)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme().dim)
        }
    };

    // Each tab renders as " i:chassis " followed by a separator space. When the same chassis
    // appears more than once, disambiguate with a trailing letter in roster order (Atlas A,
    // Atlas B, …) so identical tabs are tellable apart — like labelling minis on the table.
    let mechs = &app.session.mechs;
    let labels: Vec<String> = mechs
        .iter()
        .enumerate()
        .map(|(i, m)| {
            let chassis = &m.spec.chassis;
            let dup = mechs.iter().filter(|o| &o.spec.chassis == chassis).count() > 1;
            let suffix = if dup {
                // This unit's ordinal among same-chassis units; A..Z (roster caps at 12, so the
                // Z clamp is just a safety net and never actually hit).
                let ord = mechs[..i]
                    .iter()
                    .filter(|o| &o.spec.chassis == chassis)
                    .count();
                format!(" {}", (b'A' + ord.min(25) as u8) as char)
            } else {
                String::new()
            };
            format!(" {}:{}{} ", i + 1, chassis, suffix)
        })
        .collect();
    let tab_w = |i: usize| labels[i].chars().count() + 1; // label + trailing separator space

    // Game-log indicator: how many snapshots taken this session (only once logging has started).
    let log = (app.session.turn > 0).then(|| format!(" · log {}", app.session.turn));
    let log_w = log.as_ref().map_or(0, |s| s.chars().count());

    // Pick the contiguous window of tabs to show. If they all fit, show them all; otherwise
    // window around the active tab and mark how many are hidden on each side (e.g. `‹3 … 5›`
    // renders as a `‹3` prefix and a `5›` suffix bracketing the visible tabs).
    let budget = area.width as usize;
    let full_w = 1 + (0..total).map(tab_w).sum::<usize>() + log_w; // 1 = leading space
    let (lo, hi) = if total == 0 || full_w <= budget {
        (0, total)
    } else {
        // Reserve room for the leading space, the log indicator, and both overflow markers.
        const MARKER: usize = 5; // generous: e.g. "‹12 "
        let tab_budget = budget.saturating_sub(1 + log_w + 2 * MARKER);
        let mut lo = active;
        let mut hi = active + 1;
        let mut used = tab_w(active);
        loop {
            let mut grew = false;
            if hi < total && used + tab_w(hi) <= tab_budget {
                used += tab_w(hi);
                hi += 1;
                grew = true;
            }
            if lo > 0 && used + tab_w(lo - 1) <= tab_budget {
                lo -= 1;
                used += tab_w(lo);
                grew = true;
            }
            if !grew {
                break;
            }
        }
        (lo, hi)
    };

    let mut spans = vec![Span::raw(" ")];
    if lo > 0 {
        spans.push(Span::styled(
            format!("‹{lo} "),
            Style::default().fg(theme().dim),
        ));
    }
    for (i, label) in labels.iter().enumerate().take(hi).skip(lo) {
        spans.push(Span::styled(label.clone(), tab_style(i == active)));
        spans.push(Span::raw(" "));
    }
    if hi < total {
        spans.push(Span::styled(
            format!("{}›", total - hi),
            Style::default().fg(theme().dim),
        ));
    }
    if let Some(log) = log {
        spans.push(Span::styled(log, Style::default().fg(theme().dim)));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn draw_doll(f: &mut Frame, area: Rect, app: &App, tm: &TrackedMech) {
    // 'Mechs use a 3-row doll (torsos in the middle); vehicles a 4-row one (front / turret+sides /
    // body / rear). Each location is placed by its shared `grid_pos`.
    let vehicle = tm.spec.is_vehicle();
    let row_c: &[Constraint] = if vehicle {
        &[Constraint::Ratio(1, 4); 4]
    } else {
        &[
            Constraint::Percentage(30),
            Constraint::Percentage(40),
            Constraint::Percentage(30),
        ]
    };
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints(row_c)
        .split(area);
    let cols = |r: Rect| {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Ratio(1, 5); 5])
            .split(r)
    };
    let grid: Vec<_> = rows.iter().map(|r| cols(*r)).collect();

    let destroyed = tm.destroyed_reason();
    if let Some(reason) = destroyed {
        // Out of action — collapse the middle-row centre cells (cols 1-3) into a DESTROYED banner.
        let banner = Rect {
            x: grid[1][1].x,
            y: grid[1][1].y,
            width: (grid[1][3].x + grid[1][3].width).saturating_sub(grid[1][1].x),
            height: grid[1][1].height,
        };
        draw_destroyed_banner(f, banner, reason);
    }

    for loc in tm.spec.locations() {
        let (r, c) = super::app::grid_pos(loc);
        // Centre cells of the middle row sit behind the banner when destroyed.
        if destroyed.is_some() && r == 1 && (1..=3).contains(&c) {
            continue;
        }
        if let Some(cell) = grid.get(r as usize).and_then(|row| row.get(c as usize)) {
            draw_location_box(f, *cell, app, tm, loc);
        }
    }
}

fn draw_destroyed_banner(f: &mut Frame, area: Rect, reason: &str) {
    let red = Style::default()
        .fg(theme().danger)
        .add_modifier(Modifier::BOLD);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(red);
    // Vertically center the word + cause within the box's inner height.
    let inner_h = area.height.saturating_sub(2) as usize;
    let pad = inner_h.saturating_sub(2) / 2;
    let mut lines: Vec<Line> = vec![Line::from(""); pad];
    lines.push(Line::from(Span::styled(
        "*** DESTROYED ***",
        red.add_modifier(Modifier::REVERSED),
    )));
    lines.push(Line::from(Span::styled(format!("({reason})"), red)));
    f.render_widget(
        Paragraph::new(lines)
            .alignment(Alignment::Center)
            .block(block),
        area,
    );
}

fn draw_location_box(f: &mut Frame, area: Rect, app: &App, tm: &TrackedMech, loc: Location) {
    let focused = app.focus == Focus::Doll && app.cursor == loc;
    let a_rem = tm.armor_remaining(loc, Facing::Front);
    let a_max = tm.spec.armor.get(&loc).map(|a| a.armor_max).unwrap_or(0);
    let i_rem = tm.internal_remaining(loc);
    let i_max = tm.spec.armor.get(&loc).map(|a| a.internal_max).unwrap_or(0);
    let destroyed = tm.is_destroyed(loc);

    let border_style = if focused {
        Style::default()
            .fg(theme().accent)
            .add_modifier(Modifier::BOLD)
    } else if destroyed {
        Style::default().fg(theme().dim)
    } else {
        Style::default().fg(frac_color(a_rem + i_rem, a_max + i_max))
    };
    let mut title = loc.code().to_string();
    if focused && loc.has_rear() {
        title = format!(
            "{title} ▸{}",
            if app.facing == Facing::Rear { "R" } else { "F" }
        );
    }
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(if focused {
            BorderType::Double
        } else {
            BorderType::Plain
        })
        .border_style(border_style)
        .title(title);

    let inner_w = area.width.saturating_sub(2) as usize;
    let mut lines = Vec::new();
    let push_stat = |lines: &mut Vec<Line>, label: &str, rem: u16, max: u16, hi: bool| {
        let col = frac_color(rem, max);
        let mut s = Style::default().fg(col);
        if hi {
            s = s.add_modifier(Modifier::BOLD | Modifier::REVERSED);
        }
        lines.push(Line::from(Span::styled(format!("{label} {rem}/{max}"), s)));
        lines.push(Line::from(Span::styled(
            bar(rem, max, inner_w),
            Style::default().fg(col),
        )));
    };

    let front_hi = focused && app.facing == Facing::Front;
    // Show only the bars that exist: an infantry platoon / aero SI pool has no armor; an aero arc
    // has no internal of its own (it spills into the shared SI).
    if a_max > 0 {
        push_stat(&mut lines, "A", a_rem, a_max, front_hi);
    }
    if loc.has_rear() {
        let r_rem = tm.armor_remaining(loc, Facing::Rear);
        let r_max = tm.spec.armor.get(&loc).map(|a| a.rear_max).unwrap_or(0);
        let rear_hi = focused && app.facing == Facing::Rear;
        push_stat(&mut lines, "R", r_rem, r_max, rear_hi);
    }
    if i_max > 0 {
        // The platoon's "internal" is its troop strength.
        let i_label = if loc == Location::Platoon { "S" } else { "I" };
        push_stat(&mut lines, i_label, i_rem, i_max, false);
    }

    f.render_widget(Paragraph::new(lines).block(block), area);
}

fn draw_heat(f: &mut Frame, area: Rect, tm: &TrackedMech) {
    let e = tm.heat_effects();
    let heat = tm.heat;
    let heat_col = theme().heat_mech(heat);
    let inner_w = area.width.saturating_sub(2) as usize;
    let engine_heat = tm.engine_heat();
    let mut header = vec![
        Span::raw("Heat "),
        Span::styled(
            format!("{heat}"),
            Style::default().fg(heat_col).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(" / 30   diss {}", tm.dissipation()),
            Style::default().fg(theme().dim),
        ),
    ];
    if engine_heat > 0 {
        // Engine criticals add heat each end-turn; surface it next to dissipation.
        header.push(Span::styled(
            format!("   eng +{engine_heat}"),
            Style::default().fg(theme().danger),
        ));
    }
    let sink_loss = tm.sink_dissipation_lost();
    let mut sinks_line = vec![Span::styled(
        format!(
            "sinks {}× {}",
            tm.spec.heat_sinks,
            tm.spec.heat_sink_type.label()
        ),
        Style::default().fg(theme().dim),
    )];
    if sink_loss > 0 {
        // Destroyed heat-sink crit slots cut into end-turn dissipation.
        sinks_line.push(Span::styled(
            format!("  −{sink_loss} crit"),
            Style::default().fg(theme().danger),
        ));
    }
    let mut lines = vec![
        Line::from(header),
        Line::from(Span::styled(
            bar(heat.clamp(0, 30) as u16, 30, inner_w),
            Style::default().fg(heat_col),
        )),
        Line::from(sinks_line),
    ];
    let mut fx = Vec::new();
    if tm.spec.is_aerospace() {
        // Aero heat: no thrust loss — a control roll instead — plus a pilot-damage line.
        let a = tm.aero_heat_effects();
        if a.to_hit_penalty > 0 {
            fx.push(format!("+{} hit", a.to_hit_penalty));
        }
        if let Some(s) = a.control_avoid {
            fx.push(format!("control {s}+"));
        }
        if let Some(s) = a.shutdown_avoid {
            fx.push(format!("shutdn {s}+"));
        }
        if let Some(s) = a.ammo_explosion_avoid {
            fx.push(format!("ammo {s}+"));
        }
        if let Some(s) = a.pilot_damage_avoid {
            fx.push(format!("pilot {s}+"));
        }
        // Avionics crit adds to any control roll (separate from heat).
        let av = tm.aero_control_modifier();
        if av > 0 {
            fx.push(format!("avionics +{av} ctrl"));
        }
    } else {
        if e.movement_penalty > 0 {
            fx.push(format!("-{} MP", e.movement_penalty));
        }
        if e.to_hit_penalty > 0 {
            fx.push(format!("+{} hit", e.to_hit_penalty));
        }
        if let Some(s) = e.shutdown_avoid {
            fx.push(format!("shutdn {s}+"));
        }
        if let Some(s) = e.ammo_explosion_avoid {
            fx.push(format!("ammo {s}+"));
        }
    }
    if fx.is_empty() {
        lines.push(Line::from(Span::styled(
            "no penalties",
            Style::default().fg(theme().good),
        )));
    } else {
        // One line normally; wrap to two only when the effects overflow (aero at high heat has the
        // most: to-hit + control + shutdown + ammo + pilot).
        let yellow = Style::default().fg(theme().warning);
        let rows: Vec<String> = if fx.join("  ").chars().count() <= inner_w {
            vec![fx.join("  ")]
        } else {
            let mid = fx.len().div_ceil(2);
            vec![fx[..mid].join("  "), fx[mid..].join("  ")]
        };
        for r in rows {
            lines.push(Line::from(Span::styled(r, yellow)));
        }
    }
    if e.auto_shutdown || tm.shutdown {
        lines.push(Line::from(Span::styled(
            "** SHUTDOWN **",
            Style::default()
                .fg(theme().danger)
                .add_modifier(Modifier::BOLD | Modifier::REVERSED),
        )));
    }

    f.render_widget(
        Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title(" HEAT ")),
        area,
    );
}

fn draw_move(f: &mut Frame, area: Rect, tm: &TrackedMech) {
    let mv = tm.movement();
    let vehicle = tm.spec.is_vehicle();
    let infantry = tm.spec.is_infantry();
    let aero = tm.spec.is_aerospace();
    let label = |s: &'static str| Span::styled(s, Style::default().fg(theme().dim));
    // Vehicles move at Cruise/Flank; aerospace at Safe/Max thrust; 'Mechs walk/run/jump.
    let (l1, l2, l3) = if vehicle {
        (" Cruise ", "  Flank ", "  ")
    } else if aero {
        (" Thrust safe ", "  max ", "  ")
    } else if infantry {
        // Infantry have one ground speed (no Walk/Run) — show Move + Jump.
        (" Move ", "", "  Jump ")
    } else {
        (" Walk ", "  Run ", "  Jump ")
    };

    // Line 1: effective Walk / Run / Jump (or an immobile banner), with the reason for any cut.
    let line1 = if mv.immobile {
        Line::from(vec![
            Span::styled(
                " ** IMMOBILE **",
                Style::default()
                    .fg(theme().danger)
                    .add_modifier(Modifier::BOLD | Modifier::REVERSED),
            ),
            Span::styled(
                format!("  {}", mv.note.unwrap_or("can't move")),
                Style::default().fg(theme().danger),
            ),
        ])
    } else {
        // Green at full; yellow once anything is reduced below the sheet value.
        let reduced = mv.walk < tm.spec.walk || mv.run < tm.spec.run;
        let mp = if reduced {
            theme().warning
        } else {
            theme().good
        };
        let num = |n: u8, lit: bool| {
            Span::styled(
                format!("{n}"),
                Style::default()
                    .fg(if lit { mp } else { theme().dim })
                    .add_modifier(Modifier::BOLD),
            )
        };
        // Infantry have one ground speed (run == walk) — show Walk + Jump only.
        let mut spans = vec![label(l1), num(mv.walk, true)];
        if !infantry {
            spans.push(label(l2));
            spans.push(num(mv.run, true));
        }
        if (!vehicle && !aero) || mv.jump > 0 {
            spans.push(label(l3));
            spans.push(num(mv.jump, mv.jump > 0));
        }
        if let Some(note) = mv.note {
            spans.push(Span::styled(
                format!("  {note}"),
                Style::default().fg(theme().danger),
            ));
        } else if let Some(boost) = tm.mp_boost_label() {
            // Run boosted by an engaged MASC / Supercharger.
            spans.push(Span::styled(
                format!("  {boost}↑"),
                Style::default()
                    .fg(theme().accent)
                    .add_modifier(Modifier::BOLD),
            ));
        } else if reduced {
            spans.push(Span::styled(
                if vehicle { "  reduced" } else { "  heat" },
                Style::default().fg(theme().warning),
            ));
        }
        Line::from(spans)
    };

    // Line 2: aerospace shows its velocity + altitude + TMM (set with `v`, persists); ground units
    // show this turn's movement mode/hexes + to-hit consequences.
    let mut line2 = Vec::new();
    if aero {
        line2.push(Span::styled(
            format!(" Vel {}  Alt {}", tm.velocity, tm.altitude),
            Style::default()
                .fg(theme().accent)
                .add_modifier(Modifier::BOLD),
        ));
        line2.push(Span::styled("   [v]", Style::default().fg(theme().dim)));
    } else if tm.move_mode == MoveMode::Stationary && tm.hexes_moved == 0 {
        let tag = if mv.immobile { "  TMM -4" } else { "" };
        line2.push(Span::styled(
            format!(" stationary{tag}   [v]"),
            Style::default().fg(theme().dim),
        ));
    } else {
        line2.push(Span::styled(
            format!(
                " {} {}",
                tm.move_mode.label(vehicle, infantry),
                tm.hexes_moved
            ),
            Style::default()
                .fg(theme().accent)
                .add_modifier(Modifier::BOLD),
        ));
        line2.push(Span::styled(
            format!("  atk {:+}  TMM {:+}", tm.attack_move_modifier(), tm.tmm()),
            Style::default().fg(theme().accent),
        ));
    }

    // Line 3: vehicles show motive status; infantry their deployment state; 'Mechs show
    // prone + any PSR owed.
    let due = tm.psr_due();
    let target = tm.psr_target();
    let mut line3 = Vec::new();
    if infantry {
        let (txt, col) = if mv.immobile {
            ("** WIPED OUT **", theme().danger)
        } else {
            ("deployed", theme().dim)
        };
        line3.push(Span::styled(format!(" {txt}"), Style::default().fg(col)));
    } else if vehicle {
        let note = match mv.note {
            Some("immobilized") => "** IMMOBILIZED **",
            Some("motive") => "motive damaged",
            Some(n) => n,
            None => "mobile",
        };
        let col = if mv.immobile {
            theme().danger
        } else if mv.note.is_some() {
            theme().warning
        } else {
            theme().dim
        };
        line3.push(Span::styled(format!(" {note}"), Style::default().fg(col)));
    } else if aero {
        // Aerospace: no prone/PSR — surface marked critical-damage results with their hit counts
        // (set with `c`); the effects show on this MOVE/HEAT panel and EQUIP to-hit.
        let marked: Vec<String> = tm
            .unit_crits()
            .iter()
            .enumerate()
            .filter_map(|(i, n)| {
                let h = tm.crit_hits_at(i);
                (h > 0).then(|| {
                    if h > 1 {
                        format!("{n}×{h}")
                    } else {
                        (*n).to_string()
                    }
                })
            })
            .collect();
        if marked.is_empty() {
            line3.push(Span::styled(
                " no crits   [c]",
                Style::default().fg(theme().dim),
            ));
        } else {
            line3.push(Span::styled(
                format!(" crits: {}", marked.join(" · ")),
                Style::default()
                    .fg(theme().danger)
                    .add_modifier(Modifier::BOLD),
            ));
        }
    } else if tm.prone {
        line3.push(Span::styled(
            " ** PRONE **",
            Style::default()
                .fg(theme().warning)
                .add_modifier(Modifier::BOLD | Modifier::REVERSED),
        ));
        line3.push(Span::styled(
            format!("  stand: PSR {target}+"),
            Style::default().fg(theme().warning),
        ));
    } else if tm.auto_fall().is_some() {
        // A destroyed leg (shown as "leg gone" on the line above) is an automatic fall — no roll
        // to stay up. The PSR shown is the pilot-damage check from the fall (a stand-up PSR
        // follows once prone).
        line3.push(Span::styled(
            " ⚠ auto-fall",
            Style::default()
                .fg(theme().danger)
                .add_modifier(Modifier::BOLD),
        ));
        line3.push(Span::styled(
            format!("  PSR {target}+ pilot dmg"),
            Style::default().fg(theme().warning),
        ));
    } else if !due.is_empty() {
        line3.push(Span::styled(
            format!(" ⚠ PSR {target}+: {}", due.join(", ")),
            Style::default()
                .fg(theme().warning)
                .add_modifier(Modifier::BOLD),
        ));
    } else {
        line3.push(Span::styled(" standing", Style::default().fg(theme().dim)));
        if tm.psr_modifier() > 0 {
            line3.push(Span::styled(
                format!("   PSR {target}+"),
                Style::default().fg(theme().dim),
            ));
        }
    }

    f.render_widget(
        Paragraph::new(vec![line1, Line::from(line2), Line::from(line3)])
            .block(Block::default().borders(Borders::ALL).title(" MOVE ")),
        area,
    );
}

/// Infantry troop-strength panel (replaces HEAT): headcount remaining + a strength bar.
fn draw_troops(f: &mut Frame, area: Rect, tm: &TrackedMech) {
    let inner_w = area.width.saturating_sub(2) as usize;
    let ba = tm.spec.unit_type == UnitType::BattleArmor;
    // BA strength = troopers standing / squad size; CI = platoon strength points.
    let (rem, max) = if ba {
        let total = Location::TROOPERS
            .iter()
            .filter(|l| tm.spec.armor.contains_key(l))
            .count() as u16;
        (tm.troopers_remaining(), total)
    } else {
        let total = tm
            .spec
            .armor
            .get(&Location::Platoon)
            .map_or(0, |a| a.internal_max);
        (tm.troopers_remaining(), total)
    };
    let col = frac_color(rem, max);
    let label = if ba { "Troopers" } else { "Strength" };
    let mut lines = vec![
        Line::from(vec![
            Span::styled(format!("{label} "), Style::default().fg(theme().dim)),
            Span::styled(
                format!("{rem}/{max}"),
                Style::default().fg(col).add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(Span::styled(
            bar(rem, max, inner_w),
            Style::default().fg(col),
        )),
    ];
    if rem == 0 {
        lines.push(Line::from(Span::styled(
            "** WIPED OUT **",
            Style::default()
                .fg(theme().danger)
                .add_modifier(Modifier::BOLD | Modifier::REVERSED),
        )));
    } else if ba {
        let suits = tm.suits();
        // Per-trooper armor summary, e.g. "T1 6  T2 4  T3 —  T4 6"; the firing suit is highlighted.
        let mut spans = Vec::new();
        for (i, &l) in suits.iter().enumerate() {
            let txt = if tm.is_destroyed(l) {
                format!("{} —  ", l.code())
            } else {
                format!("{} {}  ", l.code(), tm.armor_remaining(l, Facing::Front))
            };
            let mut st = if tm.is_destroyed(l) {
                Style::default().fg(theme().dim)
            } else {
                Style::default().fg(theme().good)
            };
            if i == tm.active_suit {
                st = st.add_modifier(Modifier::REVERSED | Modifier::BOLD);
            }
            spans.push(Span::styled(txt, st));
        }
        lines.push(Line::from(spans));
        // Per-suit ammo for each bin, e.g. "SRM 2  T1 2  T2 1  T3 —  T4 2" (firing suit
        // highlighted). Only shown when the squad actually carries ammo.
        for b in &tm.spec.ammo {
            // Drop the " Ammo" suffix so the per-suit counts aren't crowded (e.g. "SRM 2").
            let label = b.name.strip_suffix(" Ammo").unwrap_or(&b.name);
            let mut spans = vec![Span::styled(
                format!("{:<8}", truncate(label, 7)),
                Style::default().fg(theme().dim),
            )];
            for (i, &l) in suits.iter().enumerate() {
                let dead = tm.is_destroyed(l);
                let txt = if dead {
                    "—  ".to_string()
                } else {
                    format!("{}  ", tm.suit_ammo_remaining(b.id, i))
                };
                let mut st = if dead {
                    Style::default().fg(theme().dim)
                } else if tm.suit_ammo_remaining(b.id, i) == 0 {
                    Style::default().fg(theme().danger)
                } else {
                    Style::default().fg(theme().warning)
                };
                if i == tm.active_suit {
                    st = st.add_modifier(Modifier::REVERSED | Modifier::BOLD);
                }
                spans.push(Span::styled(txt, st));
            }
            lines.push(Line::from(spans));
        }
        if !tm.spec.ammo.is_empty() {
            lines.push(Line::from(Span::styled(
                "[move doll cursor to pick firing suit]",
                Style::default().fg(theme().dim),
            )));
        }
    } else if tm.spec.dpt > 0 {
        // Conventional infantry: one combined platoon damage value (Mekbay's `dpt`), scaled by
        // surviving troopers — far more meaningful than the per-trooper weapon figures.
        lines.push(Line::from(vec![
            Span::styled("Damage ≈ ", Style::default().fg(theme().dim)),
            Span::styled(
                format!("{}", tm.infantry_damage()),
                Style::default()
                    .fg(theme().warning)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("  (full {})", tm.spec.dpt),
                Style::default().fg(theme().dim),
            ),
        ]));
    }
    let title = if ba { " SQUAD " } else { " PLATOON " };
    f.render_widget(
        Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title(title)),
        area,
    );
}

/// Infantry skills panel (replaces PILOT): gunnery + anti-'Mech.
/// A `BV 3187` span: the unit's skill-adjusted Battle Value (the cost it adds to a Classic force),
/// shown on the skill panels so the effect of a skill change is visible. Empty for 0-BV specs.
fn adj_bv_span(tm: &TrackedMech) -> Span<'static> {
    let bv = skill_adjusted_bv(tm.spec.bv, tm.gunnery, tm.piloting);
    if bv == 0 {
        Span::raw("")
    } else {
        Span::styled(format!("   BV {bv}"), Style::default().fg(theme().dim))
    }
}

fn draw_infantry_skills(f: &mut Frame, area: Rect, tm: &TrackedMech) {
    let cyan = Style::default()
        .fg(theme().accent)
        .add_modifier(Modifier::BOLD);
    let skills = Line::from(vec![
        Span::styled(" Gunnery ", Style::default().fg(theme().dim)),
        Span::styled(format!("{}+", tm.gunnery), cyan),
        Span::styled("   Anti-Mech ", Style::default().fg(theme().dim)),
        Span::styled(format!("{}+", tm.piloting), cyan),
        adj_bv_span(tm),
    ]);
    f.render_widget(
        Paragraph::new(vec![skills])
            .block(Block::default().borders(Borders::ALL).title(" SKILLS ")),
        area,
    );
}

/// Vehicle crew panel (replaces PILOT): gunnery/driving skills + the crew-hit track.
fn draw_crew(f: &mut Frame, area: Rect, tm: &TrackedMech) {
    let cyan = Style::default()
        .fg(theme().accent)
        .add_modifier(Modifier::BOLD);
    let skills = Line::from(vec![
        Span::styled(" Gunnery ", Style::default().fg(theme().dim)),
        Span::styled(format!("{}+", tm.gunnery), cyan),
        Span::styled("   Driving ", Style::default().fg(theme().dim)),
        Span::styled(format!("{}+", tm.piloting), cyan),
        adj_bv_span(tm),
    ]);
    let hits = tm.crew_hits.min(CREW_MAX);
    let mut crew = vec![
        Span::raw(" "),
        Span::styled(
            "█".repeat(hits as usize),
            Style::default().fg(theme().danger),
        ),
        Span::styled(
            "░".repeat((CREW_MAX - hits) as usize),
            Style::default().fg(theme().dim),
        ),
        Span::raw("   "),
    ];
    if hits >= CREW_MAX {
        crew.push(Span::styled(
            "** CREW OUT **",
            Style::default()
                .fg(theme().danger)
                .add_modifier(Modifier::BOLD | Modifier::REVERSED),
        ));
    } else if hits > 0 {
        crew.push(Span::styled(
            format!("crew hit (+{hits} to-hit)"),
            Style::default().fg(theme().warning),
        ));
    } else {
        crew.push(Span::styled("crew ok", Style::default().fg(theme().good)));
    }
    f.render_widget(
        Paragraph::new(vec![skills, Line::from(crew)])
            .block(Block::default().borders(Borders::ALL).title(" CREW ")),
        area,
    );
}

/// Vehicle critical-hits panel (replaces HEAT): motive track + the marked crit results.
fn draw_vehicle_crits(f: &mut Frame, area: Rect, tm: &TrackedMech) {
    let mut lines = Vec::new();
    // Motive system: Cruise MP loss + steering penalty from the Motive System Damage table.
    let immobilized = tm.motive_immobilized();
    let lost = tm.motive_mp_lost();
    let (mcol, status) = if immobilized {
        (theme().danger, "  ** IMMOBILIZED **".to_string())
    } else if lost > 0 {
        (
            theme().warning,
            format!("  −{lost} MP  steer +{}", tm.motive_steering()),
        )
    } else {
        (theme().good, "  ok".to_string())
    };
    lines.push(Line::from(vec![
        Span::styled(" Motive ", Style::default().fg(theme().dim)),
        Span::styled(
            status,
            Style::default().fg(mcol).add_modifier(Modifier::BOLD),
        ),
    ]));
    // Crit results — marked ones in red, the rest dim.
    let mut spans = vec![Span::raw(" ")];
    for (i, name) in VEHICLE_CRITS.iter().enumerate() {
        let hit = tm.is_vehicle_crit(name);
        let style = if hit {
            Style::default()
                .fg(theme().danger)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme().dim)
        };
        spans.push(Span::styled(format!("{name} "), style));
        if i == 3 {
            // wrap onto a second line after four results
            lines.push(Line::from(std::mem::take(&mut spans)));
            spans.push(Span::raw(" "));
        }
    }
    lines.push(Line::from(spans));
    // Transport / storage bays carried (Infantry Compartment, Cargo, …).
    if !tm.spec.transport.is_empty() {
        lines.push(Line::from(vec![
            Span::styled(" carries ", Style::default().fg(theme().dim)),
            Span::styled(
                tm.spec.transport.join(", "),
                Style::default().fg(theme().accent),
            ),
        ]));
    }
    lines.push(Line::from(Span::styled(
        " [c] crits   [m] motive   [p/P] crew",
        Style::default().fg(theme().dim),
    )));
    f.render_widget(
        Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title(" CRITS ")),
        area,
    );
}

fn draw_pilot(f: &mut Frame, area: Rect, tm: &TrackedMech) {
    let hits = tm.pilot_hits.min(PILOT_MAX);
    // Line 1: the pilot's skills (edit with `g`).
    let skills = Line::from(vec![
        Span::styled(" Gunnery ", Style::default().fg(theme().dim)),
        Span::styled(
            format!("{}+", tm.gunnery),
            Style::default()
                .fg(theme().accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("   Piloting ", Style::default().fg(theme().dim)),
        Span::styled(
            format!("{}+", tm.piloting),
            Style::default()
                .fg(theme().accent)
                .add_modifier(Modifier::BOLD),
        ),
        adj_bv_span(tm),
    ]);
    // Line 2: the 6-box damage track + consciousness status.
    let mut spans = vec![
        Span::raw(" "),
        Span::styled(
            "█".repeat(hits as usize),
            Style::default().fg(theme().danger),
        ),
        Span::styled(
            "░".repeat((PILOT_MAX - hits) as usize),
            Style::default().fg(theme().dim),
        ),
        Span::raw("   "),
    ];
    if tm.pilot_dead() {
        spans.push(Span::styled(
            "** PILOT KIA **",
            Style::default()
                .fg(theme().danger)
                .add_modifier(Modifier::BOLD | Modifier::REVERSED),
        ));
    } else if tm.pilot_unconscious {
        spans.push(Span::styled(
            "** UNCONSCIOUS **",
            Style::default()
                .fg(theme().warning)
                .add_modifier(Modifier::BOLD | Modifier::REVERSED),
        ));
        if let Some(n) = tm.consciousness_avoid() {
            spans.push(Span::styled(
                format!("  wake {n}+"),
                Style::default().fg(theme().dim),
            ));
        }
    } else if let Some(n) = tm.consciousness_avoid() {
        // 2d6 target to stay conscious — climbs as the pilot is hurt.
        spans.push(Span::styled(
            format!("conscious {n}+"),
            Style::default().fg(theme().warning),
        ));
    } else {
        spans.push(Span::styled("unhurt", Style::default().fg(theme().good)));
    }
    f.render_widget(
        Paragraph::new(vec![skills, Line::from(spans)])
            .block(Block::default().borders(Borders::ALL).title(" PILOT ")),
        area,
    );
}

/// For a conventional-infantry weapon whose `range` is a single range class (1–7), return the hex
/// span (`"0-3"`) and the per-hex to-hit brackets (`"0:-2  1:0  2:+2  3:+4"`) from the Conventional
/// Infantry Range Modifier Table. `None` when the range isn't a single class (BA / support weapons
/// carry standard `S/M/L` brackets instead and are shown verbatim).
fn infantry_range_display(range: &str) -> Option<(String, String)> {
    let class: u8 = range.trim().parse().ok()?;
    let max = infantry_max_range(class);
    let fmt_mod = |m: i32| {
        if m == 0 {
            "0".to_string()
        } else {
            format!("{m:+}")
        }
    };
    // The to-hit modifier at each hex 0..=max, in order (reads as the table row, e.g. "-2/0/+2/+4").
    let mods: Vec<String> = (0..=max)
        .map(|d| fmt_mod(infantry_range_mod(class, d).unwrap_or(0)))
        .collect();
    Some((format!("0-{max}"), mods.join("/")))
}

/// A weapon's GATOR shot result against the active Classic target (§24), used by both the
/// weapon-row to-hit column and the detail-line breakdown.
enum WeaponTn {
    /// No target set (or the weapon has no bracketable short/medium/long range) — the row falls
    /// back to showing the bare equipment-derived to-hit modifier.
    NoTarget,
    /// The target is beyond this weapon's extreme range — it cannot fire.
    OutOfRange,
    /// The assembled target number, with the range bracket it falls in and that bracket's modifier.
    Hit {
        bracket: RangeBracket,
        tn: i32,
        range_mod: i32,
    },
}

/// Assemble one weapon's GATOR to-hit against the active target: gunnery + attacker movement +
/// target movement + range bracket (from the target distance) + equipment/heat/aero-crit "Other".
/// Minimum-range penalty is omitted (not yet baked; see `engine::gator`).
fn gator_weapon_tn(tm: &TrackedMech, w: &WeaponMount) -> WeaponTn {
    let Some(t) = tm.ct_target else {
        return WeaponTn::NoTarget;
    };
    let Some((s, m, l)) = parse_ranges(&w.range) else {
        return WeaponTn::NoTarget;
    };
    let Some(bracket) = range_bracket(s, m, l, t.distance) else {
        return WeaponTn::OutOfRange;
    };
    let tgt = target_modifier(t.hexes_moved, t.jumped, t.immobile);
    let other = tm.spec.weapon_to_hit(w)
        + tm.aero_weapon_to_hit()
        + tm.heat_effects().to_hit_penalty as i32;
    let tn = gator_to_hit(
        tm.gunnery,
        tm.attack_move_modifier(),
        tgt,
        bracket.modifier(),
        0,
        other,
    );
    WeaponTn::Hit {
        bracket,
        tn,
        range_mod: bracket.modifier(),
    }
}

fn draw_equip(f: &mut Frame, area: Rect, app: &App, tm: &TrackedMech) {
    let focused = app.focus == Focus::Equipment;
    let border_style = if focused {
        Style::default()
            .fg(theme().accent)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title(" WEAPONS / AMMO / EQUIP ");
    let inner = block.inner(area);

    let rows = app.equip_rows();
    // When focused, pin a detail line (selected item's full name + range / shots) to the bottom;
    // the list scrolls in the space above it. Keeps the rows themselves uncluttered.
    let detail = focused.then(|| rows.get(app.equip_sel).copied()).flatten();
    // Conventional-infantry weapons get a dedicated detail line for the range span + per-hex to-hit
    // table — it's too long to share the (often long) weapon-name line.
    let detail_extra: Option<String> = detail.and_then(|row| match row {
        EquipRow::Weapon(id) if tm.spec.unit_type == UnitType::Infantry => tm
            .spec
            .weapons
            .iter()
            .find(|w| w.id == id)
            .and_then(|w| infantry_range_display(&w.range))
            .map(|(span, mods)| format!("range {span}  ({mods})")),
        _ => None,
    });
    let detail_h: u16 = if detail.is_some() {
        2 + u16::from(detail_extra.is_some())
    } else {
        0
    };
    let list_h = inner.height.saturating_sub(detail_h);
    let visible = list_h as usize;
    let offset = if focused {
        app.equip_sel.saturating_sub(visible.saturating_sub(1))
    } else {
        0
    };

    let mut lines = Vec::new();
    for (i, row) in rows.iter().enumerate().skip(offset).take(visible) {
        let selected = focused && i == app.equip_sel;
        let marker = if selected { "▶" } else { " " };
        let line: Vec<Span> = match row {
            EquipRow::Weapon(id) => {
                let w = tm.spec.weapons.iter().find(|w| w.id == *id).unwrap();
                // 'Mechs show weapon heat here; conventional infantry repurpose the column for the
                // trooper count wielding this weapon (×N — the platoon's weapon mix). Battle Armor
                // (per-suit, count 1) and vehicles leave it blank.
                let heat = if tm.spec.unit_type == UnitType::Infantry {
                    format!("×{}", w.count)
                } else if tm.spec.is_vehicle() || tm.spec.is_infantry() {
                    String::new()
                } else if w.heat == 0 {
                    "—".to_string()
                } else {
                    format!("H{}", w.heat)
                };
                // Fired-this-turn weapons read green with a trailing ✓ (kept separate from the
                // cursor ▶ so it shows even on the selected row); a weapon whose crit slot is
                // destroyed reads red (still fireable — damage is simultaneous) and takes priority.
                let fired = tm.is_fired(*id);
                let jammed = tm.is_jammed(*id);
                // A destroyed weapon reads red and wins; a jammed UAC/RAC reads amber; then fired
                // green, else dim.
                let base = Style::default().fg(if tm.is_weapon_disabled(w) {
                    theme().danger
                } else if jammed {
                    theme().warning
                } else if fired {
                    theme().good
                } else {
                    theme().dim
                });
                // Heat in red and set apart from damage so the two columns read at a glance.
                let heat_style = if w.heat == 0 {
                    base
                } else {
                    Style::default().fg(theme().danger)
                };
                // With a GATOR target set (§24) this column shows the assembled per-weapon target
                // number (`8+`, gold) or `X` when the target is out of range; otherwise it falls
                // back to the bare equipment-derived to-hit modifier (pulse/heavy + targeting
                // computer), green for a bonus / red for a penalty.
                let th = match gator_weapon_tn(tm, w) {
                    WeaponTn::Hit { tn, .. } => Span::styled(
                        format!("  {tn}+"),
                        Style::default()
                            .fg(theme().warning)
                            .add_modifier(Modifier::BOLD),
                    ),
                    WeaponTn::OutOfRange => {
                        Span::styled("  X".to_string(), Style::default().fg(theme().dim))
                    }
                    WeaponTn::NoTarget => {
                        let to_hit = tm.spec.weapon_to_hit(w);
                        if to_hit != 0 {
                            Span::styled(
                                format!("  {to_hit:+}"),
                                Style::default().fg(if to_hit < 0 {
                                    theme().good
                                } else {
                                    theme().danger
                                }),
                            )
                        } else {
                            Span::raw("")
                        }
                    }
                };
                // Fired marker. Battle Armor fires per suit, so show how many of the living suits
                // have fired this weapon (✓N/M); other units show a plain ✓ (or shots/max for
                // Ultra/Rotary).
                let fired_mark = if tm.spec.unit_type == UnitType::BattleArmor {
                    let n = tm.living_suits_fired(*id);
                    if n > 0 {
                        Span::styled(
                            format!("  ✓{n}/{}", tm.troopers_remaining()),
                            Style::default()
                                .fg(theme().good)
                                .add_modifier(Modifier::BOLD),
                        )
                    } else {
                        Span::raw("")
                    }
                } else {
                    let shots = tm.shots_fired(*id);
                    if shots > 0 {
                        let max = w.max_shots();
                        let txt = if max > 1 {
                            format!("  ✓{shots}/{max}")
                        } else {
                            "  ✓".into()
                        };
                        Span::styled(
                            txt,
                            Style::default()
                                .fg(theme().good)
                                .add_modifier(Modifier::BOLD),
                        )
                    } else {
                        Span::raw("")
                    }
                };
                // Conventional infantry `damage` is *per trooper*; show the group's damage
                // (count × per-trooper, rounded) so the column reads as a real number, not "0.52".
                let dmg = if tm.spec.unit_type == UnitType::Infantry {
                    w.damage
                        .parse::<f32>()
                        .map(|per| ((per * f32::from(w.count)).round() as u32).to_string())
                        .unwrap_or_else(|_| w.damage.clone())
                } else {
                    w.damage.clone()
                };
                let mut spans = vec![
                    Span::styled(
                        format!(
                            "{marker}{:<15} {:<2}  ",
                            truncate(&w.name, 15),
                            w.location.code()
                        ),
                        base,
                    ),
                    Span::styled(format!("{heat:<4}"), heat_style),
                    Span::styled(format!("  {:<5}", dmg), base),
                ];
                // Conventional infantry weapons reach `class × 3` hexes (the baked range is the
                // range class, not a hex count) — show the full span compactly, e.g. "r0-3" for a
                // Machine Gun; the per-hex to-hit table is on the selected weapon's detail line.
                if tm.spec.unit_type == UnitType::Infantry && !w.range.is_empty() {
                    let span = infantry_range_display(&w.range)
                        .map_or_else(|| w.range.clone(), |(s, _)| s);
                    spans.push(Span::styled(
                        format!("r{span}"),
                        Style::default().fg(theme().dim),
                    ));
                }
                spans.push(th);
                spans.push(fired_mark);
                if jammed {
                    spans.push(Span::styled(
                        "  JAM",
                        Style::default()
                            .fg(theme().warning)
                            .add_modifier(Modifier::BOLD),
                    ));
                }
                spans
            }
            EquipRow::Ammo(id) => {
                let b = tm.spec.ammo.iter().find(|b| b.id == *id).unwrap();
                let rem = tm.ammo_remaining(*id);
                vec![Span::styled(
                    format!(
                        "{marker}{:<17}{}/{}",
                        truncate(&b.name, 17),
                        rem,
                        b.shots_max()
                    ),
                    Style::default().fg(frac_color(rem, b.shots_max())),
                )]
            }
            EquipRow::Equip(idx) => {
                let e = &tm.spec.equipment[*idx];
                // Stateful gear (MASC/Supercharger/ECM/Stealth) that's currently engaged reads accent
                // and carries an ON marker; a destroyed-by-crit item shows red, like a dead weapon.
                let toggle = tm.equip_toggle(e);
                let col = if tm.is_equipment_disabled(e) {
                    theme().danger
                } else if matches!(toggle, Some((_, true))) {
                    theme().accent
                } else {
                    theme().dim
                };
                let mut spans = vec![Span::styled(
                    format!(
                        "{marker}{:<18} {}",
                        truncate(&e.name, 18),
                        e.location.code()
                    ),
                    Style::default().fg(col),
                )];
                // Only annotate the *active* state (an engaged booster / live ECM-Stealth); an
                // off toggle reads as plain dim gear, like an unjammed weapon.
                if matches!(toggle, Some((_, true))) {
                    spans.push(Span::styled(
                        "  ● ON",
                        Style::default()
                            .fg(theme().accent)
                            .add_modifier(Modifier::BOLD),
                    ));
                }
                spans
            }
        };
        let style = if selected {
            Style::default().add_modifier(Modifier::REVERSED)
        } else {
            Style::default()
        };
        lines.push(Line::from(line).style(style));
    }
    if lines.is_empty() {
        lines.push(Line::from(Span::styled(
            "  (no weapons, ammo, or equipment)",
            Style::default().fg(theme().dim),
        )));
    }
    f.render_widget(block, area);
    let list_area = Rect {
        height: list_h,
        ..inner
    };
    f.render_widget(Paragraph::new(lines), list_area);

    // Scrollbar when the list is taller than the panel, so it's clear there's more off-screen.
    if rows.len() > visible && visible > 0 {
        // ratatui maps `position` over 0..content_length-1, where the max means the *last item is
        // at the top* of the viewport. Our `offset` instead stops with the last item at the
        // *bottom* (max offset = len - visible), so size content_length to that range — otherwise
        // the thumb only travels to ~halfway at the bottom of the list.
        let mut sb = ScrollbarState::new(rows.len() - visible + 1)
            .viewport_content_length(visible)
            .position(offset);
        f.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .thumb_style(Style::default().fg(theme().accent))
                .track_style(Style::default().fg(theme().dim)),
            // Sit on the panel's right border, spanning just the list rows (not the corners or
            // the detail line).
            Rect {
                x: area.x,
                y: inner.y,
                width: area.width,
                height: list_h,
            },
            &mut sb,
        );
    }

    if let Some(row) = detail {
        let (name, tail) = match row {
            EquipRow::Weapon(id) => {
                let w = tm.spec.weapons.iter().find(|w| w.id == id).unwrap();
                // Conventional infantry: expand the range class into the hex span + the per-hex
                // to-hit modifiers (Conventional Infantry Range Modifier Table).
                let inf_range = (tm.spec.unit_type == UnitType::Infantry)
                    .then(|| infantry_range_display(&w.range))
                    .flatten();
                let range = match &inf_range {
                    Some((span, mods)) => format!("{span}  ({mods})"),
                    None if w.range.is_empty() => "—".to_string(),
                    None => w.range.clone(),
                };
                let equip = tm.spec.weapon_to_hit(w);
                let moved = tm.attack_move_modifier();
                let crit = tm.aero_weapon_to_hit(); // aero sensor + FCS damage
                let move_label = tm
                    .move_mode
                    .label(tm.spec.is_vehicle(), tm.spec.is_infantry());
                let to_hit = match gator_weapon_tn(tm, w) {
                    // GATOR target set: spell out the full assembly (G·A·T·R·O) and the number.
                    WeaponTn::Hit {
                        bracket,
                        tn,
                        range_mod,
                    } => {
                        let tgt = tm
                            .ct_target
                            .map_or(0, |t| target_modifier(t.hexes_moved, t.jumped, t.immobile));
                        let heat = tm.heat_effects().to_hit_penalty as i32;
                        let mut tags = vec![format!("G{}", tm.gunnery)];
                        if moved != 0 {
                            tags.push(format!("{move_label} {moved:+}"));
                        }
                        if tgt != 0 {
                            tags.push(format!("tgt {tgt:+}"));
                        }
                        tags.push(format!("{} {range_mod:+}", bracket.code()));
                        if heat != 0 {
                            tags.push(format!("heat +{heat}"));
                        }
                        if equip != 0 {
                            let lbl = if tm.spec.has_targeting_computer() && w.tc_eligible {
                                "TC"
                            } else {
                                "eqp"
                            };
                            tags.push(format!("{lbl} {equip:+}"));
                        }
                        if crit != 0 {
                            tags.push(format!("crit {crit:+}"));
                        }
                        format!("to-hit {tn}+ ({})   ", tags.join(" "))
                    }
                    WeaponTn::OutOfRange => "to-hit — out of range   ".to_string(),
                    // No target: the bare equipment + movement modifier, as before. Compact for the
                    // narrow panel — the weapon name already shows pulse/heavy, so just tag TC/move.
                    WeaponTn::NoTarget => {
                        let total = equip + moved + crit;
                        if equip != 0 || moved != 0 || crit != 0 {
                            let mut tags = Vec::new();
                            if tm.spec.has_targeting_computer() && w.tc_eligible {
                                tags.push("TC".to_string());
                            }
                            if moved != 0 {
                                tags.push(move_label.to_string());
                            }
                            if crit != 0 {
                                tags.push("crit".to_string());
                            }
                            let tags = if tags.is_empty() {
                                String::new()
                            } else {
                                format!(" ({})", tags.join(", "))
                            };
                            format!("to-hit {total:+}{tags}   ")
                        } else {
                            String::new()
                        }
                    }
                };
                // Show which ammo bin this weapon will draw from, and its munition if non-standard.
                let ammo = tm.weapon_bin(id).map_or(String::new(), |bin| {
                    // Battle Armor fires from the active suit, so label the ammo with that suit;
                    // every other unit labels it with the bin's own location.
                    let code = if tm.spec.unit_type == UnitType::BattleArmor {
                        tm.suits().get(tm.active_suit).map_or("?", |l| l.code())
                    } else {
                        tm.spec
                            .ammo
                            .iter()
                            .find(|b| b.id == bin)
                            .map_or("?", |b| b.location.code())
                    };
                    let m = tm.bin_munition(bin);
                    let munition = if m.is_empty() || m == STANDARD_MUNITION {
                        String::new()
                    } else {
                        format!(" ({m})")
                    };
                    format!(
                        "   ammo {} {}/{}{}",
                        code,
                        tm.ammo_remaining(bin),
                        tm.ammo_max(bin),
                        munition
                    )
                });
                // When the range table is on its own line (infantry), keep it off the name line.
                let tail = if detail_extra.is_some() {
                    format!("{to_hit}{ammo}")
                } else {
                    format!("{to_hit}range {range}{ammo}")
                };
                (w.name.clone(), tail)
            }
            EquipRow::Ammo(id) => {
                let b = tm.spec.ammo.iter().find(|b| b.id == id).unwrap();
                (
                    b.name.clone(),
                    format!("{}/{} shots", tm.ammo_remaining(id), b.shots_max()),
                )
            }
            EquipRow::Equip(idx) => {
                let e = &tm.spec.equipment[idx];
                (e.name.clone(), format!("equipment · {}", e.location.code()))
            }
        };
        let detail_area = Rect {
            x: inner.x,
            y: inner.y + list_h,
            width: inner.width,
            height: detail_h,
        };
        let mut dlines = vec![
            Line::from(Span::styled(
                "─".repeat(inner.width as usize),
                Style::default().fg(theme().dim),
            )),
            Line::from(vec![
                Span::raw(" "),
                Span::styled(name, Style::default().fg(theme().dim)),
                Span::styled(format!("   {tail}"), Style::default().fg(theme().accent)),
            ]),
        ];
        if let Some(extra) = detail_extra {
            dlines.push(Line::from(vec![
                Span::raw(" "),
                Span::styled(extra, Style::default().fg(theme().accent)),
            ]));
        }
        f.render_widget(Paragraph::new(dlines), detail_area);
    }
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        let t: String = s.chars().take(n.saturating_sub(1)).collect();
        format!("{t}…")
    }
}

fn draw_status(f: &mut Frame, area: Rect, app: &App) {
    // Just the high-traffic keys; the full reference lives in the [?] help modal.
    let help = match app.focus {
        Focus::Doll => "Spc:dmg  u:rep  c:crit  p:pilot  f:face  Tab:panel  ?:help",
        Focus::Equipment => "Spc:fire  u:unfire  c:crit  p:pilot  Tab:panel  ?:help",
    };
    // Status message leads so it's never clipped by the (long) help text.
    let mut spans = vec![Span::raw(" ")];
    if !app.status.is_empty() {
        spans.push(Span::styled(
            format!("{} ", app.status),
            Style::default()
                .fg(theme().accent)
                .add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled("| ", Style::default().fg(theme().dim)));
    }
    spans.push(Span::styled(
        format!("{help} "),
        Style::default().fg(theme().dim),
    ));
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}
