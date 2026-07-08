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

//! Crate-wide error type for the non-UI layers.

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("parse error in {file}: {msg}")]
    Parse { file: String, msg: String },

    #[error("unknown location code: {0}")]
    BadLocation(String),

    #[error("mech not found: {0}")]
    MechNotFound(String),

    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    #[error("json: {0}")]
    Json(#[from] serde_json::Error),

    #[error("bincode decode: {0}")]
    BincodeDecode(#[from] bincode::error::DecodeError),

    #[error("bincode encode: {0}")]
    BincodeEncode(#[from] bincode::error::EncodeError),
}

pub type Result<T> = std::result::Result<T, AppError>;
