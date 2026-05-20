//! Resolve the version string compiled into the binary.
//!
//! Precedence is an explicit `SUNSETR_VERSION` from the environment, then
//! `git describe` for source checkouts, then the Cargo manifest version for
//! release tarballs that ship without a `.git` directory.

use std::process::Command;

fn main() {
    println!("cargo:rerun-if-env-changed=SUNSETR_VERSION");
    println!("cargo:rerun-if-changed=.git/HEAD");

    let version = env_override()
        .or_else(git_describe)
        .unwrap_or_else(|| std::env::var("CARGO_PKG_VERSION").unwrap());

    println!("cargo:rustc-env=SUNSETR_VERSION={version}");
}

fn env_override() -> Option<String> {
    match std::env::var("SUNSETR_VERSION") {
        Ok(v) if !v.trim().is_empty() => Some(v.trim().to_string()),
        _ => None,
    }
}

fn git_describe() -> Option<String> {
    let output = Command::new("git")
        .args(["describe", "--tags", "--always", "--dirty"])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let described = String::from_utf8(output.stdout).ok()?;
    let described = described.trim();
    if described.is_empty() {
        return None;
    }

    Some(described.strip_prefix('v').unwrap_or(described).to_string())
}
