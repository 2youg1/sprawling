# Contributing — how work is done in this repository

> **For someone about to change this code.** It gives the rules, the gate that holds each one, and how to get a green run locally.
>
> It does not explain what the code is made of or how it is verified in layers ([`../ARCHITECTURE.md`](../ARCHITECTURE.md)), nor what the words mean ([`glossary.md`](glossary.md)). The instructions an agent reads first are [`../AGENTS.md`](../AGENTS.md), and this file is the longer version of the same rules.
>
> Most of this code was written by models, and the rules below are written for whoever comes next — a person typing it by hand and a model that does not remember yesterday are held to exactly the same ones. That is the explanation for what follows: every rule exists so that a contributor without yesterday's context still produces work that holds.
>
> The rules are stated as the thing to do. Where a rule must never be broken, a machine gate enforces it, and the gate names the fix in its own output — so the first thing you write can be the right thing, rather than something to be corrected later.

## 0 Thirty seconds

```bash
cargo install just cargo-nextest --locked   # once
just check                                  # the closing condition for every change
```

`just check` is fmt + clippy (`-D warnings`, `--all-features`) + nextest + every machine gate. **A change is finished when that is green.** "I finished it" is a claim; a green run is the evidence.

## 1 Read before you write

**Read the official documentation of a tool before you use it.** This applies to a language feature, a crate, a CLI, a front-end framework, and a build system alike. Load the vendor's own agent guide or skill when one exists, then act. Working from memory of an older version is how a change acquires a defect that the compiler cannot see.

Two places where this is a hard requirement, because both have moved recently:

| Area | Read first |
|---|---|
| Front-end code (`crates/web`) | The framework's own agent guide and its `llms.txt` before touching a component. The framework in force is **Dioxus**, built without `dx`: `cargo build --target wasm32-unknown-unknown` plus a pinned `wasm-bindgen` CLI. Read <https://dioxuslabs.com/learn/0.7/> first; when the framework changes, this row changes with it. |
| `wasm-bindgen` and the wasm build | `https://wasm-bindgen.github.io/wasm-bindgen/`. The CLI version must equal the `wasm-bindgen` crate version. |

Then read, in order: this file, `ARCHITECTURE.md` (what the code is made of and why it has this shape), `crates/<crate>/<crate>-SPEC.md` for the crate you are touching (its interfaces and decisions, written before its code), `docs/glossary.md` (the vocabulary; the lexicon gate enforces it), and the tests next to the code you are about to touch.

> **Everything that explains this code ships with it.** The module map and the seam list are sections of `ARCHITECTURE.md`, and each crate's SPEC sits beside that crate. The only thing kept back is one machine's working notes, which are in `.gitignore` and which nothing here may depend on.

## 2 The five steps of one change

1. **Take one bounded piece of work** — one session's worth, small enough to finish and verify. Read what it touches in full before you start: the crate's SPEC, the neighbouring modules, and their tests.
2. **Write the SPEC first.** Interfaces and decisions land in the crate's SPEC before the code exists. A new module states which of the seven shapes it instantiates; when there is no answer, stop and ask rather than write.
3. **Red.** Write the failing test and **run it once to see it fail**. That run is what tells you the test can bite.
4. **Green.** Implement until it passes, no more. When the implementation wants to differ from the SPEC, change the SPEC first and then continue.
5. **Close.** Four things, all of them: `just check` green | the red-to-green transition visible in the commit order | SPEC and code in step | the module map in `ARCHITECTURE.md` updated.

When a session ends with the work unfinished, write the remaining state — where you got to, what blocked you, what comes next — into the SPEC section it belongs to. A handoff that lives only in a person's head is a handoff that did not happen.

## 3 The rules, and the gate that holds each one

Every row is enforced by a machine. Violating one turns CI red with a message naming the rule, the violation, and an alternative.

| Write it this way | Held by |
|---|---|
| Return failure through `Result`; propagate arithmetic, indexing and conversion errors instead of ending the process. Use `checked_*` arithmetic, `TryFrom` for narrowing conversions, and pattern matching with an explicit fallback for lookups. | workspace lints: `unwrap_used`, `expect_used`, `panic`, `indexing_slicing`, `arithmetic_side_effects`, `as_conversions`, all `deny` |
| One module, one file, semantically named. Register the file in the module table, then create it. Keep `lib.rs` and index files free of logic. | `xtask modmap` |
| Default to `pub(crate)`. Declare a `pub` trait only in a file on the seam list. | `xtask depmap` |
| Start every `.rs` file with the MPL-2.0 notice, then the copyright line. | `xtask header` |
| Take the time as a parameter. The single sampling point is `bin::assembly`. | `clippy.toml` disallowed methods |
| Use `BTreeMap` on kernel decision paths; keep floats out of ledger payloads; start tasks from the one spawn point. | review plus the determinism tests in citysim |
| Use one name per concept, taken from `docs/glossary.md`. | `xtask lexicon`, data face `xtask/lexicon.toml` |
| Change a crate's public surface and its SPEC in the same commit. Adding one `pub use` line is a public-surface change. | `xtask apisync` |
| Keep credentials as `secret:realm/name` references; let plaintext reach the Vault only. | `xtask secret` |
| Take colour from `web::theme`; express a colour as a ratio of the gamut limit. | `xtask color` |
| Keep sizes inside their budget, and the badges in `README.md` in step with the artifacts they measure. `just dist` rewrites them; nobody types a size into a document. | `xtask budget` |
| Publish nothing that names one machine's home directory or its working notes. | `xtask release` |
| Fix the cause when a gate goes red. Changing `xtask/`, root `Cargo.toml`, `deny.toml`, `clippy.toml`, `justfile`, or `.github/` **in the same commit as `crates/`, `citysim/` or `fuzz/`** requires a `Verdict:` trailer — that is, an explicit ruling from the person. Re-pricing a rule in a commit of its own does not. | `xtask guard` |

The `guard` row is the load-bearing one: it closes the single universal escape hatch, which is loosening a gate in order to pass it. What the machine looks for is the pair — a gate change and the source that gate judges, arriving together, so that one green run reports both. A commit whose whole diff is gate machinery is a re-pricing, and it needs no ruling: its diff says nothing except that a rule now costs something different, which is exactly what a reviewer has to read. Charging a ruling for that too is what once kept a rule alive a year past its argument.

**Every gate has been seen red.** Each had a violation injected when it went live and was confirmed to bite; a gate that has never failed is indistinguishable from a gate that does not exist. Four occasions when a gate changed the design rather than being loosened are in [`../ARCHITECTURE.md`](../ARCHITECTURE.md).

**The ratchet holds only where a machine measures twice the same way.** Sizes are gated, because a byte count does not depend on how busy the machine was. Wall-clock figures live in the same register with what they would need in order to be measured, and are not gated: a slow runner is not a defect, and a gate that says it is teaches people to ignore gates.

## 3.1 Continuous integration

**`ci` runs on every push to `main` and on every pull request**; its five jobs together are exactly `just check` plus `just check-web` plus the supply-chain read, and nothing else - a green CI implies at least what a green `just check` implies. `platforms` and `nightly` answer questions no one waits for (macOS/kani, fuzz, advisories) and run on a schedule; `upstream-watch` asks the two provider-intelligence upstreams whether they moved, daily.

Three things run there and not here: `cargo-deny` when it is not installed locally, the formal-verification job (Linux only, mirrored locally by properties), and the nightly fuzz and mutation batches. Everything else is `just check`.

## 4 Comments and documentation

**Write the code so that it explains itself, and reach for a comment when it cannot.** Four kinds earn their place: the MPL notice, public interface documentation, a warning about consequences, and a statement of intent that the code cannot carry.

In rustdoc, write what the signature cannot say — invariants, failure modes, call ordering, ownership. The parameter names are already visible.

### Languages

| Where | Language |
|---|---|
| Identifiers, event names, error codes, rustdoc, commit subjects | English |
| `README.md`, `AGENTS.md`, `ARCHITECTURE.md`, `docs/` | English, except `README.zh-CN.md` and `getting-started.zh-CN.md` |
| Crate SPECs and design discussion | Chinese, with concept names kept in their English form |
| **Pull requests, issues, review comments** | **your own language** |

That last row is deliberate. Write the description in the language you think in; a precise sentence in your own language is worth more than an approximate one in someone else's. If you would like a second pair of eyes to move faster, attach a parallel translation — English if you wrote in another language, Chinese if you wrote in English. It is welcome rather than required, and it has a second benefit: with both versions side by side, a mistranslation is visible instead of silent, whether it came from a person or from a model.

## 5 Commit messages

The first line is `card-<stage>.<index>: <what>`, for example `card-S4.02: the wire, and the two frames a socket cannot spell`.

The body records **what you found**, since what you did is already in the diff. Three findings are always worth the space: a gate that changed your design, a red-to-green transition that exposed a real defect, and a choice between two approaches whose reason is not obvious from the result.

`deps:` is its own family and carries no card number: that prefix is what dependabot writes, and a bot does not plan work. A bump that touches a guarded path (a root `Cargo.toml`, a workflow file) still needs the `Verdict:` trailer like any other change to one; at merge time, the person merging supplies it.

## 6 Tests

- Reach for properties before examples (`proptest`), `insta` for golden output, and `trybuild` when the point is that something cannot be expressed at all.
- **Tests use the same doors as production code.** To exercise something internal, put a seam on that face and give it a second adapter, or drive it from outside through citysim.
- Test modules may relax lints locally with `#[allow]` on the tests module. Production code carries the lints as written.

One question decides whether a test earns its lines: **would a real defect turn it red?** A test that only reacts to a rewritten implementation costs more than it returns.

## 7 Environment

The repository pins the toolchain and leaves the environment to you. `rust-toolchain.toml` installs the pinned toolchain automatically; the rest:

| Tool | Purpose | Needed |
|---|---|---|
| `just`, `cargo-nextest` | the daily command surface | always |
| `cargo-deny` | dependency audit | optional locally, always in CI |
| `wasm32-unknown-unknown` target | front-end build | when touching `web` |
| `wasm-bindgen-cli` | front-end bundling | same; **its version must equal the `wasm-bindgen` crate version** |
| `cargo-public-api` + nightly | recomputing the public-surface baselines | when changing a public surface |

## 8 Command surface

| Command | What it does |
|---|---|
| `just check` | the closing condition: fmt + clippy + nextest + every machine gate |
| `just gates` | the gates alone |
| `just check-web` | clippy on the wasm32 target, covering what `just check` cannot reach |
| `just build-web` | build the front-end artifact, without `dx` |
| `just dist` | the whole deliverable: client, binary, bill of materials, and the size badges |
| `just budget` / `just bench` | every budget with what it costs today; the three wall-clock measurements |
| `just sim [seed]` | citysim scenarios; a failure reproduces from its seed |
| `just spec <crate>` | generate a SPEC skeleton |
| `just api-baseline` | recompute the public-surface baselines |
| `just replay <log>` | verify a ledger chain offline, read-only |
| `just mem [pid]` | measure resident memory in this platform's own vocabulary |
| `just fuzz <target>` / `just mutants` | fuzz targets / mutation testing |
