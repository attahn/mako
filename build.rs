fn main() {
    // Optional short git hash for `mako version -v`.
    // Prefer explicit env (CI), else best-effort `git rev-parse`.
    if let Ok(h) = std::env::var("MAKO_GIT_HASH") {
        if !h.trim().is_empty() {
            println!("cargo:rustc-env=MAKO_GIT_HASH={}", h.trim());
        }
    } else if let Ok(out) = std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
    {
        if out.status.success() {
            let hash = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !hash.is_empty() {
                println!("cargo:rustc-env=MAKO_GIT_HASH={hash}");
            }
        }
    }
    println!("cargo:rerun-if-env-changed=MAKO_GIT_HASH");
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=Cargo.toml");
}
