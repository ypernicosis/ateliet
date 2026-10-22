use std::path::Path;
use std::process::Command;

fn main() {
    // The self-updater (src/update.rs) compares the commit this binary was
    // built from with the upstream branch of the same checkout, and rebuilds
    // with the same features.
    let dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let root = Path::new(&dir).parent().map(|p| p.to_path_buf()).unwrap_or_else(|| Path::new(&dir).to_path_buf());
    let commit = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(&root)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    println!("cargo:rustc-env=ATELIET_GIT_COMMIT={commit}");
    println!("cargo:rustc-env=ATELIET_SOURCE_DIR={}", root.display());
    let features: Vec<String> = ["vision", "vision-dynamic", "audio", "clip"]
        .iter()
        .filter(|f| std::env::var_os(format!("CARGO_FEATURE_{}", f.to_uppercase().replace('-', "_"))).is_some())
        .map(|f| f.to_string())
        .collect();
    println!("cargo:rustc-env=ATELIET_FEATURES={}", features.join(","));
    for p in [".git/HEAD", ".git/refs/heads", ".git/packed-refs"] {
        if root.join(p).exists() {
            println!("cargo:rerun-if-changed={}", root.join(p).display());
        }
    }
}
