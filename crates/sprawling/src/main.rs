// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! CLI entry. Subcommands land with their stages and are refused honestly
//! until then — a refusal that names what is missing beats a stub that
//! pretends (sprawling-SPEC.md). Live now: status, replay, init, serve,
//! export, restore, resume, fork.

mod assembly;
mod mcp_http;
mod mcp_stdio;

use std::process::ExitCode;

// CLIENT_FILES and CLIENT_COMPLETE: the gzipped client bundle the build
// wrote, and whether it is the whole client or only the page shell.
include!(concat!(env!("OUT_DIR"), "/client_embed.rs"));

/// Every crate this binary is built from, `name version` per line -
/// the embedded half of the bill of materials (`xtask sbom` writes the
/// CycloneDX file half).
const DEPENDENCIES: &str = include_str!(concat!(env!("OUT_DIR"), "/deps.txt"));

/// One line a person can read about what this binary carries.
fn client_summary() -> String {
    if CLIENT_COMPLETE {
        let total: usize = CLIENT_FILES.iter().map(|f| f.gz.len()).sum();
        format!(
            "client: embedded, {} file(s), {total} gzipped byte(s)",
            CLIENT_FILES.len()
        )
    } else {
        "client: page shell only - run `just build-web`, then rebuild this binary".to_owned()
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("status") => status(&args),
        Some("replay") => replay(args.get(1)),
        Some("init") => init(args.get(1)),
        Some("serve") => serve(args.get(1), args.get(2), &args),
        Some("export") => export(args.get(1), args.get(2)),
        Some("restore") => restore(args.get(1), args.get(2)),
        Some("resume") => resume(args.get(1)),
        Some("fork") => fork(&args),
        Some("adopt") => adopt(args.get(1), args.get(2)),
        Some(other) => {
            eprintln!("unknown subcommand: {other}");
            eprintln!(
                "available: status, init <dir>, serve <dir> [addr], resume <dir>, \
                 fork <dir> <run> <seq> [addr], adopt <dir> <addr>, replay <dir>, \
                 export <city> <dest>, restore <bundle> <city>"
            );
            ExitCode::from(2)
        }
        None => {
            eprintln!("usage: sprawling status [--log <level>]");
            ExitCode::from(2)
        }
    }
}

/// The genesis write: a city is born when city_initialized becomes line
/// zero of its ledger (walkthrough step 1).
fn init(dir: Option<&String>) -> ExitCode {
    let Some(dir) = dir else {
        eprintln!("usage: sprawling init <city-dir>");
        return ExitCode::from(2);
    };
    match assembly::init_city(std::path::Path::new(dir)) {
        Ok(report) => {
            println!(
                "city initialized: ledger at {} (genesis seq {})",
                report.ledger_dir.display(),
                report.genesis.seq().value()
            );
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("{err}");
            eprintln!("recovery: {}", err.recovery());
            ExitCode::FAILURE
        }
    }
}

/// Writes a bundle: the history, the objects it points at, and the work.
/// Credentials are not in it, because they are not in the city.
fn export(city: Option<&String>, dest: Option<&String>) -> ExitCode {
    let (Some(city), Some(dest)) = (city, dest) else {
        eprintln!("usage: sprawling export <city-dir> <bundle-dir>");
        return ExitCode::from(2);
    };
    match memory::Bundle::export(std::path::Path::new(city), std::path::Path::new(dest)) {
        Ok(manifest) => {
            println!(
                "exported {} record(s), {} object(s), {} file(s) to {dest}",
                manifest.records(),
                manifest.cas_objects(),
                manifest.files()
            );
            println!("chain head: {}", manifest.head());
            ExitCode::SUCCESS
        }
        Err(err) => report(err.into_ax()),
    }
}

/// Reads a bundle back into an empty directory. The chain is walked and
/// compared against the manifest before this reports success, so a short
/// copy is refused here rather than discovered later.
fn restore(bundle: Option<&String>, city: Option<&String>) -> ExitCode {
    let (Some(bundle), Some(city)) = (bundle, city) else {
        eprintln!("usage: sprawling restore <bundle-dir> <city-dir>");
        return ExitCode::from(2);
    };
    match memory::Bundle::restore(std::path::Path::new(bundle), std::path::Path::new(city)) {
        Ok(manifest) => {
            println!(
                "restored {} record(s) into {city}; chain head {}",
                manifest.records(),
                manifest.head()
            );
            ExitCode::SUCCESS
        }
        Err(err) => report(err.into_ax()),
    }
}

fn report(err: kernel::AxError) -> ExitCode {
    eprintln!("{err}");
    eprintln!("recovery: {}", err.recovery());
    ExitCode::FAILURE
}

/// Binds the control surface. Loopback unless an address says otherwise,
/// and an address beyond this machine needs `SPRAWLING_PAIRING_TOKEN` -
/// refused at startup, not at connect time.
fn serve(dir: Option<&String>, addr: Option<&String>, args: &[String]) -> ExitCode {
    let Some(dir) = dir else {
        eprintln!("usage: sprawling serve <city-dir> [addr] [--log <level>] [--web-dir <dir>]");
        return ExitCode::from(2);
    };
    // The address is the first non-flag argument after the city dir, so
    // `serve city --log off` does not read `--log` as an address.
    let raw = addr
        .filter(|a| !a.starts_with("--"))
        .map_or("127.0.0.1:8787", String::as_str);
    let Ok(bind) = raw.parse() else {
        eprintln!("not a socket address: {raw}");
        eprintln!("recovery: give host:port, for example 127.0.0.1:8787");
        return ExitCode::from(2);
    };
    // The client source: embedded by default; a directory for the
    // development loop, read per request so an edit shows on refresh.
    let client = match flag_value(args, "--web-dir") {
        Some(dir) => channels::ClientAssets::Disk(std::path::PathBuf::from(dir)),
        None => channels::ClientAssets::Embedded(CLIENT_FILES),
    };
    if let channels::ClientAssets::Embedded(_) = &client
        && !CLIENT_COMPLETE
    {
        eprintln!(
            "warning: this binary carries the page shell only; the browser will get an empty \
             page. Run `just build-web`, rebuild, or pass --web-dir target/web-dist"
        );
    }
    // The token is read once here and never stored: `serve` is handed a
    // digest. This is the only place in the process that sees it.
    let token = std::env::var("SPRAWLING_PAIRING_TOKEN").ok();
    let runtime = match tokio::runtime::Runtime::new() {
        Ok(runtime) => runtime,
        Err(err) => {
            eprintln!("could not start the async runtime: {err}");
            return ExitCode::FAILURE;
        }
    };
    println!("serving {raw} - {}", client_summary());
    if let channels::ClientAssets::Disk(dir) = &client {
        println!("client: read per request from {}", dir.display());
    }
    let log = match log_floor(args) {
        Ok(Some(level)) => {
            println!("log: {level}");
            // The one place a diagnostic line is written out, and the
            // one place a clock may be sampled: a sink that wants a
            // timestamp adds it here, never in the library.
            runtime::diagnostics::Diagnostics::new(
                level,
                Box::new(|line: &str| eprintln!("{line}")),
            )
        }
        Ok(None) => runtime::diagnostics::Diagnostics::off(),
        Err(unknown) => {
            eprintln!("not a log level: {unknown}");
            eprintln!("recovery: {}", log_levels());
            return ExitCode::from(2);
        }
    };
    let (vault, vault_notice) = assembly::open_vault();
    match runtime.block_on(assembly::serve(
        std::path::Path::new(dir),
        bind,
        token.as_deref(),
        client,
        vault,
        vault_notice,
        log,
    )) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{err}");
            eprintln!("recovery: {}", err.recovery());
            ExitCode::FAILURE
        }
    }
}

/// The startup scan: verify the chain, close every tool call whose
/// outcome a process death left unknown, and say what still waits on a
/// person. `serve` continues approved work; this is the offline half.
fn resume(dir: Option<&String>) -> ExitCode {
    let Some(dir) = dir else {
        eprintln!("usage: sprawling resume <city-dir>");
        return ExitCode::from(2);
    };
    let (vault, _notice) = assembly::open_vault();
    let outcome = assembly::RunWorker::new(
        std::path::Path::new(dir),
        vault,
        runtime::diagnostics::Diagnostics::off(),
    )
    .and_then(|mut worker| worker.startup_scan());
    match outcome {
        Ok(report) => {
            println!("{}", report.summary());
            if report.waiting_approvals > 0 {
                println!("answer them in the interface: sprawling serve {dir}");
            }
            ExitCode::SUCCESS
        }
        Err(err) => report(err),
    }
}

/// Records a fork: a new run identity branched from an event node. The
/// lineage is the record; dispatching into it is the person's next move.
fn fork(args: &[String]) -> ExitCode {
    let (Some(dir), Some(run_raw), Some(seq_raw)) = (args.get(1), args.get(2), args.get(3)) else {
        eprintln!("usage: sprawling fork <city-dir> <run-id> <at-seq> [addr]");
        return ExitCode::from(2);
    };
    let run = match kernel::RunId::parse(run_raw) {
        Ok(run) => run,
        Err(err) => return report(err),
    };
    let Ok(seq) = seq_raw.parse::<u64>() else {
        eprintln!("not a sequence number: {seq_raw}");
        return ExitCode::from(2);
    };
    let addr = match args.get(4).map(|raw| kernel::Address::parse(raw)) {
        None => None,
        Some(Ok(addr)) => Some(addr),
        Some(Err(err)) => return report(err),
    };
    let (vault, _notice) = assembly::open_vault();
    let outcome = assembly::RunWorker::new(
        std::path::Path::new(dir),
        vault,
        runtime::diagnostics::Diagnostics::off(),
    )
    .and_then(|mut worker| worker.fork(run, kernel::Seq::new(seq), addr));
    match outcome {
        Ok(new_run) => {
            println!("forked as {new_run}");
            println!("dispatch into the address when ready; the lineage is recorded");
            ExitCode::SUCCESS
        }
        Err(err) => report(err),
    }
}

/// Adopts an existing directory under the city as a building: BUILDING.md
/// and the missing spine files are laid, nothing found is overwritten.
fn adopt(dir: Option<&String>, addr: Option<&String>) -> ExitCode {
    let (Some(dir), Some(addr_raw)) = (dir, addr) else {
        eprintln!("usage: sprawling adopt <city-dir> <addr>");
        eprintln!("move or clone the directory under the city first, then adopt it");
        return ExitCode::from(2);
    };
    let addr = match kernel::Address::parse(addr_raw) {
        Ok(addr) => addr,
        Err(err) => return report(err),
    };
    let (vault, _notice) = assembly::open_vault();
    let outcome = assembly::RunWorker::new(
        std::path::Path::new(dir),
        vault,
        runtime::diagnostics::Diagnostics::off(),
    )
    .and_then(|mut worker| worker.adopt_building(addr.clone()));
    match outcome {
        Ok(()) => {
            println!(
                "adopted {} - its files are untouched, its rules are new",
                addr.as_str()
            );
            println!("edit {}/BUILDING.md to shape them", addr.as_str());
            ExitCode::SUCCESS
        }
        Err(err) => report(err),
    }
}

/// Offline chain verification (A2); strictly read-only.
fn replay(dir: Option<&String>) -> ExitCode {
    let Some(dir) = dir else {
        eprintln!("usage: sprawling replay <ledger-dir>");
        return ExitCode::from(2);
    };
    match runtime::replay::verify_ledger_dir(std::path::Path::new(dir)) {
        Ok(verified) => {
            println!(
                "chain verified: {} line(s), tail seq {}",
                verified.raw_lines().len(),
                verified
                    .tail_seq()
                    .map_or_else(|| "none".to_string(), |s| s.value().to_string())
            );
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("{err}");
            eprintln!("recovery: {}", err.recovery());
            ExitCode::FAILURE
        }
    }
}

fn status(args: &[String]) -> ExitCode {
    if args.iter().any(|a| a == "--deps") {
        print!("{DEPENDENCIES}");
        return ExitCode::SUCCESS;
    }
    println!("sprawling {} (pre-alpha)", env!("CARGO_PKG_VERSION"));
    println!("{}", client_summary());
    println!(
        "built from {} crate(s); list them with status --deps",
        DEPENDENCIES.lines().count()
    );
    match log_floor(args) {
        Ok(Some(level)) => println!("log: {level} and everything a wider audience reads"),
        Ok(None) => println!("log: off"),
        Err(unknown) => {
            eprintln!("not a log level: {unknown}");
            eprintln!("recovery: {}", log_levels());
            return ExitCode::from(2);
        }
    }
    ExitCode::SUCCESS
}

/// The value following a `--flag`, if present.
fn flag_value(args: &[String], flag: &str) -> Option<String> {
    let mut pairs = args.windows(2);
    pairs.find_map(|pair| match pair {
        [name, value] if name == flag => Some(value.clone()),
        _ => None,
    })
}

/// The floor `--log <level>` asks for. Absent means the default floor;
/// `--log off` means nothing is written.
///
/// # Errors
/// Returns the word that is not a level, so the caller can name it.
fn log_floor(args: &[String]) -> Result<Option<runtime::diagnostics::Level>, String> {
    let Some(asked) = flag_value(args, "--log") else {
        return Ok(Some(runtime::diagnostics::Level::DEFAULT));
    };
    if asked == "off" {
        return Ok(None);
    }
    runtime::diagnostics::Level::parse(&asked)
        .map(Some)
        .ok_or(asked)
}

fn log_levels() -> String {
    let names: Vec<&str> = runtime::diagnostics::Level::ALL
        .into_iter()
        .map(|level| level.as_str())
        .collect();
    format!("use --log with one of {}, or off", names.join(", "))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
mod tests {
    use super::{CLIENT_COMPLETE, CLIENT_FILES};

    /// The embed chain delivers a file table with the page shell in it;
    /// when the wasm client was built, the table carries it too.
    #[test]
    fn embedded_client_table_is_present_and_marked() {
        let index = CLIENT_FILES
            .iter()
            .find(|f| f.path == "index.html")
            .expect("the page shell is always embedded");
        assert!(!index.gz.is_empty());
        if CLIENT_COMPLETE {
            for needed in ["web.js", "web_bg.wasm"] {
                assert!(
                    CLIENT_FILES.iter().any(|f| f.path == needed),
                    "a complete client carries {needed}"
                );
            }
        }
    }
}
