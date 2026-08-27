// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Copyright (c) 2026 2youg1

//! Putting this binary where a shell will find it, and taking it back
//! out again (sprawling-SPEC.md section 8-9).
//!
//! Two things happen and exactly those two are reversed: the running
//! binary is copied into the per-user program directory, and that
//! directory is put on the user's own search path. Nothing outside the
//! person's profile is touched, so nothing here wants administrator
//! rights.
//!
//! The judgements live in `plan_append` and `plan_remove`, which are
//! pure functions over the search path string. Everything below them is
//! a copy, a registry write and a broadcast.

use kernel::{AxCode, AxError};
use std::path::{Path, PathBuf};

/// The word this binary is installed as, whatever the archive called the
/// file. Making `sprawling` typeable cannot depend on who unpacked it.
const INSTALLED_STEM: &str = "sprawling";

/// The search path separator, which is the platform's and not ours.
#[cfg(target_os = "windows")]
const SEPARATOR: char = ';';
#[cfg(not(target_os = "windows"))]
const SEPARATOR: char = ':';

/// What installing does to the search path.
///
/// Two states rather than a bool, because "it was already there" and "I
/// put it there" are different things to tell a person afterwards.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum PathEdit {
    AlreadyPresent,
    Append(String),
}

/// What uninstalling does to the search path.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum PathRemoval {
    Absent,
    Rewrite(String),
}

/// What actually happened, in the order a person needs to hear it.
pub(crate) struct Report {
    pub(crate) binary: PathBuf,
    pub(crate) path: PathOutcome,
    /// Present when something worked only halfway: the search path was
    /// written but the desktop was not told.
    pub(crate) notice: Option<String>,
}

pub(crate) enum PathOutcome {
    /// The search path already said what it needed to say.
    Unchanged,
    /// The search path was rewritten and running shells will not see it.
    Rewritten,
    /// This platform does not have the search path edited for it; the
    /// line a person adds themselves travels with the outcome.
    #[cfg_attr(
        target_os = "windows",
        expect(
            dead_code,
            reason = "only the non-Windows search path hands the line back to the person"
        )
    )]
    SelfService(String),
}

/// The per-user program directory: where a binary a person installed for
/// themselves belongs on this platform.
///
/// `None` means neither location could be derived, which is the honest
/// answer on a machine with no home and no `LOCALAPPDATA`.
pub(crate) fn program_dir(local_app_data: Option<&Path>, home: Option<&Path>) -> Option<PathBuf> {
    if cfg!(target_os = "windows") {
        return local_app_data
            .map(|root| root.join("Programs").join(INSTALLED_STEM))
            .or_else(|| home.map(|home| home.join(".local").join("bin")));
    }
    home.map(|home| home.join(".local").join("bin"))
}

/// The file name this binary is installed under.
pub(crate) fn installed_name() -> String {
    format!("{INSTALLED_STEM}{}", std::env::consts::EXE_SUFFIX)
}

/// Whether a search path entry names the same directory as `dir`.
///
/// Compares case-insensitively on Windows because its file system does,
/// and ignores a trailing separator because `C:\x` and `C:\x\` are the
/// same directory to every shell that reads this string.
fn same_directory(entry: &str, dir: &str) -> bool {
    let normalise = |raw: &str| {
        let trimmed = raw.trim().trim_end_matches(['\\', '/']);
        if cfg!(target_os = "windows") {
            trimmed.to_lowercase()
        } else {
            trimmed.to_owned()
        }
    };
    !dir.trim().is_empty() && normalise(entry) == normalise(dir)
}

/// What the search path becomes when `dir` joins it.
///
/// Appends rather than prepends: a directory a person installed into
/// should not shadow what their system already resolves.
pub(crate) fn plan_append(current: &str, dir: &str) -> PathEdit {
    if on_search_path(current, dir) {
        return PathEdit::AlreadyPresent;
    }
    if current.trim().is_empty() {
        return PathEdit::Append(dir.to_owned());
    }
    let joined = format!("{}{SEPARATOR}{dir}", current.trim_end_matches(SEPARATOR));
    PathEdit::Append(joined)
}

/// What the search path becomes when `dir` leaves it.
///
/// Every other entry survives byte for byte, including an empty one:
/// this reverses one append and audits nothing else.
pub(crate) fn plan_remove(current: &str, dir: &str) -> PathRemoval {
    let kept: Vec<&str> = current
        .split(SEPARATOR)
        .filter(|entry| !same_directory(entry, dir))
        .collect();
    if kept.len() == current.split(SEPARATOR).count() {
        return PathRemoval::Absent;
    }
    PathRemoval::Rewrite(kept.join(&SEPARATOR.to_string()))
}

/// Whether a directory is already reachable from this search path.
pub(crate) fn on_search_path(search_path: &str, dir: &str) -> bool {
    search_path
        .split(SEPARATOR)
        .any(|entry| same_directory(entry, dir))
}

/// Copies the running binary into `dir` under the installed name.
fn place(dir: &Path) -> Result<PathBuf, AxError> {
    let source = std::env::current_exe().map_err(|err| {
        AxError::failure(AxCode::PathNotFound, "find this binary", err.to_string())
            .with_recovery("run the binary by its path rather than through a shim")
    })?;
    std::fs::create_dir_all(dir).map_err(|err| {
        AxError::failure(
            AxCode::StorageFatal,
            "create the program directory",
            format!("{}: {err}", dir.display()),
        )
        .with_recovery("check that this account may write there")
    })?;
    let target = dir.join(installed_name());
    if target != source {
        std::fs::copy(&source, &target).map_err(|err| {
            AxError::failure(
                AxCode::StorageFatal,
                "copy this binary into place",
                format!("{}: {err}", target.display()),
            )
            .with_recovery("close any running sprawling and try again")
        })?;
    }
    Ok(target)
}

/// Removes the copy this put there, and says whether there was one.
fn displace(dir: &Path) -> Result<bool, AxError> {
    let target = dir.join(installed_name());
    match std::fs::remove_file(&target) {
        Ok(()) => Ok(true),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(err) => Err(AxError::failure(
            AxCode::StorageFatal,
            "remove the installed binary",
            format!("{}: {err}", target.display()),
        )
        .with_recovery("close any running sprawling and try again")),
    }
}

fn no_home() -> AxError {
    AxError::failure(
        AxCode::PathNotFound,
        "find a per-user program directory",
        "neither LOCALAPPDATA nor a home directory is set",
    )
    .with_recovery("set HOME, or copy the binary somewhere on PATH yourself")
}

fn dirs() -> (Option<PathBuf>, Option<PathBuf>) {
    let local_app_data = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from);
    (local_app_data, home)
}

/// Installs, or reverses an install.
///
/// # Errors
/// Fails when there is nowhere to install to, when the copy cannot be
/// made or removed, or when the search path cannot be read or written.
pub(crate) fn install(uninstall: bool) -> Result<Report, AxError> {
    let (local_app_data, home) = dirs();
    let dir = program_dir(local_app_data.as_deref(), home.as_deref()).ok_or_else(no_home)?;
    let shown = dir.display().to_string();
    if uninstall {
        let removed = displace(&dir)?;
        let path = retract(&shown)?;
        return Ok(Report {
            binary: dir.join(installed_name()),
            path,
            notice: (!removed).then(|| format!("nothing was installed in {shown}")),
        });
    }
    let binary = place(&dir)?;
    let (path, notice) = extend(&shown)?;
    Ok(Report {
        binary,
        path,
        notice,
    })
}

#[cfg(target_os = "windows")]
mod search_path {
    use super::{PathEdit, PathOutcome, PathRemoval, plan_append, plan_remove};
    use kernel::{AxCode, AxError};

    /// Reads `HKCU\Environment\Path` without expanding it, and says which
    /// registry type it has.
    ///
    /// The raw value is what has to be rewritten: expanding `%VAR%` and
    /// writing the result back is how a search path silently stops
    /// following the variables a person put in it.
    const READ: &str = r"
$ErrorActionPreference='Stop'
$k = Get-Item -LiteralPath 'HKCU:\Environment'
if ($k.GetValueNames() -contains 'Path') {
  $kind = $k.GetValueKind('Path')
  $v = $k.GetValue('Path','',[Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
} else { $kind = 'ExpandString'; $v = '' }
[IO.File]::WriteAllText($env:SPRAWLING_PATH_FILE, $v, (New-Object Text.UTF8Encoding $false))
Write-Output $kind
";

    /// Writes the value back under the type it was read as, then tells
    /// every top-level window that the environment moved.
    ///
    /// `[Environment]::SetEnvironmentVariable` is the obvious call and
    /// the wrong one: it always writes REG_SZ, which demotes a
    /// REG_EXPAND_SZ search path so its `%VAR%` entries stop expanding
    /// (dotnet/runtime#1442). Writing the registry directly means the
    /// broadcast is ours to send, and `#![forbid(unsafe_code)]` puts
    /// `SendMessageTimeout` out of Rust's reach - so it is sent from
    /// here.
    const WRITE: &str = r#"
$ErrorActionPreference='Stop'
$v = [IO.File]::ReadAllText($env:SPRAWLING_PATH_FILE, (New-Object Text.UTF8Encoding $false))
Set-ItemProperty -LiteralPath 'HKCU:\Environment' -Name 'Path' -Value $v -Type $env:SPRAWLING_PATH_KIND
Add-Type -Namespace SprawlingNative -Name Env -MemberDefinition @'
[DllImport("user32.dll", SetLastError=true, CharSet=CharSet.Auto)]
public static extern IntPtr SendMessageTimeout(IntPtr hWnd, uint Msg, UIntPtr wParam, string lParam, uint fuFlags, uint uTimeout, out UIntPtr lpdwResult);
'@
$r = [UIntPtr]::Zero
[void][SprawlingNative.Env]::SendMessageTimeout([IntPtr]0xffff, 0x1A, [UIntPtr]::Zero, 'Environment', 2, 5000, [ref]$r)
"#;

    struct Raw {
        value: String,
        kind: String,
        carrier: std::path::PathBuf,
    }

    fn carrier_path() -> std::path::PathBuf {
        std::env::temp_dir().join(format!("sprawling-path-{}.txt", std::process::id()))
    }

    fn powershell(
        script: &str,
        carrier: &std::path::Path,
        kind: Option<&str>,
    ) -> Result<String, AxError> {
        let mut command = std::process::Command::new("powershell");
        command
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-Command",
                script,
            ])
            .env("SPRAWLING_PATH_FILE", carrier);
        if let Some(kind) = kind {
            command.env("SPRAWLING_PATH_KIND", kind);
        }
        let done = command.output().map_err(|err| {
            AxError::failure(AxCode::StorageFatal, "run powershell", err.to_string())
                .with_recovery("powershell is how a user-level PATH is edited on this platform")
        })?;
        if !done.status.success() {
            return Err(AxError::failure(
                AxCode::StorageFatal,
                "edit the user search path",
                String::from_utf8_lossy(&done.stderr).trim().to_owned(),
            )
            .with_recovery("no change was made; edit PATH in System Properties instead"));
        }
        Ok(String::from_utf8_lossy(&done.stdout).trim().to_owned())
    }

    fn read() -> Result<Raw, AxError> {
        let carrier = carrier_path();
        let kind = powershell(READ, &carrier, None)?;
        let value = std::fs::read_to_string(&carrier).map_err(|err| {
            AxError::failure(
                AxCode::StorageFatal,
                "read the user search path",
                err.to_string(),
            )
            .with_recovery("no change was made")
        })?;
        Ok(Raw {
            value,
            kind,
            carrier,
        })
    }

    fn write(raw: &Raw, value: &str) -> Result<Option<String>, AxError> {
        std::fs::write(&raw.carrier, value).map_err(|err| {
            AxError::failure(
                AxCode::StorageFatal,
                "stage the new search path",
                err.to_string(),
            )
            .with_recovery("no change was made")
        })?;
        let outcome = powershell(WRITE, &raw.carrier, Some(&raw.kind));
        // The carrier held nothing secret, but it held the whole of this
        // person's search path, so it does not outlive the edit.
        let _ = std::fs::remove_file(&raw.carrier);
        outcome?;
        Ok(None)
    }

    pub(super) fn extend(dir: &str) -> Result<(PathOutcome, Option<String>), AxError> {
        let raw = read()?;
        match plan_append(&raw.value, dir) {
            PathEdit::AlreadyPresent => {
                let _ = std::fs::remove_file(&raw.carrier);
                Ok((PathOutcome::Unchanged, None))
            }
            PathEdit::Append(next) => {
                let notice = write(&raw, &next)?;
                Ok((PathOutcome::Rewritten, notice))
            }
        }
    }

    pub(super) fn retract(dir: &str) -> Result<PathOutcome, AxError> {
        let raw = read()?;
        match plan_remove(&raw.value, dir) {
            PathRemoval::Absent => {
                let _ = std::fs::remove_file(&raw.carrier);
                Ok(PathOutcome::Unchanged)
            }
            PathRemoval::Rewrite(next) => {
                write(&raw, &next)?;
                Ok(PathOutcome::Rewritten)
            }
        }
    }
}

#[cfg(not(target_os = "windows"))]
mod search_path {
    use super::{PathOutcome, on_search_path};
    use kernel::AxError;

    /// Nothing here writes a shell startup file. Which one to write is a
    /// guess between `.profile`, `.bashrc`, `.zshrc` and fish's own
    /// syntax, and guessing wrong leaves a line in somebody's login
    /// script that does nothing and that they have to find to remove.
    /// `~/.local/bin` is already on the search path on current
    /// distributions; when it is not, the exact line travels back with
    /// the outcome and the person places it where they keep such lines.
    fn line(dir: &str) -> String {
        format!("export PATH=\"{dir}:$PATH\"")
    }

    fn current() -> String {
        std::env::var("PATH").unwrap_or_default()
    }

    pub(super) fn extend(dir: &str) -> Result<(PathOutcome, Option<String>), AxError> {
        if on_search_path(&current(), dir) {
            return Ok((PathOutcome::Unchanged, None));
        }
        Ok((PathOutcome::SelfService(line(dir)), None))
    }

    /// Nothing was written, so nothing is taken back: removing the copy
    /// is the whole of an uninstall on this platform.
    pub(super) fn retract(_dir: &str) -> Result<PathOutcome, AxError> {
        Ok(PathOutcome::Unchanged)
    }
}

use search_path::{extend, retract};

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "test code"
)]
mod tests {
    use super::{PathEdit, PathRemoval, SEPARATOR, plan_append, plan_remove, program_dir};
    use std::path::{Path, PathBuf};

    /// Stand-ins, not paths on any machine: the release gate refuses a
    /// published file that names somebody's home directory, and a test
    /// fixture is published like everything else.
    fn dir() -> &'static str {
        if SEPARATOR == ';' {
            r"X:\elsewhere\Local\Programs\sprawling"
        } else {
            "/elsewhere/.local/bin"
        }
    }

    fn other() -> String {
        if SEPARATOR == ';' {
            r"X:\unpacked\system32".to_owned()
        } else {
            "/unpacked/bin".to_owned()
        }
    }

    #[test]
    fn an_empty_search_path_becomes_the_one_directory() {
        assert_eq!(plan_append("", dir()), PathEdit::Append(dir().to_owned()));
    }

    #[test]
    fn a_directory_already_there_is_not_added_twice() {
        let current = format!("{}{SEPARATOR}{}", other(), dir());
        assert_eq!(plan_append(&current, dir()), PathEdit::AlreadyPresent);
    }

    #[test]
    fn a_trailing_separator_does_not_make_a_second_entry() {
        let current = format!("{}{SEPARATOR}{}{SEPARATOR}", other(), dir());
        assert_eq!(plan_append(&current, dir()), PathEdit::AlreadyPresent);
    }

    /// Windows resolves paths case-insensitively, so a differently-cased
    /// entry is the same entry and adding another is a duplicate.
    #[cfg(target_os = "windows")]
    #[test]
    fn a_differently_cased_entry_is_the_same_entry() {
        let current = format!("{}{SEPARATOR}{}", other(), dir().to_uppercase());
        assert_eq!(plan_append(&current, dir()), PathEdit::AlreadyPresent);
    }

    #[test]
    fn a_directory_that_is_not_there_is_appended_after_what_was() {
        let current = other();
        let expected = format!("{}{SEPARATOR}{}", other(), dir());
        assert_eq!(plan_append(&current, dir()), PathEdit::Append(expected));
    }

    #[test]
    fn removing_a_directory_that_was_never_added_changes_nothing() {
        assert_eq!(plan_remove(&other(), dir()), PathRemoval::Absent);
    }

    #[test]
    fn removing_keeps_every_other_entry_including_an_empty_one() {
        let current = format!("{}{SEPARATOR}{}{SEPARATOR}", other(), dir());
        assert_eq!(
            plan_remove(&current, dir()),
            PathRemoval::Rewrite(format!("{}{SEPARATOR}", other()))
        );
    }

    /// The pair of properties the whole card rests on: installing twice
    /// is installing once, and uninstalling returns the exact string.
    #[test]
    fn append_then_remove_returns_the_original_search_path() {
        for current in ["", &other(), &format!("{}{SEPARATOR}", other())] {
            let PathEdit::Append(extended) = plan_append(current, dir()) else {
                panic!("{current:?} does not contain the directory yet");
            };
            assert_eq!(plan_append(&extended, dir()), PathEdit::AlreadyPresent);
            let PathRemoval::Rewrite(back) = plan_remove(&extended, dir()) else {
                panic!("the directory was just added");
            };
            assert_eq!(
                back.trim_end_matches(SEPARATOR),
                current.trim_end_matches(SEPARATOR)
            );
        }
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_installs_under_the_per_user_program_directory() {
        let dir = program_dir(
            Some(Path::new(r"X:\appdata")),
            Some(Path::new(r"X:\elsewhere")),
        );
        assert_eq!(dir, Some(PathBuf::from(r"X:\appdata\Programs\sprawling")));
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn other_platforms_install_under_the_xdg_user_binary_directory() {
        let dir = program_dir(None, Some(Path::new("/elsewhere")));
        assert_eq!(dir, Some(PathBuf::from("/elsewhere/.local/bin")));
    }

    #[test]
    fn with_nowhere_to_install_the_answer_is_nowhere() {
        assert_eq!(program_dir(None, None), None);
    }
}
