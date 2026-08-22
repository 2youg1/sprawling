// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A10's three assertions, on the real engine.
//!
//! The guests are hand-written WAT, so the gate proves the boundary
//! without needing a 30 MB CPython artifact on disk. Each module does
//! exactly one thing:
//!
//! 1. succeeds inside its fuel budget,
//! 2. tries to open a path it was never granted, and fails visibly,
//! 3. loops until the fuel runs out.
//!
//! The point of assertion 2 is that the host does not cover for the
//! guest: a denied capability surfaces as a guest-visible failure, not
//! as a silent success or an empty read.

#![cfg(feature = "wasm")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "test code"
)]

use std::path::{Path, PathBuf};

use runtime::{Fuel, Mount, Sandbox, SandboxExit, SandboxJob, WasmtimeSandbox};

/// Writes a WAT source out as a .wasm module and returns its path.
fn build(dir: &Path, name: &str, wat: &str) -> PathBuf {
    let bytes = wat::parse_str(wat).expect("assemble wat");
    let path = dir.join(format!("{name}.wasm"));
    std::fs::write(&path, bytes).expect("write module");
    path
}

/// Writes `msg` to fd 1 and returns cleanly.
const WAT_SUCCEEDS: &str = r#"
(module
  (import "wasi_snapshot_preview1" "fd_write"
    (func $fd_write (param i32 i32 i32 i32) (result i32)))
  (memory (export "memory") 1)
  (data (i32.const 100) "ok\n")
  (func (export "_start")
    ;; iovec { base = 100, len = 3 }
    (i32.store (i32.const 8) (i32.const 100))
    (i32.store (i32.const 12) (i32.const 3))
    (drop (call $fd_write (i32.const 1) (i32.const 8) (i32.const 1) (i32.const 20)))
  )
)
"#;

/// Tries to open "secret.txt" relative to fd 3. With no preopen granted
/// there is no fd 3 at all, so the call fails and the guest exits
/// non-zero — the failure is the guest's, and it is visible.
const WAT_DENIED: &str = r#"
(module
  (import "wasi_snapshot_preview1" "path_open"
    (func $path_open (param i32 i32 i32 i32 i32 i64 i64 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "proc_exit" (func $proc_exit (param i32)))
  (memory (export "memory") 1)
  (data (i32.const 100) "secret.txt")
  (func (export "_start")
    (local $rc i32)
    (local.set $rc
      (call $path_open
        (i32.const 3)     ;; dirfd: the first preopen, if any was granted
        (i32.const 0)     ;; dirflags
        (i32.const 100)   ;; path ptr
        (i32.const 10)    ;; path len
        (i32.const 0)     ;; oflags
        (i64.const 2)     ;; fs_rights_base: fd_read only (all-ones is
        (i64.const 2)     ;; fs_rights_inheriting: not a valid bit set)
        (i32.const 0)     ;; fdflags
        (i32.const 200))) ;; out: opened fd
    (if (i32.ne (local.get $rc) (i32.const 0))
      (then (call $proc_exit (i32.const 7))))
  )
)
"#;

/// Spins forever. Only the fuel budget can end this.
const WAT_BURNS_FUEL: &str = r#"
(module
  (memory (export "memory") 1)
  (func (export "_start")
    (local $i i64)
    (loop $forever
      (local.set $i (i64.add (local.get $i) (i64.const 1)))
      (br $forever))
  )
)
"#;

#[test]
fn a10_1_a_guest_inside_its_budget_succeeds() {
    let tmp = tempfile::tempdir().unwrap();
    let module = build(tmp.path(), "ok", WAT_SUCCEEDS);
    let mut sandbox = WasmtimeSandbox::new().unwrap();
    let outcome = sandbox
        .run(&SandboxJob {
            wasm: module,
            argv: vec!["prog".to_owned()],
            env: vec![],
            stdin: Vec::new(),
            mounts: vec![],
            fuel: Fuel(1_000_000),
        })
        .unwrap();
    assert_eq!(outcome.exit, SandboxExit::Success, "{outcome:?}");
    assert_eq!(outcome.stdout, b"ok\n");
}

#[test]
fn a10_2_an_ungranted_capability_is_refused_and_the_guest_sees_it() {
    let tmp = tempfile::tempdir().unwrap();
    let module = build(tmp.path(), "denied", WAT_DENIED);
    // A file exists on the host, but no mount names its directory.
    let outside = tmp.path().join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("secret.txt"), b"not yours").unwrap();

    let mut sandbox = WasmtimeSandbox::new().unwrap();
    let outcome = sandbox
        .run(&SandboxJob {
            wasm: module.clone(),
            argv: vec!["prog".to_owned()],
            env: vec![],
            stdin: Vec::new(),
            mounts: vec![],
            fuel: Fuel(1_000_000),
        })
        .unwrap();
    assert_eq!(
        outcome.exit,
        SandboxExit::Failure { code: 7 },
        "no preopen means the open fails and the guest exits non-zero: {outcome:?}"
    );

    // Granting the directory changes the answer — which is what makes
    // the refusal above a capability decision rather than a broken test.
    let granted = sandbox
        .run(&SandboxJob {
            wasm: module,
            argv: vec!["prog".to_owned()],
            env: vec![],
            stdin: Vec::new(),
            mounts: vec![Mount {
                host: outside.clone(),
                guest: "/work".to_owned(),
                writable: false,
            }],
            fuel: Fuel(1_000_000),
        })
        .unwrap();
    assert_eq!(
        granted.exit,
        SandboxExit::Success,
        "the same guest succeeds once the capability is granted: {granted:?}"
    );
}

#[test]
fn a10_3_fuel_exhaustion_stops_the_guest_and_is_not_a_host_error() {
    let tmp = tempfile::tempdir().unwrap();
    let module = build(tmp.path(), "burn", WAT_BURNS_FUEL);
    let mut sandbox = WasmtimeSandbox::new().unwrap();
    let outcome = sandbox
        .run(&SandboxJob {
            wasm: module,
            argv: vec!["prog".to_owned()],
            env: vec![],
            stdin: Vec::new(),
            mounts: vec![],
            fuel: Fuel(10_000),
        })
        .expect("exhaustion is an outcome, never a host error");
    assert_eq!(outcome.exit, SandboxExit::FuelExhausted, "{outcome:?}");
}

#[test]
fn the_boundary_has_no_network_because_no_socket_can_be_opened() {
    // The Python arm's no-network proof is structural, and it is worth
    // stating precisely. wasip1 *does* export sock_send and sock_recv —
    // they operate on an already-open socket and answer ENOTSOCK for
    // anything else. What it exports no way of doing is *obtaining* a
    // socket: there is no sock_open, no sock_connect, no sock_bind. So
    // a guest asking for one fails to link, and every fd it can reach
    // came from a preopen, which is a directory.
    let tmp = tempfile::tempdir().unwrap();
    let module = build(
        tmp.path(),
        "dialer",
        r#"
        (module
          (import "wasi_snapshot_preview1" "sock_connect"
            (func $sock_connect (param i32 i32 i32) (result i32)))
          (func (export "_start") (drop (call $sock_connect
            (i32.const 0) (i32.const 0) (i32.const 0))))
        )
        "#,
    );
    let mut sandbox = WasmtimeSandbox::new().unwrap();
    let result = sandbox.run(&SandboxJob {
        wasm: module,
        argv: vec!["prog".to_owned()],
        env: vec![],
        stdin: Vec::new(),
        mounts: vec![],
        fuel: Fuel(1_000_000),
    });
    let err = match result {
        Err(err) => err,
        Ok(outcome) => panic!("a guest that can dial out must not link: {outcome:?}"),
    };
    assert_eq!(*err.code(), kernel::AxCode::SandboxDenied);
}

#[test]
fn back_to_back_jobs_do_not_poison_each_other() {
    let tmp = tempfile::tempdir().unwrap();
    let module = build(tmp.path(), "ok2", WAT_SUCCEEDS);
    let mut sandbox = WasmtimeSandbox::new().unwrap();
    let job = SandboxJob {
        wasm: module,
        argv: vec!["prog".to_owned()],
        env: vec![],
        stdin: Vec::new(),
        mounts: vec![],
        fuel: Fuel(1_000_000),
    };
    let first = sandbox.run(&job).unwrap();
    let second = sandbox.run(&job).unwrap();
    assert_eq!(first, second, "each job starts from a clean store");
}
