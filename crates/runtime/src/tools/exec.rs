// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The exec tool: exactly three arms, each with its own failure story.
//!
//! Program runs a host process with a pinned working directory and an
//! environment allowlist — secrets are never passed through, because a
//! child process inherits whatever it is given and cannot be asked to
//! forget. Python runs inside the sandbox, where the capability surface
//! is the mount list and there is no network at all. Shell probes for
//! an interpreter and **refuses when there is none**, rather than
//! quietly rewriting the request as a Program call: a shell line that
//! silently becomes something else is a worse answer than a refusal
//! naming what is missing.
//!
//! A missing component is `E_TOOL_UNAVAILABLE` carrying the alternative
//! that would work, so the caller redirects instead of guessing.

use std::path::PathBuf;

use kernel::{
    AxCode, AxError, CostTier, Effect, ExecArm, Payload, RenderIntent, Temporal, Tool, ToolCall,
    ToolMeta, ToolName, ToolOutcome,
};
use serde_json::{Map, Value};

use crate::sandbox::{Fuel, Mount, Sandbox, SandboxExit, SandboxJob};

/// Environment variables a child may inherit. Everything else is
/// dropped: an allowlist stays safe when the process environment grows,
/// which a denylist does not.
const ENV_ALLOWLIST: [&str; 4] = ["PATH", "LANG", "LC_ALL", "TZ"];

pub struct ExecTool {
    workdir: PathBuf,
    mounts: Vec<Mount>,
    python_wasm: Option<PathBuf>,
    sandbox: Box<dyn Sandbox>,
    shell: Option<PathBuf>,
    fuel: Fuel,
    meta: ToolMeta,
}

impl ExecTool {
    pub fn new(
        workdir: PathBuf,
        mounts: Vec<Mount>,
        python_wasm: Option<PathBuf>,
        sandbox: Box<dyn Sandbox>,
        shell: Option<PathBuf>,
        fuel: Fuel,
        domain: kernel::Address,
    ) -> Result<ExecTool, AxError> {
        let mut params = Map::new();
        params.insert("type".to_owned(), Value::String("object".to_owned()));
        let mut properties = Map::new();
        let mut arm = Map::new();
        arm.insert("type".to_owned(), Value::String("object".to_owned()));
        arm.insert(
            "description".to_owned(),
            Value::String(
                "one of {program:{path,args}}, {python:{code}}, {shell:{text}}".to_owned(),
            ),
        );
        properties.insert("arm".to_owned(), Value::Object(arm));
        params.insert("properties".to_owned(), Value::Object(properties));
        params.insert(
            "required".to_owned(),
            Value::Array(vec![Value::String("arm".to_owned())]),
        );
        Ok(ExecTool {
            workdir,
            mounts,
            python_wasm,
            sandbox,
            shell,
            fuel,
            meta: ToolMeta {
                name: ToolName::parse("exec")?,
                disclosure: "Run a program, a Python snippet, or a shell line.".to_owned(),
                params: Payload::new(params)?,
                effect: Effect::Write { domain },
                cost_tier: CostTier::Heavy,
                timeout: None,
                render: RenderIntent::Terminal,
                temporal: Temporal::Timestamped,
            },
        })
    }

    fn run_program(&self, path: &str, args: &[String]) -> Result<ToolOutcome, AxError> {
        let mut command = std::process::Command::new(path);
        command.current_dir(&self.workdir).args(args).env_clear();
        for key in ENV_ALLOWLIST {
            if let Ok(value) = std::env::var(key) {
                command.env(key, value);
            }
        }
        let output = command.output().map_err(|err| {
            AxError::failure(
                AxCode::ToolUnavailable,
                "run program",
                format!("{path}: {err}"),
            )
            .with_recovery("check the program name, or use the shell arm")
        })?;
        let code = output.status.code().unwrap_or(-1);
        outcome(&output.stdout, &output.stderr, i64::from(code), "program")
    }

    fn run_python(&mut self, code: &str) -> Result<ToolOutcome, AxError> {
        let Some(wasm) = self.python_wasm.clone() else {
            return Err(AxError::failure(
                AxCode::ToolUnavailable,
                "run python",
                "no CPython-WASI component is configured",
            )
            .with_recovery("use the program arm, or configure a CPython-WASI component"));
        };
        let job = SandboxJob {
            wasm,
            argv: vec!["python".to_owned(), "-c".to_owned(), code.to_owned()],
            env: Vec::new(),
            stdin: Vec::new(),
            mounts: self.mounts.clone(),
            fuel: self.fuel,
        };
        let result = self.sandbox.run(&job)?;
        let exit_code = match &result.exit {
            SandboxExit::Success => 0,
            SandboxExit::Failure { code } => i64::try_from(*code).unwrap_or(i64::MAX),
            // Exhaustion and traps are guest facts the caller must see
            // as themselves, not flattened into a generic non-zero exit.
            SandboxExit::FuelExhausted => {
                return exceptional(&result.stdout, &result.stderr, "fuel_exhausted", None);
            }
            SandboxExit::Trap { message } => {
                return exceptional(
                    &result.stdout,
                    &result.stderr,
                    "trap",
                    Some(message.clone()),
                );
            }
        };
        outcome(&result.stdout, &result.stderr, exit_code, "python")
    }

    fn run_shell(&self, text: &str) -> Result<ToolOutcome, AxError> {
        let Some(shell) = &self.shell else {
            return Err(AxError::failure(
                AxCode::ToolUnavailable,
                "run shell",
                "no shell interpreter was found",
            )
            .with_recovery("use the program arm with an explicit executable"));
        };
        let flag = if cfg!(windows) { "/C" } else { "-c" };
        let mut command = std::process::Command::new(shell);
        command
            .current_dir(&self.workdir)
            .arg(flag)
            .arg(text)
            .env_clear();
        for key in ENV_ALLOWLIST {
            if let Ok(value) = std::env::var(key) {
                command.env(key, value);
            }
        }
        let output = command.output().map_err(|err| {
            AxError::failure(
                AxCode::ToolUnavailable,
                "run shell",
                format!("{}: {err}", shell.display()),
            )
        })?;
        let code = output.status.code().unwrap_or(-1);
        outcome(&output.stdout, &output.stderr, i64::from(code), "shell")
    }
}

fn outcome(
    stdout: &[u8],
    stderr: &[u8],
    exit_code: i64,
    arm: &str,
) -> Result<ToolOutcome, AxError> {
    let mut result = Map::new();
    result.insert("arm".to_owned(), Value::String(arm.to_owned()));
    result.insert(
        "stdout".to_owned(),
        Value::String(String::from_utf8_lossy(stdout).into_owned()),
    );
    result.insert(
        "stderr".to_owned(),
        Value::String(String::from_utf8_lossy(stderr).into_owned()),
    );
    result.insert("exit_code".to_owned(), Value::Number(exit_code.into()));
    Ok(ToolOutcome {
        result: Payload::new(result)?,
    })
}

fn exceptional(
    stdout: &[u8],
    stderr: &[u8],
    kind: &str,
    detail: Option<String>,
) -> Result<ToolOutcome, AxError> {
    let mut result = Map::new();
    result.insert("arm".to_owned(), Value::String("python".to_owned()));
    result.insert(
        "stdout".to_owned(),
        Value::String(String::from_utf8_lossy(stdout).into_owned()),
    );
    result.insert(
        "stderr".to_owned(),
        Value::String(String::from_utf8_lossy(stderr).into_owned()),
    );
    result.insert("outcome".to_owned(), Value::String(kind.to_owned()));
    if let Some(detail) = detail {
        result.insert("detail".to_owned(), Value::String(detail));
    }
    Ok(ToolOutcome {
        result: Payload::new(result)?,
    })
}

/// Reads the arm out of the call. An unrecognised shape is refused
/// rather than defaulted to shell — guessing which arm was meant is how
/// a program run becomes a shell injection.
pub fn parse_arm(args: &Map<String, Value>) -> Result<ExecArm, AxError> {
    let arm = args
        .get("arm")
        .ok_or_else(|| AxError::failure(AxCode::InvalidArgs, "run exec", "missing `arm`"))?;
    serde_json::from_value(arm.clone()).map_err(|err| {
        AxError::failure(
            AxCode::InvalidArgs,
            "run exec",
            format!("unrecognised arm: {err}"),
        )
        .with_recovery("use one of program, python, shell")
    })
}

impl Tool for ExecTool {
    fn meta(&self) -> &ToolMeta {
        &self.meta
    }

    fn invoke(&mut self, call: &ToolCall) -> Result<ToolOutcome, AxError> {
        if call.name != self.meta.name {
            return Err(AxError::failure(
                AxCode::InvalidArgs,
                "run exec",
                format!("call routed to the wrong tool: {}", call.name.as_str()),
            ));
        }
        match parse_arm(call.args.as_map())? {
            ExecArm::Program { path, args } => self.run_program(&path, &args),
            ExecArm::Python { code } => self.run_python(&code),
            ExecArm::Shell { text } => self.run_shell(&text),
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
    use crate::sandbox::{EchoSandbox, FaultSandbox};
    use kernel::Address;

    fn call(arm: Value) -> ToolCall {
        let mut args = Map::new();
        args.insert("arm".to_owned(), arm);
        ToolCall {
            id: "c1".to_owned(),
            name: ToolName::parse("exec").unwrap(),
            args: Payload::new(args).unwrap(),
        }
    }

    fn tool(
        python: Option<PathBuf>,
        sandbox: Box<dyn Sandbox>,
        shell: Option<PathBuf>,
    ) -> ExecTool {
        ExecTool::new(
            std::env::temp_dir(),
            Vec::new(),
            python,
            sandbox,
            shell,
            Fuel(1_000_000),
            Address::parse("work").unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn a_missing_component_refuses_and_names_the_alternative() {
        let mut tool = tool(None, Box::new(EchoSandbox::new()), None);
        let err = match tool.invoke(&call(
            serde_json::json!({ "python": { "code": "print(1)" } }),
        )) {
            Err(err) => err,
            Ok(_) => panic!("a missing component must refuse"),
        };
        assert_eq!(*err.code(), AxCode::ToolUnavailable);
        assert!(
            err.recovery().contains("program arm"),
            "the refusal carries the alternative"
        );

        // Shell refuses too rather than silently becoming a program run.
        let err = match tool.invoke(&call(serde_json::json!({ "shell": { "text": "ls" } }))) {
            Err(err) => err,
            Ok(_) => panic!("a missing shell must refuse"),
        };
        assert_eq!(*err.code(), AxCode::ToolUnavailable);
    }

    #[test]
    fn the_python_arm_runs_in_the_sandbox_and_reports_its_own_exit() {
        let mut echo = EchoSandbox::new();
        echo.queue_stdout(b"42\n".to_vec());
        let mut tool = tool(Some(PathBuf::from("python.wasm")), Box::new(echo), None);
        let outcome = tool
            .invoke(&call(
                serde_json::json!({ "python": { "code": "print(42)" } }),
            ))
            .unwrap();
        let result = serde_json::to_value(&outcome.result).unwrap();
        assert_eq!(result["stdout"], "42\n");
        assert_eq!(result["exit_code"], 0);
        assert_eq!(result["arm"], "python");
    }

    #[test]
    fn exhaustion_and_traps_reach_the_caller_as_themselves() {
        let sandbox = FaultSandbox::new(vec![
            SandboxExit::FuelExhausted,
            SandboxExit::Trap {
                message: "unreachable".to_owned(),
            },
        ]);
        let mut tool = tool(Some(PathBuf::from("python.wasm")), Box::new(sandbox), None);
        let first = tool
            .invoke(&call(
                serde_json::json!({ "python": { "code": "while True: pass" } }),
            ))
            .unwrap();
        assert_eq!(
            serde_json::to_value(&first.result).unwrap()["outcome"],
            "fuel_exhausted"
        );
        let second = tool
            .invoke(&call(serde_json::json!({ "python": { "code": "boom" } })))
            .unwrap();
        let value = serde_json::to_value(&second.result).unwrap();
        assert_eq!(value["outcome"], "trap");
        assert_eq!(value["detail"], "unreachable");
    }

    #[test]
    fn an_unrecognised_arm_is_refused_not_guessed() {
        let mut tool = tool(None, Box::new(EchoSandbox::new()), None);
        let err = match tool.invoke(&call(serde_json::json!({ "bash": { "text": "ls" } }))) {
            Err(err) => err,
            Ok(_) => panic!("an unknown arm must refuse"),
        };
        assert_eq!(*err.code(), AxCode::InvalidArgs);
    }

    #[test]
    fn the_program_arm_runs_a_real_child_with_a_scrubbed_environment() {
        let mut tool = tool(None, Box::new(EchoSandbox::new()), None);
        // A program every supported host has, printing nothing useful:
        // the assertion is that it ran and reported its own exit code.
        let (path, args) = if cfg!(windows) {
            ("cmd", vec!["/C".to_owned(), "exit 3".to_owned()])
        } else {
            ("sh", vec!["-c".to_owned(), "exit 3".to_owned()])
        };
        let outcome = tool
            .invoke(&call(serde_json::json!({
                "program": { "path": path, "args": args }
            })))
            .unwrap();
        let result = serde_json::to_value(&outcome.result).unwrap();
        assert_eq!(result["exit_code"], 3, "{result}");
        assert_eq!(result["arm"], "program");
    }
}
