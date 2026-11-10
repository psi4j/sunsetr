//! The last GeoClue fix, kept in the state directory across restarts.
//!
//! The file holds one line, `<latitude> <longitude> <unix seconds>`, with the
//! coordinates at two decimals. It lives next to the preset marker, so presets
//! under one configuration directory share it.

use anyhow::{Context, Result};
use chrono::DateTime;
use std::fs;
use std::path::{Path, PathBuf};

use crate::common::utils::format_coordinate;
use crate::geo::geoclue::Fix;
use crate::state::preset::{get_state_dir, write_atomic};

const FILE_NAME: &str = "geoclue_fix";

fn state_dir() -> Result<PathBuf> {
    let config_dir = crate::config::get_custom_config_dir();
    get_state_dir(config_dir.as_deref())
}

/// The cached fix for the current configuration directory, `None` when there
/// is no readable one.
pub fn read_cached_fix() -> Option<Fix> {
    read_fix_from(&state_dir().ok()?)
}

pub fn write_cached_fix(fix: Fix) -> Result<()> {
    let dir = state_dir()?;
    fs::create_dir_all(&dir).with_context(|| format!("Failed to create {}", dir.display()))?;
    write_fix_to(&dir, fix)
}

fn read_fix_from(dir: &Path) -> Option<Fix> {
    let contents = fs::read_to_string(dir.join(FILE_NAME)).ok()?;
    let mut fields = contents.split_whitespace();
    let latitude = fields.next()?.parse().ok()?;
    let longitude = fields.next()?.parse().ok()?;
    let fixed_at = DateTime::from_timestamp(fields.next()?.parse().ok()?, 0)?;
    Some(Fix {
        latitude,
        longitude,
        fixed_at,
        cached: true,
    })
}

fn write_fix_to(dir: &Path, fix: Fix) -> Result<()> {
    let line = format!(
        "{} {} {}",
        format_coordinate(fix.latitude),
        format_coordinate(fix.longitude),
        fix.fixed_at.timestamp()
    );
    write_atomic(dir, FILE_NAME, &line).context("Failed to write the GeoClue fix cache")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_written_fix_reads_back_rounded_and_marked_cached() {
        let dir = tempfile::tempdir().unwrap();
        let fixed_at = DateTime::from_timestamp(1_700_000_000, 0).unwrap();
        let fix = Fix {
            latitude: 35.6762,
            longitude: 139.6503,
            fixed_at,
            cached: false,
        };

        write_fix_to(dir.path(), fix).unwrap();
        let read = read_fix_from(dir.path()).unwrap();

        assert_eq!(read.latitude, 35.68);
        assert_eq!(read.longitude, 139.65);
        assert_eq!(read.fixed_at, fixed_at);
        assert!(read.cached);
        assert_eq!(
            fs::read_to_string(dir.path().join(FILE_NAME)).unwrap(),
            "35.68 139.65 1700000000"
        );
    }

    #[test]
    fn a_missing_or_malformed_file_reads_as_none() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read_fix_from(dir.path()).is_none());

        fs::write(dir.path().join(FILE_NAME), "35.68 east 1700000000").unwrap();
        assert!(read_fix_from(dir.path()).is_none());

        fs::write(dir.path().join(FILE_NAME), "35.68 139.65").unwrap();
        assert!(read_fix_from(dir.path()).is_none());
    }

    #[test]
    fn writing_replaces_the_previous_fix() {
        let dir = tempfile::tempdir().unwrap();
        write_fix_to(dir.path(), Fix::live(51.5, -0.13)).unwrap();
        write_fix_to(dir.path(), Fix::live(35.68, 139.65)).unwrap();

        let read = read_fix_from(dir.path()).unwrap();
        assert_eq!((read.latitude, read.longitude), (35.68, 139.65));
    }
}
