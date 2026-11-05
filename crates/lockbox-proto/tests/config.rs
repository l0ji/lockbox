//! Tests for [`lockbox_proto::config`].

mod common;

use std::{collections::HashMap, ffi::OsString, fs, io, path::Path};

use common::Temp;
use lockbox_proto::config;

/// Fixed environment from a set of variables.
fn env_vars(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
    let vars: HashMap<String, OsString> = pairs
        .iter()
        .map(|(name, value)| ((*name).to_owned(), OsString::from(value)))
        .collect();
    move |name| vars.get(name).cloned()
}

/// Environment for an user's $HOME, with the given variables on top of `$HOME`.
fn env_with_home(home: &Path, pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
    let mut vars: Vec<(String, String)> = pairs
        .iter()
        .map(|(name, value)| (name.to_string(), value.to_string()))
        .collect();
    if !vars.iter().any(|(name, _)| name == "HOME") {
        let home = home.to_str().expect("temporary path is UTF-8");
        vars.push(("HOME".to_owned(), home.to_owned()));
    }
    move |name| {
        vars.iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| OsString::from(value))
    }
}

/// System directory that no test machine has.
const MISSING_SYSTEM: &str = "/nonexistent/lockbox";

#[test]
fn directory_override_wins() {
    let temp = Temp::new("override");
    let keys = temp.path().join("files");
    let system = Temp::new("override-system");
    let dir = config::dir_with(
        system.path(),
        env_with_home(system.path(), &[("LOCKBOX_DIR", keys.to_str().unwrap())]),
    )
    .expect("override directory");
    assert_eq!(dir, keys);
    assert!(dir.is_dir(), "{}", dir.display());
}

#[test]
fn an_empty_override_is_ignored() {
    let temp = Temp::new("empty-override");
    let dir = config::dir_with(
        Path::new(MISSING_SYSTEM),
        env_with_home(temp.path(), &[("LOCKBOX_DIR", "")]),
    )
    .expect("config directory");
    assert_eq!(dir, temp.path().join(".config/lockbox"));
}

#[test]
fn the_system_directory_wins_when_it_exists() {
    let system = Temp::new("system");
    let temp = Temp::new("system-home");
    let dir =
        config::dir_with(system.path(), env_with_home(temp.path(), &[])).expect("system directory");
    assert_eq!(dir, system.path());
    assert!(!temp.path().join(".config").exists());
}

#[test]
fn the_config_directory_is_created_when_missing() {
    let temp = Temp::new("config");
    let xdg = temp.path().join("xdg");
    let dir = config::dir_with(
        Path::new(MISSING_SYSTEM),
        env_with_home(temp.path(), &[("XDG_CONFIG_HOME", xdg.to_str().unwrap())]),
    )
    .expect("config directory");
    assert_eq!(dir, xdg.join(config::CONFIG_DIR));
    assert!(dir.is_dir(), "{}", dir.display());
}

#[test]
fn the_home_fallback_is_used_when_the_config_directory_is_locked() {
    let temp = Temp::new("locked");
    // A file where the config directory should be makes creating it impossible
    let blocked = temp.path().join("blocked");
    fs::write(&blocked, "not a directory").expect("blocking file");
    let dir = config::dir_with(
        Path::new(MISSING_SYSTEM),
        env_with_home(
            temp.path(),
            &[("XDG_CONFIG_HOME", blocked.to_str().unwrap())],
        ),
    )
    .expect("home fallback");
    assert_eq!(dir, temp.path().join(config::HOME_DIR));
    assert!(dir.is_dir(), "{}", dir.display());
}

#[test]
fn a_daemon_without_a_home_or_config_directory_fails() {
    let err = config::dir_with(Path::new(MISSING_SYSTEM), env_vars(&[]))
        .expect_err("nothing to write into");
    assert_eq!(err.kind(), io::ErrorKind::NotFound);
    assert!(err.to_string().contains(config::DIR_ENV), "{err}");
}

#[test]
fn an_empty_home_is_treated_as_unset() {
    let temp = Temp::new("empty-home");
    let err = config::dir_with(
        Path::new(MISSING_SYSTEM),
        env_with_home(temp.path(), &[("HOME", "")]),
    )
    .expect_err("no home directory");
    assert_eq!(err.kind(), io::ErrorKind::NotFound);
}
