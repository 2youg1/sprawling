// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Copyright (c) 2026 2youg1

//! CLI entry. Subcommands land with their stages and are refused honestly
//! until then — a refusal that names what is missing beats a stub that
//! pretends (sprawling-SPEC.md). Live now: status, replay, init, serve,
//! export, restore, resume, fork.

// The city harness is the library half of this package (`src/lib.rs`);
// these two are the binary's own. `install` puts this executable where a
// shell will find it, and `wire_client` talks to a served city from a
// terminal - both are about the command line rather than about a city.
mod install;
mod wire_client;

use sprawling::{assembly, console, firstrun};

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
            "embedded, {} file(s), {total} gzipped byte(s)",
            CLIENT_FILES.len()
        )
    } else {
        "page shell only - run `just build-web`, then rebuild this binary".to_owned()
    }
}

/// The command list, in one place. The refusal of an unknown subcommand
/// and the first screen print the same text, so neither can fall behind
/// what the binary actually accepts.
const COMMANDS: &str = "\
commands:
  up [dir] [addr]              raise a city here if needed, serve it, open the WebUI
  install [--uninstall]        put this binary on your PATH, or take it back off
  init <dir> [--adopt]         raise a city: writes the genesis record
                               (--adopt: every folder there becomes a building)
  serve <dir> [addr] [--open]  serve a city that already exists
                               (--console enters it, --no-console does not)
  resume <dir>                 after a restart: verify, close what was lost, report
  status [--deps]              this binary: version, client, what it is built from
  fork <dir> <run> <seq>       branch a lineage from one step of a run
  adopt <dir> <addr>           take an existing directory in as a building
  call <frame|-> [--at a]      send one wire frame, print every frame back
  enrol <realm>/<name>         read a credential from stdin, hand it to a city
  replay <dir>                 verify a chain offline, read-only
  export <city> <dest>         pack a whole city
  restore <bundle> <city>      unpack it on another machine";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("status") => status(&args),
        Some("replay") => replay(named(&args, 1)),
        Some("init") => init(&args),
        Some("up") => up(&args),
        Some("install") => install(&args),
        Some("call") => call(&args),
        Some("enrol" | "enroll") => enrol(&args),
        Some("serve") => serve(named(&args, 1), named(&args, 2), &args),
        Some("export") => export(named(&args, 1), named(&args, 2)),
        Some("restore") => restore(named(&args, 1), named(&args, 2)),
        Some("resume") => resume(named(&args, 1)),
        Some("fork") => fork(&args),
        Some("adopt") => adopt(named(&args, 1), named(&args, 2)),
        Some("help" | "--help" | "-h") => {
            println!("{COMMANDS}");
            ExitCode::SUCCESS
        }
        Some(other) => {
            eprintln!("unknown subcommand: {other}");
            eprintln!("{COMMANDS}");
            ExitCode::from(2)
        }
        None => first_screen(),
    }
}

/// The nth positional argument: a word that is not a flag.
///
/// Without this, `sprawling init --help` raises a city in a directory
/// called `--help` - which is what the repository root of this project
/// held for a day. A flag is never a path, and the subcommands that take
/// a path all read it through here.
fn named(args: &[String], nth: usize) -> Option<&String> {
    args.iter()
        .skip(1)
        .filter(|arg| !arg.starts_with("--"))
        .nth(nth.saturating_sub(1))
}

/// What a launch with no command gets. Most of those come from a file
/// manager, where the console closes the moment this returns - so every
/// path out of here holds the window until somebody has read it.
fn first_screen() -> ExitCode {
    let city = default_city_location();
    let answered = firstrun::ask(&city, &mut std::io::stdin().lock(), &mut std::io::stdout());
    let code = match answered {
        Ok(firstrun::FirstScreen::Start(city)) => up_at(&city, "127.0.0.1:8787", &[]),
        Ok(firstrun::FirstScreen::Use(folder)) => use_folder(&folder),
        Ok(firstrun::FirstScreen::Quit) => {
            println!("{COMMANDS}");
            ExitCode::from(2)
        }
        Err(err) => {
            eprintln!("could not read the answer: {err}");
            ExitCode::FAILURE
        }
    };
    hold();
    code
}

/// Keeps the window long enough to be read. A console opened by a file
/// manager closes with the process, which is how a refusal becomes an
/// unexplained flash. With nobody at the keyboard this returns at once.
fn hold() {
    println!("\n  Press Enter to close.");
    let mut ignored = String::new();
    // A failed read means there is nobody to wait for, which is the same
    // outcome as being waited for: this process is ending either way.
    let _ = std::io::stdin().read_line(&mut ignored);
}

/// Where `up` and the first screen put a city nobody named.
fn default_city_location() -> std::path::PathBuf {
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(std::path::PathBuf::from);
    match std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(std::path::Path::to_path_buf))
    {
        Some(beside) => {
            let writable = firstrun::is_writable(&beside);
            firstrun::default_city(&beside, home.as_deref(), writable)
        }
        None => firstrun::default_city(std::path::Path::new("."), home.as_deref(), true),
    }
}

/// The one command that makes a city run: raise it when it is not there,
/// serve it, and open the WebUI once the port answers. The first screen
/// and the launcher in the release archive both arrive here, so the
/// sequence has exactly one definition and `init` and `serve` keep theirs.
fn up(args: &[String]) -> ExitCode {
    let city = match args.get(1).filter(|a| !a.starts_with("--")) {
        Some(dir) => std::path::PathBuf::from(dir),
        None => default_city_location(),
    };
    let addr = args
        .get(2)
        .filter(|a| !a.starts_with("--"))
        .map_or("127.0.0.1:8787", String::as_str);
    up_at(&city, addr, args)
}

/// A folder the person already works in becomes a city around their
/// work.
///
/// The folder has to be there. A path that is not a directory is
/// reported and nothing is created: the alternative is making a city out
/// of a typo, at a location nobody looked at.
fn use_folder(folder: &std::path::Path) -> ExitCode {
    if !folder.is_dir() {
        eprintln!("{} is not a folder on this machine", folder.display());
        eprintln!(
            "recovery: paste the path of a folder you already work in, or press Enter to start a new city"
        );
        return ExitCode::FAILURE;
    }
    if assembly::has_history(folder) {
        println!("{} is already a city; opening it", folder.display());
        return serve_city(folder, "127.0.0.1:8787", &[], true);
    }
    match assembly::form_city(folder, assembly::Adopt::EveryFolder) {
        Ok(report) => {
            report_standing(&report);
            serve_city(folder, "127.0.0.1:8787", &[], true)
        }
        Err(err) => report(err),
    }
}

/// What forming a city found, and what it did about it. Printed rather
/// than assumed, because the person is watching their own work being
/// taken in.
fn report_standing(report: &assembly::InitReport) {
    println!(
        "city raised: ledger at {} (genesis seq {})",
        report.ledger_dir.display(),
        report.genesis.seq().value()
    );
    match &report.standing {
        city::Standing::Empty => println!("the folder was empty; nothing was there to touch"),
        city::Standing::AlreadyACity => println!("the folder was already a city"),
        city::Standing::Work { adoptable, loose } => {
            println!(
                "found {} folder(s) and {loose} other item(s); nothing in them was read, moved or rewritten",
                adoptable.len()
            );
        }
        // A standing this build does not know is reported by name
        // rather than passed over: the person is watching their own
        // work being taken in.
        other => println!("the folder is in a state this build does not describe: {other:?}"),
    }
    for addr in &report.adopted {
        println!(
            "  {} is now a building - edit its rules on its page",
            addr.as_str()
        );
    }
}

fn up_at(city: &std::path::Path, raw: &str, args: &[String]) -> ExitCode {
    if !assembly::has_history(city) {
        match assembly::init_city(city) {
            Ok(raised) => println!(
                "city raised at {} (genesis seq {})",
                raised.ledger_dir.display(),
                raised.genesis.seq().value()
            ),
            Err(err) => return report(err),
        }
    }
    serve_city(city, raw, args, true)
}

/// The genesis write: a city is born when city_initialized becomes line
/// zero of its ledger (walkthrough step 1).
fn init(args: &[String]) -> ExitCode {
    let Some(dir) = named(args, 1) else {
        eprintln!("usage: sprawling init <city-dir> [--adopt]");
        eprintln!("--adopt turns every folder already there into a building");
        return ExitCode::from(2);
    };
    let adopt = if args.iter().any(|arg| arg == "--adopt") {
        assembly::Adopt::EveryFolder
    } else {
        assembly::Adopt::Nothing
    };
    match assembly::form_city(std::path::Path::new(dir), adopt) {
        Ok(report) => {
            report_standing(&report);
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("{err}");
            eprintln!("recovery: {}", err.recovery());
            ExitCode::FAILURE
        }
    }
}

/// Where a running city is unless somebody says otherwise. The same
/// address `up` and `serve` bind by default, so the common case needs
/// no flag at all.
const DEFAULT_AT: &str = "127.0.0.1:8787";

/// Sends one frame over the wire and prints everything that comes back.
///
/// Exits 1 when the city refused something, so an agent driving this
/// learns the outcome from the exit code rather than by parsing JSON.
fn call(args: &[String]) -> ExitCode {
    let Some(frame) = args.get(1).filter(|a| !a.starts_with("--")) else {
        eprintln!(
            "usage: sprawling call <frame-json|-> [--at host:port] [--token T] [--quiet-ms N]"
        );
        eprintln!("commands: {}", channels::COMMAND_NAMES.join(", "));
        eprintln!("queries:  {}", channels::QUERY_NAMES.join(", "));
        return ExitCode::from(2);
    };
    // `-` reads the frame from stdin, which is how a frame too long for
    // one command line, or one a script generated, gets in.
    let frame = if frame == "-" {
        match std::io::read_to_string(std::io::stdin()) {
            Ok(text) => text,
            Err(err) => {
                eprintln!("could not read the frame from stdin: {err}");
                return ExitCode::FAILURE;
            }
        }
    } else {
        frame.clone()
    };
    let at = flag_value(args, "--at").unwrap_or_else(|| DEFAULT_AT.to_owned());
    let token = flag_value(args, "--token");
    let quiet = match flag_value(args, "--quiet-ms") {
        None => 2_000,
        Some(raw) => match raw.parse::<u64>() {
            Ok(ms) => ms,
            Err(_) => {
                eprintln!("not a number of milliseconds: {raw}");
                return ExitCode::from(2);
            }
        },
    };
    match wire_client::call(
        &at,
        &frame,
        token.as_deref(),
        std::time::Duration::from_millis(quiet),
    ) {
        Ok(heard) => {
            eprintln!("{} frame(s), {} refusal(s)", heard.frames, heard.refusals);
            if heard.refusals > 0 {
                ExitCode::FAILURE
            } else {
                ExitCode::SUCCESS
            }
        }
        Err(err) => report(err),
    }
}

/// Hands a credential to a city on this machine, reading it from stdin.
///
/// Never from `argv`: a key on a command line is in the process table,
/// in shell history, and in the log of whatever started this process.
fn enrol(args: &[String]) -> ExitCode {
    let Some(reference) = args.get(1).filter(|a| !a.starts_with("--")) else {
        eprintln!("usage: sprawling enrol <realm>/<name> [--at host:port]");
        eprintln!("the value is read from stdin, never from the command line");
        return ExitCode::from(2);
    };
    let Some((realm, name)) = wire_client::split_reference(reference) else {
        eprintln!("not a credential reference: {reference}");
        eprintln!("recovery: give it as <realm>/<name>, for example modelscope/api");
        return ExitCode::from(2);
    };
    let value = match std::io::read_to_string(std::io::stdin()) {
        Ok(text) => text.trim().to_owned(),
        Err(err) => {
            eprintln!("could not read the credential from stdin: {err}");
            return ExitCode::FAILURE;
        }
    };
    if value.is_empty() {
        eprintln!("nothing arrived on stdin");
        eprintln!(
            "recovery: pipe the value in, for example: cat key.txt | sprawling enrol {reference}"
        );
        return ExitCode::from(2);
    }
    let at = flag_value(args, "--at").unwrap_or_else(|| DEFAULT_AT.to_owned());
    match wire_client::enrol(&at, realm, name, &value) {
        Ok(reference) => {
            println!("{reference}");
            // Accepted, not yet stored: the route answers before the
            // worker has taken it (channels-SPEC.md section 8).
            eprintln!("accepted; the city stores it as soon as its worker is free");
            ExitCode::SUCCESS
        }
        Err(err) => report(err),
    }
}

/// Makes `sprawling` a word this machine's shells resolve, or unmakes
/// it. Nothing here needs administrator rights, because nothing outside
/// the person's own profile is touched.
fn install(args: &[String]) -> ExitCode {
    let uninstall = args.iter().any(|a| a == "--uninstall");
    let done = match install::install(uninstall) {
        Ok(done) => done,
        Err(err) => return report(err),
    };
    println!();
    if uninstall {
        println!("  removed {}", done.binary.display());
    } else {
        println!("  installed {}", done.binary.display());
    }
    match done.path {
        install::PathOutcome::Unchanged => println!("  PATH already said what it needed to say"),
        install::PathOutcome::Rewritten => {
            println!("  PATH rewritten for this user account");
            println!();
            println!("  Open a NEW shell window - a running one keeps the PATH it started with.");
        }
        install::PathOutcome::SelfService(line) => {
            println!("  that directory is not on PATH; add this line where you keep such lines:");
            println!();
            println!("      {line}");
        }
    }
    if let Some(notice) = done.notice {
        println!("  note: {notice}");
    }
    println!();
    ExitCode::SUCCESS
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
    // `--help` after a subcommand asks about the subcommand, not for a
    // city called `--help`; without this the storage layer reported that
    // it could not list `--help\.sprawling\ledger`.
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("{COMMANDS}");
        return ExitCode::SUCCESS;
    }
    let Some(dir) = dir.filter(|a| !a.starts_with("--")) else {
        eprintln!("usage: sprawling serve <city-dir> [addr] [--log <level>] [--web-dir <dir>]");
        return ExitCode::from(2);
    };
    // The address is the first non-flag argument after the city dir, so
    // `serve city --log off` does not read `--log` as an address.
    let raw = addr
        .filter(|a| !a.starts_with("--"))
        .map_or("127.0.0.1:8787", String::as_str);
    let open = args.iter().any(|a| a == "--open");
    serve_city(std::path::Path::new(dir), raw, args, open)
}

/// Serving proper, reached from `serve` and from `up`. `open` is the only
/// difference between them: `up` is the appliance and opens the WebUI,
/// `serve` stays where a person put it unless asked.
fn serve_city(city: &std::path::Path, raw: &str, args: &[String], open: bool) -> ExitCode {
    // A directory with no history is not a city, and saying so beats the
    // storage layer's report that it could not list a ledger directory -
    // which is true, unhelpful, and names a path nobody chose.
    if !assembly::has_history(city) {
        eprintln!("no city at {}", city.display());
        eprintln!(
            "recovery: `sprawling up {0}` raises one and serves it",
            city.display()
        );
        eprintln!(
            "          `sprawling init {0}` raises one and stops",
            city.display()
        );
        return ExitCode::from(2);
    }
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
    // The key is settled before anything binds. A configured token is
    // adopted; an address that reaches past this machine and has none
    // gets one minted for this serve alone. Read once here and never
    // stored - `serve` is handed a digest.
    let keyed = match assembly::key_for(bind, std::env::var("SPRAWLING_PAIRING_TOKEN").ok()) {
        Ok(keyed) => keyed,
        Err(err) => return report(err),
    };
    let token = keyed.code().map(str::to_owned);
    let runtime = match tokio::runtime::Runtime::new() {
        Ok(runtime) => runtime,
        Err(err) => {
            eprintln!("could not start the async runtime: {err}");
            return ExitCode::FAILURE;
        }
    };
    let client_line = match &client {
        channels::ClientAssets::Disk(dir) => {
            format!("read per request from {}", dir.display())
        }
        channels::ClientAssets::Embedded(_) => client_summary(),
    };
    let url = firstrun::local_url(bind);
    println!();
    println!("  sprawling is running.");
    println!();
    println!("    city     {}", city.display());
    println!("    WebUI    {url}");
    println!("    client   {client_line}");
    println!();
    match &keyed {
        assembly::Keyed::NothingToPresent => {}
        assembly::Keyed::Adopted(_) => {
            println!("    key      the one you configured; this city will ask for it");
            println!();
        }
        // Shown here and nowhere else, for as long as this process
        // lives. Nothing writes it down, so a person who loses it stops
        // and starts the city again rather than looking for a file.
        assembly::Keyed::Minted(code) => {
            println!("    key      {code}");
            println!();
            println!("  This address reaches past this machine, so the city minted a key.");
            println!("  It is shown once, kept nowhere, and replaced the next time you start.");
            println!("  Open:    {}/?token={code}", url.trim_end_matches('/'));
            println!();
        }
    }
    println!("  Open the WebUI in a browser. Ctrl-C stops the city.");
    println!();
    if open {
        firstrun::open_when_ready(bind, url);
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
    // The terminal this city runs in becomes its console when `up`
    // started it, or when `serve` was asked. `--no-console` is the way
    // out for a supervisor that wants the old blocking shape.
    let wanted = (open || args.iter().any(|a| a == "--console"))
        && !args.iter().any(|a| a == "--no-console");
    let console = wanted.then(|| console::Terminal {
        url: firstrun::local_url(bind),
        token: token.clone(),
        // The three facts the banner above just printed. `/serving`
        // reprints them on demand, because the event stream scrolls
        // them away within seconds of a city getting busy.
        city: city.display().to_string(),
        client: client_line.clone(),
        bind,
    });
    if console.is_some() {
        println!("  This terminal is the console. `/help` lists what it takes,");
        println!("  and `/serving` says where this city listens and what is running in it.");
        println!();
    }
    let (vault, vault_notice) = assembly::open_vault();
    match runtime.block_on(assembly::serve(assembly::Serving {
        city_root: city.to_path_buf(),
        addr: bind,
        token,
        client,
        vault,
        vault_notice,
        log,
        console,
    })) {
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
    println!("client: {}", client_summary());
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
    use super::{CLIENT_COMPLETE, CLIENT_FILES, named};

    /// A flag is not a path.
    ///
    /// `sprawling init --help` used to raise a city in a directory
    /// called `--help`, because `init` read `args[1]` whatever it was.
    /// This repository's own root held one of those for a day.
    #[test]
    fn a_flag_is_never_read_as_the_path_a_subcommand_wanted() {
        let words =
            |raw: &[&str]| -> Vec<String> { raw.iter().map(|word| (*word).to_owned()).collect() };
        let asked = words(&["init", "--help"]);
        assert_eq!(named(&asked, 1), None, "--help became a city directory");

        let two = words(&["export", "--verbose", "city", "bundle"]);
        assert_eq!(named(&two, 1).map(String::as_str), Some("city"));
        assert_eq!(named(&two, 2).map(String::as_str), Some("bundle"));

        let plain = words(&["serve", "city", "127.0.0.1:8787"]);
        assert_eq!(named(&plain, 1).map(String::as_str), Some("city"));
        assert_eq!(named(&plain, 2).map(String::as_str), Some("127.0.0.1:8787"));
    }

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
