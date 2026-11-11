//! Builds the Svelte viewer so `viewer.rs` can embed `apps/viewer/dist`.
//!
//! `CITY_VIEWER_PREBUILT=1` skips the build and requires `dist` to exist
//! already (the Docker image builds the viewer in its own stage).

use std::path::Path;
use std::process::Command;

const VIEWER_DIR: &str = "../../apps/viewer";

fn main() {
    for input in ["src", "index.html", "package.json", "bun.lock", "vite.config.ts"] {
        println!("cargo:rerun-if-changed={VIEWER_DIR}/{input}");
    }
    println!("cargo:rerun-if-env-changed=CITY_VIEWER_PREBUILT");

    if std::env::var_os("CITY_VIEWER_PREBUILT").is_some() {
        assert!(
            Path::new(VIEWER_DIR).join("dist/index.html").exists(),
            "CITY_VIEWER_PREBUILT is set but {VIEWER_DIR}/dist/index.html is missing"
        );
        return;
    }
    bun(&["install", "--frozen-lockfile"]);
    bun(&["run", "build"]);
}

fn bun(args: &[&str]) {
    let status = Command::new("bun")
        .args(args)
        .current_dir(VIEWER_DIR)
        .status()
        .unwrap_or_else(|err| panic!("cannot run bun (install it to build the viewer): {err}"));
    assert!(status.success(), "`bun {}` failed in {VIEWER_DIR}", args.join(" "));
}
