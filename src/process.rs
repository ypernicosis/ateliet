//! `child_process` for plugins: spawn/exec with piped stdio, streamed to the
//! window as `proc` events. Plugins in Obsidian have the same power (Node).

use serde::Serialize;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use tauri::Emitter;

#[derive(Default)]
pub struct Procs {
    next: AtomicU32,
    stdin: Mutex<HashMap<u32, ChildStdin>>,
    children: Arc<Mutex<HashMap<u32, Child>>>,
}

#[derive(Serialize, Clone)]
struct ProcEvent {
    id: u32,
    kind: &'static str,
    data: Option<String>,
    code: Option<i32>,
}

/// GUI apps on macOS start with a minimal PATH; add the usual tool locations.
pub fn augmented_path() -> String {
    let base = std::env::var("PATH").unwrap_or_default();
    let mut extra = vec!["/opt/homebrew/bin", "/opt/homebrew/sbin", "/usr/local/bin", "/usr/bin", "/bin", "/usr/sbin", "/sbin"]
        .into_iter()
        .map(String::from)
        .collect::<Vec<_>>();
    if let Some(h) = dirs::home_dir() {
        extra.push(h.join(".cargo/bin").to_string_lossy().into_owned());
        extra.push(h.join(".local/bin").to_string_lossy().into_owned());
    }
    let sep = if cfg!(windows) { ";" } else { ":" };
    let mut parts: Vec<String> = base.split(sep).filter(|s| !s.is_empty()).map(String::from).collect();
    for e in extra {
        if !parts.contains(&e) {
            parts.push(e);
        }
    }
    parts.join(sep)
}

pub fn build_command(cmd: &str, args: &[String], shell: bool, cwd: Option<&str>, env: Option<&HashMap<String, String>>) -> Command {
    let mut c = if shell {
        let full = if args.is_empty() { cmd.to_string() } else { format!("{cmd} {}", args.join(" ")) };
        if cfg!(windows) {
            let mut c = Command::new("cmd");
            c.arg("/C").arg(full);
            c
        } else {
            let mut c = Command::new("/bin/sh");
            c.arg("-c").arg(full);
            c
        }
    } else {
        let mut c = Command::new(cmd);
        c.args(args);
        c
    };
    c.env("PATH", augmented_path());
    if let Some(e) = env {
        c.envs(e);
    }
    if let Some(d) = cwd.filter(|d| !d.is_empty()) {
        c.current_dir(d);
    }
    c
}

#[tauri::command]
pub fn proc_spawn(
    window: tauri::Window,
    state: tauri::State<'_, Procs>,
    cmd: String,
    args: Vec<String>,
    cwd: Option<String>,
    env: Option<HashMap<String, String>>,
    shell: bool,
) -> Result<(u32, u32), String> {
    let mut child = build_command(&cmd, &args, shell, cwd.as_deref(), env.as_ref())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("spawn {cmd}: {e}"))?;
    let id = state.next.fetch_add(1, Ordering::SeqCst) + 1;
    let pid = child.id();
    let label = window.label().to_string();
    for (kind, stream) in [("stdout", child.stdout.take().map(|s| Box::new(s) as Box<dyn Read + Send>)), ("stderr", child.stderr.take().map(|s| Box::new(s) as Box<dyn Read + Send>))] {
        let Some(mut stream) = stream else { continue };
        let w = window.clone();
        let label = label.clone();
        std::thread::spawn(move || {
            let mut buf = [0u8; 8192];
            loop {
                match stream.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        let data = String::from_utf8_lossy(&buf[..n]).into_owned();
                        let _ = w.emit_to(label.as_str(), "proc", ProcEvent { id, kind, data: Some(data), code: None });
                    }
                }
            }
        });
    }
    if let Some(stdin) = child.stdin.take() {
        state.stdin.lock().unwrap().insert(id, stdin);
    }
    state.children.lock().unwrap().insert(id, child);
    let children = state.children.clone();
    let w = window.clone();
    std::thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_millis(50));
        let mut map = children.lock().unwrap();
        let Some(c) = map.get_mut(&id) else { break };
        if let Ok(Some(status)) = c.try_wait() {
            map.remove(&id);
            drop(map);
            // give the reader threads a moment to flush
            std::thread::sleep(std::time::Duration::from_millis(30));
            let _ = w.emit_to(label.as_str(), "proc", ProcEvent { id, kind: "exit", data: None, code: status.code() });
            break;
        }
    });
    Ok((id, pid))
}

#[tauri::command]
pub fn proc_write(state: tauri::State<'_, Procs>, id: u32, data: String) -> Result<(), String> {
    let mut m = state.stdin.lock().unwrap();
    let s = m.get_mut(&id).ok_or("stdin closed")?;
    s.write_all(data.as_bytes()).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn proc_close_stdin(state: tauri::State<'_, Procs>, id: u32) {
    state.stdin.lock().unwrap().remove(&id);
}

#[tauri::command]
pub fn proc_kill(state: tauri::State<'_, Procs>, id: u32) {
    if let Some(c) = state.children.lock().unwrap().get_mut(&id) {
        let _ = c.kill();
    }
}

/// Synchronous exec (for `execSync`), used by the protocol handler.
pub fn exec_sync(cmd: &str, cwd: Option<&str>, input: &[u8]) -> serde_json::Value {
    let child = build_command(cmd, &[], true, cwd, None).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn();
    match child {
        Err(e) => serde_json::json!({ "error": e.to_string(), "status": 127 }),
        Ok(mut c) => {
            if let Some(mut si) = c.stdin.take() {
                let _ = si.write_all(input);
            }
            match c.wait_with_output() {
                Ok(o) => serde_json::json!({
                    "stdout": String::from_utf8_lossy(&o.stdout),
                    "stderr": String::from_utf8_lossy(&o.stderr),
                    "status": o.status.code(),
                }),
                Err(e) => serde_json::json!({ "error": e.to_string(), "status": 1 }),
            }
        }
    }
}
