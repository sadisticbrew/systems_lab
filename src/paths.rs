//! Where SystemsLab keeps per-user files: notes, viewer preferences, bundled experiments.

use std::ffi::OsString;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::exit;

/// `$var` if it is set to an absolute path, else `$HOME/<fallback>`.
fn xdg_dir(var: Option<OsString>, home: Option<OsString>, fallback: &str) -> Result<PathBuf> {
    if let Some(v) = var.map(PathBuf::from).filter(|p| p.is_absolute()) {
        return Ok(v.join("systemslab"));
    }
    match home.map(PathBuf::from).filter(|p| p.is_absolute()) {
        Some(h) => Ok(h.join(fallback).join("systemslab")),
        None => Err(exit::usage(
            "cannot find a place for SystemsLab's files: set HOME (or XDG_DATA_HOME and XDG_CONFIG_HOME) to an absolute path",
        )),
    }
}

/// `$XDG_DATA_HOME/systemslab`, else `$HOME/.local/share/systemslab`.
pub fn data_dir() -> Result<PathBuf> {
    xdg_dir(std::env::var_os("XDG_DATA_HOME"), std::env::var_os("HOME"), ".local/share")
}

/// `$XDG_CONFIG_HOME/systemslab`, else `$HOME/.config/systemslab`.
pub fn config_dir() -> Result<PathBuf> {
    xdg_dir(std::env::var_os("XDG_CONFIG_HOME"), std::env::var_os("HOME"), ".config")
}

/// 64-bit FNV-1a hash.
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, &b| (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3))
}

/// The notes file of a trace: `data_dir()/notes/<hash of the canonical trace path>.json`.
pub fn notes_file(trace: &Path) -> Result<PathBuf> {
    let canon = std::fs::canonicalize(trace).map_err(|e| exit::usage(format!("cannot resolve {}: {e}", trace.display())))?;
    Ok(data_dir()?.join("notes").join(format!("{:016x}.json", fnv1a64(canon.as_os_str().as_bytes()))))
}

/// The viewer preferences file: `config_dir()/viewer.json`.
pub fn prefs_file() -> Result<PathBuf> {
    Ok(config_dir()?.join("viewer.json"))
}

/// Where `systemslab try` keeps its editable copies: `data_dir()/experiments`.
pub fn experiments_dir() -> Result<PathBuf> {
    Ok(data_dir()?.join("experiments"))
}

/// Writes `data` to `<path>.tmp` and renames it over `path`, so readers see the old or the new
/// file, never a partial one. Removes the temp file on failure; the parent directory must exist.
pub fn write_atomic(path: &Path, data: &[u8]) -> io::Result<()> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    let result = std::fs::write(&tmp, data).and_then(|()| std::fs::rename(&tmp, path));
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn os(s: &str) -> Option<OsString> {
        Some(OsString::from(s))
    }

    #[test]
    fn fnv1a64_matches_the_published_vectors() {
        assert_eq!(fnv1a64(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a64(b"a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(fnv1a64(b"foobar"), 0x8594_4171_f739_67e8);
    }

    #[test]
    fn xdg_variable_wins_only_when_absolute() {
        assert_eq!(xdg_dir(os("/x/data"), os("/home/u"), ".local/share").unwrap(), Path::new("/x/data/systemslab"));
        assert_eq!(xdg_dir(None, os("/home/u"), ".local/share").unwrap(), Path::new("/home/u/.local/share/systemslab"));
        assert_eq!(xdg_dir(os("relative"), os("/home/u"), ".config").unwrap(), Path::new("/home/u/.config/systemslab"));
        assert_eq!(xdg_dir(os(""), os("/home/u"), ".config").unwrap(), Path::new("/home/u/.config/systemslab"));
    }

    #[test]
    fn missing_home_and_xdg_is_a_usage_error() {
        for err in [xdg_dir(None, None, ".config"), xdg_dir(os("rel"), os("rel"), ".config")] {
            assert_eq!(exit::code_of(&err.unwrap_err()), exit::USAGE);
        }
    }

    #[test]
    fn write_atomic_replaces_the_file_and_leaves_no_temp_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("file.json");
        write_atomic(&path, b"one").unwrap();
        write_atomic(&path, b"two").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"two");
        assert!(!dir.path().join("file.json.tmp").exists());
        // A failed rename (the target is a directory) cleans up after itself.
        let blocked = dir.path().join("blocked");
        std::fs::create_dir(&blocked).unwrap();
        assert!(write_atomic(&blocked, b"x").is_err());
        assert!(!dir.path().join("blocked.tmp").exists());
    }
}
