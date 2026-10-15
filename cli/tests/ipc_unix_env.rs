// SPDX-License-Identifier: Apache-2.0

//! The user's run directory from the environment (G9, #70; C2 §4): `$XDG_RUNTIME_DIR/oac`,
//! else `~/.oac/run`, created `0700`, and refused when it exists but others can enter it.
//!
//! This file holds one test, so it runs as its own process with no other test thread:
//! setting and removing an environment variable is sound only while no other thread uses the
//! environment.

#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use oac_cli::ipc::Scope;

fn mode(path: &Path) -> u32 {
    fs::metadata(path).unwrap().permissions().mode() & 0o777
}

#[test]
fn the_users_run_directory_follows_the_environment_and_is_private() {
    let base = std::env::temp_dir().join(format!("oac-env-{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    fs::create_dir_all(&base).unwrap();
    let xdg = base.join("xdg");
    let home = base.join("home");
    fs::create_dir_all(&xdg).unwrap();
    fs::create_dir_all(&home).unwrap();
    fs::set_permissions(&xdg, fs::Permissions::from_mode(0o700)).unwrap();

    // SAFETY: this test binary runs this single test; no other thread reads or writes the
    // environment while it runs.
    unsafe {
        std::env::set_var("XDG_RUNTIME_DIR", &xdg);
        std::env::set_var("HOME", &home);
    }
    // The XDG directory wins; `oac` is made inside it, 0700.
    Scope::for_user().expect("a private run directory is made");
    assert_eq!(mode(&xdg.join("oac")), 0o700);

    // One that exists and others can enter is refused, and its mode is left as found.
    fs::set_permissions(xdg.join("oac"), fs::Permissions::from_mode(0o755)).unwrap();
    let refused = Scope::for_user().unwrap_err();
    assert_eq!(refused.kind(), std::io::ErrorKind::PermissionDenied);
    assert_eq!(mode(&xdg.join("oac")), 0o755);

    // A link in its place is refused as well.
    fs::remove_dir_all(xdg.join("oac")).unwrap();
    std::os::unix::fs::symlink(&home, xdg.join("oac")).unwrap();
    assert!(Scope::for_user().is_err());

    // Without XDG_RUNTIME_DIR: `~/.oac/run`, with every directory made 0700.
    // SAFETY: as above.
    unsafe { std::env::remove_var("XDG_RUNTIME_DIR") };
    Scope::for_user().expect("the home directory fallback");
    assert_eq!(mode(&home.join(".oac")), 0o700);
    assert_eq!(mode(&home.join(".oac").join("run")), 0o700);

    // Neither set: no run directory.
    // SAFETY: as above.
    unsafe { std::env::remove_var("HOME") };
    assert_eq!(
        Scope::for_user().unwrap_err().kind(),
        std::io::ErrorKind::NotFound
    );
    let _ = fs::remove_dir_all(&base);
}
