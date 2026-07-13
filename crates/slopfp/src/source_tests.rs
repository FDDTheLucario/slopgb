//! Filesystem tests: exercises `read_dir`/`home`/`roots`/`make_dir` against a
//! real temp directory (see the module doc on `source.rs` for why this is the
//! one file allowed to touch disk).
use super::*;
use std::sync::atomic::{AtomicU32, Ordering};

/// A fresh, unique temp dir under `std::env::temp_dir()` (pid + atomic counter,
/// no rand/deps). Caller is responsible for best-effort cleanup.
fn unique_temp_dir(tag: &str) -> PathBuf {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("slopfp-test-{tag}-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create unique temp dir");
    dir
}

#[test]
fn read_dir_maps_entries() {
    let dir = unique_temp_dir("readdir");
    std::fs::write(dir.join("file.txt"), b"hello").unwrap();
    std::fs::create_dir(dir.join("sub")).unwrap();

    let entries = read_dir(&dir).unwrap();
    assert_eq!(entries.len(), 2);

    let file = entries
        .iter()
        .find(|e| e.name == "file.txt")
        .expect("file.txt listed");
    assert!(!file.is_dir);
    assert_eq!(file.size, Some(5));
    assert!(file.mtime.is_some());

    let sub = entries
        .iter()
        .find(|e| e.name == "sub")
        .expect("sub listed");
    assert!(sub.is_dir);
    assert_eq!(sub.size, None);

    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(unix)]
#[test]
fn symlink_to_dir_is_dir() {
    let dir = unique_temp_dir("symdir");
    std::fs::create_dir(dir.join("realdir")).unwrap();
    std::os::unix::fs::symlink(dir.join("realdir"), dir.join("linkdir")).unwrap();

    let entries = read_dir(&dir).unwrap();
    let link = entries
        .iter()
        .find(|e| e.name == "linkdir")
        .expect("linkdir listed");
    assert!(link.is_dir);

    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(unix)]
#[test]
fn broken_symlink_listed_not_fatal() {
    let dir = unique_temp_dir("brokensym");
    std::os::unix::fs::symlink(dir.join("does-not-exist"), dir.join("broken")).unwrap();

    let entries = read_dir(&dir).unwrap();
    let broken = entries
        .iter()
        .find(|e| e.name == "broken")
        .expect("broken symlink still listed");
    assert!(!broken.is_dir);
    assert_eq!(broken.size, None);
    assert_eq!(broken.mtime, None);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn home_reads_the_platform_home_var() {
    // ponytail: edition-2024's `forbid(unsafe_code)` bans `std::env::set_var`
    // (now unsafe), so we can't feed `home()` a known value — instead pin its
    // contract against the ambient env with the var NAME hardcoded as the spec.
    // This is not a pure mirror: a regression that read the wrong variable, or
    // returned `None`/a relative path when the var is set, fails here.
    #[cfg(windows)]
    let name = "USERPROFILE";
    #[cfg(not(windows))]
    let name = "HOME";

    match std::env::var_os(name) {
        Some(v) => {
            let h = home().expect("home() is Some when the platform var is set");
            assert_eq!(h, PathBuf::from(&v), "home() is exactly ${name}");
            assert!(h.is_absolute(), "a real home path is absolute: {h:?}");
        }
        None => assert!(home().is_none(), "home() is None when ${name} is unset"),
    }
    // Pure function of the env: repeated calls agree (no hidden state).
    assert_eq!(home(), home());
}

#[cfg(unix)]
#[test]
fn roots_contains_root() {
    let r = roots();
    assert!(!r.is_empty());
    assert!(r.contains(&PathBuf::from("/")));
}

#[test]
fn make_dir_creates() {
    let parent = unique_temp_dir("mkdir");
    let p = make_dir(&parent, "sub").unwrap();
    assert!(p.is_dir());
    let _ = std::fs::remove_dir_all(&parent);
}

#[test]
fn make_dir_errors_on_dup() {
    let parent = unique_temp_dir("mkdir-dup");
    make_dir(&parent, "sub").unwrap();
    assert!(make_dir(&parent, "sub").is_err());
    let _ = std::fs::remove_dir_all(&parent);
}

#[cfg(unix)]
#[test]
fn permission_denied_is_graceful() {
    use std::os::unix::fs::PermissionsExt;

    let parent = unique_temp_dir("perm");
    let locked = parent.join("locked");
    std::fs::create_dir(&locked).unwrap();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();

    // Root bypasses directory permissions, so this may still be Ok — the only
    // real requirement is that it never panics.
    let _ = read_dir(&locked);

    // Restore perms so the temp dir can be removed.
    let _ = std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755));
    let _ = std::fs::remove_dir_all(&parent);
}
