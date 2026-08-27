# AGENTS.md — how work is done in this repository

**For any agent or person about to change this code.** Read this file to the end before the first edit; it is short on purpose, and everything it does not cover is one link away.

sprawling is a locally deployed agent-city harness: one Rust binary, a WebAssembly client embedded inside it, and a city that keeps its whole history in one append-only Ledger. Most of it was written by models, and every rule below exists for one reason that applies to a person just as much: **a contributor who does not remember yesterday still has to produce work that holds.**

## The loop

```bash
cargo install just cargo-nextest --locked   # once; the toolchain installs itself from rust-toolchain.toml
just check                                  # fmt + clippy (-D warnings, --all-features) + nextest + every machine gate
```

**A change is finished when `just check` is green.** "I finished it" is a claim; a green run is the evidence.

| Command | What it does |
|---|---|
| `just check` | the closing condition for every change |
| `just gates` | the machine gates alone |
| `just check-web` | clippy on the wasm target — the two places `just check` cannot reach |
| `just build-web` | build the client bundle, without `dx` |
| `just dist` | the whole deliverable: client, binary, bill of materials, size badges |
| `just sim [seed]` | citysim scenarios; a failure reproduces from its seed |
| `just spec <crate>` | generate a SPEC skeleton |
| `just api-baseline` | recompute the public-surface baselines |
| `just replay <log>` | verify a ledger chain offline, read-only |
| `just mem [pid]` / `just bench` / `just budget` | the measurements, in this platform's own vocabulary |
| `just fuzz <target>` / `just mutants` | fuzz targets / mutation testing |

## Read before you write

1. [`ARCHITECTURE.md`](ARCHITECTURE.md) — what the code is made of, how the twelve crates are wired, what one dispatch does end to end. Its `depmap` block and module map are machine authorities.
2. The SPEC of the crate you are touching, `crates/<crate>/<crate>-SPEC.md` — the interfaces and decisions, written before the code.
3. [`docs/glossary.md`](docs/glossary.md) — one name per concept, enforced by a gate.
4. The tests next to the code you are about to change.

**Read the official documentation of a tool before you use it** — a language feature, a crate, a CLI, a framework. Load the vendor's own agent guide or skill when one exists. Two are hard requirements because both moved recently: the front end is **Dioxus** built without `dx` (<https://dioxuslabs.com/learn/0.7/>), and the wasm build needs a `wasm-bindgen` CLI whose version equals the crate version.

## One change, five steps

1. **Take one piece of work.** One session, one bounded change, read its context in full before starting.
2. **Write the SPEC first.** Interfaces and decisions land in the crate's SPEC before the code exists. A new module states which of the seven shapes it instantiates ([`ARCHITECTURE.md`](ARCHITECTURE.md) §9); when there is no answer, stop and ask rather than write.
3. **Red.** Write the failing test and **run it once to watch it fail**. That run is what proves the test can bite.
4. **Green.** Implement until it passes, no more. When the implementation wants to differ from the SPEC, change the SPEC first.
5. **Close.** All four: `just check` green | the red-to-green transition visible in the commit order | SPEC and code in step | the module map updated.

When a session ends with the work unfinished, write what is left — where you got to, what blocked you, what comes next — into the SPEC section it belongs to, not into your own memory.

## The rules a machine holds

Violating any of these turns CI red with a message naming the rule, the violation, and an alternative.

| Write it this way | Held by |
|---|---|
| Return failure through `Result`. No `unwrap`, `expect`, `panic!`, `todo!`, `unreachable!`, bare indexing or slicing in non-test code. Checked arithmetic; `TryFrom` for narrowing; no `as` casts; no `unsafe`. | workspace lints, `-D warnings` |
| One module, one file, semantically named. Register the file in the module map, then create it. Keep `lib.rs` and index files free of logic. | `xtask modmap` |
| Default to `pub(crate)`. Declare a `pub` trait only in a file on the seam list. | `xtask depmap` |
| Start every `.rs` file with the MPL-2.0 notice, then the copyright line. | `xtask header` |
| Take the time as a parameter. The single sampling point is `bin::assembly`. | `clippy.toml` disallowed methods |
| Use `BTreeMap` on kernel decision paths; keep floats out of ledger payloads; start tasks from the one spawn point. | review, plus the citysim determinism scenarios |
| One name per concept, taken from the glossary. | `xtask lexicon` with `xtask/lexicon.toml` |
| Change a crate's public surface and its SPEC in the same commit. Adding one `pub use` is a public-surface change. | `xtask apisync` |
| Keep credentials as `secret:realm/name` references; let plaintext reach the vault only. | `xtask secret` |
| Take colour from `web::theme`; express a colour as a ratio of the gamut limit. | `xtask color` |
| Keep sizes inside their budget, and badges in step with the artifacts. | `xtask budget` |
| Publish nothing that names one machine's home directory or its working notes. | `xtask release` |
| **Fix the cause when a gate goes red.** Changing `xtask/`, the root `Cargo.toml`, `deny.toml`, `clippy.toml`, the `justfile`, `.github/`, or deleting a module row requires an explicit ruling from the person, recorded as a `Verdict:` trailer. | `xtask guard` |

The last row is the load-bearing one: it closes the single universal escape hatch, which is loosening a gate in order to pass it.

**Tests use the same doors as production code.** To exercise something internal, put a seam on that face and give it a second adapter, or drive it from outside through citysim. Test modules may relax lints locally with `#[allow]` on the test module; production code carries them as written.

## Language

| Where | Language |
|---|---|
| Identifiers, event names, error codes, rustdoc, commit subjects | English |
| `README.md`, `AGENTS.md`, `ARCHITECTURE.md`, `docs/` | English, except `README.zh-CN.md` and `docs/getting-started.zh-CN.md` |
| Crate SPECs and design discussion | Chinese, with concept names kept in their English form |
| Pull requests, issues, review comments | **your own language.** Attaching a parallel translation — English if you wrote another language, Chinese if you wrote English — is welcome rather than required: with both side by side a reader is faster, and a mistranslation is visible instead of silent |

Comments are a failure signal by default. Four kinds earn their place: the MPL notice, public interface documentation, a warning about consequences, and a statement of intent the code cannot carry. In rustdoc, write what the signature cannot say — invariants, failure modes, call ordering, ownership.

## Commits

The first line is `card-<stage>.<index>: <what>`, for example `card-S4.02: the wire, and the two frames a socket cannot spell`. The body records **what you found**, since what you did is already in the diff: a gate that changed your design, a red-to-green transition that exposed a real defect, a choice between two approaches whose reason the result does not show.

A commit that touches anything in the `guard` row carries a `Verdict: user-approved` trailer. The wording of the ruling itself stays with the person; the trailer records that there was one.

## Where the authorities are

1. The person's ruling.
2. [`ARCHITECTURE.md`](ARCHITECTURE.md) — structure: crate topology, seams, module map, shapes, determinism, verification.
3. `crates/<crate>/<crate>-SPEC.md` — that crate's interfaces and decisions, ahead of its code.
4. The code and its tests.

Where a lower level contradicts a higher one, the higher wins and the lower is corrected in the same change. Where reality contradicts all of them, reality wins and the document is corrected first, with its reason.

One directory named in `.gitignore` holds one machine's working notes. Nothing in this repository may depend on it, and `xtask release` refuses a document that sends a reader there.
