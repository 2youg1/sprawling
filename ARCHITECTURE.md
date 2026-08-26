# ARCHITECTURE — sprawling

> **For someone about to change this code**, and for anyone who wants to know what it is made of and why it has this shape rather than another one.
>
> It answers: what runs, what the stack is and what each choice costs, how the twelve units are wired, what happens end to end when you dispatch one piece of work, what is on disk, what crosses the wire, which parts you can replace, and how the whole thing is verified.
>
> It does not teach the vocabulary ([`docs/glossary.md`](docs/glossary.md)), install anything ([`docs/getting-started.md`](docs/getting-started.md)), or list the rules a change must satisfy ([`docs/CONTRIBUTING.md`](docs/CONTRIBUTING.md)).
>
> **Two tables here are machine authorities**: the fenced `depmap` block in §3 and the module map in §12. `cargo xtask depmap` and `cargo xtask modmap` parse them, so an edge or a file that disagrees with them turns CI red. Their column shapes are fixed; the prose around them is not.

## 1 What runs

One process, one page. The client is Rust compiled to WebAssembly and embedded in the binary at build time; there is no second client, and no npm or node appears in the build chain, which `cargo xtask zerojs` checks rather than a convention.

```
                      one machine
┌──────────────────────────────────────────────────────────────┐
│  sprawling (one binary)                                      │
│                                                              │
│   bin::assembly ── the only omniscient point: it holds every │
│        │           concrete type, samples the clock, hands   │
│        │           out seeds, and starts every task          │
│        │                                                     │
│        ├── runtime ── turns, tools, sandbox, watchdog        │
│        ├── collab  ── signals, drafts, pull requests         │
│        ├── city    ── space, residents, archive, schedule    │
│        ├── eval    ── suites, probes, scoring                │
│        ├── browser ── WebDriver BiDi sessions                │
│        ├── protocol── MCP outbound, ACP inbound              │
│        ├── memory  ── Ledger on disk, CAS, projections, git  │
│        ├── gateway ── model routing, dialects, credentials   │
│        └── channels ─ WebSocket server, Command/Query/Event  │
│                 │                                            │
│                 │  ws://127.0.0.1:8787/ws                    │
│                 ▼                                            │
│          web (wasm, served from inside the binary)           │
└──────────────────────────────────────────────────────────────┘
        │                                    │
        ▼                                    ▼
   the city directory                 model providers, MCP servers
   (a tree on this disk)              (only over one gateway endpoint)
```

## 2 The stack, and what each choice costs

The pinned versions live in `Cargo.toml`; this table says why each is there. Where a choice has a cost, the cost is stated rather than implied.

| Concern | Choice | Why it, and what it costs |
|---|---|---|
| Language | Rust, edition 2024, toolchain pinned in `rust-toolchain.toml`, MSRV 1.97 | The invariants this design cares about are expressible as types, and `#![forbid(unsafe_code)]` holds workspace-wide. Cost: compile times, and a wasm client that has to be built before the binary that embeds it. |
| Async runtime | `tokio` 1, only in `channels` and the binary | The turn loop is synchronous on purpose — a decision that awaits is a decision that interleaves. Async stops at the process boundary. Cost: one blocking HTTP call per model request, paid inside a worker rather than a reactor. |
| HTTP server | `axum` 0.8 with its `ws` feature | It carries the WebSocket implementation itself, so the protocol has one version authority rather than two. |
| HTTP client | `reqwest` 0.13, blocking, `rustls`, no default features | One client for the whole workspace: providers and HTTP-reached MCP servers. Two clients would mean two TLS stacks in one binary. |
| Client | `dioxus` 0.7.10, `default-features = false`, `minimal` + `web`, built **without** `dx` | Rust to `wasm32-unknown-unknown` plus a pinned `wasm-bindgen` CLI. Dropping default features removes the `asset!` macro and devtools, both of which need the CLI we deliberately do not use. Cost: no hot reload; the CLI version must equal the crate version or the build breaks quietly. |
| History | JSONL segments, appended, chain-verified | A history a person can read with `tail` and a machine can verify byte by byte. Cost: the Ledger's throughput is the city's throughput (§11). |
| Cold views | `redb` 4.2 | Embedded, transactional, crash-safe. The projection is derived, so its file is disposable and never a second authority. |
| Content store | BLAKE3 (`blake3` 1.8) | One hash for the whole library: content addressing and `IdemKey` derivation. Identical content is stored once. |
| Restoration | `git2` 0.21, vendored libgit2 | Git is the restoration authority for tracked files, so a discarded file points at a checkpoint commit. Also one worktree per reviewing run. Cost: a C library in the tree, vendored so there is no system dependency. |
| Sandbox | `wasmtime` 48 + `wasmtime-wasi`, wasip1 only | Fuel-metered execution with **no socket host implementation** — the Python arm's mechanical proof that it cannot reach the network. Cost: an optional feature; a build without it refuses tool execution in three parts rather than pretending. |
| Credentials | `keyring` 3 (platform credential service), `secrecy` 0.10, `zeroize` 1.9 | Plaintext lives in the operating system's own vault, never in a file we wrote. `sha2` 0.10 is present for one external protocol fact: PKCE mandates SHA-256. |
| Entropy | `getrandom` 0.3 | OS entropy for the PKCE verifier and the login state. It is *not* the seeded RNG the simulator uses, and must never become it. |
| Serialisation | `serde` 1, `serde_json` 1, `toml` 0.8 | JSON on the wire and in the Ledger because the receiver may be a browser and a person still has to read it. TOML for configuration a person edits. |
| Errors | `thiserror` 2 | One error shape, `AxError`, defined in `kernel::error` and mapped at every crate boundary. |
| Release profile | `lto = "fat"`, one codegen unit, symbols stripped, `panic = "abort"` | Crash-only delivery: there is no unwinding path to maintain, because there is nothing to catch. |
| Dependency count | 497 packages in `Cargo.lock` | Listed by `sprawling status --deps`, licence-checked one by one by `cargo deny` against `deny.toml`. |

**Verification tools**, kept out of the shipped binary: `proptest` (properties before examples), `insta` (golden output), `trybuild` (proof that something cannot be expressed), `kani` (bounded proof, Linux CI), `cargo-mutants` (do the tests bite), `cargo-fuzz` (parsers against hostile bytes).

## 3 Twelve units and the dependency law

Crates are not a reuse mechanism here. They exist so that **the dependency rules are executed by the compiler**: the wall `pub(crate)` builds is drawn at the crate boundary, so twelve crates are twelve walls that actually close.

```
sprawling (bin: init/serve/resume/replay/fork/adopt/export/restore/status)
   ├─ runtime ──→ kernel, memory, gateway   turns, tools, sandbox, watchdog, fork
   ├─ collab  ──→ kernel, memory            inbox, drafts, workshop, fan-in, pull requests
   ├─ city    ──→ kernel                    space, residents, archive, library, schedule
   ├─ eval    ──→ kernel, memory            suites, probes, scoring, metabolism
   ├─ browser ──→ kernel                    WebDriver BiDi sessions, page snapshots
   ├─ protocol──→ kernel                    MCP outbound, ACP inbound
   ├─ memory  ──→ kernel                    Ledger, CAS, projections, attribution, git
   ├─ gateway ──→ kernel                    routing, dialects, market, cost, credentials
   ├─ channels──→ kernel                    Command/Query/Event, WebSocket server, auth
   └─ web     ──→ channels                  the one client, in a browser (wasm)
kernel: no internal dependencies.
```

The block below is what the machine reads. Actual edges must be a **subset** of it, so a hidden edge is a red build rather than a discovery:

```depmap
kernel:
memory: kernel
gateway: kernel
runtime: kernel, memory, gateway
collab: kernel, memory
city: kernel
eval: kernel, memory
browser: kernel
protocol: kernel
channels: kernel
web: channels
sprawling: kernel, memory, gateway, runtime, collab, city, eval, browser, protocol, channels
```

**Three rules.** Dependencies point inward, and never back. A seam declares its trait in the inner layer and implements it in the outer one, so `kernel` can define what a Ledger *is* without knowing where it is written. And splitting into crates buys compiler-enforced layering, not reuse — nothing here is published.

**Three kinds of edge**, and telling them apart is what makes the repository readable:

| Edge | When it exists | Example |
|---|---|---|
| Dependency | compile time | `runtime → kernel` |
| Assembly | run time, only in `bin::assembly` | the upload sink in `channels::server` receiving `memory::cas` |
| Event | anywhere a `kernel::Ledger` handle is held | writing `tool_result` after a tool runs |

`runtime` has the widest fan-out — three crates at once. It may **use** their interfaces and nothing more; the moment a runtime module starts passing concrete types between `memory` and `gateway`, that edge moves up into the assembly layer.

`xtask` and `citysim` are workspace members outside the product graph. **citysim drives the turn loop a second time**: `runtime::run::drive` with simulated adapters — a scripted model, scripted tools, an in-memory Ledger — which is how one seed reproduces a run. It stops below `bin::assembly`, whose `RunWorker` builds its model adapter out of the endpoint book rather than receiving one; the dispatch policy above that line is held by that module's own tests. `sprawling` carries a lib target so the policy is at least *reachable* — an integration test enters by the same door `channels::server` uses — and inverting the model seam is what a seeded scenario would still need.

## 4 Seams

A seam is a trait declared in the inner layer and implemented outside it. **One adapter is a hypothetical seam; two make it real** — so every seam ships with a second implementation, and `cargo xtask depmap` refuses a `pub trait` declared anywhere but the files below.

| Seam | Declared in | Production adapter | Second adapter |
|---|---|---|---|
| `kernel::ledger` | crates/kernel/src/ledger.rs | memory: jsonl segments with tail recovery | citysim: in-memory Ledger |
| `kernel::tool` | crates/kernel/src/tool.rs | runtime tools, collab tools, browser, protocol | citysim: scripted tools |
| `kernel::model` | crates/kernel/src/model.rs | gateway: native and endpoint | citysim: scripted model |
| `runtime::sandbox` | crates/runtime/src/sandbox.rs | wasmtime with fuel metering | pass-through and fault doubles |
| `browser::port` | crates/browser/src/port.rs | WebDriver BiDi session layer | recording and replay adapter |
| `protocol::mcp` | crates/protocol/src/mcp.rs | stdio child process, or HTTP | `ScriptedOutbound` for offline replay |

**Two inner seams** stay `pub(crate)` because nothing outside their crate needs them: `memory`'s `Vfs` (real filesystem / deterministic power-loss model) and `gateway`'s `Vault` (platform credential service / in-session store).

**Deliberately not seams**: `gateway::dialect` is a pure function and needs no trait; the internals of `city`, `collab` and `eval` have one implementation each and are driven from outside by citysim; `git2` and `redb` are used directly, because an interface with one implementation is decoration.

## 5 One dispatch, end to end

This is the path everything else supports. Following it once explains more than any diagram of boxes.

1. **The page sends a Command.** A person fills in the control surface — address, what to produce, what counts as done — and `web::socket` sends `Command::Dispatch` over the WebSocket. The frame carries no budget: nobody can price a piece of work before it runs.
2. **`channels::server` decides whether to accept it.** Two pure judgements — may this address be bound, may this peer be accepted — with the socket code that surrounds them making no judgement at all.
3. **`bin::assembly` turns it into work.** This is the only place that samples the clock, so the timestamp enters as a parameter from here on. A worker takes the dispatch.
4. **The city writes `run_started` before anything happens.** Every effect becomes an event first; that ordering is the design's load-bearing rule, not a logging preference.
5. **`runtime::prefix` assembles the frozen prefix** in four segments — city, building, resident, run — from `city::spine_files`, `city::policy` and `city::resident`. Assembling it is itself an event, and the result is frozen for the whole run.
6. **`runtime::catalog` decides what the model may see**: the three built-in tools, the collaboration tools this building admits, the skills its reading room allows, and any MCP tools discovered from the building's `CONFIG.toml`. `city::neighbourhood` is scanned in the same breath, so the run also knows which addresses it can reach and who stands at them — without it, `signal` takes an address the model has to have been told.
7. **`runtime::turn` enters its typestate**: Assembling → Calling → Applying → Settling, with four cancellation-safe points. An interruption inside a phase cannot be spelled.
8. **`gateway` makes the call.** `gateway::router` picks the endpoint attached to this tag; `gateway::dialect` translates the canonical Anthropic-shaped conversation into the provider's dialect; `gateway::credential` redeems a `secret:realm/name` reference into a header at the last moment; `gateway::admission` holds the provider's concurrency limit.
9. **The reply is scanned before it is recorded.** `runtime::redact` puts model output through the same secret scan as everything else, so a key a model repeated does not become permanent.
10. **Tools run behind gates.** `kernel::gate` answers with an exhaustive verdict — allowed, refused in three parts, or escalated to a person. `memory::checkpoint` puts a git fence before the wave and scans the worktree after it, so anything that disappeared becomes a `file_discarded` event carrying the way back.
11. **The result comes back shaped.** `runtime::pipeline` builds the result envelope — clock stamp, network reminder, any steer a person sent — and `runtime::compaction` shortens what is too long, always reporting how much it dropped.
12. **Everything lands in the Ledger, and the views follow.** `memory::hot`, `memory::projection` and `memory::attribution` fold the same event stream into what the pages ask for. The server pushes each event; `web::app` folds it into a `Snapshot`. The same fold, on both sides of the wire.

13. **A signal reaches whoever it names, working or not.** After the run freezes, each signal it sent is recorded and then delivered. A steer-kind signal slips under the door of a run that is already going, landing at that run's next safe point with `@` and the sender's address in front of it; anyone else who was spoken to is *knocked* — `bin::assembly` starts a run for them, whose brief names the resident who spoke. Only the person's own entrance can render as `user`, which is what makes an answer go to the right place. A knock addresses a resident, never a frozen run: history is read, not woken.

When the process dies mid-call, `sprawling resume` verifies the chain, closes tool calls whose outcome was lost as *unknown* rather than as failed, and reports what waits for a person.

## 6 On disk

A city is one directory. Copy it and it is the same city; delete it and nothing outside it changes.

```
<city>/
├─ .sprawling/                 the city's own reserved subtree
│  ├─ ledger/                  the only history — jsonl segments, appended, chain-verified
│  ├─ cas/                     content-addressed store, BLAKE3, one copy per content
│  ├─ views/                   redb: cold projections, disposable, rebuilt from the ledger
│  ├─ worktrees/               one git worktree per reviewing run, objects shared
│  ├─ staging/                 uploads land here read-only, never in a worktree
│  ├─ library/                 skills more than one building admits
│  └─ CONFIG.toml              city layer of the three-layer configuration
└─ <building>/                 one building, one line of business
   ├─ .sprawling/              the building's reserved subtree: what governs it (F2.09, F2.10)
   │  ├─ BUILDING.md           the building's rules: confidential, review, write domains
   │  ├─ CONFIG.toml           building layer, including its MCP servers
   │  └─ skills/               skills only this building admits
   ├─ Roadmap.md               the only task table, and the denominator of every progress reading
   ├─ Memo.md                  decisions and corrections
   ├─ Handoff.md               what the next session needs
   └─ <room>/                  one session's workplace, named by the person who started it
      ├─ URBANITE.md           who this resident is and how it works
      └─ JOB.md                the task for this session
```

**One rule, applied at every scope: what governs a scope lives in that scope's `.sprawling/`, and no write domain reaches it.** `is_reserved` answers true for an address with `.sprawling` in any segment, so the check is one predicate in `kernel::address` rather than a list of protected file names. An agent therefore cannot edit its own accounting, its own configuration, its own building's rules, or the history of what it did.

A run's write domain is what its building's `BUILDING.md` declares, and **the whole building when it declares nothing** — which is the shipped template. The room is where a session works, not the boundary that contains it.

## 7 The wire

One WebSocket, three kinds of frame, and a schema hash that both ends check on connect: a page from a different build refuses rather than misreads. `WIRE_V` is 4.

| Frame | Count | What it is |
|---|---|---|
| `Command` | 20 | something a person wants done: dispatch, steer, cancel, approve, halt, raise a building, attach an endpoint |
| `Query` | 11 | something a page wants to know: the city, one run, approvals, cost, the ledger, archive, discards, inboxes |
| `Event` | the Ledger's own kinds | what happened, pushed as it happens |

Two properties are worth stating because they are enforced by types rather than by review. A `Command` carrying a credential **cannot be serialised**: `Sealed<T>` has no `Serialize`, and the `PutSecret` payload of the remote command type is an uninhabited type, so entering a key over the network is not a request that can be spelled. And every `Query` must be answered exhaustively — the answer match has no catch-all, so adding a query without answering it does not compile.

## 8 Parts you can replace

This repository bundles nobody's key, pays for nothing, and proxies nothing. Everything that reaches outside is therefore an adapter you can swap, and this section says where each one lives.

### Provider intelligence — followed from codex and pi

Signing in to a provider means knowing four things: authorization endpoint, token endpoint, client id, scopes. Those are facts, and they change without warning, so they are followed from two actively maintained projects rather than watched by hand: [`openai/codex`](https://github.com/openai/codex) (Apache-2.0) for OpenAI, [`earendil-works/pi`](https://github.com/earendil-works/pi) (MIT) for Anthropic and the rest. **What is followed is intelligence, not code** — see [`docs/third-party.md`](docs/third-party.md) for the obligations and how to re-check.

| To do this | Change this |
|---|---|
| add or correct a subscription provider | `gateway::oauth_profiles` — a table with data and zero branches |
| change how a login is begun, finished or renewed | `gateway::credential` |
| use an API key instead | the settings page: base URL, dialect, key |
| speak a third dialect | `gateway::dialect`, a pure two-way translation with the canonical shape in the middle |
| run a local model | `gateway::native` — local inference never goes through the outbound gateway |

**What you cannot move out**: credential custody. Plaintext reaches the platform credential service and nothing else, configuration holds a `secret:realm/name` reference, and that is part of what the product promises rather than an implementation detail.

### Outside applications — MCP, and Composio as one server among many

Mail, GitHub, Figma, Discord: writing an integration for each is a weekly chore unrelated to the problem here, so the whole class is outsourced over **MCP**. [Composio](https://composio.dev) is the first choice and is reached the same way any other server is — this code never knows what Composio is.

| To do this | Change this |
|---|---|
| give a building tools from a server | its `CONFIG.toml`: a `command` starts a child process, a `url` reaches a hosted server |
| point at a different provider of the same tools | the same URL field. Nothing else changes |
| add a transport | `bin::mcp_stdio` and `bin::mcp_http` are the two adapters behind `protocol::mcp`'s `Outbound` seam |
| drive this city from an editor | `protocol::acp` accepts an outside request as an ordinary dispatch |

A confidential building constructs none of them: data may enter and may not leave.

### The rest

| Part | Seam or surface | Note |
|---|---|---|
| execution sandbox | `runtime::sandbox` | implement the trait, pass its conformance suite; the shipped adapter is wasmtime with fuel |
| the client | `channels::wire` | the wire is the whole API; a second client writes against it |
| the browser driver | `browser::port` | frames in, replies out; the shipped adapter speaks WebDriver BiDi |
| where views are stored | `memory::projection` | delete the store and it rebuilds from the Ledger, byte-identical |

## 9 Seven shapes

Every module instantiates exactly one of these. The classification earns its place by what it catches: a module that cannot name its shape usually holds two things that want to be separate files.

| # | Shape | The test for it |
|---|---|---|
| 1 | decision | no I/O, no clock, no global state; returns an exhaustive enum, never a bool |
| 2 | value | invariants enforced at one construction point; private fields; no setters |
| 3 | port | a trait on a seam, delivered with its conformance suite |
| 4 | adapter | thin, no policy: swapping in a second implementation changes no policy |
| 5 | typestate | phase changes enforced by types; no method returns a previous phase |
| 6 | data | data only, no branches. Editing it is editing behaviour |
| 7 | projection | folds the event stream into a view; deleting it and rebuilding gives the same bytes |

**The Humble Object is the recurring move**: the hard-to-test end is stripped to nothing and the thick end stays pure. `web::app`, `runtime::watchdog`, `gateway::endpoint`, `memory::projection` and the sandbox adapters are all instances of it.

### Making illegal states unrepresentable

Each of these is a type, not a slogan, and each has a compile-failure counterexample in the test suite — because "unrepresentable" is itself a claim that needs testing.

**These expectations are byte comparisons against a compiler's output, so the machine is part of them.** Installing the `rust-src` component makes rustc render a source snippet inside a `note:` that the committed `.stderr` files do not carry, and every counterexample using one goes red without a line of this repository changing. `cargo public-api` pulls that component in, so `just api-baseline` can turn `just check` red on the next run; remove it (`rustup component remove rust-src`) rather than blessing the longer output, which would only move the failure to CI.

- `EventRef` has no public constructor and no serde ⇒ **a forged event reference cannot be spelled**.
- `Completion::Done` always carries `Evidence`, and `Completion` has no serde ⇒ **a deserialised "finished" cannot be spelled**.
- A `Delegate` value has no `delegate` method ⇒ **a grand-delegate cannot be spelled**; delegation is one level deep.
- `Discard`'s constructor requires a `Restoration` ⇒ **a deletion with no way back cannot be spelled**.
- `Sealed<T>` has no `Serialize` ⇒ **entering a credential remotely cannot be spelled**.
- `UnplannedProgress` has no `ratio` method ⇒ **a percentage with no denominator cannot be drawn**; there is nothing to call.

## 10 Determinism and hardening

The whole city runs as real code, single-threaded, seed-driven, on a virtual clock. That is not a testing convenience — it is the property that makes a failure reproducible from one seed, and these seven rules are its admission conditions.

| # | Rule | Held by |
|---|---|---|
| 1 | Decision paths iterate `BTreeMap`; never a hash order | review, plus the citysim determinism scenarios |
| 2 | Time arrives as a parameter; the one sampling point is `bin::assembly` | `clippy.toml` disallowed methods |
| 3 | One spawn point | review; the exception is runtime's concurrent wave, with structured cancellation |
| 4 | Seeded RNG handed out from one place | assembly derives per session |
| 5 | Execute in parallel, account in series, ordered by `seq` | the Ledger port owns `seq` and `prev` |
| 6 | Ledger payloads hold integers; timestamps are integer milliseconds; field order is declaration order | cross-OS byte fixtures |
| 7 | `IdemKey` derives from `(run, seq, normalised action)` — never from a clock or a random number | property tests |

Rules 1 and 6 together give a checkable property: **the same event sequence replays byte-for-byte identically on any machine.**

**Hardening** is compile-time, workspace-wide, and identical in tests and production except where a test module relaxes it locally: no `unwrap`, `expect`, `panic!`, `todo!`, `unreachable!`, bare indexing or slicing; arithmetic is checked; narrowing goes through `TryFrom`; `as` casts are denied; `unsafe_code` is forbidden outright. Money and quantities are integer newtypes (`UsdMicros`, `Tokens`, `ByteLen`, `Seq`), and floats stay out of every decision path.

**The most fragile point in the design is one paragraph long.** A database handle, one `Instant::now()`, or a bare spawn inside `kernel` disables replay, formal verification and deterministic simulation at the same time. The gates hold that line so the property survives a builder who has never read this file.

## 11 How this is verified, and what it costs

Ten layers, each catching what the layer above cannot. They deliberately do not overlap: overlapping verification reads as more coverage than it is.

| Layer | Catches | Today |
|---|---|---|
| V0 unrepresentable | a whole class of error moved out of what can be written | 15 compile-failure counterexamples |
| V1 types and lints | null, overflow, silent truncation, hidden panics | workspace lints, `-D warnings`, `--all-features` |
| V2 unit and property | a function wrong across a class of inputs | 1,085 tests, properties before examples |
| V3 conformance | a second adapter behaving unlike the first | one suite per port |
| V4 fuzz | parsers meeting hostile bytes | three targets: address, locator, truncated ledger tail |
| V5 formal | termination, absence of overflow, monotonicity | 11 kani harnesses, Linux CI |
| V6 deterministic simulation | components each correct and wrong together | citysim, six scenario files, failures reproduced from a seed |
| V7 mutation | tests that do not bite | `cargo-mutants`, by `just mutants` |
| V8 cross-version, cross-OS fixtures | byte drift after an upgrade or a platform change | golden ledgers in `fixtures/` |
| V9 end to end | the thing a person actually wants to do | the real client in a real browser against a real server, on a developer machine |

**Three gaps, named rather than hidden.** CI has no browser driver, so V9 is a command a developer runs rather than a gate. The isometric city compares display lists rather than bitmaps: the preconditions for bitmap comparison are paid for — placement is a pure function of the id, painter order is total, projection and its inverse are exact — but there is no rasteriser. And V6 stops below `bin::assembly` (§3): a seeded scenario reproduces a run, not a dispatch, because `RunWorker` builds its model adapter instead of receiving one. What holds the dispatch policy is that module's own tests, plus the integration tests the lib target makes possible.

### Four times a gate changed the design

Evidence that the mechanism pays for itself. In none of them was the gate loosened.

1. **The secret gate caught an `.expose()` on a boundary.** `ServeConfig` now takes a digest only, so `channels` cannot obtain the pairing token in plaintext at all — which also removed a length side channel.
2. **The secret gate caught a token's display form.** `PairingToken` now holds only a hash. Sealing a value and unsealing it on the next line is theatre; not holding it is the property that was wanted.
3. **The colour gate found a contradiction between two documents.** One set the brightest step at L=0.930 while a token table placed a hover variant at 0.945. The reading that makes both legal is that the bound delimits the greyscale information surface, and interactive variants sit above it by design.
4. **The public-surface gate caught "just one more `pub use` line" three times.** Re-exporting one item is a public-surface change, and to a builder it feels like it is not.

### The performance register

Sizes are gated because a byte count does not depend on how busy the machine was. Wall-clock figures are measured, reported with the machine that produced them, and never gated: a slow runner is not a defect, and a gate that says it is teaches people to ignore gates. The full register is `xtask/budgets.toml`; `cargo xtask budget` prints it.

| Metric | Budget | Measured | Gated |
|---|---|---|---|
| Client bundle, gzipped | ≤2 MB | 558,419 B — 3.8× headroom | yes |
| The installed binary | ≤128 MB | 8,620,032 B, client included | yes |
| Resident memory, one session | ≤30 MB | 4.3 MB idle, 7.8 MB running a real run | no: the counter means something different on each platform |
| Ledger append plus fsync | p50 ≤5 ms, p99 ≤20 ms | 0.97 ms / 1.61 ms on one NVMe machine | no |
| Projection rebuild | ≥50,000 records/s | about 493,000 records/s on the same machine | no |
| Prefix assembly | ≤1 ms | 0.022 ms for 16.5 KB over four slots | no |
| Resident segment, catalog included | none stated | 815 B on one real dispatch, with thirteen tools admitted | no: what a building admits is the building's, and a reading room with more in it is not a defect |
| Kernel mutation score | ≥90% | by `just mutants` | by that command, not by `just check` |

The two size rows are also rendered as the badges in `README.md`, from this same reading — `cargo xtask badge --write`, which `just dist` ends with. Nobody types a size into a document.

**One honest trade.** With network and model time removed, the throughput ceiling of a city is the throughput ceiling of its Ledger. That is the price of "the Ledger is the only history", stated in the open. The first five walls, in the order they are reached: provider-side rate limits (a few dozen concurrent calls), Ledger fsync, worktree disk, file-descriptor limits, then the blocking pool. RAM is not among them.

## 12 Module map

The machine's data face, parsed by `cargo xtask modmap`: a `.rs` file under `crates/*/src` that is not in this table turns CI red, and so does a row whose file is missing. `lib.rs` and pure index files are exempt because they hold no logic — that too is checked.

Columns are fixed: **Module | File | What it owns | Shape** (§9) **| Since** (the construction stage that introduced it: S0–S5 skeleton, P1–P4 product, R1 repair, F1 front end, P5–P7 documents, measurement and delivery) **| Status** (`planned`, `building`, `built`, `frozen`).

### kernel (27) — every decision in the city, and nothing that touches a disk

| Module | File | What it owns | Shape | Since | Status |
|---|---|---|---|---|---|
| kernel::address | crates/kernel/src/address.rs | canonical relative paths, write-domain primitive, reserved prefix | value | S1 | built |
| kernel::locator | crates/kernel/src/locator.rs | the one grammar for referring to content: `cas:` and `file:`, fail-closed | value | S1 | built |
| kernel::ledger (port) | crates/kernel/src/ledger.rs | the only write entrance to history; owns `seq` and `prev` | port | S1 | built |
| kernel::event | crates/kernel/src/event.rs | EventRecord, the closed EventKind set, and the unforgeable EventRef | value | S1 | built |
| kernel::error | crates/kernel/src/error.rs | AxError and the closed AxCode set, each with its carrier event | value | S1 | built |
| kernel::version | crates/kernel/src/version.rs | optimistic concurrency: a write carries the version it read | value | S1 | built |
| kernel::idem | crates/kernel/src/idem.rs | the deduplication key for outward actions, derived deterministically | value | S1 | built |
| kernel::consts_external | crates/kernel/src/consts_external.rs | constants that follow the outside world, each with its source | data | S1 | built |
| kernel::consts_policy | crates/kernel/src/consts_policy.rs | constants that are our choice; changing one needs evidence | data | S1 | built |
| kernel::gate | crates/kernel/src/gate.rs | the five doors, idempotent dedup, and refusal in three parts | decision | S2 | built |
| kernel::taint | crates/kernel/src/taint.rs | outside content is data: union propagation, no unwrapping surface | value | S2 | built |
| kernel::write_domain | crates/kernel/src/write_domain.rs | which prefixes a resident may write, and edit-war detection | decision | S2 | built |
| kernel::budget | crates/kernel/src/budget.rs | money and tokens as integers, three layers of ceiling | decision | S2 | built |
| kernel::backpressure | crates/kernel/src/backpressure.rs | the city-wide shedding posture: admit or shed, with a reason | decision | S2 | built |
| kernel::stall | crates/kernel/src/stall.rs | the sole criterion for "this run is going nowhere" | decision | S2 | built |
| kernel::goal | crates/kernel/src/goal.rs | two goals wanting the same resource | decision | S2 | built |
| kernel::spine | crates/kernel/src/spine.rs | the Roadmap table's shape and the three moments it may be written | decision | S2 | built |
| kernel::completion | crates/kernel/src/completion.rs | done requires evidence; progress has two states and no third | value | S2 | built |
| kernel::registry | crates/kernel/src/registry.rs | the three books: artifact, asset, skill | value | S2 | built |
| kernel::approval | crates/kernel/src/approval.rs | what waits for a person, its cluster key, and a policy that expires | value | S2 | built |
| kernel::delegation | crates/kernel/src/delegation.rs | two kinds of delegate, one level deep, no grand-delegate | value | S2 | built |
| kernel::repair | crates/kernel/src/repair.rs | leases for when the environment itself is broken | decision | S2 | built |
| kernel::config | crates/kernel/src/config.rs | three-layer resolution, and the frozen/live split with no shared field | decision | S2 | built |
| kernel::tool (port) | crates/kernel/src/tool.rs | what a tool is, in eight fields | port | S2 | built |
| kernel::model (port) | crates/kernel/src/model.rs | what a model call is, carrying the building's policy with it | port | S2 | built |
| kernel::secret | crates/kernel/src/secret.rs | secret-shape judgement, the `secret:` grammar, and `Sealed<T>` | decision | S2 | built |
| kernel::discard | crates/kernel/src/discard.rs | deletion as an effect class; a Discard without a Restoration cannot exist | decision | S2 | built |

### memory (12) — persistence, and every view derived from it

| Module | File | What it owns | Shape | Since | Status |
|---|---|---|---|---|---|
| memory::jsonl | crates/memory/src/jsonl.rs | the durable Ledger: segments, chain verification, tail recovery, group commit | adapter | S1 | built |
| memory::cas | crates/memory/src/cas.rs | content-addressed storage under BLAKE3, written through a temporary file | adapter | S1 | built |
| memory::fault_fs | crates/memory/src/fault_fs.rs | the second filesystem adapter: a deterministic power-loss model | adapter | S1 | built |
| memory::index | crates/memory/src/index.rs | seq to byte offset; disposable, rebuilt when damaged | projection | S3 | built |
| memory::hot | crates/memory/src/hot.rs | the in-memory view the interface reads without touching disk | projection | S3 | built |
| memory::projection | crates/memory/src/projection.rs | the cold view: questions too big for memory, and recovery after restart | projection | S3 | built |
| memory::attribution | crates/memory/src/attribution.rs | where the money went, in five independent cuts that reconcile | projection | S3 | built |
| memory::checkpoint | crates/memory/src/checkpoint.rs | git fences around a tool wave, and what disappeared between them | adapter | S3 | built |
| memory::worktree | crates/memory/src/worktree.rs | one node, one working tree, objects shared and files not | adapter | P2 | built |
| memory::queue | crates/memory/src/queue.rs | one queue implementation serving three lanes | value | S3 | built |
| memory::digest_cache | crates/memory/src/digest_cache.rs | the same bytes summarised once in their lifetime | projection | S3 | built |
| memory::bundle | crates/memory/src/bundle.rs | export and restore; the manifest is the completeness test | adapter | P1 | built |

### gateway (9) — everything between a decision to call a model and the bytes on the wire

| Module | File | What it owns | Shape | Since | Status |
|---|---|---|---|---|---|
| gateway::router | crates/gateway/src/router.rs | the book of attached endpoints and the model chosen per tag | projection | P1 | built |
| gateway::dialect | crates/gateway/src/dialect.rs | pure two-way translation between the canonical shape and a provider's | decision | S3 | built |
| gateway::native | crates/gateway/src/native.rs | local inference, which never leaves the machine | adapter | S3 | built |
| gateway::endpoint | crates/gateway/src/endpoint.rs | the external provider: a self-written wire format over one HTTP client | adapter | S3 | built |
| gateway::oauth_profiles | crates/gateway/src/oauth_profiles.rs | subscription-login intelligence: data only, zero branches | data | S3 | built |
| gateway::admission | crates/gateway/src/admission.rs | the provider's concurrency limit and a deterministic minimum interval | decision | S3 | built |
| gateway::market | crates/gateway/src/market.rs | the model catalogue snapshot, pinned so a price cannot move under a run | value | S3 | built |
| gateway::cost | crates/gateway/src/cost.rs | per-call settlement, with the provider's own figure preferred | decision | S3 | built |
| gateway::credential | crates/gateway/src/credential.rs | custody: capture, replace with a reference, redeem at the wire, renew before expiry | adapter | S3 | built |

### runtime (21) — one run, from dispatch to freeze

| Module | File | What it owns | Shape | Since | Status |
|---|---|---|---|---|---|
| runtime::turn | crates/runtime/src/turn.rs | the turn typestate: four phases, four cancellation-safe points | typestate | S2 | built |
| runtime::prefix | crates/runtime/src/prefix.rs | frozen prefix assembly in four segments, each hashed | decision | S2 | built |
| runtime::handoff | crates/runtime/src/handoff.rs | freezing and resuming: the five-section artifact and its one construction point | value | S2 | built |
| runtime::fork | crates/runtime/src/fork.rs | a new run whose in-window history is a byte-identical prefix of another | decision | S1 | built |
| runtime::compaction | crates/runtime/src/compaction.rs | when to shorten something and what to keep; never larger than its input | decision | P3 | built |
| runtime::redact | crates/runtime/src/redact.rs | what a model said, scanned on its way into history | decision | P3 | built |
| runtime::replay | crates/runtime/src/replay.rs | offline replay: re-verify without re-executing | decision | S1 | built |
| runtime::digest | crates/runtime/src/digest.rs | what a long document looks like from outside, summarised once | decision | P1 | built |
| runtime::pipeline | crates/runtime/src/pipeline.rs | the result envelope and the order in which a result is shrunk | decision | S3 | built |
| runtime::offload | crates/runtime/src/offload.rs | the shared shrink primitive: lossy but restorable, four invariants | decision | S3 | built |
| runtime::watchdog | crates/runtime/src/watchdog.rs | disposal in order: correct, then stall, then freeze | decision | S3 | built |
| runtime::sandbox (port) | crates/runtime/src/sandbox.rs | the execution boundary: capabilities in, outcome out | port | S3 | built |
| runtime::catalog | crates/runtime/src/catalog.rs | progressive disclosure: which tools and skills a run is told about | decision | S3 | built |
| runtime::mode | crates/runtime/src/mode.rs | the modes a run may sit in, and what each admits | decision | S3 | built |
| runtime::clock | crates/runtime/src/clock.rs | formatting an injected instant; it never samples one | value | S3 | built |
| runtime::tools::exec | crates/runtime/src/tools/exec.rs | the exec tool: three arms, each with its own failure story | adapter | S3 | built |
| runtime::tools::read | crates/runtime/src/tools/read.rs | the read tool: a path the reserved subtree closes, or a name the reading room opens | adapter | P6 | built |
| runtime::tools::edit | crates/runtime/src/tools/edit.rs | the edit tool: optimistic concurrency against the version the caller read | adapter | S3 | built |
| runtime::tools::status | crates/runtime/src/tools/status.rs | the model's view of its own situation, in thirteen fields | adapter | S3 | built |
| runtime::run | crates/runtime/src/run.rs | the run driver: dispatch, turns, freeze — one authority for the loop | typestate | P1 | built |
| runtime::diagnostics | crates/runtime/src/diagnostics.rs | the diagnostic log: write-only, five levels, anchored to a Ledger position | adapter | P1 | built |

### collab (16) — several residents in one building, without stepping on each other

| Module | File | What it owns | Shape | Since | Status |
|---|---|---|---|---|---|
| collab::inbox | crates/collab/src/inbox.rs | signals between residents: at-least-once, deduplicated before any effect | decision | P2 | built |
| collab::draft | crates/collab/src/draft.rs | what happens when the room moved while you were writing | typestate | P2 | built |
| collab::steer | crates/collab/src/steer.rs | speaking into a run that is already working, at its next safe point | decision | P2 | built |
| collab::workshop | crates/collab/src/workshop.rs | one creation split into nodes, and the order they run in | decision | P2 | built |
| collab::fanin | crates/collab/src/fanin.rs | where the branches come back together, verified before they merge | decision | P2 | built |
| collab::pr | crates/collab/src/pr.rs | an implementer cannot verify their own work — a compile error, not a rule | typestate | P2 | built |
| collab::arbiter | crates/collab/src/arbiter.rs | who decides when two goals collide, and how far up it goes | decision | P2 | built |
| collab::signal_tool | crates/collab/src/signal_tool.rs | the face the inbox shows a model: send and pull | adapter | P3 | built |
| collab::delegate_tool | crates/collab/src/delegate_tool.rs | the face delegation shows a model: one level down, and the desk that remembers what was asked | adapter | P1 | built |
| collab::handback | crates/collab/src/handback.rs | what a run is told about work it handed down, and who is allowed to say it finished | decision | P1 | built |
| collab::goal_tool | crates/collab/src/goal_tool.rs | the face goal detection and arbitration show a model | adapter | P3 | built |
| collab::pr_tool | crates/collab/src/pr_tool.rs | the face pull requests show a model: open, list, check | adapter | P3 | built |
| collab::archive_tool | crates/collab/src/archive_tool.rs | writing something down so the next run need not be told twice | adapter | P4 | built |
| collab::claim_tool | crates/collab/src/claim_tool.rs | the face `Roadmap.md` shows a model: one claimed row at a time | adapter | P4 | built |
| collab::workshop_tool | crates/collab/src/workshop_tool.rs | the face a workshop shows a model: lay out, ask the join, judge it | adapter | P1 | built |
| collab::triage | crates/collab/src/triage.rs | where something from outside lands, and whether it starts work | decision | P3 | built |

### city (14) — space, identity, and the documents a building keeps

| Module | File | What it owns | Shape | Since | Status |
|---|---|---|---|---|---|
| city::building | crates/city/src/building.rs | which building governs an address, and how a new one comes into being | decision | P2 | built |
| city::resident | crates/city/src/resident.rs | standing identity: an address plus the file that says who lives there | value | P1 | built |
| city::spine_files | crates/city/src/spine_files.rs | the documents a building keeps its long work in, and the job file a run reads | adapter | P2 | built |
| city::archive | crates/city/src/archive.rs | what a building remembers between runs, indexed by computing it | projection | P3 | built |
| city::library | crates/city/src/library.rs | the city's stock of settled work, and the reading room each building admits | decision | P3 | built |
| city::config_layers | crates/city/src/config_layers.rs | the three configuration files a run is governed by | decision | P2 | built |
| city::policy | crates/city/src/policy.rs | `BUILDING.md` evaluated into rules a machine can hold | decision | P1 | built |
| city::rules_tool | crates/city/src/rules_tool.rs | the face a building's rules show a model: read them, propose the whole of them | adapter | P2 | built |
| city::schedule | crates/city/src/schedule.rs | work that starts by itself, counted in whole minutes | decision | P2 | built |
| city::room | crates/city/src/room.rs | which room a named session works in, and how a new one comes into being | decision | F2 | built |
| city::watch | crates/city/src/watch.rs | what the city is listening to, and which building answers | value | P4 | built |
| city::wizard | crates/city/src/wizard.rs | starting a city, and moving a resident inside one | decision | P4 | built |
| city::neighbourhood | crates/city/src/neighbourhood.rs | which addresses a run can reach and who stands at them, in less detail the further away they are | decision | P3 | built |
| city::neighbours_tool | crates/city/src/neighbours_tool.rs | the face the neighbourhood shows a model: this building's addresses, or the city's buildings by name | adapter | P3 | built |

### eval (4) — whether a change made the city better

| Module | File | What it owns | Shape | Since | Status |
|---|---|---|---|---|---|
| eval::suite | crates/eval/src/suite.rs | real tasks split into what a change may learn from and what judges it | decision | P3 | built |
| eval::probe | crates/eval/src/probe.rs | the same questions asked twice, versioned so two versions never compare | value | P3 | built |
| eval::score | crates/eval/src/score.rs | what a settled asset is worth, in integer thousandths | decision | P3 | built |
| eval::metabolism | crates/eval/src/metabolism.rs | clearing out: warn first, retire second, delete never | decision | P3 | built |

### channels (5) — the process boundary

| Module | File | What it owns | Shape | Since | Status |
|---|---|---|---|---|---|
| channels::wire | crates/channels/src/wire.rs | the boundary's vocabulary: Commands, Queries, Events, and the schema hash | value | S4 | built |
| channels::server | crates/channels/src/server.rs | the listening end; the judgements are pure and the socket makes none | adapter | S4 | built |
| channels::control | crates/channels/src/control.rs | the five verbs a person has, and which of them owe a handoff | decision | S4 | built |
| channels::auth | crates/channels/src/auth.rs | pairing tokens: minting, the one readable form, constant-time comparison | value | S4 | built |
| channels::aggregate | crates/channels/src/aggregate.rs | watching several cities from one interface, queries and events only | decision | S4 | built |

### web (20) — the only client, compiled to WebAssembly

| Module | File | What it owns | Shape | Since | Status |
|---|---|---|---|---|---|
| web::app | crates/web/src/app.rs | what the client believes, folded forward from events; holds no business state | projection | S4 | built |
| web::socket | crates/web/src/socket.rs | the only place in this crate that talks to the server | adapter | S4 | built |
| web::city_view | crates/web/src/city_view.rs | the isometric canvas: one geometry for projection and its inverse | decision | S4 | built |
| web::progress | crates/web/src/progress.rs | the one place a progress bar is drawn, for all three of its callers | decision | S4 | built |
| web::dashboard | crates/web/src/dashboard.rs | cost in five cuts, with shares against the authoritative total | decision | S4 | built |
| web::live | crates/web/src/live.rs | watching one session as it happens, in a window that says what it dropped | decision | S4 | built |
| web::approval | crates/web/src/approval.rs | two lists that share one shape: what waits for a person, and what was discarded | decision | S4 | built |
| web::ledger_view | crates/web/src/ledger_view.rs | browsing the one history; a filter always says how much it hid | decision | S4 | built |
| web::alert | crates/web/src/alert.rs | the only module that may interrupt a person, and only once per fact | decision | S4 | built |
| web::lang | crates/web/src/lang.rs | every word this client says, in the two languages it says them in | data | F2 | built |
| web::theme | crates/web/src/theme.rs | the single-hue language: the only place that produces a colour | data | S4 | built |
| web::building_view | crates/web/src/building_view.rs | one building, what it has written down, and what waits in each room | decision | R1 | built |
| web::reach | crates/web/src/reach.rs | what a building's runs may reach, and the one form that sets it | decision | P3 | built |
| web::drop | crates/web/src/drop.rs | what a drag onto a place means, and what it is refused for | decision | P0 | built |
| web::vitals | crates/web/src/vitals.rs | the few numbers no other surface states, and the four it refuses to state | decision | F1 | built |
| web::archive_search | crates/web/src/archive_search.rs | what this city wrote down: the shelves and the record, never merged | decision | F1 | built |
| web::settings | crates/web/src/settings.rs | turning a URL and a key into a model a run can be given | decision | P1 | built |
| web::route | crates/web/src/route.rs | the one translation between a View and the address bar, both ways | decision | F2 | built |
| web::panel | crates/web/src/panel.rs | the one version of a centre panel: conclusion, scope, body, and where the numbers came from | decision | F2 | built |
| web::overview | crates/web/src/overview.rs | the first screen: how much of this city is working, on what, and what waits on a person | decision | F2 | built |

### browser (6), protocol (2), bin (5)

| Module | File | What it owns | Shape | Since | Status |
|---|---|---|---|---|---|
| browser::port (port) | crates/browser/src/port.rs | the browser seam: frames out, replies in; a refusal is an answer | port | P4 | built |
| browser::session | crates/browser/src/session.rs | which frames a conversation with a browser is made of | adapter | P4 | built |
| browser::snapshot | crates/browser/src/snapshot.rs | what a model may see of a page: accessibility tree, never raw DOM | decision | P4 | built |
| browser::act | crates/browser/src/act.rs | turning an intention into frames, with reference and generation both checked | decision | P4 | built |
| browser::devloop | crates/browser/src/devloop.rs | change something, look at it, decide: four outcomes and always an end | decision | P4 | built |
| browser::profile | crates/browser/src/profile.rs | where a browser keeps what it remembers, and who that belongs to | decision | P4 | built |
| protocol::mcp | crates/protocol/src/mcp.rs | reaching an MCP server, and the seam its transports sit behind | adapter | P4 | built |
| protocol::acp | crates/protocol/src/acp.rs | the other direction: an outside editor driving this city | decision | P4 | built |
| bin::main | crates/sprawling/src/main.rs | the command line, each subcommand refused honestly until it exists | adapter | S0 | built |
| bin::assembly | crates/sprawling/src/assembly.rs | the assembly point: the dirtiest component and the only omniscient one | adapter | S0 | built |
| bin::mcp_stdio | crates/sprawling/src/mcp_stdio.rs | an MCP server as a child process, one line per message | adapter | R1 | built |
| bin::mcp_http | crates/sprawling/src/mcp_http.rs | an MCP server over HTTP: one request, one message, no session | adapter | R1 | built |
| bin::firstrun | crates/sprawling/src/firstrun.rs | the first screen, where a city goes when nobody said, and handing a URL to the desktop | adapter | P7 | built |
| bin::install | crates/sprawling/src/install.rs | putting this binary where a shell will find it, and taking it back out | adapter | P0 | built |
| bin::wire_client | crates/sprawling/src/wire_client.rs | the second client of the wire: one frame out, every frame back, and enrolment from stdin | adapter | P3 | built |
| bin::console | crates/sprawling/src/console.rs | what a served city says to the terminal it is running in, and what a line typed there means | decision | P1 | built |

## 13 Changing this document

- **Structure is add-only.** The topology in §3, the seam list in §4, the shape set in §9 and the module map's column contract change only with an explicit ruling, recorded in the commit that changes them.
- **A rejected alternative is recorded where the decision lives**, in the crate's SPEC, rather than in a separate register of regrets.
- **Adding a module row is the registration step**: the row lands in the same change as the file, before it is written.
- **Removing a module row requires a ruling** — `cargo xtask guard` refuses the commit otherwise, because a row that quietly disappears is a rule that quietly stops being enforced.
