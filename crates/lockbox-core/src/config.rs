//! Where lockbox stores its configuration files.

use std::{
    ffi::OsString,
    fs, io,
    path::{Path, PathBuf},
};

pub use lockbox_proto::config::{CONFIG_DIR, DIR_ENV, HOME_DIR, SYSTEM_DIR};

/// Directory for lockbox's files, created when needed.
///
/// [`DIR_ENV`] wins when it is set and non-empty. Otherwise [`SYSTEM_DIR`] is
/// used when it exists, so files that an administrator installed there are never
/// shadowed by ones created in the user's own directory.
///
/// Everything else is kept in [`CONFIG_DIR`] under `$XDG_CONFIG_HOME`, or under
/// `$HOME/.config` when that is unset, and the directory is created when it is
/// missing. When it cannot be created or written, [`HOME_DIR`] under `$HOME` is
/// used instead.
pub fn dir() -> io::Result<PathBuf> {
    dir_with(Path::new(SYSTEM_DIR), |name: &str| std::env::var_os(name))
}

/// [`dir`] for an explicit system directory and environment.
pub fn dir_with(system: &Path, var: impl Fn(&str) -> Option<OsString>) -> io::Result<PathBuf> {
    match non_empty(var(DIR_ENV)) {
        Some(dir) => prepare(Path::new(&dir)),
        None if is_dir(system) => Ok(system.to_path_buf()),
        None => user_dir_with(var),
    }
}

/// Directory for lockbox's files in the user's own account.
///
/// Like [`dir`], but [`SYSTEM_DIR`] is never used, so a keypair installed by an
/// administrator does not hide the user's own.
pub fn user_dir() -> io::Result<PathBuf> {
    user_dir_with(|name: &str| std::env::var_os(name))
}

/// [`user_dir`] for an explicitly defined environment.
pub fn user_dir_with(var: impl Fn(&str) -> Option<OsString>) -> io::Result<PathBuf> {
    if let Some(dir) = non_empty(var(DIR_ENV)) {
        return prepare(Path::new(&dir));
    }
    let home = non_empty(var("HOME")).map(PathBuf::from);
    let config = non_empty(var("XDG_CONFIG_HOME"))
        .map(PathBuf::from)
        .or_else(|| home.as_ref().map(|home| home.join(".config")));
    if let Some(config) = config
        && let Ok(dir) = prepare(&config.join(CONFIG_DIR))
    {
        return Ok(dir);
    }
    if let Some(home) = home
        && let Ok(dir) = prepare(&home.join(HOME_DIR))
    {
        return Ok(dir);
    }
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        format!("no directory to keep lockbox files in; set {DIR_ENV}"),
    ))
}

/// Create `dir` and check that a file can be written into it.
fn prepare(dir: &Path) -> io::Result<PathBuf> {
    let check = || {
        fs::create_dir_all(dir)?;
        let probe = dir.join(format!(".probe-{}", std::process::id()));
        fs::File::create_new(&probe)?;
        fs::remove_file(&probe)
    };
    check().map_err(|err| {
        io::Error::new(err.kind(), format!("cannot use {}: {err}", dir.display()))
    })?;
    Ok(dir.to_path_buf())
}

/// Safe function for verifying if `path` is a directory.
fn is_dir(path: &Path) -> bool {
    fs::metadata(path).is_ok_and(|meta| meta.is_dir())
}

fn non_empty(value: Option<OsString>) -> Option<OsString> {
    value.filter(|value| !value.is_empty())
}
