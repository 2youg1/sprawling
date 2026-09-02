# Third-party projects and licensing

> **For anyone who needs to know whose work this stands on and what is owed for it.** It covers the part a machine cannot see: which upstreams are followed for intelligence, which work is outsourced to an outside service, and how a future bolt-on crate is licensed.
>
> Section 3 lists the crates this repository chose and the licence each declares. The full resolved graph is the lockfile's, checked crate by crate by `cargo deny` against the allowlist in `deny.toml` on every CI run.

sprawling is MPL-2.0; see `LICENSE`.

Two kinds of outside thing appear here, and the boundary differs, so they get a section each: **intelligence** is a constant copied down, and a **service** is a third party that does work for the user at run time.

## 1 Where the intelligence comes from

Signing in to a provider requires knowing four things: the authorization endpoint, the token endpoint, the client id, and the scopes. Those are **facts** rather than works, and citing them creates no licence obligation. The source still has to be written down, or "check periodically whether upstream changed" is a discipline with no address to go to.

| Project | Licence | What is followed | Where to look | Tracked to |
|---|---|---|---|---|
| [openai/codex](https://github.com/openai/codex) | Apache-2.0 | OpenAI's subscription login: endpoints, client id, scopes, device-code flow | `codex-rs/login/` | `13fe2bcb7a3b` |
| [earendil-works/pi](https://github.com/earendil-works/pi) | MIT | the same intelligence for Anthropic and the other subscription providers | `packages/ai/src/auth/oauth/` | `55b0db4d3e90` |

> **Machine authority**: `.github/workflows/upstream-watch.yml` parses the five
> columns of the two rows above - project, watch path, tracked commit. The
> column shape is fixed; the prose around them is not (cf. ARCHITECTURE.md
> §3, §12 tables). |

The split is by provider: codex covers the OpenAI side, pi covers Anthropic and the rest. **Only these two.** A third source would buy one cross-check and cost an extra place to read on every review, plus a round of judgement whenever the three disagree.

**How to re-check**: watch those two paths for changes rather than watching releases. An endpoint migration often arrives in a patch version with no mention in the changelog. Where two sources disagree, the provider's own documentation decides, not the majority.

**The watch is automated.** Every day `upstream-watch` asks each path for its newest commit and compares it with `Tracked to`; a difference opens one issue naming the commit, a compare view, and what to re-check. `Tracked to` advances only in the PR that actually realigns the constants - the same change-set that carries the new facts, so the watermark never runs ahead of what the code knows.

**Why follow intelligence and not code**, three reasons, the last one learned by measurement:

1. "Has this endpoint expired?" is not machine-decidable. It is a periodic human task, so the thinner the dependency the better.
2. Facts carry no copyright and code does. Following intelligence makes this file a courtesy; following code would make it an obligation.
3. **Upstream flow code can be wrong while the constants in the same files are right.** Measured in 2026-08: `pi` set the OAuth `state` parameter to the same value as the PKCE `verifier`. The same pattern had been found and fixed by another harness that year, because Anthropic's endpoint refuses such a request with `400 invalid_grant`; every endpoint constant in `pi` was correct. **So this code refuses `state == code_verifier` in `oauth_begin`**: somebody copying the upstream shape while wiring it is the one path by which that defect could arrive here.

## 2 The outsourced service: outside applications and wake-ups

A user may want the city connected to dozens of outside applications - mail, GitHub, Figma, Discord. Writing an integration for each means following each of their APIs, which is a weekly chore unrelated to the problem this repository solves. So that whole class of work is outsourced, and the first choice is [Composio](https://composio.dev) (SDK monorepo [ComposioHQ/composio](https://github.com/ComposioHQ/composio), MIT).

| It does | This code only does |
|---|---|
| each application's OAuth and connection management | read the tool table it offers |
| tool discovery and call execution | write every call into the Ledger, so it replays offline |
| listening for outside events - new mail, a new pull request - and pushing them here | accept what is pushed, and wake the building it belongs to |

The connection is **MCP**, not their SDK: opening the `mcp` option when a session is created yields an MCP endpoint URL that any MCP client can reach. (The older standalone `composio.mcp` service-management API is deprecated; no new code is written against it.) That choice has a direct consequence: **this code never knows what Composio is.** `protocol::mcp` connects to any MCP server, and Composio is one URL among them. A user who does not trust it points at another, or runs their own, and not one line changes here.

Four boundaries, each of them part of what the product promises:

1. **The account is the user's.** No key is bundled, nothing is paid on their behalf, nothing is proxied.
2. **Never poll from here.** Events are pushed and the city receives them. A city that asked "anything new?" on a timer would be generating traffic nobody reads, and would be a second authority on who arrived first.
3. **No redelivery after a disconnection.** A broken machine or router is something a person notices; no compatibility layer is built for it. But nothing degrades silently either: connecting and disconnecting each land an event, so "when was this building not listening" is a readable fact rather than a guess.
4. **A building that is taken down stops listening.** A building is one line of business; when the business ends its outside connections end with it, and no second cleanup list is needed.

Everything an outside tool brings back joins the taint set - outside content is data, never instructions - because those tools cross the same seam as the built-in ones. A confidential building refuses all outbound calls.

## 3 The code this binary is built from

Two lists exist and they answer different questions, so both are kept and neither is a copy of the other.

**What this repository chose** is the table below: the thirty-one crates named in a `Cargo.toml` of this workspace, each with the licence its own manifest declares. A person asking "whose work did these authors decide to stand on" reads this.

**What ends up in the binary** is 413 packages once every transitive dependency is resolved, and that list is the lockfile's. `cargo deny check` reads it on every CI run and refuses any licence outside the allowlist in `deny.toml`; `cargo xtask sbom` writes it out as CycloneDX, which `just dist` produces beside the release artifacts. **That is the machine authority, and this table is deliberately not a second one** — it is thirty-one rows a reader can hold in their head, and it goes stale the way any prose does, while the gate does not.

| Crate | Version | Licence |
|---|---|---|
| [axum](https://crates.io/crates/axum) | 0.8.9 | MIT |
| [blake3](https://crates.io/crates/blake3) | 1.8.7 | CC0-1.0 OR Apache-2.0, or Apache-2.0 with the LLVM exception |
| [dioxus](https://crates.io/crates/dioxus) | 0.7.10 | MIT OR Apache-2.0 |
| [flate2](https://crates.io/crates/flate2) | 1.1.10 | MIT OR Apache-2.0 |
| [futures-util](https://crates.io/crates/futures-util) | 0.3.34 | MIT OR Apache-2.0 |
| [getrandom](https://crates.io/crates/getrandom) | 0.4.3 | MIT OR Apache-2.0 |
| [git2](https://crates.io/crates/git2) | 0.21.0 | MIT OR Apache-2.0 |
| [insta](https://crates.io/crates/insta) | 1.48.0 | Apache-2.0 |
| [js-sys](https://crates.io/crates/js-sys) | 0.3.104 | MIT OR Apache-2.0 |
| [keyring](https://crates.io/crates/keyring) | 3.6.3 | MIT OR Apache-2.0 |
| [proc-macro2](https://crates.io/crates/proc-macro2) | 1.0.107 | MIT OR Apache-2.0 |
| [proptest](https://crates.io/crates/proptest) | 1.11.0 | MIT OR Apache-2.0 |
| [redb](https://crates.io/crates/redb) | 4.2.0 | MIT OR Apache-2.0 |
| [reqwest](https://crates.io/crates/reqwest) | 0.13.4 | MIT OR Apache-2.0 |
| [secrecy](https://crates.io/crates/secrecy) | 0.10.3 | Apache-2.0 OR MIT |
| [serde](https://crates.io/crates/serde) | 1.0.229 | MIT OR Apache-2.0 |
| [serde_json](https://crates.io/crates/serde_json) | 1.0.151 | MIT OR Apache-2.0 |
| [sha2](https://crates.io/crates/sha2) | 0.11.0 | MIT OR Apache-2.0 |
| [syn](https://crates.io/crates/syn) | 3.0.3 | MIT OR Apache-2.0 |
| [tempfile](https://crates.io/crates/tempfile) | 3.27.0 | MIT OR Apache-2.0 |
| [thiserror](https://crates.io/crates/thiserror) | 2.0.20 | MIT OR Apache-2.0 |
| [tokio](https://crates.io/crates/tokio) | 1.53.1 | MIT |
| [tokio-tungstenite](https://crates.io/crates/tokio-tungstenite) | 0.30.0 | MIT |
| [toml](https://crates.io/crates/toml) | 1.1.4 | MIT OR Apache-2.0 |
| [trybuild](https://crates.io/crates/trybuild) | 1.0.120 | MIT OR Apache-2.0 |
| [uuid](https://crates.io/crates/uuid) | 1.26.0 | Apache-2.0 OR MIT |
| [wasm-bindgen](https://crates.io/crates/wasm-bindgen) | 0.2.127 | MIT OR Apache-2.0 |
| [wat](https://crates.io/crates/wat) | 1.258.0 | Apache-2.0 with the LLVM exception, or Apache-2.0, or MIT |
| [web-sys](https://crates.io/crates/web-sys) | 0.3.104 | MIT OR Apache-2.0 |
| [zeroize](https://crates.io/crates/zeroize) | 1.9.0 | Apache-2.0 OR MIT |
| [zip](https://crates.io/crates/zip) | 8.6.0 | MIT |

**Every licence above is permissive, and none of them is copyleft**, which is why an MPL-2.0 binary may be built from them. MPL-2.0 is file-level copyleft: it governs the files in this repository and asks nothing of the crates linked beside them.

**Two obligations travel with a release artifact rather than with this repository**, and neither is discharged by this table:

- Apache-2.0 §4 requires the NOTICE to be kept on distribution and modified files to be marked, so a release archive carries a NOTICE.
- MIT requires the copyright and licence notice to be kept, so the same.

**Links go to crates.io rather than to each project’s repository**: the question this table answers is which licence a crate declares, and crates.io shows that field beside the version a lockfile would resolve to.

**How to regenerate this table.** `cargo metadata --format-version 1 --locked`, take the packages named as a dependency by a workspace member, and read each one's `license` field. A version here that disagrees with `Cargo.lock` is this table being stale, and the lockfile wins.

## 4 A future bolt-on crate

The provider intelligence table is planned to move out into a crate of its own under its own licence (MIT or Apache-2.0), outside this repository's MPL notice, because it is not part of this work.

The cost has to be stated plainly: it would be a **build-time dependency**, so its code enters the shipped binary. The licence obligations therefore **travel with the binary rather than with the repository**:

- Apache-2.0 §4 requires keeping the NOTICE on distribution and marking modified files, so a release artifact must carry a NOTICE.
- MIT requires keeping the copyright and licence notice, so the same.

The acknowledgement in `README.md` does not discharge either. **An acknowledgement is a courtesy and a NOTICE is an obligation; both are required and neither substitutes for the other.**

The same measure governs something not yet started: **upstream synchronisation**. When an upstream publishes a new version - usually because a new model appeared - realigning the bolt-on crate should be **one run opening one pull request**, rather than a person periodically reading two repositories' diffs. It is work the city can do itself, so no separate mechanism is built for it: a schedule entry and a building that owns the bolt-on are enough.

## 5 What will not be done

**Upstream code is not vendored in.** Every `.rs` file here carries the MPL-2.0 notice, and a file pasted from an MIT or Apache-2.0 project cannot wear one. Note that the machine only checks whether a notice is present, not whether it is the right one - **so this rule is held by people, not by a gate.**

**Credential custody is not delegated.** Plaintext reaches only the credential service on this machine. Which facts a bolt-on may hold (endpoints, client ids, scopes) and which never leave (custody, redemption, renewal) is part of what the product promises rather than an organisational convenience.

**No outside service's key is bundled, nothing is paid, nothing is proxied.** Which service to connect and whose account to use is the user's decision.
