// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! Gate runner. One gate per module; `gates` runs them all in order.
//! Exit codes: 0 clean, 1 violations found, 2 usage or gate-internal failure.
//! A broken gate must fail loudly (code 2): silent passes are the worst
//! failure mode a gate can have (xtask-SPEC.md section 12).

mod apisync;
mod ax;
mod badge;
mod budget;
mod color;
mod depmap;
mod gates;
mod guard;
mod header;
mod length;
mod lexicon;
mod mem;
mod modmap;
mod package;
mod release;
mod report;
mod repro;
mod sbom;
mod secret;
mod spec;
mod specalign;
mod vocabulary;
mod walk;
mod wiring;

use std::path::PathBuf;
use std::process::ExitCode;

use report::XtaskError;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let root = match repo_root() {
        Ok(root) => root,
        Err(err) => return report::internal_failure(&err),
    };
    let range = range_arg(&args);
    match args.first().map(String::as_str) {
        Some("gates") => gates::run(&root, range.as_deref()),
        Some("color") => report::finish("color", color::check(&root)),
        Some("ax") => report::finish("ax", ax::check(&root)),
        Some("budget") => match budget::report(&root) {
            Ok(text) => {
                print!("{text}");
                ExitCode::SUCCESS
            }
            Err(err) => report::internal_failure(&err),
        },
        Some("badge") if args.iter().any(|a| a == "--write") => match badge::write(&root) {
            Ok(message) => {
                print!("{message}");
                ExitCode::SUCCESS
            }
            Err(err) => report::internal_failure(&err),
        },
        Some("badge") => report::finish("budget", badge::check(&root)),
        Some("mem") => match mem::run(&root, args.get(1).map(String::as_str)) {
            Ok(text) => {
                println!("{text}");
                ExitCode::SUCCESS
            }
            Err(err) => report::internal_failure(&err),
        },
        Some("package") => match package::run(&root) {
            Ok(message) => {
                print!("{message}");
                ExitCode::SUCCESS
            }
            Err(err) => report::internal_failure(&err),
        },
        Some("sbom") => match sbom::run(&root) {
            Ok(message) => {
                println!("{message}");
                ExitCode::SUCCESS
            }
            Err(err) => report::internal_failure(&err),
        },
        Some("repro") => match repro::run(&root, args.iter().any(|a| a == "--full")) {
            Ok(message) => {
                println!("{message}");
                ExitCode::SUCCESS
            }
            Err(err) => report::internal_failure(&err),
        },
        Some("secret") => report::finish("secret", secret::check(&root)),
        Some("specalign") => report::finish("specalign", specalign::check(&root)),
        Some("wiring") => report::finish("wiring", wiring::check(&root)),
        Some("apisync") if args.iter().any(|a| a == "--write") => match apisync::write(&root) {
            Ok(()) => ExitCode::SUCCESS,
            Err(err) => report::internal_failure(&err),
        },
        Some("apisync") => report::finish("apisync", apisync::check(&root, range.as_deref())),
        Some("header") => report::finish("header", header::check(&root)),
        Some("lexicon") => report::finish("lexicon", lexicon::check(&root)),
        Some("length") => report::finish("length", length::check(&root)),
        Some("modmap") => report::finish("modmap", modmap::check(&root)),
        Some("depmap") => report::finish("depmap", depmap::check(&root)),
        Some("guard") => report::finish("guard", guard::check(&root, range.as_deref())),
        Some("release") => report::finish("release", release::check(&root)),
        Some("spec") => match spec::run(&root, args.get(1).map(String::as_str)) {
            Ok(message) => {
                println!("{message}");
                ExitCode::SUCCESS
            }
            Err(err) => report::internal_failure(&err),
        },
        Some(other) => {
            eprintln!("unknown subcommand: {other}");
            usage();
            ExitCode::from(2)
        }
        None => {
            usage();
            ExitCode::from(2)
        }
    }
}

/// The repo root is the parent of the xtask manifest directory.
fn repo_root() -> Result<PathBuf, XtaskError> {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    match manifest.parent() {
        Some(parent) => Ok(parent.to_path_buf()),
        None => Err(XtaskError::Doc {
            file: "CARGO_MANIFEST_DIR".to_owned(),
            msg: "xtask manifest directory has no parent".to_owned(),
        }),
    }
}

/// `--range <spec>` anywhere after the subcommand (used by guard, forwarded by gates).
fn range_arg(args: &[String]) -> Option<String> {
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        if arg == "--range" {
            return it.next().cloned();
        }
    }
    None
}

fn usage() {
    eprintln!(
        "usage: cargo xtask <gates|header|lexicon|modmap|depmap|secret|color|ax|wiring|specalign|apisync|guard> [--range a..b] [--write]"
    );
    eprintln!(
        "       cargo xtask spec <crate> | budget | badge [--write] | mem [pid] | sbom | package | repro [--full]"
    );
}
