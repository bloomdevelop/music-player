// SPDX-License-Identifier: MPL-2.0

use std::path::{PathBuf};
use std::fs;

/// A simple queue/playlist manager.
#[derive(Debug, Default, Clone)]
pub struct Queue {
    tracks: Vec<PathBuf>,
    index: usize,
}

impl Queue {
    pub fn new() -> Self {
        Self { tracks: Vec::new(), index: 0 }
    }

    pub fn from_vec(v: Vec<PathBuf>) -> Self {
        Self { tracks: v, index: 0 }
    }

    pub fn push(&mut self, path: PathBuf) {
        self.tracks.push(path);
    }

    pub fn pop(&mut self) -> Option<PathBuf> {
        self.tracks.pop()
    }

    pub fn next(&mut self) -> Option<&PathBuf> {
        if self.tracks.is_empty() {
            return None;
        }

        if self.index + 1 < self.tracks.len() {
            self.index += 1;
        } else {
            self.index = 0;
        }

        self.tracks.get(self.index)
    }

    pub fn prev(&mut self) -> Option<&PathBuf> {
        if self.tracks.is_empty() {
            return None;
        }

        if self.index > 0 {
            self.index -= 1;
        } else {
            self.index = self.tracks.len().saturating_sub(1);
        }

        self.tracks.get(self.index)
    }

    pub fn current(&self) -> Option<&PathBuf> {
        self.tracks.get(self.index)
    }

    pub fn len(&self) -> usize {
        self.tracks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tracks.is_empty()
    }

    pub fn clear(&mut self) {
        self.tracks.clear();
        self.index = 0;
    }

    /// Return the internal tracks slice for read-only iteration in the UI.
    pub fn tracks(&self) -> &[PathBuf] {
        &self.tracks
    }

    /// Ensure the given path is in the queue and set it as the current index.
    /// If the path already exists in the queue, moves the index to that item.
    /// If it does not exist, pushes it to the end and selects it.
    pub fn select_or_push(&mut self, path: PathBuf) {
        if let Some(pos) = self.tracks.iter().position(|p| p == &path) {
            self.index = pos;
        } else {
            self.tracks.push(path);
            if !self.tracks.is_empty() {
                self.index = self.tracks.len() - 1;
            } else {
                self.index = 0;
            }
        }
    }
}

/// Recursively scan a directory for common audio file extensions.
pub fn scan_music_dir(dir: impl Into<PathBuf>) -> Vec<PathBuf> {
    let dir = dir.into();
    let mut found = Vec::new();

    let exts = ["mp3", "flac", "wav", "ogg", "m4a"];

    fn visit(path: &PathBuf, exts: &[&str], out: &mut Vec<PathBuf>) {
        if let Ok(metadata) = fs::metadata(path) {
            if metadata.is_dir() {
                if let Ok(mut entries) = fs::read_dir(path) {
                    while let Some(Ok(entry)) = entries.next() {
                        visit(&entry.path().to_path_buf(), exts, out);
                    }
                }
            } else if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                if exts.iter().any(|x| x.eq_ignore_ascii_case(ext)) {
                    out.push(path.to_path_buf());
                }
            }
        }
    }

    visit(&dir, &exts, &mut found);

    found
}
