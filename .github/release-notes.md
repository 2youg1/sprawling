# v0.0.2 — pre-alpha

Everything the v0.0.1 notes said about being pre-alpha still holds, and I am not going to say it again at length: this is a learning project, the front end is the least-tested part of it, and criticism is worth more to me than patience.

What is different about this release is what it is made of. Thirty-five commits, and almost none of them add a capability. They connect capabilities v0.0.1 already claimed to a build that reaches them — because a surprising number of them were not reached by any build at all. Each one below was found the same way: by asking who calls a thing, and finding nobody.

## Three features that existed and could not be reached

**A city could not accept a browser from another machine.** Exposing the WebUI beyond loopback is a four-link chain, and three of the links were cut. `PairingToken::mint` had no caller in the product — a type whose documentation says it hands a code back "to show to a person once" had never shown one. The page built its link without the token, and the socket URL kept only the host, dropping the query string on the way. So a city bound to `0.0.0.0` with `SPRAWLING_PAIRING_TOKEN` set asked every peer for a token while its own client never sent one: **the server refused its own WebUI, in the exact configuration the token exists to enable.** The key is now minted, carried, and presented.

**The console could not answer a question.** `post` replied to every query with the text of a `sprawling call` command — it asked a person already standing inside a city to open a second terminal and interrogate it from outside. The answering function had been built one line above and handed only to the socket. The console and the socket now share it, and queries print as JSONL line by line, which is what the specification had claimed all along.

**`exec`'s refusal named an install nobody could perform.** `dispatch_in` wrote `AbsentSandbox` as a literal and no feature of the crate reached the wasm runtime, so `WasmtimeSandbox` had no caller outside its own tests. The refusal reads *"this build carries no execution engine — use the program arm, or install a build with the `wasm` feature"*, and **that build did not exist**. It exists now, behind `--features sandbox`. Read the last section before you expect it in this archive.

## Four failures that were reported as something else

**A run under review put its decisions on the building's shelf.** A building under review lends every run its own tree, so that nothing the run writes is the building's until somebody checks it. The archive drain wrote to the city root, outside that tree.

**A file that would not open looked like a file somebody wrote badly.** An unreadable building plan was flattened to an empty string and then reported as a plan somebody had changed. An unreadable handoff meant "there was no handoff". Both now say what actually happened, and the difference matters most in the case where it is least visible.

**A change could outrun its own record.** A line the history refused is now a change the city never made, and a merge waits for the line that announces it.

**A ceiling could be lost twice.** Work handed down to another resident is now done under the ceiling that sent it. Answering an approval resumes the same work instead of restarting it under a default ceiling, which it did because the job was rebuilt from a record that never carried the number.

**The same ask arriving twice is one piece of work.** A retry and a second frame now settle against the idempotency key the command already carried; four senders used to mint one from content instead of reading it.

## Under the surface

The dispatch path went from a single 1069-line function to 158 across named phases. A thirteenth machine gate now fails the build on any production function past 200 lines, and nothing was exempted to make it pass. CI runs as five parallel jobs instead of one serial one.

---

## Getting it running

**Download the archive for your system, unpack it, and run the launcher inside.** Nothing is installed and nothing outside that folder is written to.

| System | Archive | What to run |
|---|---|---|
| Windows | `sprawling-0.0.2-windows-x86_64.zip` | double-click `start.cmd` |
| macOS | `sprawling-0.0.2-macos-aarch64.zip` | `./start.sh` in a terminal |

A console window opens and stays open — **that window is the city**. Your browser opens at <http://127.0.0.1:8787>. `Ctrl-C` in the window stops the city.

**These binaries are not code-signed.** Windows will say *"Windows protected your PC"*: choose **More info → Run anyway**. macOS will refuse the first run: open it once from Finder's right-click menu, or clear the quarantine attribute.

**Before it can do anything you need a model to call** — an API key for a provider speaking the OpenAI or Anthropic dialect, or a subscription login. This program schedules agents and records what they do; it does not think by itself.

`QUICKSTART.md` inside the archive walks the first ten minutes. Every archive also carries `sbom.cdx.json`, the full bill of materials for the binary beside it.

## Known before you start

- **This archive does not carry the execution engine.** `sandbox` is off by default and the release build does not turn it on, because wasmtime is a large binary and that trade was not reopened here. Anything routed through the sandbox still answers `this build carries no execution engine`; the arm that runs a program on your machine works as before. What v0.0.2 changes is that the recovery sentence finally names a build somebody can produce — `cargo build --release -p sprawling --features sandbox`.

- **Linux archives are not built in this release** — the Linux pipeline was ruled out rather than debugged; Windows and macOS are what this release ships.

- **Nobody has driven this client in a real browser.** The capabilities above were verified through the wire protocol and the console, which are debugging doors rather than the product. The face you will judge this by is still the least-tested part of it.

- **First-run has two steps that are easy to miss**: after attaching a provider you must pick a model for `main` *and* for `digest`, or every dispatch is refused with `no model is chosen for this tag`.

- **Exposing a city beyond loopback needs `SPRAWLING_PAIRING_TOKEN`.** Binding to a non-loopback address without one is refused on purpose. With one set, the console prints a URL carrying the key — open that URL, not a hand-typed one, or the server will refuse you the way it used to refuse itself.

**For anything else, email me. I check often.**
