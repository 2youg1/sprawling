// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Copyright (c) 2026 2youg1

//! The deliverable: the one archive a person downloads, unpacks and runs.
//!
//! A release somebody assembled by hand is a release nobody can check.
//! This assembles the same archive on every platform, out of artifacts the
//! gates have already weighed, and refuses a binary whose client is only
//! the page shell - the defect that survives a green build and reaches the
//! person as an empty browser window.
//!
//! Entry timestamps are fixed rather than taken from the file system, so
//! two builds of one tree produce the same archive bytes.

use std::io::Write as _;
use std::path::{Path, PathBuf};

use crate::report::XtaskError;

pub(crate) fn run(root: &Path) -> Result<String, XtaskError> {
    let binary = crate::budget::binary_path(root).ok_or_else(|| XtaskError::Cmd {
        cmd: "package".to_owned(),
        msg: "no release binary in target/release; run `just dist` first".to_owned(),
    })?;
    if !crate::budget::carries_client(&binary)? {
        return Err(XtaskError::Cmd {
            cmd: "package".to_owned(),
            msg: "this binary carries the page shell only and would serve an empty page; \
                  run `just build-web`, then `just dist`"
                .to_owned(),
        });
    }

    let stem = format!("sprawling-{}-{}", version(root)?, platform());
    let out_dir = root.join("target").join("package");
    std::fs::create_dir_all(&out_dir).map_err(|source| XtaskError::Io {
        path: out_dir.display().to_string(),
        source,
    })?;
    let archive = out_dir.join(format!("{stem}.zip"));

    let mut entries: Vec<(String, PathBuf)> = Vec::new();
    let binary_name = if cfg!(windows) {
        "sprawling.exe"
    } else {
        "sprawling"
    };
    entries.push((binary_name.to_owned(), binary));
    entries.push((
        "QUICKSTART.md".to_owned(),
        root.join("dist").join("QUICKSTART.md"),
    ));
    entries.push(("LICENSE".to_owned(), root.join("LICENSE")));
    // The bill of materials when `just dist` produced one; a person who
    // wants to know what is inside the binary should not have to build it.
    let sbom = root.join("target").join("sbom.cdx.json");
    if sbom.is_file() {
        entries.push(("sbom.cdx.json".to_owned(), sbom));
    }

    write_archive(&archive, &stem, &entries)?;
    let size = std::fs::metadata(&archive)
        .map(|meta| meta.len())
        .map_err(|source| XtaskError::Io {
            path: archive.display().to_string(),
            source,
        })?;
    Ok(format!(
        "packaged {} ({size} bytes, {} entries)\n",
        archive.display(),
        entries.len()
    ))
}

/// Writes the archive with every entry under one directory, so unpacking
/// it produces a folder rather than scattering files where it landed.
fn write_archive(
    archive: &Path,
    stem: &str,
    entries: &[(String, PathBuf)],
) -> Result<(), XtaskError> {
    let file = std::fs::File::create(archive).map_err(|source| XtaskError::Io {
        path: archive.display().to_string(),
        source,
    })?;
    let mut zip = zip::ZipWriter::new(file);
    let io = |path: &Path, source: std::io::Error| XtaskError::Io {
        path: path.display().to_string(),
        source,
    };
    for (name, source) in entries {
        let bytes = std::fs::read(source).map_err(|err| io(source, err))?;
        // Executable bits: the archive is the only thing that carries them
        // to a machine that has never seen this file, and a binary that
        // arrives without them is a binary nobody can run.
        let mode = if name == "sprawling" { 0o755 } else { 0o644 };
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .last_modified_time(zip::DateTime::default())
            .unix_permissions(mode);
        zip.start_file(format!("{stem}/{name}"), options)
            .map_err(|err| XtaskError::Cmd {
                cmd: "package".to_owned(),
                msg: format!("{name}: {err}"),
            })?;
        zip.write_all(&bytes).map_err(|err| io(archive, err))?;
    }
    zip.finish().map_err(|err| XtaskError::Cmd {
        cmd: "package".to_owned(),
        msg: err.to_string(),
    })?;
    Ok(())
}

/// The workspace version, read from the manifest that defines it rather
/// than from this tool's own compiled-in copy.
fn version(root: &Path) -> Result<String, XtaskError> {
    let path = root.join("Cargo.toml");
    let text = std::fs::read_to_string(&path).map_err(|source| XtaskError::Io {
        path: path.display().to_string(),
        source,
    })?;
    let parsed: toml::Value = toml::from_str(&text).map_err(|err| XtaskError::Doc {
        file: path.display().to_string(),
        msg: err.to_string(),
    })?;
    parsed
        .get("workspace")
        .and_then(|w| w.get("package"))
        .and_then(|p| p.get("version"))
        .and_then(toml::Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| XtaskError::Doc {
            file: path.display().to_string(),
            msg: "workspace.package.version is missing".to_owned(),
        })
}

/// What a person needs to recognise in a list of assets: the system the
/// binary runs on, in the words that system is known by.
fn platform() -> String {
    format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "test code"
)]
mod tests {
    use super::{platform, write_archive};

    /// The archive nests under one directory: unpacking it into a folder
    /// full of other things must not scatter six files across it.
    #[test]
    fn every_entry_sits_under_one_directory() {
        let dir = std::env::temp_dir().join("sprawling-package-test");
        std::fs::create_dir_all(&dir).unwrap();
        let source = dir.join("payload.txt");
        std::fs::write(&source, b"payload").unwrap();
        let archive = dir.join("out.zip");
        write_archive(
            &archive,
            "sprawling-9.9.9-test",
            &[("QUICKSTART.md".to_owned(), source)],
        )
        .unwrap();

        let bytes = std::fs::read(&archive).unwrap();
        let listing = String::from_utf8_lossy(&bytes);
        assert!(
            listing.contains("sprawling-9.9.9-test/QUICKSTART.md"),
            "entry is not under the archive's own directory"
        );
    }

    /// Two runs over one tree produce one archive: a release that differs
    /// from its own rebuild cannot be checked against its source.
    #[test]
    fn two_runs_over_the_same_files_produce_the_same_bytes() {
        let dir = std::env::temp_dir().join("sprawling-package-repro");
        std::fs::create_dir_all(&dir).unwrap();
        let source = dir.join("payload.txt");
        std::fs::write(&source, b"payload").unwrap();
        let entries = [("QUICKSTART.md".to_owned(), source)];

        let first = dir.join("first.zip");
        let second = dir.join("second.zip");
        write_archive(&first, "sprawling-9.9.9-test", &entries).unwrap();
        write_archive(&second, "sprawling-9.9.9-test", &entries).unwrap();
        assert_eq!(
            std::fs::read(&first).unwrap(),
            std::fs::read(&second).unwrap()
        );
    }

    #[test]
    fn the_platform_names_both_the_system_and_the_architecture() {
        let named = platform();
        assert!(named.contains('-'), "platform: {named}");
        assert!(!named.starts_with('-'), "platform: {named}");
    }
}
