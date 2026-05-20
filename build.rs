//! Resolve the version string compiled into the binary.
//!
//! Precedence is an explicit `SUNSETR_VERSION` from the environment, then
//! `git describe` for source checkouts, then the Cargo manifest version for
//! release tarballs that ship without a `.git` directory.

use std::process::Command;

fn main() {
    println!("cargo:rerun-if-env-changed=SUNSETR_VERSION");

    let version = env_override()
        .or_else(git_describe)
        .unwrap_or_else(|| std::env::var("CARGO_PKG_VERSION").unwrap());

    println!("cargo:rustc-env=SUNSETR_VERSION={version}");
}

/// Honor a version pinned by a packager such as the flake or a PKGBUILD.
fn env_override() -> Option<String> {
    match std::env::var("SUNSETR_VERSION") {
        Ok(v) if !v.trim().is_empty() => Some(v.trim().to_string()),
        _ => None,
    }
}

/// Yields the nearest tag plus the commit count and short hash since
/// it, a `-dirty` marker for a modified tree, and no leading `v`.
/// Returns `None` when git is unavailable or `.git` is absent.
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

    track_git_head();

    Some(described.strip_prefix('v').unwrap_or(described).to_string())
}

/// Rebuild when HEAD or the branch it points at moves so the embedded
/// version stays in step with the checkout.
fn track_git_head() {
    let head = std::path::Path::new(".git/HEAD");
    if !head.exists() {
        return;
    }

    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/packed-refs");

    if let Ok(contents) = std::fs::read_to_string(head)
        && let Some(reference) = contents.strip_prefix("ref: ")
    {
        println!("cargo:rerun-if-changed=.git/{}", reference.trim());
    }
}
