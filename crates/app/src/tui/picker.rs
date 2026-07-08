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

//! Fuzzy unit picker state, backed by nucleo-matcher, composed with faceted [`Filters`].

use super::filters::{self, Filters};
use neurohelmet_core::data::bundle::Bundle;
use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};

pub struct Picker {
    pub query: String,
    pub selected: usize,
    /// Indices into the bundle's mech list, best match first.
    pub filtered: Vec<usize>,
    /// Visible row count of the list, updated each render — the step for PageUp/PageDown.
    pub page: usize,
    matcher: Matcher,
}

impl Picker {
    pub fn new(total: usize) -> Self {
        Picker {
            query: String::new(),
            selected: 0,
            filtered: (0..total).collect(),
            page: 10,
            matcher: Matcher::new(Config::DEFAULT),
        }
    }

    /// Recompute the filtered list against `names` (parallel to `bundle.mechs`), keeping only units
    /// that pass `filters`, then fuzzy-ranking the survivors by the query. With an empty query the
    /// result is the filter-passing units in bundle order.
    pub fn refilter(&mut self, names: &[String], bundle: &Bundle, filters: &Filters) {
        self.filtered.clear();
        let passes = |i: usize| filters::matches(filters, &bundle.mechs[i]);
        if self.query.is_empty() {
            self.filtered.extend((0..names.len()).filter(|&i| passes(i)));
        } else {
            let pat = Pattern::parse(&self.query, CaseMatching::Ignore, Normalization::Smart);
            let mut buf = Vec::new();
            let mut scored: Vec<(usize, u32)> = Vec::new();
            for (i, name) in names.iter().enumerate() {
                if !passes(i) {
                    continue;
                }
                let h = Utf32Str::new(name, &mut buf);
                if let Some(s) = pat.score(h, &mut self.matcher) {
                    scored.push((i, s));
                }
            }
            scored.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
            self.filtered = scored.into_iter().map(|(i, _)| i).collect();
        }
        // §35: with the availability lens active and no query driving the order, sort most-available
        // first (units in the same rarity tier keep their alphabetical bundle order — stable sort).
        if self.query.is_empty() {
            if let Some((era, fac)) = filters.avail_context() {
                // Cached key: compute each unit's rarity once, not on every comparison (the catalog
                // is ~9k units, so an uncached key recomputes rarity O(n log n) times → visible lag).
                self.filtered
                    .sort_by_cached_key(|&i| std::cmp::Reverse(bundle.mechs[i].rarity(era, fac).rank()));
            }
        }
        if self.selected >= self.filtered.len() {
            self.selected = self.filtered.len().saturating_sub(1);
        }
    }

    pub fn move_selection(&mut self, delta: i32) {
        if self.filtered.is_empty() {
            return;
        }
        let n = self.filtered.len() as i32;
        self.selected = (((self.selected as i32 + delta) % n + n) % n) as usize;
    }

    /// Jump a whole page (PageUp/PageDown), clamped to the list ends rather than wrapping.
    pub fn page_jump(&mut self, dir: i32) {
        if self.filtered.is_empty() {
            return;
        }
        let last = self.filtered.len() as i32 - 1;
        let step = dir * self.page.max(1) as i32;
        self.selected = (self.selected as i32 + step).clamp(0, last) as usize;
    }

    /// The bundle index currently highlighted, if any.
    pub fn current(&self) -> Option<usize> {
        self.filtered.get(self.selected).copied()
    }

    pub fn reset(&mut self) {
        self.query.clear();
        self.selected = 0;
    }
}
