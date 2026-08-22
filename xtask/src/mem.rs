// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A11: how heavy is one session.
//!
//! **What this measures and what it does not.** A11 constrains how heavy we
//! build a session; it is not a threshold the runtime reads and not a
//! promise to a user. The server renders no interface, so the figure holds
//! no rendering context, no glyph bitmaps and no GPU textures; the browser's
//! own footprint belongs to the client machine and is not counted here.
//!
//! **One command, three platforms, rather than three scripts.** Three shell
//! scripts would be three authorities on what "resident" means, and the two
//! nobody runs today would be the two that are wrong tomorrow. Each platform
//! is asked in its own vocabulary, because the words genuinely differ:
//!
//! | platform | counter | why this one |
//! |---|---|---|
//! | Linux | `/proc/<pid>/smaps_rollup` Pss | shared pages counted once, divided by their sharers - the honest reading when several Runs share one binary |
//! | macOS | `ps -o rss` | phys_footprint needs a private API; RSS over-reports shared pages and is therefore a conservative bound |
//! | Windows | `Get-Process` `WorkingSet64` | the private working set is what Task Manager shows and what an operator will compare against |
//!
//! The counters are not interchangeable, so the report names which one it
//! read. A number without its definition is how a budget quietly becomes
//! three different budgets.

use std::path::Path;
// Linux reads a file; the other two ask a program. The import carries the
// same condition as its only caller, or it is unused on Linux and the
// zero-warning build stops there.
#[cfg(any(target_os = "macos", target_os = "windows"))]
use std::process::Command;

use crate::report::XtaskError;

/// The budget A11 states, in bytes.
pub(crate) const SESSION_BUDGET_BYTES: u64 = 30 * 1024 * 1024;

pub(crate) struct Reading {
    pub(crate) counter: &'static str,
    pub(crate) bytes: u64,
}

/// Measures a process, defaulting to this one.
pub(crate) fn run(root: &Path, pid: Option<&str>) -> Result<String, XtaskError> {
    let _ = root;
    let pid = match pid {
        Some(raw) => raw.to_owned(),
        None => std::process::id().to_string(),
    };
    let reading = measure(&pid)?;
    let budget = SESSION_BUDGET_BYTES;
    let verdict = if reading.bytes <= budget {
        "within"
    } else {
        "over"
    };
    Ok(format!(
        "A11 resident measurement\n  pid      {pid}\n  counter  {}\n  bytes    {}\n  budget   {budget}\n  verdict  {verdict} the per-session budget\n\
         \n  note: P0 records the trend; the gate arrives at P1.",
        reading.counter, reading.bytes
    ))
}

#[cfg(target_os = "linux")]
fn measure(pid: &str) -> Result<Reading, XtaskError> {
    let path = format!("/proc/{pid}/smaps_rollup");
    let text = std::fs::read_to_string(&path).map_err(|source| XtaskError::Io {
        path: path.clone(),
        source,
    })?;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("Pss:")
            && let Some(kib) = rest.split_whitespace().next()
            && let Ok(value) = kib.parse::<u64>()
        {
            return Ok(Reading {
                counter: "linux smaps_rollup Pss",
                bytes: value.saturating_mul(1024),
            });
        }
    }
    Err(XtaskError::Doc {
        file: path,
        msg: "no Pss line; the kernel may be too old for smaps_rollup".to_owned(),
    })
}

#[cfg(target_os = "macos")]
fn measure(pid: &str) -> Result<Reading, XtaskError> {
    let out = capture("ps", &["-o", "rss=", "-p", pid])?;
    let value = out.trim().parse::<u64>().map_err(|_| XtaskError::Doc {
        file: "ps".to_owned(),
        msg: format!("unreadable rss: {out}"),
    })?;
    Ok(Reading {
        counter: "macos ps rss (conservative: shared pages counted in full)",
        bytes: value.saturating_mul(1024),
    })
}

#[cfg(target_os = "windows")]
fn measure(pid: &str) -> Result<Reading, XtaskError> {
    let script = format!("(Get-Process -Id {pid}).WorkingSet64");
    let out = capture(
        "powershell",
        &["-NoProfile", "-NonInteractive", "-Command", &script],
    )?;
    let value = out.trim().parse::<u64>().map_err(|_| XtaskError::Doc {
        file: "powershell".to_owned(),
        msg: format!("unreadable working set: {out}"),
    })?;
    Ok(Reading {
        counter: "windows WorkingSet64",
        bytes: value,
    })
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn measure(_pid: &str) -> Result<Reading, XtaskError> {
    Err(XtaskError::Doc {
        file: "mem".to_owned(),
        msg: "no resident-memory counter is defined for this platform; \
              add one here rather than reporting a number of unknown meaning"
            .to_owned(),
    })
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn capture(program: &str, args: &[&str]) -> Result<String, XtaskError> {
    let out = Command::new(program)
        .args(args)
        .output()
        .map_err(|source| XtaskError::Io {
            path: program.to_owned(),
            source,
        })?;
    if !out.status.success() {
        return Err(XtaskError::Doc {
            file: program.to_owned(),
            msg: String::from_utf8_lossy(&out.stderr).trim().to_owned(),
        });
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "test code"
)]
mod tests {
    use super::*;

    #[test]
    fn the_budget_is_the_one_a11_states() {
        assert_eq!(SESSION_BUDGET_BYTES, 31_457_280, "30 MiB");
    }

    #[test]
    fn this_platform_can_answer_and_names_its_counter() {
        // The measurement must work wherever it is run, and must say which
        // counter it read - a number without its definition is how one
        // budget quietly becomes three.
        let reading = measure(&std::process::id().to_string())
            .expect("this platform has a resident-memory counter");
        assert!(reading.bytes > 0, "a live process occupies something");
        assert!(!reading.counter.is_empty());
    }

    #[test]
    fn the_report_states_the_budget_and_the_verdict() {
        let text = run(Path::new("."), None).unwrap();
        assert!(text.contains("counter"));
        assert!(text.contains("budget"));
        assert!(text.contains("verdict"));
        assert!(
            text.contains("P1"),
            "P0 records the trend; the report must not read as a gate"
        );
    }
}
