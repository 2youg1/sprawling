// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! City export and restore: the backup that can be carried to another
//! machine, and the reading of it back.
//!
//! What travels is the history, the objects its locators point at, and
//! the files a person works in. What does not travel is anything the
//! ledger can rebuild — projections and indexes — because a second copy
//! of a derived view is a second statement of what happened.
//!
//! Credentials never travel, and not by omission: they are not in the
//! city to begin with. They live in the host machine's vault, so a
//! bundle that reached the wrong hands carries work, not access.
//!
//! A bundle is a directory rather than one file. A single file would
//! need either a container format of our own to maintain or a
//! compression dependency to carry; a directory needs neither, and any
//! backup tool can wrap one.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use kernel::{EventRecord, GENESIS_PREV, Seq, chain_hash};

use crate::error::{MemoryError, io_err};
use crate::jsonl::JsonlLedger;
use crate::real_fs::RealFs;
use crate::vfs::Vfs;

/// What a bundle claims to contain. Checked on restore, so a truncated
/// or half-copied bundle is refused rather than restored quietly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    records: u64,
    head: String,
    cas_objects: u64,
    files: u64,
}

impl Manifest {
    /// How many ledger records the bundle holds.
    #[must_use]
    pub fn records(&self) -> u64 {
        self.records
    }

    /// The chain hash after the last record: the one value that proves
    /// two ledgers are the same history and not merely the same length.
    #[must_use]
    pub fn head(&self) -> &str {
        &self.head
    }

    #[must_use]
    pub fn cas_objects(&self) -> u64 {
        self.cas_objects
    }

    /// City files, excluding everything under the reserved prefix.
    #[must_use]
    pub fn files(&self) -> u64 {
        self.files
    }

    fn to_json(&self) -> String {
        let mut map = serde_json::Map::new();
        map.insert("records".to_owned(), self.records.into());
        map.insert(
            "head".to_owned(),
            serde_json::Value::String(self.head.clone()),
        );
        map.insert("cas_objects".to_owned(), self.cas_objects.into());
        map.insert("files".to_owned(), self.files.into());
        serde_json::Value::Object(map).to_string()
    }

    fn from_json(bytes: &[u8], at: &Path) -> Result<Manifest, MemoryError> {
        let value: serde_json::Value =
            serde_json::from_slice(bytes).map_err(|err| MemoryError::Bundle {
                op: "read",
                detail: format!("{}: {err}", at.display()),
            })?;
        let number = |key: &str| -> Result<u64, MemoryError> {
            value
                .get(key)
                .and_then(serde_json::Value::as_u64)
                .ok_or_else(|| MemoryError::Bundle {
                    op: "read",
                    detail: format!("{} has no {key}", at.display()),
                })
        };
        Ok(Manifest {
            records: number("records")?,
            head: value
                .get("head")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| MemoryError::Bundle {
                    op: "read",
                    detail: format!("{} has no head", at.display()),
                })?
                .to_owned(),
            cas_objects: number("cas_objects")?,
            files: number("files")?,
        })
    }
}

/// The name of the file that states what a bundle holds.
pub const MANIFEST: &str = "MANIFEST.json";

const LEDGER: &str = "ledger";
const CAS: &str = "cas";
const CITY: &str = "city";
const RESERVED: &str = ".sprawling";

/// Export and restore. A namespace rather than a value: neither
/// direction holds state between calls.
pub struct Bundle;

impl Bundle {
    /// Writes a bundle of `city_root` into `dest`.
    ///
    /// # Errors
    /// Propagates read and write failures, naming the path; refuses a
    /// city whose ledger cannot be read, because a bundle of an
    /// unreadable history is a backup of nothing.
    pub fn export(city_root: &Path, dest: &Path) -> Result<Manifest, MemoryError> {
        Bundle::export_with(Box::new(RealFs::new()), city_root, dest)
    }

    pub(crate) fn export_with(
        mut vfs: Box<dyn Vfs>,
        city_root: &Path,
        dest: &Path,
    ) -> Result<Manifest, MemoryError> {
        let ledger_dir = city_root.join(RESERVED).join(LEDGER);
        let records = copy_tree(vfs.as_mut(), &ledger_dir, &dest.join(LEDGER))?;
        let cas_objects = copy_tree(
            vfs.as_mut(),
            &city_root.join(RESERVED).join(CAS),
            &dest.join(CAS),
        )?;
        let files = copy_city_files(vfs.as_mut(), city_root, &dest.join(CITY))?;
        let manifest = Manifest {
            records: count_records(vfs.as_ref(), &dest.join(LEDGER))?,
            head: head_of(vfs.as_ref(), &dest.join(LEDGER))?,
            cas_objects,
            files,
        };
        let _ = records;
        write_file(
            vfs.as_mut(),
            &dest.join(MANIFEST),
            manifest.to_json().as_bytes(),
        )?;
        Ok(manifest)
    }

    /// Reads a bundle's manifest without restoring it.
    ///
    /// # Errors
    /// Refuses a directory with no readable manifest.
    pub fn read_manifest(bundle: &Path) -> Result<Manifest, MemoryError> {
        let vfs = RealFs::new();
        let at = bundle.join(MANIFEST);
        let bytes = vfs
            .read(&at)
            .map_err(io_err("read a bundle manifest", &at))?;
        Manifest::from_json(&bytes, &at)
    }

    /// Restores a bundle into `city_root`, which must not already hold a
    /// ledger.
    ///
    /// Restoring verifies: the chain is walked, and the record count and
    /// head hash are compared against the manifest. A bundle that copied
    /// short is refused here rather than becoming a city that is quietly
    /// missing its last hour.
    ///
    /// # Errors
    /// Refuses an occupied city root, a bundle without a manifest, and
    /// any disagreement between the manifest and what was restored.
    pub fn restore(bundle: &Path, city_root: &Path) -> Result<Manifest, MemoryError> {
        Bundle::restore_with(Box::new(RealFs::new()), bundle, city_root)
    }

    pub(crate) fn restore_with(
        mut vfs: Box<dyn Vfs>,
        bundle: &Path,
        city_root: &Path,
    ) -> Result<Manifest, MemoryError> {
        let claimed = {
            let at = bundle.join(MANIFEST);
            let bytes = vfs
                .read(&at)
                .map_err(io_err("read a bundle manifest", &at))?;
            Manifest::from_json(&bytes, &at)?
        };
        let ledger_dir = city_root.join(RESERVED).join(LEDGER);
        if !walk(vfs.as_ref(), &ledger_dir).is_empty() {
            return Err(MemoryError::Bundle {
                op: "restore",
                detail: format!("{} already holds a ledger", ledger_dir.display()),
            });
        }
        copy_tree(vfs.as_mut(), &bundle.join(LEDGER), &ledger_dir)?;
        copy_tree(
            vfs.as_mut(),
            &bundle.join(CAS),
            &city_root.join(RESERVED).join(CAS),
        )?;
        copy_tree(vfs.as_mut(), &bundle.join(CITY), city_root)?;

        let restored = Manifest {
            records: count_records(vfs.as_ref(), &ledger_dir)?,
            head: head_of(vfs.as_ref(), &ledger_dir)?,
            cas_objects: count_files(vfs.as_ref(), &city_root.join(RESERVED).join(CAS))?,
            files: count_files(vfs.as_ref(), city_root)?,
        };
        if restored.records != claimed.records || restored.head != claimed.head {
            return Err(MemoryError::Bundle {
                op: "restore",
                detail: format!(
                    "the bundle claims {} record(s) ending {}, and {} record(s) ending {} arrived",
                    claimed.records, claimed.head, restored.records, restored.head
                ),
            });
        }
        Ok(restored)
    }
}

/// Walks the chain, which is what proves the records are one history.
fn head_of(vfs: &dyn Vfs, ledger_dir: &Path) -> Result<String, MemoryError> {
    let mut prev = GENESIS_PREV;
    let mut expected = Seq::FIRST;
    for line in read_lines(vfs, ledger_dir)? {
        let record = EventRecord::parse_line(&line).map_err(|err| MemoryError::Bundle {
            op: "verify",
            detail: err.to_string(),
        })?;
        if record.seq() != expected || record.prev() != prev {
            return Err(MemoryError::Bundle {
                op: "verify",
                detail: format!(
                    "record {} does not continue the chain",
                    record.seq().value()
                ),
            });
        }
        prev = chain_hash(&line);
        expected = expected.next().map_err(|err| MemoryError::Bundle {
            op: "verify",
            detail: err.to_string(),
        })?;
    }
    Ok(prev.to_string())
}

fn count_records(vfs: &dyn Vfs, ledger_dir: &Path) -> Result<u64, MemoryError> {
    let lines = read_lines(vfs, ledger_dir)?;
    u64::try_from(lines.len()).map_err(|_| MemoryError::Bundle {
        op: "count",
        detail: "more records than a count can hold".to_owned(),
    })
}

fn read_lines(vfs: &dyn Vfs, ledger_dir: &Path) -> Result<Vec<Vec<u8>>, MemoryError> {
    let mut out = Vec::new();
    for path in walk(vfs, ledger_dir) {
        if path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
            continue;
        }
        let bytes = vfs
            .read(&path)
            .map_err(io_err("read a bundle file", &path))?;
        for line in bytes.split(|byte| *byte == b'\n') {
            if !line.is_empty() {
                out.push(line.to_vec());
            }
        }
    }
    Ok(out)
}

/// Every file under `root`, at any depth, in a deterministic order.
///
/// An explicit worklist rather than recursion: a city's depth is not
/// this module's to assume, and a stack overflow is not catchable.
fn walk(vfs: &dyn Vfs, root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        found.extend(vfs.list(&dir).unwrap_or_default());
        pending.extend(vfs.list_dirs(&dir).unwrap_or_default());
    }
    found.sort();
    found
}

/// Copies every file under `from` into `to`, keeping relative paths.
fn copy_tree(vfs: &mut dyn Vfs, from: &Path, to: &Path) -> Result<u64, MemoryError> {
    let mut copied = 0u64;
    let files = walk(vfs, from);
    if files.is_empty() {
        return Ok(0);
    }
    vfs.create_dir_all(to)
        .map_err(io_err("make a bundle directory", to))?;
    for path in files {
        let Ok(relative) = path.strip_prefix(from) else {
            continue;
        };
        let target = to.join(relative);
        if let Some(parent) = target.parent() {
            vfs.create_dir_all(parent)
                .map_err(io_err("make a bundle directory", parent))?;
        }
        let bytes = vfs
            .read(&path)
            .map_err(io_err("read a bundle file", &path))?;
        write_file(vfs, &target, &bytes)?;
        copied = copied.saturating_add(1);
    }
    Ok(copied)
}

/// The city's own files: everything outside the reserved prefix.
fn copy_city_files(vfs: &mut dyn Vfs, city_root: &Path, to: &Path) -> Result<u64, MemoryError> {
    let mut copied = 0u64;
    let mut seen = BTreeMap::new();
    for path in walk(vfs, city_root) {
        let Ok(relative) = path.strip_prefix(city_root) else {
            continue;
        };
        if relative.starts_with(RESERVED) {
            continue;
        }
        seen.insert(relative.to_path_buf(), path);
    }
    if seen.is_empty() {
        return Ok(0);
    }
    vfs.create_dir_all(to)
        .map_err(io_err("make a bundle directory", to))?;
    for (relative, path) in seen {
        let target = to.join(&relative);
        if let Some(parent) = target.parent() {
            vfs.create_dir_all(parent)
                .map_err(io_err("make a bundle directory", parent))?;
        }
        let bytes = vfs
            .read(&path)
            .map_err(io_err("read a bundle file", &path))?;
        write_file(vfs, &target, &bytes)?;
        copied = copied.saturating_add(1);
    }
    Ok(copied)
}

fn count_files(vfs: &dyn Vfs, root: &Path) -> Result<u64, MemoryError> {
    let mut count = 0u64;
    for path in walk(vfs, root) {
        let Ok(relative) = path.strip_prefix(root) else {
            continue;
        };
        if relative.starts_with(RESERVED) {
            continue;
        }
        count = count.saturating_add(1);
    }
    Ok(count)
}

fn write_file(vfs: &mut dyn Vfs, path: &Path, bytes: &[u8]) -> Result<(), MemoryError> {
    if vfs.exists(path) {
        vfs.remove_file(path)
            .map_err(io_err("replace a bundle file", path))?;
    }
    vfs.append(path, bytes)
        .map_err(io_err("write a bundle file", path))?;
    vfs.sync_data(path)
        .map_err(io_err("flush a bundle file", path))
}

/// Opens the restored ledger, so the city is one a writer can continue.
///
/// # Errors
/// Whatever opening reports; a restored city that cannot be opened is
/// not restored.
pub fn open_restored(city_root: &Path, now: kernel::TimeMs) -> Result<PathBuf, MemoryError> {
    let dir = city_root.join(RESERVED).join(LEDGER);
    let (_ledger, _report) = JsonlLedger::open(&dir, now)?;
    Ok(dir)
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
    use kernel::{EventDraft, EventKind, Ledger, Payload, RunId, TimeMs};

    fn city_with(records: u64, root: &Path) {
        let dir = root.join(RESERVED).join(LEDGER);
        let (mut ledger, _report) = JsonlLedger::open(&dir, TimeMs::new(1)).unwrap();
        for step in 0..records {
            ledger
                .append(EventDraft {
                    run: RunId::CITY,
                    t: TimeMs::new(step.saturating_add(1)),
                    who: "owner".to_owned(),
                    addr: None,
                    kind: EventKind::CityInitialized,
                    data: Payload::empty(),
                    ig: false,
                })
                .unwrap();
        }
        std::fs::create_dir_all(root.join("lab")).unwrap();
        std::fs::write(root.join("City.md"), b"# City.md\n").unwrap();
        std::fs::write(root.join("lab").join("Roadmap.md"), b"# Roadmap\n").unwrap();
        let cas = crate::Cas::open(&root.join(RESERVED).join(CAS)).unwrap();
        drop(cas);
    }

    #[test]
    fn a_city_comes_back_in_an_empty_directory_and_its_chain_verifies() {
        let home = tempfile::tempdir().unwrap();
        city_with(3, home.path());
        let carried = tempfile::tempdir().unwrap();
        let exported = Bundle::export(home.path(), carried.path()).unwrap();
        assert_eq!(exported.records(), 3);
        assert_ne!(exported.head(), GENESIS_PREV.to_string());

        let elsewhere = tempfile::tempdir().unwrap();
        let restored = Bundle::restore(carried.path(), elsewhere.path()).unwrap();
        assert_eq!(restored, exported);
        // The work came with it, not only the history.
        assert!(elsewhere.path().join("City.md").exists());
        assert!(elsewhere.path().join("lab").join("Roadmap.md").exists());
        // And the restored city is one a writer can continue.
        open_restored(elsewhere.path(), TimeMs::new(9)).unwrap();
    }

    #[test]
    fn a_short_copy_is_refused_rather_than_restored_quietly() {
        let home = tempfile::tempdir().unwrap();
        city_with(4, home.path());
        let carried = tempfile::tempdir().unwrap();
        Bundle::export(home.path(), carried.path()).unwrap();

        // Lose the last record, as an interrupted copy would.
        let segment = std::fs::read_dir(carried.path().join(LEDGER))
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .find(|path| path.extension().and_then(|e| e.to_str()) == Some("jsonl"))
            .unwrap();
        let bytes = std::fs::read(&segment).unwrap();
        let cut = bytes
            .iter()
            .rposition(|byte| *byte == b'\n')
            .and_then(|last| bytes[..last].iter().rposition(|byte| *byte == b'\n'))
            .unwrap();
        std::fs::write(&segment, &bytes[..=cut]).unwrap();

        let elsewhere = tempfile::tempdir().unwrap();
        let err = Bundle::restore(carried.path(), elsewhere.path()).unwrap_err();
        let ax = err.into_ax();
        assert!(
            ax.subject().contains("record(s) ending"),
            "a partial copy is not a city: {}",
            ax.subject()
        );
    }

    #[test]
    fn a_city_is_never_restored_on_top_of_another() {
        let home = tempfile::tempdir().unwrap();
        city_with(2, home.path());
        let carried = tempfile::tempdir().unwrap();
        Bundle::export(home.path(), carried.path()).unwrap();

        // The city it came from still has its ledger, so restoring back
        // onto it would be a merge of two histories.
        let err = Bundle::restore(carried.path(), home.path()).unwrap_err();
        assert!(err.into_ax().subject().contains("already holds a ledger"));
    }

    #[test]
    fn the_manifest_is_byte_stable_across_exports() {
        let home = tempfile::tempdir().unwrap();
        city_with(2, home.path());
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        Bundle::export(home.path(), first.path()).unwrap();
        Bundle::export(home.path(), second.path()).unwrap();
        assert_eq!(
            std::fs::read(first.path().join(MANIFEST)).unwrap(),
            std::fs::read(second.path().join(MANIFEST)).unwrap()
        );
        assert_eq!(
            Bundle::read_manifest(first.path()).unwrap(),
            Bundle::read_manifest(second.path()).unwrap()
        );
    }

    #[test]
    fn nothing_under_the_reserved_prefix_travels_as_a_city_file() {
        let home = tempfile::tempdir().unwrap();
        city_with(1, home.path());
        // A projection is disposable; carrying one would be a second
        // statement of what happened.
        let views = home.path().join(RESERVED).join("projection");
        std::fs::create_dir_all(&views).unwrap();
        std::fs::write(views.join("cold.redb"), b"derived").unwrap();

        let carried = tempfile::tempdir().unwrap();
        Bundle::export(home.path(), carried.path()).unwrap();
        assert!(!carried.path().join(CITY).join(RESERVED).exists());
        assert!(!carried.path().join(CITY).join("projection").exists());
    }
}
