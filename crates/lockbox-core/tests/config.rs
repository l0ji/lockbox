//! Tests for [`lockbox_core::config`].

mod common;

use std::{collections::HashMap, ffi::OsString, fs, io, path::Path};

use common::Temp;
use lockbox_core::config;

/// Fixed environment from a set of variables.
fn env_vars(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> + use<> {
    let vars: HashMap<String, OsString> = pairs
        .iter()
        .map(|(name, value)| ((*name).to_owned(), OsString::from(value)))
        .collect();
    move |name| vars.get(name).cloned()
}

/// Environment for an user's $HOME, with the given variables on top of `$HOME`.
fn env_with_home(home: &Path, pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> + use<> {
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
    assert!(
        !temp
            .path()
            .join(".config")
            .try_exists()
            .expect("check config dir")
    );
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

#[test]
fn the_user_directory_ignores_the_system_one() {
    let system = Temp::new("scope-system");
    let home = Temp::new("scope-home");
    let xdg = home.path().join("xdg");
    let vars = env_with_home(home.path(), &[("XDG_CONFIG_HOME", xdg.to_str().unwrap())]);

    let default = config::dir_with(system.path(), &vars).expect("default directory");
    let user = config::user_dir_with(&vars).expect("user directory");

    assert_eq!(default, system.path());
    assert_eq!(user, xdg.join(config::CONFIG_DIR));
}

#[test]
fn the_user_directory_still_honours_the_override() {
    let home = Temp::new("scope-override");
    let files = home.path().join("files");
    let dir = config::user_dir_with(env_with_home(
        home.path(),
        &[("LOCKBOX_DIR", files.to_str().unwrap())],
    ))
    .expect("override directory");

    assert_eq!(dir, files);
}

#[cfg(unix)]
#[test]
fn an_unwritable_system_directory_is_still_chosen() {
    let system = Temp::new("system-locked");
    let home = Temp::new("system-locked-home");
    common::lock(system.path());

    let dir = config::dir_with(system.path(), env_with_home(home.path(), &[]));
    common::unlock(system.path());

    assert_eq!(dir.expect("system directory"), system.path());
    assert!(!home.path().join(".config").exists());
}

#[cfg(unix)]
#[test]
fn a_locked_override_names_the_directory() {
    let temp = Temp::new("locked-override");
    let files = temp.path().join("files");
    fs::create_dir_all(&files).expect("override directory");
    if !common::lock(&files) {
        common::unlock(&files);
        return;
    }

    let result = config::user_dir_with(env_vars(&[("LOCKBOX_DIR", files.to_str().unwrap())]));
    common::unlock(&files);

    let err = result.expect_err("a locked override has to be refused");
    assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
    assert!(err.to_string().contains(files.to_str().unwrap()), "{err}");
}
