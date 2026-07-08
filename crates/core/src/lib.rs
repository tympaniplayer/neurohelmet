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

//! neurohelmet-core: data model, BattleTech rules engine, session/persistence, and the
//! bundled-data loader. Deliberately free of any TUI/terminal dependency so every layer
//! is unit-testable without a terminal.

pub mod data;
pub mod domain;
pub mod engine;
pub mod error;
pub mod log;
pub mod session;

pub use error::{AppError, Result};
