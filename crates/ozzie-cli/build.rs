use std::process::Command;

fn main() {
    // Priority: OZZIE_VERSION env var (set by CI from git tag)
    // Fallback: Cargo.toml version + git short hash
    let version = std::env::var("OZZIE_VERSION").unwrap_or_else(|_| {
        let pkg_version = env!("CARGO_PKG_VERSION");
        let commit = Command::new("git")
            .args(["rev-parse", "--short", "HEAD"])
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|| "unknown".to_string());
        format!("{pkg_version}-dev+{commit}")
    });

    println!("cargo:rustc-env=OZZIE_VERSION={version}");

    // Rebuild only when these change
    println!("cargo:rerun-if-env-changed=OZZIE_VERSION");
    println!("cargo:rerun-if-changed=build.rs");
}
