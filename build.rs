use std::path::Path;
use std::process::Command;

fn main() {
    // The self-updater (src/update.rs) compares the commit this binary was
    // built from with the upstream branch of the same checkout, and rebuilds
    // with the same features.
    let dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let root = Path::new(&dir).to_path_buf();
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
    link_clang_rt();
}

// The prebuilt ONNX Runtime objects (CoreML provider) use `@available`, which
// compiles to calls to `__isPlatformVersionAtLeast` from clang's compiler-rt.
// rustc links with `-nodefaultlibs`, so that runtime is never pulled in and
// the link fails with an undefined symbol. Passing the archive as the last
// link argument fills only what is still missing.
fn link_clang_rt() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") || std::env::var_os("CARGO_FEATURE_VISION").is_none() {
        return;
    }
    let cc = std::env::var("CC").unwrap_or_else(|_| "cc".into());
    let Some(dir) = Command::new(cc)
        .arg("-print-resource-dir")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
    else {
        println!("cargo:warning=clang resource dir not found; compiler-rt not linked");
        return;
    };
    let lib = Path::new(&dir).join("lib/darwin/libclang_rt.osx.a");
    if lib.exists() {
        println!("cargo:rustc-link-arg={}", lib.display());
    } else {
        println!("cargo:warning={} not found; compiler-rt not linked", lib.display());
    }
}
