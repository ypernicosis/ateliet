use std::path::Path;
use std::process::Command;

fn main() {
    // The self-updater (src/update.rs) compares the commit this binary was
    // built from with the upstream branch of the same checkout.
    let dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let commit = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(&dir)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    println!("cargo:rustc-env=ATELIET_GIT_COMMIT={commit}");
    println!("cargo:rustc-env=ATELIET_SOURCE_DIR={dir}");
    for p in [".git/HEAD", ".git/refs/heads", ".git/packed-refs"] {
        if Path::new(&dir).join(p).exists() {
            println!("cargo:rerun-if-changed={p}");
        }
    }
    tauri_build::build()
}
