//! Self-update from the git checkout this binary was built from, as the web
//! shell does: `git fetch` tells whether the upstream branch moved past the
//! built commit; updating pulls, rebuilds the native app with the same
//! features and swaps the running executable. Checked every half hour; with
//! automatic updates on (the default), the new build is made in the
//! background and starts with the app, or right away on a restart.

use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

const BUILT_COMMIT: &str = env!("ATELIET_GIT_COMMIT");
const SOURCE_DIR: &str = env!("ATELIET_SOURCE_DIR");
const FEATURES: &str = env!("ATELIET_FEATURES");
const CHECK_EVERY: Duration = Duration::from_secs(30 * 60);

#[derive(Clone, Debug, Default)]
pub struct Status {
    pub available: bool,
    pub behind: u32,
    pub current: String,
    pub latest: String,
    pub upstream: String,
    pub commits: Vec<String>,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum State {
    Idle,
    Checking,
    Building,
    /// Built: the new version starts with the app.
    Ready,
    Failed,
}

enum Msg {
    Checked(Result<Status, String>),
    Step(String),
    Log(String),
    Done(Result<(), String>),
}

pub struct Updater {
    pub status: Option<Status>,
    pub error: Option<String>,
    pub state: State,
    pub step: String,
    pub log: Vec<String>,
    pub auto: bool,
    pub open: bool,
    last_check: Option<Instant>,
    /// The commit a background build was tried for (not tried again).
    tried: String,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    pub notice: Option<String>,
}

fn source_dir() -> Result<PathBuf, String> {
    let dir = std::env::var("ATELIET_SOURCE").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from(SOURCE_DIR));
    if dir.join(".git").exists() {
        Ok(dir)
    } else {
        Err(format!("Não há checkout git em {} (defina ATELIET_SOURCE com a pasta do repositório)", dir.display()))
    }
}

fn command(dir: &Path, prog: &str, args: &[&str]) -> Command {
    let mut c = Command::new(prog);
    // no terminal to answer a prompt: fail instead of waiting for a password
    c.args(args).current_dir(dir).env("GIT_TERMINAL_PROMPT", "0").env("GIT_SSH_COMMAND", "ssh -o BatchMode=yes").env("NO_COLOR", "1").env("CARGO_TERM_COLOR", "never").stdin(Stdio::null());
    if let Some(home) = std::env::var_os("HOME") {
        // cargo installed by rustup is not always on the PATH of an app started from the desktop
        let cargo = PathBuf::from(home).join(".cargo/bin");
        let path = std::env::var_os("PATH").unwrap_or_default();
        let mut parts: Vec<PathBuf> = std::env::split_paths(&path).collect();
        parts.push(cargo);
        parts.extend(["/opt/homebrew/bin", "/usr/local/bin"].iter().map(PathBuf::from));
        if let Ok(p) = std::env::join_paths(parts) {
            c.env("PATH", p);
        }
    }
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

pub fn check_in(dir: &Path, built: &str) -> Result<Status, String> {
    git(dir, &["fetch", "--quiet"])?;
    let upstream = git(dir, &["rev-parse", "--abbrev-ref", "@{u}"]).map_err(|_| "O ramo do checkout não tem de onde atualizar (upstream)".to_string())?;
    let latest = git(dir, &["rev-parse", "@{u}"])?;
    let known = !built.is_empty() && git(dir, &["cat-file", "-e", &format!("{built}^{{commit}}")]).is_ok();
    let current = if known { built.to_string() } else { git(dir, &["rev-parse", "HEAD"])? };
    let range = format!("{current}..{latest}");
    let behind: u32 = git(dir, &["rev-list", "--count", &range])?.parse().unwrap_or(0);
    let commits = if behind > 0 { git(dir, &["log", "--format=%h %s", "-n", "30", &range])?.lines().map(String::from).collect() } else { Vec::new() };
    Ok(Status { available: behind > 0, behind, current, latest, upstream, commits })
}

/// A line as a terminal would show it: escapes removed, the last redraw only.
fn plain(line: &str) -> String {
    let line = line.rsplit('\r').find(|s| !s.is_empty()).unwrap_or("");
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\x1b' {
            out.push(c);
            continue;
        }
        if chars.next() == Some('[') {
            for c in chars.by_ref() {
                if ('@'..='~').contains(&c) {
                    break;
                }
            }
        }
    }
    out
}

fn run(tx: &Sender<Msg>, dir: &Path, step: &str, prog: &str, args: &[&str]) -> Result<(), String> {
    let _ = tx.send(Msg::Step(step.into()));
    let mut child = command(dir, prog, args).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|e| format!("{step}: {prog}: {e}"))?;
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let t2 = tx.clone();
    let out = std::thread::spawn(move || {
        for l in BufReader::new(stdout).lines().map_while(Result::ok) {
            let _ = t2.send(Msg::Log(plain(&l)));
        }
    });
    let mut tail: Vec<String> = Vec::new();
    for l in BufReader::new(stderr).lines().map_while(Result::ok) {
        let l = plain(&l);
        let _ = tx.send(Msg::Log(l.clone()));
        tail.push(l);
        if tail.len() > 12 {
            tail.remove(0);
        }
    }
    let _ = out.join();
    if child.wait().map_err(|e| e.to_string())?.success() {
        Ok(())
    } else {
        Err(format!("{step} falhou:\n{}", tail.join("\n")))
    }
}

/// Puts `new` in place of the running executable (renaming keeps the running process valid).
fn install(new: &Path, exe: &Path) -> Result<(), String> {
    let staged = exe.with_file_name(".ateliet-native-update");
    let old = exe.with_file_name(".ateliet-native-old");
    fs::copy(new, &staged).map_err(|e| format!("copiar {}: {e}", staged.display()))?;
    let _ = fs::remove_file(&old);
    fs::rename(exe, &old).map_err(|e| format!("substituir {}: {e}", exe.display()))?;
    if let Err(e) = fs::rename(&staged, exe) {
        let _ = fs::rename(&old, exe);
        return Err(format!("substituir {}: {e}", exe.display()));
    }
    let _ = fs::remove_file(&old);
    #[cfg(target_os = "macos")]
    if let Some(bundle) = exe.ancestors().find(|p| p.extension().is_some_and(|e| e == "app")) {
        let _ = Command::new("codesign").args(["--force", "--deep", "--sign", "-"]).arg(bundle).status();
    }
    Ok(())
}

fn apply(tx: &Sender<Msg>, dir: &Path) -> Result<(), String> {
    let exe = std::env::current_exe().and_then(fs::canonicalize).map_err(|e| e.to_string())?;
    let target = fs::canonicalize(dir.join("target")).unwrap_or_else(|_| dir.join("target"));
    // a binary in the checkout's target is rebuilt in place with its own
    // profile; anything else gets a release build copied over it
    let in_checkout = exe.parent().is_some_and(|p| p == target.join("debug") || p == target.join("release"));
    let release = !in_checkout || exe.parent() == Some(target.join("release").as_path());
    run(tx, dir, "Baixando as mudanças", "git", &["pull", "--ff-only"])?;
    let mut args = vec!["build", "-p", "ateliet-native", "--no-default-features"];
    if !FEATURES.is_empty() {
        args.extend(["--features", FEATURES]);
    }
    if release {
        args.push("--release");
    }
    run(tx, dir, "Compilando o Ateliet", "cargo", &args)?;
    if !in_checkout {
        let _ = tx.send(Msg::Step("Instalando".into()));
        let built = target.join("release").join(if cfg!(windows) { "ateliet-native.exe" } else { "ateliet-native" });
        install(&built, &exe)?;
    }
    Ok(())
}

impl Updater {
    pub fn new() -> Updater {
        let (tx, rx) = mpsc::channel();
        let auto = crate::config::load_app_config()["autoUpdate"].as_bool().unwrap_or(true);
        Updater { status: None, error: None, state: State::Idle, step: String::new(), log: Vec::new(), auto, open: false, last_check: None, tried: String::new(), tx, rx, notice: None }
    }

    pub fn set_auto(&mut self, on: bool) {
        self.auto = on;
        let mut cfg = crate::config::load_app_config();
        cfg["autoUpdate"] = serde_json::json!(on);
        let file = crate::config::app_config_path();
        if let Some(d) = file.parent() {
            let _ = fs::create_dir_all(d);
        }
        let _ = fs::write(file, serde_json::to_string_pretty(&cfg).unwrap_or_default());
    }

    pub fn check(&mut self) {
        if matches!(self.state, State::Checking | State::Building) {
            return;
        }
        self.state = State::Checking;
        self.last_check = Some(Instant::now());
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let r = source_dir().and_then(|d| check_in(&d, BUILT_COMMIT));
            let _ = tx.send(Msg::Checked(r));
        });
    }

    pub fn update(&mut self) {
        if self.state == State::Building {
            return;
        }
        let dir = match source_dir() {
            Ok(d) => d,
            Err(e) => {
                self.error = Some(e);
                self.state = State::Failed;
                return;
            }
        };
        self.state = State::Building;
        self.log.clear();
        self.error = None;
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let r = apply(&tx, &dir);
            let _ = tx.send(Msg::Done(r));
        });
    }

    /// Start the new build in place of this process.
    pub fn restart(&self) -> ! {
        if let Ok(exe) = std::env::current_exe() {
            let _ = Command::new(exe).args(std::env::args().skip(1)).spawn();
        }
        std::process::exit(0)
    }

    /// Messages from the threads, and the periodic check.
    pub fn tick(&mut self, ctx: &eframe::egui::Context) {
        let first = self.last_check.is_none();
        if self.state == State::Idle && self.last_check.is_none_or(|t| t.elapsed() > CHECK_EVERY) {
            if first {
                // a little after start, not while the window opens
                self.last_check = Some(Instant::now() - CHECK_EVERY + Duration::from_secs(5));
            } else {
                self.check();
            }
        }
        while let Ok(m) = self.rx.try_recv() {
            match m {
                Msg::Checked(Ok(s)) => {
                    self.state = State::Idle;
                    self.error = None;
                    if s.available && self.auto && self.tried != s.latest && !self.open {
                        self.tried = s.latest.clone();
                        self.notice = Some(format!("Atualizando o Ateliet em segundo plano ({} {}).", s.behind, if s.behind == 1 { "commit novo" } else { "commits novos" }));
                        self.status = Some(s);
                        self.update();
                    } else {
                        if s.available && !self.auto && self.tried != s.latest {
                            self.tried = s.latest.clone();
                            self.notice = Some("O Ateliet tem uma atualização: clique na seta no pé da barra lateral.".into());
                        }
                        self.status = Some(s);
                    }
                }
                Msg::Checked(Err(e)) => {
                    self.state = State::Idle;
                    self.error = Some(e);
                }
                Msg::Step(s) => self.step = s,
                Msg::Log(l) => {
                    self.log.push(l);
                    if self.log.len() > 400 {
                        self.log.remove(0);
                    }
                }
                Msg::Done(Ok(())) => {
                    self.state = State::Ready;
                    self.notice = Some("O Ateliet foi atualizado: reinicie para usar a versão nova.".into());
                }
                Msg::Done(Err(e)) => {
                    self.state = State::Failed;
                    self.notice = Some(format!("A atualização automática falhou: {}", e.lines().next().unwrap_or("")));
                    self.error = Some(e);
                }
            }
        }
        if matches!(self.state, State::Checking | State::Building) {
            ctx.request_repaint_after(Duration::from_millis(300));
        } else {
            ctx.request_repaint_after(Duration::from_secs(60));
        }
    }

    pub fn available(&self) -> bool {
        self.status.as_ref().is_some_and(|s| s.available) || self.state == State::Ready
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sh(dir: &Path, script: &str) {
        let ok = Command::new("sh").arg("-c").arg(script).current_dir(dir).env("GIT_AUTHOR_NAME", "t").env("GIT_AUTHOR_EMAIL", "t@t").env("GIT_COMMITTER_NAME", "t").env("GIT_COMMITTER_EMAIL", "t@t").status().unwrap().success();
        assert!(ok, "{script}");
    }

    #[test]
    fn sees_upstream_commits() {
        let base = std::env::temp_dir().join(format!("ateliet-upd-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        sh(&base, "git init -q --bare origin.git && git clone -q origin.git a 2>/dev/null && cd a && git commit -q --allow-empty -m one && git push -q origin HEAD 2>/dev/null");
        sh(&base, "git clone -q origin.git b && cd a && git commit -q --allow-empty -m two && git push -q origin HEAD 2>/dev/null");
        let built = git(&base.join("b"), &["rev-parse", "HEAD"]).unwrap();
        let s = check_in(&base.join("b"), &built).unwrap();
        assert!(s.available);
        assert_eq!(s.behind, 1);
        assert!(s.commits[0].ends_with("two"));
        assert_eq!(plain("\x1b[1mCompiling\x1b[0m x\r  Building 3/9"), "  Building 3/9");
        let _ = fs::remove_dir_all(&base);
    }
}
