// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Copyright (c) 2026 2youg1

//! The shared shrink primitive: lossy-but-restorable,
//! owned here and nowhere else. Four invariants, each enforced in code:
//! store-before-cut; substitute (hint included) never larger than the
//! original nor the cap; only lossy transforms store; the substitute
//! always carries a materialized read-only rest path. CAS dedup makes
//! repeated offloads of the same bytes idempotent.

use std::path::{Path, PathBuf};

use kernel::{AxCode, AxError, Locator};
use memory::Cas;

/// Where offloaded bytes live: the CAS for the original, the run's
/// environment directory for the materialized rest file.
pub struct OffloadSite<'a> {
    pub cas: &'a mut Cas,
    pub environment: &'a Path,
}

/// The outcome: a substitute that fits the cap, the original pinned in
/// CAS, and a read-only path the model can keep reading from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OffloadRecord {
    pub substitute: Vec<u8>,
    pub original: Locator,
    pub rest_path: PathBuf,
    pub original_len: u64,
}

fn hint_line(total: u64, rest_path: &Path, locator: &Locator) -> String {
    format!(
        "\n[offloaded: total {total} bytes; rest at {}; original {locator}]",
        rest_path.display()
    )
}

fn rest_file_name(locator: &Locator) -> String {
    let mut tail: String = locator
        .to_string()
        .chars()
        .rev()
        .take(16)
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    tail.retain(|c| c.is_ascii_alphanumeric());
    format!("rest-{tail}.dat")
}

fn materialize(bytes: &[u8], path: &Path) -> Result<(), AxError> {
    let io = |err: std::io::Error| {
        AxError::failure(
            AxCode::StorageFatal,
            "materialize rest file",
            err.to_string(),
        )
    };
    if path.exists() {
        return Ok(());
    }
    std::fs::write(path, bytes).map_err(io)?;
    let mut perms = std::fs::metadata(path).map_err(io)?.permissions();
    perms.set_readonly(true);
    std::fs::set_permissions(path, perms).map_err(io)?;
    Ok(())
}

/// Shrinks `bytes` to at most `cap_bytes`. Callers only come here for
/// lossy cases (`len > cap`); a lossless call is refused — invariant 3
/// says only lossy transforms may store.
pub fn offload(
    bytes: &[u8],
    cap_bytes: u64,
    site: &mut OffloadSite<'_>,
) -> Result<OffloadRecord, AxError> {
    let original_len = u64::try_from(bytes.len()).map_err(|_| {
        AxError::failure(AxCode::InvalidArgs, "offload result", "length exceeds u64")
    })?;
    if original_len <= cap_bytes {
        return Err(AxError::failure(
            AxCode::InvalidArgs,
            "offload result",
            "lossless input: only lossy transforms store (invariant 3)",
        ));
    }
    // Invariant 1: the full original enters the CAS before any cut.
    let hash = site.cas.put(bytes).map_err(memory::MemoryError::into_ax)?;
    let original = Locator::parse(&format!("cas:b3-{hash}"))?;
    let rest_path = site.environment.join(rest_file_name(&original));
    // Invariant 4: the substitute always points at a readable rest file.
    materialize(bytes, &rest_path)?;
    let hint = hint_line(original_len, &rest_path, &original);
    let hint_len = u64::try_from(hint.len())
        .map_err(|_| AxError::failure(AxCode::InvalidArgs, "offload result", "hint exceeds u64"))?;
    let head_budget = cap_bytes.checked_sub(hint_len).ok_or_else(|| {
        AxError::failure(
            AxCode::InvalidArgs,
            "offload result",
            format!("cap {cap_bytes} cannot even hold the {hint_len}-byte hint"),
        )
        .with_recovery("raise the cap or skip offload for this result")
    })?;
    let head_len = usize::try_from(head_budget.min(original_len)).map_err(|_| {
        AxError::failure(AxCode::InvalidArgs, "offload result", "head exceeds usize")
    })?;
    let mut substitute = bytes.get(..head_len).unwrap_or_default().to_vec();
    substitute.extend_from_slice(hint.as_bytes());
    // Invariant 2: substitute (hint included) within both bounds.
    let substitute_len = u64::try_from(substitute.len()).map_err(|_| {
        AxError::failure(
            AxCode::InvalidArgs,
            "offload result",
            "substitute exceeds u64",
        )
    })?;
    if substitute_len > cap_bytes || substitute_len > original_len {
        return Err(AxError::failure(
            AxCode::InvalidArgs,
            "offload result",
            "substitute outgrew its bounds (invariant 2)",
        ));
    }
    Ok(OffloadRecord {
        substitute,
        original,
        rest_path,
        original_len,
    })
}

/// Restores the rest file from the CAS after external cleanup; the bytes
/// are the original, verbatim (A7's third assertion).
pub fn rematerialize(locator: &Locator, site: &mut OffloadSite<'_>) -> Result<PathBuf, AxError> {
    let Locator::Cas { hash, .. } = locator else {
        return Err(AxError::failure(
            AxCode::InvalidArgs,
            "rematerialize rest file",
            "locator is not cas-addressed",
        ));
    };
    let bytes = site.cas.get(hash).map_err(memory::MemoryError::into_ax)?;
    let rest_path = site.environment.join(rest_file_name(locator));
    materialize(&bytes, &rest_path)?;
    Ok(rest_path)
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "test code"
)]
mod tests {
    use super::*;

    fn site(dir: &tempfile::TempDir) -> (Cas, PathBuf) {
        let cas = Cas::open(&dir.path().join("cas")).unwrap();
        let env = dir.path().join("env");
        std::fs::create_dir_all(&env).unwrap();
        (cas, env)
    }

    #[test]
    fn a7_roundtrip_all_four_assertions() {
        let dir = tempfile::tempdir().unwrap();
        let (mut cas, env) = site(&dir);
        let mut s = OffloadSite {
            cas: &mut cas,
            environment: &env,
        };
        let original: Vec<u8> = (0..40_000u32)
            .map(|i| u8::try_from(i % 251).unwrap())
            .collect();
        let record = offload(&original, 4_096, &mut s).unwrap();
        // Substitute within both bounds, hint included.
        assert!(record.substitute.len() <= 4_096);
        assert!(record.substitute.len() < original.len());
        assert!(
            String::from_utf8_lossy(&record.substitute).contains("[offloaded: total 40000 bytes")
        );
        // The rest path serves the full original.
        assert_eq!(std::fs::read(&record.rest_path).unwrap(), original);
        // The CAS serves the full original by locator.
        let Locator::Cas { hash, .. } = &record.original else {
            panic!("expected a cas locator");
        };
        assert_eq!(s.cas.get(hash).unwrap(), original);
        // External cleanup, then rematerialize: bytes identical.
        let mut perms = std::fs::metadata(&record.rest_path).unwrap().permissions();
        #[allow(clippy::permissions_set_readonly_false, reason = "test cleanup")]
        perms.set_readonly(false);
        std::fs::set_permissions(&record.rest_path, perms).unwrap();
        std::fs::remove_file(&record.rest_path).unwrap();
        let back = rematerialize(&record.original, &mut s).unwrap();
        assert_eq!(std::fs::read(&back).unwrap(), original);
    }

    #[test]
    fn same_bytes_offload_to_the_same_locator() {
        let dir = tempfile::tempdir().unwrap();
        let (mut cas, env) = site(&dir);
        let mut s = OffloadSite {
            cas: &mut cas,
            environment: &env,
        };
        let original = vec![7u8; 30_000];
        let one = offload(&original, 2_048, &mut s).unwrap();
        let two = offload(&original, 2_048, &mut s).unwrap();
        assert_eq!(
            one.original, two.original,
            "CAS dedup makes offload idempotent"
        );
        assert_eq!(one.rest_path, two.rest_path);
    }

    #[test]
    fn lossless_input_and_hopeless_caps_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let (mut cas, env) = site(&dir);
        let mut s = OffloadSite {
            cas: &mut cas,
            environment: &env,
        };
        let err = offload(b"small", 4_096, &mut s).unwrap_err();
        assert_eq!(*err.code(), AxCode::InvalidArgs);
        let big = vec![1u8; 20_000];
        let err = offload(&big, 16, &mut s).unwrap_err();
        assert!(err.subject().contains("hint"));
    }
}
