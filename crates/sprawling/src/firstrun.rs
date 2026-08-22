// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The first screen a person sees when the binary carries no command,
//! where a city goes when nobody named one, and handing a URL to the
//! desktop's default handler.
//!
//! Launching this binary from a file manager gives it no arguments and a
//! console that closes the moment it exits. Telling that apart from a
//! person typing `sprawling` in a shell needs `GetConsoleProcessList`,
//! which needs `unsafe`, which the workspace forbids. So nothing here
//! guesses how it was started: the three ways in are each named, and they
//! meet at `up` (sprawling-SPEC.md section 8-8).

use std::io::{BufRead, Write};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// How long the WebUI waits for its own city. Everything a city does
/// before it binds - verifying the chain, folding the views, starting the
/// worker - happens first, and on a long history that is seconds. A city
/// slower than this is one to watch starting rather than open behind.
const PROBE_ATTEMPTS: u32 = 60;
const PROBE_INTERVAL: Duration = Duration::from_millis(200);

/// What the person answered on the first screen.
///
/// A city is created only through `Start`, so the genesis write - the one
/// irreversible act in this system - always has somebody behind it.
pub(crate) enum FirstScreen {
    Start(PathBuf),
    Quit,
}

/// Where a city goes when the person named none.
///
/// Beside the binary, so the whole thing stays one folder that can be
/// moved, copied or deleted as a unit. When that directory cannot be
/// written - unpacked into `Program Files`, for instance - the city goes
/// under the home directory instead. The caller shows the result before
/// creating anything, so the fallback is visible rather than silent.
///
/// With no home to fall back to, this still answers beside the binary:
/// the screen shows that path and starting there fails with the reason,
/// which beats inventing a location nobody asked for.
pub(crate) fn default_city(exe_dir: &Path, home: Option<&Path>, exe_dir_writable: bool) -> PathBuf {
    if exe_dir_writable {
        return exe_dir.join("city");
    }
    match home {
        Some(home) => home.join("sprawling").join("city"),
        None => exe_dir.join("city"),
    }
}

/// Whether a directory accepts a file today. Probes rather than reads
/// permissions: a read-only mount, an ACL and a full disk all end the
/// same way, and only writing finds out.
pub(crate) fn is_writable(dir: &Path) -> bool {
    let probe = dir.join(".sprawling-write-probe");
    match std::fs::write(&probe, b"") {
        Ok(()) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

/// Draws the first screen and reads the one answer it asks for.
///
/// The path is on the screen before the question, so nobody consents to a
/// location they were not shown. End of input is `Quit`: a piped or
/// unattended stdin has nobody to ask, and creating a city would be
/// acting on silence.
///
/// # Errors
/// Propagates the failure of writing the screen or reading the answer.
pub(crate) fn ask<R: BufRead, W: Write>(
    city: &Path,
    input: &mut R,
    out: &mut W,
) -> std::io::Result<FirstScreen> {
    writeln!(
        out,
        "\n  sprawling - an agent city that runs on this machine.\n"
    )?;
    writeln!(out, "  No city was named. Start one here?\n")?;
    writeln!(out, "      {}\n", city.display())?;
    writeln!(out, "      [Enter]  start it, and open the WebUI")?;
    writeln!(out, "      [q]      quit, and print the command list\n")?;
    write!(out, "  > ")?;
    out.flush()?;

    let mut answer = String::new();
    let read = input.read_line(&mut answer)?;
    if read == 0 {
        return Ok(FirstScreen::Quit);
    }
    let answer = answer.trim();
    let consented =
        answer.is_empty() || answer.eq_ignore_ascii_case("y") || answer.eq_ignore_ascii_case("yes");
    if consented {
        Ok(FirstScreen::Start(city.to_path_buf()))
    } else {
        Ok(FirstScreen::Quit)
    }
}

/// The URL a person on this machine can open.
///
/// A city bound to every interface still has to be reachable from the
/// browser in front of it, and `http://0.0.0.0:8787` is not an address a
/// browser can use.
pub(crate) fn local_url(bind: SocketAddr) -> String {
    let addr = reachable(bind);
    let port = addr.port();
    match addr.ip() {
        std::net::IpAddr::V4(v4) => format!("http://{v4}:{port}"),
        std::net::IpAddr::V6(v6) => format!("http://[{v6}]:{port}"),
    }
}

/// The address something on this machine can actually connect to. A city
/// bound to every interface is still opened from the browser in front of
/// it, and `0.0.0.0` is not an address any client can dial.
fn reachable(bind: SocketAddr) -> SocketAddr {
    if bind.ip().is_unspecified() {
        let loopback = std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST);
        return SocketAddr::new(loopback, bind.port());
    }
    bind
}

/// Opens the WebUI once the city is really listening, on a thread of its
/// own so serving is not held up by a browser starting.
///
/// Waits for the port to accept rather than guessing when the bind will
/// happen, and gives up in silence when it never does - the URL was
/// printed before this started, so nothing is lost but the convenience.
pub(crate) fn open_when_ready(bind: SocketAddr, url: String) {
    let addr = reachable(bind);
    std::thread::spawn(move || {
        for _ in 0..PROBE_ATTEMPTS {
            if std::net::TcpStream::connect_timeout(&addr, PROBE_INTERVAL).is_ok() {
                // Never fatal: a machine with no handler for URLs still
                // has a working city, and its address is on the screen.
                let _ = open_in_browser(&url);
                return;
            }
            std::thread::sleep(PROBE_INTERVAL);
        }
    });
}

/// Hands a URL to whatever this desktop opens URLs with.
///
/// # Errors
/// Propagates the failure of starting the handler. Callers treat this as
/// a notice rather than a fault: the URL is printed before this runs, so
/// a machine with no handler still has a working city.
pub(crate) fn open_in_browser(url: &str) -> std::io::Result<()> {
    let mut command = handler(url);
    command.status().map(|_| ())
}

#[cfg(target_os = "windows")]
fn handler(url: &str) -> std::process::Command {
    let mut command = std::process::Command::new("cmd");
    // The empty argument is `start`'s title slot: without it a quoted URL
    // becomes the window title and nothing opens.
    command.args(["/C", "start", "", url]);
    command
}

#[cfg(target_os = "macos")]
fn handler(url: &str) -> std::process::Command {
    let mut command = std::process::Command::new("open");
    command.arg(url);
    command
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn handler(url: &str) -> std::process::Command {
    let mut command = std::process::Command::new("xdg-open");
    command.arg(url);
    command
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
    use super::{FirstScreen, ask, default_city, local_url};
    use std::path::{Path, PathBuf};

    fn screen(answer: &str) -> (FirstScreen, String) {
        let city = PathBuf::from("/tmp/here/city");
        let mut input = answer.as_bytes();
        let mut out: Vec<u8> = Vec::new();
        let outcome = ask(&city, &mut input, &mut out).unwrap();
        (outcome, String::from_utf8(out).unwrap())
    }

    /// Stand-ins for a binary's own directory and for the fallback root;
    /// neither is a real path on any machine.
    const UNPACKED: &str = "/unpacked";
    const ELSEWHERE: &str = "/elsewhere";

    #[test]
    fn city_goes_beside_the_binary_when_that_directory_is_writable() {
        let beside = default_city(Path::new(UNPACKED), Some(Path::new(ELSEWHERE)), true);
        assert_eq!(beside, PathBuf::from("/unpacked/city"));
    }

    #[test]
    fn city_falls_back_when_the_binary_directory_is_read_only() {
        let fallback = default_city(Path::new(UNPACKED), Some(Path::new(ELSEWHERE)), false);
        assert_eq!(fallback, PathBuf::from("/elsewhere/sprawling/city"));
    }

    /// With nowhere to fall back to, the answer stays beside the binary
    /// rather than becoming a location nobody asked for.
    #[test]
    fn with_no_fallback_the_answer_is_still_beside_the_binary() {
        let beside = default_city(Path::new(UNPACKED), None, false);
        assert_eq!(beside, PathBuf::from("/unpacked/city"));
    }

    #[test]
    fn the_path_is_on_the_screen_before_the_question_is_answered() {
        let (_, drawn) = screen("\n");
        assert!(drawn.contains("/tmp/here/city"), "drawn: {drawn}");
    }

    #[test]
    fn enter_starts_the_city_at_the_path_the_screen_showed() {
        let (outcome, _) = screen("\n");
        match outcome {
            FirstScreen::Start(path) => assert_eq!(path, PathBuf::from("/tmp/here/city")),
            FirstScreen::Quit => panic!("Enter starts the city"),
        }
    }

    #[test]
    fn q_quits_without_creating_anything() {
        let (outcome, _) = screen("q\n");
        assert!(matches!(outcome, FirstScreen::Quit));
    }

    #[test]
    fn end_of_input_quits_rather_than_acting_on_silence() {
        let (outcome, _) = screen("");
        assert!(matches!(outcome, FirstScreen::Quit));
    }

    #[test]
    fn a_city_bound_to_every_interface_is_shown_as_loopback() {
        let every: std::net::SocketAddr = "0.0.0.0:8787".parse().unwrap();
        assert_eq!(local_url(every), "http://127.0.0.1:8787");
    }

    #[test]
    fn a_city_bound_to_one_address_is_shown_at_that_address() {
        let one: std::net::SocketAddr = "192.168.1.9:8787".parse().unwrap();
        assert_eq!(local_url(one), "http://192.168.1.9:8787");
    }
}
