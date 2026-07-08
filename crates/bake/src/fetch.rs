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

//! Tiny caching HTTP fetcher. Anything downloaded once is cached on disk so re-bakes are
//! cheap (and so the heavy ~4k-SVG pass only pays the network cost the first time).

use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const REMOTE_HOST: &str = "https://db.mekbay.com";

pub struct Fetcher {
    agent: ureq::Agent,
    cache_dir: PathBuf,
}

impl Fetcher {
    pub fn new(cache_dir: PathBuf) -> std::io::Result<Self> {
        std::fs::create_dir_all(cache_dir.join("sheets/mek"))?;
        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(60))
            .build();
        Ok(Fetcher { agent, cache_dir })
    }

    /// Fetch `<REMOTE_HOST>/<rel>` as bytes, caching at `<cache_dir>/<rel>`.
    pub fn get(&self, rel: &str) -> Result<Vec<u8>, String> {
        self.get_url(&format!("{REMOTE_HOST}/{rel}"), rel)
    }

    /// Fetch an absolute `url` as bytes, caching at `<cache_dir>/<cache_rel>`. Used for assets that
    /// live on a different host than [`REMOTE_HOST`] (e.g. the availability table on `mekbay.com`).
    pub fn get_url(&self, url: &str, cache_rel: &str) -> Result<Vec<u8>, String> {
        let cache_path = self.cache_dir.join(cache_rel);
        if let Ok(bytes) = std::fs::read(&cache_path) {
            if !bytes.is_empty() {
                return Ok(bytes);
            }
        }
        let resp = self.call_with_retry(url)?;
        let mut bytes = Vec::new();
        resp.into_reader()
            .read_to_end(&mut bytes)
            .map_err(|e| format!("read {url}: {e}"))?;
        if let Some(parent) = cache_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        write_atomic(&cache_path, &bytes).map_err(|e| format!("cache write {cache_rel}: {e}"))?;
        Ok(bytes)
    }

    /// GET with exponential backoff on rate-limiting (429) and transient/5xx errors.
    fn call_with_retry(&self, url: &str) -> Result<ureq::Response, String> {
        let mut delay = Duration::from_millis(750);
        let mut last = String::new();
        for attempt in 0..7 {
            if attempt > 0 {
                std::thread::sleep(delay);
                delay = (delay * 2).min(Duration::from_secs(30));
            }
            match self.agent.get(url).call() {
                Ok(resp) => return Ok(resp),
                Err(ureq::Error::Status(code, _)) if code == 429 || code >= 500 => {
                    last = format!("status code {code}");
                }
                Err(ureq::Error::Transport(t)) => last = t.to_string(),
                Err(e) => return Err(format!("GET {url}: {e}")),
            }
        }
        Err(format!("GET {url}: {last} (gave up after retries)"))
    }

    pub fn get_text(&self, rel: &str) -> Result<String, String> {
        let bytes = self.get(rel)?;
        String::from_utf8(bytes).map_err(|e| format!("utf8 {rel}: {e}"))
    }

    /// Text variant of [`Self::get_url`].
    pub fn get_text_url(&self, url: &str, cache_rel: &str) -> Result<String, String> {
        let bytes = self.get_url(url, cache_rel)?;
        String::from_utf8(bytes).map_err(|e| format!("utf8 {cache_rel}: {e}"))
    }
}

fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}
