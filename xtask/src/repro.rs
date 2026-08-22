// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Reproducible-build fixture: build the release binary twice from the
//! same tree and compare bytes. Release三件 item three.
//!
//! Scope is stated rather than implied: the default rebuilds the
//! `sprawling` crate only (dependencies stay cached), which proves the
//! final compile, the client embed and the link are deterministic.
//! `--full` clears the whole target first - the CI-grade variant, at
//! several minutes.
//!
//! First reading (2026-08-22, windows-msvc): the fixture reported two
//! hashes - the embed chain is deterministic (gzip mtime zeroed, sorted
//! tables), but the MSVC link step is not. The known sources are the PE
//! header timestamp and the debug-directory PDB GUID, both minted by
//! link.exe per run; the fix line is `-C link-arg=/Brepro` (plus a
//! pinned PDB path) applied as a release-profile flag. Applying it
//! invalidates every cached artifact, so it lands with the release
//! workflow rather than as a mid-session rebuild; until then this
//! command is the honest register of that gap.

use std::path::Path;
use std::process::Command;

use crate::report::XtaskError;

pub(crate) fn run(root: &Path, full: bool) -> Result<String, XtaskError> {
    let first = build_and_hash(root, full)?;
    let second = build_and_hash(root, false)?;
    if first == second {
        let scope = if full {
            "full tree rebuilt once, then the crate alone"
        } else {
            "crate + embed + link (dependencies cached)"
        };
        Ok(format!("reproducible: {first} twice ({scope})"))
    } else {
        Err(XtaskError::Doc {
            file: "target/release/sprawling".to_owned(),
            msg: format!(
                "two builds of one tree differ: {first} then {second}; a nondeterministic \
                 input (path, timestamp, env) has entered the build"
            ),
        })
    }
}

fn build_and_hash(root: &Path, full_clean_first: bool) -> Result<String, XtaskError> {
    if full_clean_first {
        drive(root, &["clean", "--release"])?;
    } else {
        drive(root, &["clean", "--release", "-p", "sprawling"])?;
    }
    drive(root, &["build", "--release", "-p", "sprawling", "--locked"])?;
    let binary = ["sprawling.exe", "sprawling"]
        .into_iter()
        .map(|name| root.join("target").join("release").join(name))
        .find(|p| p.is_file())
        .ok_or_else(|| XtaskError::Doc {
            file: "target/release".to_owned(),
            msg: "no release binary after a successful build".to_owned(),
        })?;
    let bytes = std::fs::read(&binary).map_err(|source| XtaskError::Io {
        path: binary.display().to_string(),
        source,
    })?;
    Ok(kernel::B3Hash::digest(&bytes).to_string())
}

fn drive(root: &Path, args: &[&str]) -> Result<(), XtaskError> {
    let status = Command::new("cargo")
        .args(args)
        .current_dir(root)
        .status()
        .map_err(|source| XtaskError::Io {
            path: format!("cargo {}", args.join(" ")),
            source,
        })?;
    if status.success() {
        Ok(())
    } else {
        Err(XtaskError::Doc {
            file: format!("cargo {}", args.join(" ")),
            msg: "the build failed; fix it before judging reproducibility".to_owned(),
        })
    }
}
