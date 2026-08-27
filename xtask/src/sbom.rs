// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Copyright (c) 2026 2youg1

//! SBOM writer: a CycloneDX bill of materials from `cargo metadata`,
//! deterministic (components sorted by name@version) so two runs over
//! one lockfile produce identical bytes. Release三件 item two's file
//! half; the embedded half is the dependency list build.rs bakes into
//! the binary (`sprawling status --deps`).

use std::path::Path;
use std::process::Command;

use crate::report::XtaskError;

pub(crate) fn run(root: &Path) -> Result<String, XtaskError> {
    let out = Command::new("cargo")
        .args(["metadata", "--format-version", "1", "--locked"])
        .current_dir(root)
        .output()
        .map_err(|source| XtaskError::Io {
            path: "cargo metadata".to_owned(),
            source,
        })?;
    if !out.status.success() {
        return Err(XtaskError::Doc {
            file: "cargo metadata".to_owned(),
            msg: String::from_utf8_lossy(&out.stderr).into_owned(),
        });
    }
    let metadata: serde_json::Value =
        serde_json::from_slice(&out.stdout).map_err(|err| XtaskError::Doc {
            file: "cargo metadata".to_owned(),
            msg: err.to_string(),
        })?;
    // Read by key rather than by index throughout: `Value`'s `Index` impl
    // panics on a shape that does not match, and this reads a tool's
    // output rather than a value this crate constructed.
    let field = |value: &serde_json::Value, key: &str| value.get(key).cloned();
    let workspace: Vec<String> = field(&metadata, "workspace_members")
        .as_ref()
        .and_then(serde_json::Value::as_array)
        .map(|ids| {
            ids.iter()
                .filter_map(|id| id.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    let packages = field(&metadata, "packages")
        .and_then(|value| value.as_array().cloned())
        .unwrap_or_default();
    let mut components: Vec<(String, serde_json::Value)> = Vec::new();
    for package in &packages {
        let text = |key: &str| {
            package
                .get(key)
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_owned()
        };
        if workspace.contains(&text("id")) {
            continue; // the workspace is the application, not a component
        }
        let (name, version) = (text("name"), text("version"));
        let key = format!("{name}@{version}");
        let mut component = serde_json::json!({
            "type": "library",
            "name": name,
            "version": version,
            "purl": format!("pkg:cargo/{key}"),
        });
        let license = package.get("license").and_then(serde_json::Value::as_str);
        if let (Some(license), Some(fields)) = (license, component.as_object_mut()) {
            fields.insert(
                "licenses".to_owned(),
                serde_json::json!([{ "expression": license }]),
            );
        }
        components.push((key, component));
    }
    components.sort_by(|a, b| a.0.cmp(&b.0));
    components.dedup_by(|a, b| a.0 == b.0);

    let version = packages
        .iter()
        .find(|package| {
            package.get("name").and_then(serde_json::Value::as_str) == Some("sprawling")
        })
        .and_then(|package| package.get("version"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("0.0.0")
        .to_owned();
    let count = components.len();
    let bom = serde_json::json!({
        "bomFormat": "CycloneDX",
        "specVersion": "1.5",
        "version": 1,
        "metadata": {
            "component": {
                "type": "application",
                "name": "sprawling",
                "version": version,
                "licenses": [{ "expression": "MPL-2.0" }],
            },
        },
        "components": components.into_iter().map(|(_, c)| c).collect::<Vec<_>>(),
    });
    let path = root.join("target").join("sbom.cdx.json");
    let text = format!(
        "{}\n",
        serde_json::to_string_pretty(&bom).unwrap_or_default()
    );
    std::fs::write(&path, text).map_err(|source| XtaskError::Io {
        path: path.display().to_string(),
        source,
    })?;
    Ok(format!("sbom: {count} component(s) -> {}", path.display()))
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "test code"
)]
mod tests {
    use super::*;

    #[test]
    fn the_bom_is_written_and_deterministic() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        run(root).unwrap();
        let first = std::fs::read(root.join("target").join("sbom.cdx.json")).unwrap();
        run(root).unwrap();
        let second = std::fs::read(root.join("target").join("sbom.cdx.json")).unwrap();
        assert_eq!(first, second, "same lockfile, same bytes");
        let parsed: serde_json::Value = serde_json::from_slice(&first).unwrap();
        assert_eq!(parsed["bomFormat"], "CycloneDX");
        assert!(
            parsed["components"].as_array().unwrap().len() > 50,
            "the tree is not this small"
        );
    }
}
