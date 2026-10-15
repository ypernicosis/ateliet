//! Self-update from the git checkout this binary was built from: `git fetch`
//! tells whether the upstream branch moved past the built commit; applying
//! pulls, rebuilds (front-end + Rust) and swaps the running executable.

use crate::process::augmented_path;
use serde::Serialize;
use serde_json::json;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter};

const BUILT_COMMIT: &str = env!("ATELIET_GIT_COMMIT");
const SOURCE_DIR: &str = env!("ATELIET_SOURCE_DIR");

static RUNNING: AtomicBool = AtomicBool::new(false);
/// Every window checks on its own schedule; two `git fetch` at once fight over the ref locks.
static CHECKING: Mutex<()> = Mutex::new(());

/// The checkout to update from: `ATELIET_SOURCE` overrides the build path.
fn source_dir() -> Result<PathBuf, String> {
    let dir = std::env::var("ATELIET_SOURCE").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from(SOURCE_DIR));
    if dir.join(".git").exists() {
        Ok(dir)
    } else {
        Err(format!("No git checkout at {} (set ATELIET_SOURCE to the repository folder)", dir.display()))
    }
}

fn command(dir: &Path, prog: &str, args: &[&str]) -> Command {
    let mut c = Command::new(prog);
    // No terminal to answer a prompt: fail instead of waiting forever for a password.
    c.args(args)
        .current_dir(dir)
        .env("PATH", augmented_path())
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_SSH_COMMAND", "ssh -o BatchMode=yes")
        .stdin(Stdio::null());
    c
}

fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let out = command(dir, "git", args).output().map_err(|e| format!("git: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

#[derive(Serialize)]
pub struct UpdateStatus {
    available: bool,
    behind: u32,
    current: String,
    latest: String,
    upstream: String,
    commits: Vec<String>,
}

fn check() -> Result<UpdateStatus, String> {
    let _one_at_a_time = CHECKING.lock().unwrap_or_else(|e| e.into_inner());
    check_in(&source_dir()?, BUILT_COMMIT)
}

fn check_in(dir: &Path, built: &str) -> Result<UpdateStatus, String> {
    let dir = dir.to_path_buf();
    git(&dir, &["fetch", "--quiet"])?;
    let upstream = git(&dir, &["rev-parse", "--abbrev-ref", "@{u}"]).map_err(|_| "The checkout's branch has no upstream to update from".to_string())?;
    let latest = git(&dir, &["rev-parse", "@{u}"])?;
    // A binary built outside the checkout (or before history was rewritten) compares against HEAD.
    let built_known = !built.is_empty() && git(&dir, &["cat-file", "-e", &format!("{built}^{{commit}}")]).is_ok();
    let current = if built_known { built.to_string() } else { git(&dir, &["rev-parse", "HEAD"])? };
    let range = format!("{current}..{latest}");
    let behind: u32 = git(&dir, &["rev-list", "--count", &range])?.parse().unwrap_or(0);
    let commits = if behind > 0 {
        git(&dir, &["log", "--format=%h %s", "-n", "30", &range])?.lines().map(String::from).collect()
    } else {
        Vec::new()
    };
    Ok(UpdateStatus { available: behind > 0, behind, current, latest, upstream, commits })
}

#[tauri::command]
pub async fn update_check() -> Result<UpdateStatus, String> {
    tauri::async_runtime::spawn_blocking(check).await.map_err(|e| e.to_string())?
}

fn emit(app: &AppHandle, kind: &str, text: &str) {
    let _ = app.emit("update-progress", json!({ "kind": kind, "text": text }));
}

/// Runs one build step, streaming its output to the windows.
fn run(app: &AppHandle, dir: &Path, step: &str, prog: &str, args: &[&str]) -> Result<(), String> {
    emit(app, "step", step);
    let mut child = command(dir, prog, args).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|e| format!("{step}: {prog}: {e}"))?;
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let a = app.clone();
    let out_thread = std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            emit(&a, "log", &line);
        }
    });
    let mut tail: Vec<String> = Vec::new();
    for line in BufReader::new(stderr).lines().map_while(Result::ok) {
        emit(app, "log", &line);
        tail.push(line);
        if tail.len() > 12 {
            tail.remove(0);
        }
    }
    let _ = out_thread.join();
    let status = child.wait().map_err(|e| e.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{step} failed:\n{}", tail.join("\n")))
    }
}

/// Puts `new` in place of the running executable. Renaming (not writing over)
/// keeps the running process valid on every platform.
fn install(new: &Path, exe: &Path) -> Result<(), String> {
    let staged = exe.with_file_name(".ateliet-update");
    let old = exe.with_file_name(".ateliet-old");
    fs::copy(new, &staged).map_err(|e| format!("copy {}: {e}", staged.display()))?;
    let _ = fs::remove_file(&old);
    fs::rename(exe, &old).map_err(|e| format!("replace {}: {e}", exe.display()))?;
    if let Err(e) = fs::rename(&staged, exe) {
        let _ = fs::rename(&old, exe);
        return Err(format!("replace {}: {e}", exe.display()));
    }
    // Fails on Windows while the old binary still runs; it is removed next time.
    let _ = fs::remove_file(&old);
    #[cfg(target_os = "macos")]
    if let Some(bundle) = exe.ancestors().find(|p| p.extension().is_some_and(|e| e == "app")) {
        // The bundle's seal no longer matches the new binary; re-sign ad hoc.
        let _ = Command::new("codesign").args(["--force", "--deep", "--sign", "-"]).arg(bundle).status();
    }
    Ok(())
}

fn apply(app: &AppHandle, dir: &Path) -> Result<(), String> {
    let exe = std::env::current_exe().and_then(fs::canonicalize).map_err(|e| e.to_string())?;
    let target = fs::canonicalize(dir.join("target")).unwrap_or_else(|_| dir.join("target"));
    // A `cargo run`/`cargo build` binary (target/debug/ateliet, target/release/ateliet)
    // is rebuilt in place with its own profile. Anything else, including the
    // .app that `cargo tauri build` leaves in target/release/bundle, gets a
    // release build copied over its executable.
    let in_checkout = exe.parent().is_some_and(|p| p == target.join("debug") || p == target.join("release"));
    let release = !in_checkout || exe.parent() == Some(target.join("release").as_path());

    run(app, dir, "Pulling changes", "git", &["pull", "--ff-only"])?;
    run(app, dir, "Building interface", "sh", &["build.sh"])?;
    let mut args = vec!["build"];
    if release {
        args.push("--release");
    }
    if !tauri::is_dev() {
        // what `cargo tauri build` enables: assets served from the embedded dist
        args.extend(["--features", "tauri/custom-protocol"]);
    }
    run(app, dir, "Compiling Ateliet", "cargo", &args)?;
    if !in_checkout {
        emit(app, "step", "Installing");
        let built = target.join("release").join(if cfg!(windows) { "ateliet.exe" } else { "ateliet" });
        install(&built, &exe)?;
    }
    Ok(())
}

#[tauri::command]
pub fn update_apply(app: AppHandle) -> Result<(), String> {
    let dir = source_dir()?;
    if RUNNING.swap(true, Ordering::SeqCst) {
        return Err("An update is already running".into());
    }
    std::thread::spawn(move || {
        let res = apply(&app, &dir);
        RUNNING.store(false, Ordering::SeqCst);
        match res {
            Ok(()) => emit(&app, "done", ""),
            Err(e) => emit(&app, "error", &e),
        }
    });
    Ok(())
}

#[tauri::command]
pub fn update_restart(app: AppHandle) {
    app.restart();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sh(dir: &Path, script: &str) {
        let ok = Command::new("sh").arg("-c").arg(script).current_dir(dir).status().unwrap().success();
        assert!(ok, "{script}");
    }

    #[test]
    fn detects_upstream_commits_and_installs_binary() {
        let root = std::env::temp_dir().join(format!("ateliet-update-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let cfg = "git -c user.name=t -c user.email=t@t";
        sh(&root, &format!("git init -q -b main origin && cd origin && echo a > f && git add f && {cfg} commit -qm one"));
        sh(&root, "git clone -q origin local");
        let local = root.join("local");
        let built = git(&local, &["rev-parse", "HEAD"]).unwrap();

        let s = check_in(&local, &built).unwrap();
        assert!(!s.available);

        sh(&root.join("origin"), &format!("echo b >> f && {cfg} commit -qam two && echo c >> f && {cfg} commit -qam three"));
        let s = check_in(&local, &built).unwrap();
        assert!(s.available);
        assert_eq!(s.behind, 2);
        assert_eq!(s.upstream, "origin/main");
        assert!(s.commits[0].ends_with(" three"));
        // unknown build commit: compare against HEAD
        assert_eq!(check_in(&local, "").unwrap().behind, 2);

        let exe = root.join("app");
        let new = root.join("new");
        fs::write(&exe, "old").unwrap();
        fs::write(&new, "new").unwrap();
        install(&new, &exe).unwrap();
        assert_eq!(fs::read_to_string(&exe).unwrap(), "new");
        assert!(!root.join(".ateliet-old").exists());
        fs::remove_dir_all(&root).unwrap();
    }
}
