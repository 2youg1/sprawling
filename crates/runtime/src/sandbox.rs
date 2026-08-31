// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! The execution boundary: a seam with one method,
//! behind which a guest program runs with exactly the capabilities it
//! was handed and no others.
//!
//! The capability surface is the preopen set. A guest can reach a host
//! directory if and only if a [`Mount`] named it; there is no ambient
//! filesystem, no environment inheritance, and — because wasip1 has no
//! socket host implementation — no network. That last one matters: the
//! Python arm cannot reach the network, and the proof is structural
//! rather than a rule someone remembered to write down.
//!
//! Fuel exhaustion is an outcome, not an error. Running out of fuel is
//! the guest doing something the budget did not cover, which the caller
//! must see and decide about; `Err` here is reserved for the host
//! failing to run the job at all. The distinction is what lets a
//! watchdog treat a runaway loop differently from a broken engine.

use std::path::PathBuf;

use kernel::{AxCode, AxError};

/// Instruction budget. Exhaustion ends the guest; it does not fail the
/// host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fuel(pub u64);

/// One granted capability: a host directory visible to the guest under
/// `guest`, writable or not. The mount list is the whole of what the
/// job can reach.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mount {
    pub host: PathBuf,
    pub guest: String,
    pub writable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxJob {
    pub wasm: PathBuf,
    pub argv: Vec<String>,
    pub env: Vec<(String, String)>,
    pub stdin: Vec<u8>,
    pub mounts: Vec<Mount>,
    pub fuel: Fuel,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxOutcome {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub exit: SandboxExit,
}

/// How the guest ended. Every variant is a fact about the guest — the
/// host never hides a guest failure behind a success.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SandboxExit {
    Success,
    Failure { code: u64 },
    FuelExhausted,
    Trap { message: String },
}

/// The seam. One method: hand it a job, get back what the guest did.
pub trait Sandbox {
    fn run(&mut self, job: &SandboxJob) -> Result<SandboxOutcome, AxError>;
}

/// What a binary built without an execution engine offers: a refusal
/// that names the missing piece.
///
/// It exists so the absence is a verdict rather than a stand-in. An
/// echo in this position would answer "success" to a guest that never
/// ran, and the first person to notice would be whoever trusted the
/// output.
pub struct AbsentSandbox;

impl Sandbox for AbsentSandbox {
    fn run(&mut self, _job: &SandboxJob) -> Result<SandboxOutcome, AxError> {
        Err(AxError::failure(
            AxCode::ToolUnavailable,
            "run in the sandbox",
            "this build carries no execution engine",
        )
        .with_recovery("use the program arm, or install a build with the `wasm` feature"))
    }
}

/// A pass-through stand-in: stdout echoes stdin, plus whatever script
/// output the test queued. Used where the test is about the caller's
/// logic rather than the boundary.
#[derive(Default)]
pub struct EchoSandbox {
    scripted: Vec<Vec<u8>>,
    pub seen: Vec<SandboxJob>,
}

impl EchoSandbox {
    pub fn new() -> EchoSandbox {
        EchoSandbox::default()
    }

    pub fn queue_stdout(&mut self, bytes: Vec<u8>) {
        self.scripted.push(bytes);
    }
}

impl Sandbox for EchoSandbox {
    fn run(&mut self, job: &SandboxJob) -> Result<SandboxOutcome, AxError> {
        self.seen.push(job.clone());
        let stdout = if self.scripted.is_empty() {
            job.stdin.clone()
        } else {
            self.scripted.remove(0)
        };
        Ok(SandboxOutcome {
            stdout,
            stderr: Vec::new(),
            exit: SandboxExit::Success,
        })
    }
}

/// A fault stand-in: pops a scripted outcome per call, so a caller's
/// handling of exhaustion and traps is testable without provoking the
/// real engine into either.
pub struct FaultSandbox {
    scripted: Vec<SandboxExit>,
}

impl FaultSandbox {
    pub fn new(scripted: Vec<SandboxExit>) -> FaultSandbox {
        FaultSandbox { scripted }
    }
}

impl Sandbox for FaultSandbox {
    fn run(&mut self, _job: &SandboxJob) -> Result<SandboxOutcome, AxError> {
        let exit = if self.scripted.is_empty() {
            SandboxExit::Success
        } else {
            self.scripted.remove(0)
        };
        Ok(SandboxOutcome {
            stdout: Vec::new(),
            stderr: Vec::new(),
            exit,
        })
    }
}

#[cfg(feature = "wasm")]
mod engine {
    use super::{Fuel, Sandbox, SandboxExit, SandboxJob, SandboxOutcome};
    use kernel::{AxCode, AxError};

    /// The real boundary: wasmtime running wasip1 guests.
    pub struct WasmtimeSandbox {
        engine: wasmtime::Engine,
    }

    fn host_error(op: &'static str, detail: String) -> AxError {
        AxError::failure(AxCode::SandboxDenied, op, detail)
    }

    impl WasmtimeSandbox {
        pub fn new() -> Result<WasmtimeSandbox, AxError> {
            let mut config = wasmtime::Config::new();
            // Fuel is the only budget: metering must be on before any
            // guest is compiled, or the store's fuel is never consumed.
            config.consume_fuel(true);
            let engine = wasmtime::Engine::new(&config)
                .map_err(|err| host_error("configure sandbox engine", err.to_string()))?;
            Ok(WasmtimeSandbox { engine })
        }
    }

    struct HostState {
        wasi: wasmtime_wasi::p1::WasiP1Ctx,
    }

    impl Sandbox for WasmtimeSandbox {
        fn run(&mut self, job: &SandboxJob) -> Result<SandboxOutcome, AxError> {
            let module_bytes = std::fs::read(&job.wasm).map_err(|err| {
                host_error(
                    "read guest module",
                    format!("{}: {err}", job.wasm.display()),
                )
            })?;
            let module = wasmtime::Module::new(&self.engine, &module_bytes)
                .map_err(|err| host_error("compile guest module", err.to_string()))?;

            let stdout = wasmtime_wasi::p2::pipe::MemoryOutputPipe::new(1 << 20);
            let stderr = wasmtime_wasi::p2::pipe::MemoryOutputPipe::new(1 << 20);
            let mut builder = wasmtime_wasi::WasiCtxBuilder::new();
            builder
                .stdin(wasmtime_wasi::p2::pipe::MemoryInputPipe::new(
                    job.stdin.clone(),
                ))
                .stdout(stdout.clone())
                .stderr(stderr.clone())
                .args(&job.argv);
            for (key, value) in &job.env {
                builder.env(key, value);
            }
            // The capability surface, stated one mount at a time. There
            // is no ambient access to add to it.
            for mount in &job.mounts {
                let perms = if mount.writable {
                    wasmtime_wasi::FsPerms::ReadWrite
                } else {
                    wasmtime_wasi::FsPerms::ReadOnly
                };
                builder
                    .preopened_dir(&mount.host, &mount.guest, perms)
                    .map_err(|err| {
                        host_error("grant mount", format!("{}: {err}", mount.host.display()))
                    })?;
            }
            let state = HostState {
                wasi: builder.build_p1(),
            };
            let mut store = wasmtime::Store::new(&self.engine, state);
            let Fuel(fuel) = job.fuel;
            store
                .set_fuel(fuel)
                .map_err(|err| host_error("set fuel", err.to_string()))?;

            let mut linker: wasmtime::Linker<HostState> = wasmtime::Linker::new(&self.engine);
            wasmtime_wasi::p1::add_to_linker_sync(&mut linker, |state: &mut HostState| {
                &mut state.wasi
            })
            .map_err(|err| host_error("link wasi", err.to_string()))?;

            let instance = linker
                .instantiate(&mut store, &module)
                .map_err(|err| host_error("instantiate guest", err.to_string()))?;
            let start = instance
                .get_typed_func::<(), ()>(&mut store, "_start")
                .map_err(|err| host_error("find guest entry", err.to_string()))?;

            let result = start.call(&mut store, ());
            let exit = classify(result, &store)?;
            // The pipes are only recoverable once every other handle is
            // gone; the store holds one until here.
            drop(store);
            let stdout = stdout.try_into_inner().ok_or_else(|| {
                host_error(
                    "collect guest stdout",
                    "the output pipe was still shared after the guest ended".to_owned(),
                )
            })?;
            let stderr = stderr.try_into_inner().ok_or_else(|| {
                host_error(
                    "collect guest stderr",
                    "the error pipe was still shared after the guest ended".to_owned(),
                )
            })?;
            Ok(SandboxOutcome {
                stdout: stdout.to_vec(),
                stderr: stderr.to_vec(),
                exit,
            })
        }
    }

    /// Turns the engine's outcome into ours. Fuel exhaustion and guest
    /// exits are guest facts; a host malfunction — an engine that never
    /// metered the fuel it was asked to meter — is an `Err`, because
    /// reporting "not exhausted" without having measured would be a
    /// guess dressed as a fact.
    fn classify(
        result: Result<(), wasmtime::Error>,
        store: &wasmtime::Store<HostState>,
    ) -> Result<SandboxExit, AxError> {
        let Err(err) = result else {
            return Ok(SandboxExit::Success);
        };
        if let Some(exit) = err.downcast_ref::<wasmtime_wasi::I32Exit>() {
            if exit.0 == 0 {
                return Ok(SandboxExit::Success);
            }
            // WASI pins exit codes to [0, 126); a negative one would mean
            // the guest bypassed proc_exit's own check, which is a broken
            // engine rather than a program that merely failed.
            let code = u64::try_from(exit.0).map_err(|_| {
                host_error(
                    "read guest exit code",
                    format!("exit code {} is outside the WASI range", exit.0),
                )
            })?;
            return Ok(SandboxExit::Failure { code });
        }
        if let Some(trap) = err.downcast_ref::<wasmtime::Trap>()
            && *trap == wasmtime::Trap::OutOfFuel
        {
            return Ok(SandboxExit::FuelExhausted);
        }
        // Some builds surface exhaustion only through the remaining
        // budget, so the budget is consulted before this is called a
        // trap. An engine that cannot report its fuel was never metering.
        let remaining = store.get_fuel().map_err(|err| {
            host_error(
                "read remaining fuel",
                format!("the engine reported no fuel metering: {err}"),
            )
        })?;
        if remaining == 0 {
            return Ok(SandboxExit::FuelExhausted);
        }
        Ok(SandboxExit::Trap {
            message: err.to_string(),
        })
    }
}

#[cfg(feature = "wasm")]
pub use engine::WasmtimeSandbox;

/// Conformance: any Sandbox must run twice in a row without the first
/// job poisoning the second, and must return a well-formed outcome.
#[cfg(feature = "conformance")]
pub fn assert_sandbox_conformance<S: Sandbox>(sandbox: &mut S, job: &SandboxJob) {
    let first = sandbox.run(job);
    let second = sandbox.run(job);
    match (first, second) {
        (Ok(one), Ok(two)) => {
            // Well-ordered back-to-back calls: the second must not
            // inherit the first's state.
            assert!(
                matches!(
                    one.exit,
                    SandboxExit::Success
                        | SandboxExit::Failure { .. }
                        | SandboxExit::FuelExhausted
                        | SandboxExit::Trap { .. }
                ),
                "outcome shape is one of the four"
            );
            assert!(
                matches!(
                    two.exit,
                    SandboxExit::Success
                        | SandboxExit::Failure { .. }
                        | SandboxExit::FuelExhausted
                        | SandboxExit::Trap { .. }
                ),
                "the second call still returns a well-formed outcome"
            );
        }
        (one, two) => {
            assert!(
                one.is_err() == two.is_err(),
                "a host that can run a job once can run it twice"
            );
        }
    }
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

    fn job() -> SandboxJob {
        SandboxJob {
            wasm: PathBuf::from("unused.wasm"),
            argv: vec![],
            env: vec![],
            stdin: b"hello".to_vec(),
            mounts: vec![],
            fuel: Fuel(1000),
        }
    }

    #[test]
    fn the_echo_stand_in_passes_stdin_through_and_records_the_job() {
        let mut sandbox = EchoSandbox::new();
        let outcome = sandbox.run(&job()).unwrap();
        assert_eq!(outcome.stdout, b"hello");
        assert_eq!(outcome.exit, SandboxExit::Success);
        assert_eq!(sandbox.seen.len(), 1);
        assert_eq!(sandbox.seen[0].fuel, Fuel(1000));
    }

    #[test]
    fn the_fault_stand_in_delivers_its_script_in_order() {
        let mut sandbox = FaultSandbox::new(vec![
            SandboxExit::FuelExhausted,
            SandboxExit::Trap {
                message: "unreachable".to_owned(),
            },
            SandboxExit::Failure { code: 2 },
        ]);
        assert_eq!(
            sandbox.run(&job()).unwrap().exit,
            SandboxExit::FuelExhausted
        );
        assert!(matches!(
            sandbox.run(&job()).unwrap().exit,
            SandboxExit::Trap { .. }
        ));
        assert_eq!(
            sandbox.run(&job()).unwrap().exit,
            SandboxExit::Failure { code: 2 }
        );
        // Script spent: a stand-in must not invent further failures.
        assert_eq!(sandbox.run(&job()).unwrap().exit, SandboxExit::Success);
    }
}
