// SPDX-License-Identifier: MPL-2.0

use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{anyhow, Result};
use lofty::prelude::*;
use lofty::probe::Probe;

use super::backend::TrackMetadata;

/// Parse metadata for a single audio file using the `lofty` crate.
pub fn parse_file_metadata(path: &Path) -> Result<TrackMetadata> {
    let tagged = Probe::open(path)
        .map_err(|e| anyhow!("failed to open {:?}: {e}", path))?
        .read()
        .map_err(|e| anyhow!("failed to read tags for {:?}: {e}", path))?;

    let tag = tagged.primary_tag().or_else(|| tagged.first_tag());
    let props = tagged.properties();

    let mut md = TrackMetadata::default();

    if let Some(t) = tag.and_then(|t| t.title()) {
        md.title = Some(t.to_string());
    }
    if let Some(a) = tag.and_then(|t| t.album()) {
        md.album = Some(a.to_string());
    }
    if let Some(ar) = tag.and_then(|t| t.artist()) {
        md.artist = Some(ar.to_string());
    }

    // Duration is optional in TrackMetadata (backend), keep using backend's duration query for playback.
    let _duration: Option<Duration> = Some(props.duration());

    Ok(md)
}

/// Parse metadata for a list of files.
pub fn parse_files_metadata(paths: &[PathBuf]) -> Vec<(PathBuf, TrackMetadata)> {
    paths
        .iter()
        .map(|p| {
            let md = parse_file_metadata(p.as_path()).unwrap_or_default();
            (p.clone(), md)
        })
        .collect()
}
