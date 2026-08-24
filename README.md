# sprawling

**Turn a swarm of agents into a city on your own machine. One Rust binary. The UI lives in the browser.**

![binary](docs/badges/release_binary.svg) ![client](docs/badges/frontend_artifact.svg)

The binary the badges refer to is attached to the [latest release](../../releases/latest). Both numbers are produced by the same build gate that weighs the artifacts—no one hand-writes sizes into the docs.

> **Status: pre-alpha, research & development.** The main loop works: register a provider in the browser, raise a building, dispatch a job; the model actually calls tools and writes files into that building. Multiple agents can work at the same time, each in its own room.
>
> What’s still missing is listed under [What works / what doesn’t](#what-works-what-doesnt). Read that section before you hand it real work.
>
> 中文: [README.zh-CN.md](README.zh-CN.md)

**Strengths**: tiny footprint; concepts that feel genuinely cool; built for multi-agent from the start, not a single agent with a pile of extensions.

**Weaknesses are many**: a student project with no lab or middleware sponsor behind it; no anime mascot; the WebUI wants to be good but the implementation still lags; stability and usability both need work.

---

## Why this exists

I don’t want to sit in front of a computer 24/7 until the 5-hour quota wall hits and I finally go to sleep. Neither do you.

I’ve tried a lot of harnesses. Some feel conceptually outdated; others overshoot what’s actually useful. Take RSI: until the LLM itself leaves the stateless regime, a harness can only keep adapting to the newest models and learning a company’s existing workflows so it can run them faster. The first trend looks like an ablation study; the second needs privacy.

More and more small companies are appearing—tiny teams shipping online services with a large number of agents. Ninety-nine percent of them are a pile of Markdown plus a few talented people.

So I wanted a harness that keeps up with the emerging multi-agent (graph engineering) wave while remaining pragmatic about RSI and memory-related fashion. I put practical extensibility, saving the user’s attention, experimental cost control for agent scale-up, privacy & reliability, and long-running capability at the core of the design, and mixed in a few ideas from urban studies and sociology. That’s how sprawling took shape.

The stronger and larger agents become, the more expensive human attention gets. I refuse to let sprawling become just another app that tries to hijack yours. Unlike most harnesses that obsess over prompt writing, the best way to use sprawling is to shift toward the loop: design the workflow, let agents develop sprawling itself, and hand over your fixed work… so you can focus on designing new business, learning new skills, and only occasionally checking how things are running.

Honestly, no multi-agent scheme yet delivers performance gains that justify the cost of scale. But exploration of this technology for business automation, social simulation, and AI alignment is only just beginning. We still need a lot of effort and resources to study how models interact, collaborate, and exhibit social behavior inside agent clusters.

Agent memory is indeed an important path toward RSI, but not via harness-level injection. Your files, code, and document libraries *are* the memory. Attempts to make an agent truly grow with you are, before LLMs leave the stateless regime, mostly a drag on the model.

If you prefer a harness you already like, try RefRain. sprawling is aimed at persistent operations for small teams and at research platforms (computer science or the humanities/social sciences). It is still in R&D. Contributions and conversations are both welcome.

Apart from migrating the necessary business skills / MCP / ACP pieces, I recommend staying lean for now and only adding things manually when you hit a concrete problem. Even the same model behaves completely differently under different harnesses.

My own machine is modest, so I refuse to let multi-agent workloads explode in performance cost. That also makes it suitable for old laptops or cheap cloud boxes.

I don’t sell APIs and I can’t afford a hard drive full of your data, so everything stays local. There is a dedicated confidential building; paired with a local model it is fully usable for private data. The trade-off is that I cannot run enormous-scale tests myself.

---

## What it is

One binary, one browser page. The client is Rust compiled to WebAssembly and embedded inside that binary. There is no second client; the build chain contains neither npm nor node—this is enforced by a gate, not by good intentions.

The directory tree on disk *is* the space: a **City** is a directory tree, a project is a **Building**, an agent’s workspace is a **Room**.

**One address freezes four things at once.** `lab/room1` tells you where the files live, which files this agent may write, what context it starts with, and whom it reports to. These four never need a mechanism to stay consistent—they are the same fact.

**Agents find each other and speak without you relaying.** A run can ask who shares its building and gets back every address it can reach, each with the line that resident's own `URBANITE.md` offers about what to bring them—so "who do I talk to" has an answer that is not a guess. Speaking to somebody who is working slips the message under their door: it lands at the end of their next tool result. Speaking to somebody who is not starts a run for them. Either way the message arrives labelled `@` and the sender's address, which is also the address that answers it—**a resident can never render as you**, and that is a property of the type rather than a convention.

**The Ledger is the only history.** Every effect first becomes an event, then becomes an effect. Every view in the UI is a projection of that event stream: delete one, rebuild from the Ledger, and the bytes match. Change a single byte in the log and chain verification reports the line number and refuses to proceed.

**Deletion comes with its own undo path.** The type that means “discard a file” has no constructor without a Restoration—“deleted and gone forever” is not rejected at runtime; it cannot even be written. Every row in the recycle bin carries the exact sentence that can restore it.

**Cost is attributed across five dimensions**, each of which sums exactly to the number the provider actually bills. When a provider supplies no price (e.g. a subscription), the UI says there is no price instead of printing `$0.00`. Zero and unknown are different things.

**The UI’s design goal is not to bother you.** No red dots, no unread counts, no infinite scroll, no animated progress bars. The only thing that interrupts you is a decision that requires a human. Everything else waits where you will find it.

**Every component carries its SPEC beside it.** `crates/<crate>/<crate>-SPEC.md` states that crate's interfaces and the reasoning behind them, and it is written before the code and changed before the code changes. A person and an agent therefore alter this project by reading the same file, and a gate refuses a change whose public surface and SPEC move apart.

**Some states are not validated—they are unrepresentable.** Forging an event reference, deserializing a “completed” status, entering credentials across the network, drawing a percentage without a denominator—these cannot be expressed in the type system. Each has a compile-fail counter-example in the tests, because “cannot be written” is itself an assertion that must be proven.

## Getting it running

### Quick start

1. Download the archive for your system from the [latest release](../../releases/latest).
2. Unpack it anywhere.
3. Run **`sprawling.exe`** (Windows: double-click it) or **`./sprawling`** (macOS, Linux). It asks one question before it creates anything.

That is the whole install. Nothing is registered, and nothing outside that folder is written to—delete the folder and it is gone. A console window opens and stays open: **that window is the city**. Your browser opens at `http://127.0.0.1:8787`; if it doesn’t, open the address yourself. `Ctrl-C` in the window stops the city.

The binaries are not code-signed, so the first run trips a warning. Windows says “Windows protected your PC”—choose **More info → Run anyway**. macOS refuses the first launch—open it once from Finder’s right-click menu.

**Before it can do anything you need a model to call**: an API key for a provider speaking the OpenAI or Anthropic dialect, or a subscription login. sprawling schedules agents, records what they do, and shows it to you; it does not think by itself.

### From a terminal

One binary is enough. No npm, no node, no extra runtime.

From a terminal it is one command, and the same one the launcher runs:

```bash
sprawling up [city-dir] [addr]      # raise the city if it is not there, serve it, open the WebUI
```

Taken apart, when you want the steps separately:

```bash
sprawling init  <city-dir>          # found a city; the name is written into the genesis record
sprawling serve <city-dir> [addr]   # start the control plane; defaults to loopback only
# then open http://127.0.0.1:8787
```

> **Don’t `cargo install` this.** The client is WebAssembly, built before the binary and embedded into it. A plain cargo build cannot run that step, and yields a binary whose page is blank. Take a release archive, or build it with `just dist`.

Four steps on the page, roughly ten seconds:

1. **settings** — enter the provider’s base URL, dialect (OpenAI or Anthropic), and key. The key goes straight into the OS credential store; thereafter the page only ever sees a reference of the form `secret:realm/name`.
2. On the same page, pick models by role: `main` does the thinking, `digest` reads long documents for it.
3. **city** — raise a building.
4. The control surface at the bottom — address, what should be produced, what counts as done. **It never asks for a budget**: nobody can price a job before it runs, and subscriptions have no unit price anyway. Actual spend is reported from the record afterwards. **Nothing rations a conversation either** — when agents wake each other, how long they go on is theirs to decide. What bounds a single run is its turn limit.

Other commands:

```bash
sprawling install [--uninstall]     # make `sprawling` a word your shell resolves, or take it back off
sprawling enrol <realm>/<name>      # read a credential from stdin into the OS store; it never touches the command line
sprawling resume <city-dir>         # after a restart: verify the chain, close tool calls whose results are lost, report who is waiting for a human
sprawling fork <city> <run> <seq>   # branch a lineage from a given step of a Run
sprawling adopt <city> <dir>        # absorb an existing directory as a building without overwriting any files
sprawling replay <ledger-dir>       # offline chain verification, read-only
sprawling export <city-dir> <file>  # pack a city; the manifest is the integrity criterion
sprawling restore <file> <city-dir> # unpack it on another machine
sprawling status [--deps]           # state of this machine; --deps lists the compiled-in dependencies
sprawling help                      # every command, on one screen
```

Launched with no command at all—by double-clicking it, for instance—it shows a single screen, names the folder it would create, and waits for you to agree before creating anything. Founding a city writes the genesis record, and that does not happen because somebody double-clicked a file.

A step-by-step walk from empty directory to first Run lives in [`docs/getting-started.md`](docs/getting-started.md) ([中文](docs/getting-started.zh-CN.md)).

## Five words

| Word | What it is |
|---|---|
| **City** | One city on one machine: a directory tree, one Ledger, one complete history. Two cities never reference each other. |
| **Building** | A building inside the city; one building, one business line. Configuration, Archive, and WriteDomain are all scoped to it. |
| **Room** | A room inside a building, i.e. a subdirectory. One agent works in one room. |
| **Run** | A piece of work with a beginning and an end. **Resident is identity; Run is cost**—the two numbers differ by two orders of magnitude. |
| **Ledger** | The only history. One line, one event; append-only; offline-verifiable. |

The rest of the vocabulary is in [`docs/glossary.md`](docs/glossary.md).

## What works / what doesn’t

**Works**, each backed by an end-to-end assertion or a real measurement: register a provider and select models; raise a building and dispatch work; the model actually calls tools and writes files into that building; **residents find each other, speak, and wake each other without a person relaying a single message** — two of them held a price negotiation to a written agreement against a real provider; attach an external MCP server to a building; multiple agents working concurrently, each with its own git worktree, changes only merge back after others have reviewed them (this is a compile error, not a rule); ten pages (city, live, approvals, recycle bin, archive, cost, ledger, building, room mailbox, settings); pause a city and release it; offline chain verification; export a city and restore it on another machine.

**Not done, and why**:

| Missing piece | Reason |
|---|---|
| OS-level sandbox | Requires per-platform work; only one-third can be verified on this machine. Unverified isolation is worse than none, because people will treat it as a defense. Today’s claim is therefore “a deletion can be undone,” not “a deletion cannot happen.” |
| Browser end-to-end in CI | The loop is a local command, not a gate. **Nobody has driven this release's client in a real browser** — every session behind the claims above went through the wire, which is a debugging door rather than the product. |
| Reproducible builds | Fixtures are ready; the compiler flags that would make two builds byte-identical are not yet set. |
| Attributing spend to skills | This is a decision, not a debt: a tool call does not happen “under” a skill—a skill is a disclosure line in the prefix, not call context. Charging by skill would invent a metric. |

## What you can swap

I sell neither APIs nor account hosting, so everything external sits on a seam and can be replaced without touching the rest:

| Piece | Lives in | How to replace |
|---|---|---|
| Subscription-login intel (following openai/codex and earendil-works/pi) | `gateway::oauth_profiles` (data only, zero branches), `gateway::credential` (flow & renewal) | Add one profile line. **Credential custody is never outsourced**: plaintext reaches only the local credential store. |
| Model endpoint & dialect | `gateway::endpoint`, `gateway::dialect`; local inference via `gateway::native` | Enter base URL and dialect on the settings page; local models connect directly, bypassing the gateway. |
| SaaS & external tools ([Composio](https://composio.dev) is one MCP server among others) | `protocol::mcp` `Outbound` seam, `bin::mcp_stdio` & `bin::mcp_http`, the building’s `CONFIG.toml` | Change one URL or one command to switch servers; confidential buildings start none. |
| Sandbox | `runtime::sandbox` seam (current adapter is wasmtime fuel) | Implement the seam and pass its conformance assertion suite. |
| Client | `channels::wire` is the sole API surface | Want a second client? Write against this wire format. |

Location and replacement steps for each piece are in [`ARCHITECTURE.md`](ARCHITECTURE.md).

## Where it listens, where credentials live

**Defaults to loopback only.** To let another machine on the same network connect, bind a non-loopback address and set `SPRAWLING_PAIRING_TOKEN`. Without a pairing token it **refuses to start**—it does not come up and then reject connections one by one. Beyond that, this repository ships neither tunnel nor relay: those two things each carry their own trust model, and choosing one for you would be making a security decision on your behalf.

**Credential plaintext never enters any file, any event, or any log.** Keys go into the OS credential store; configuration keeps only `secret:realm/name`. Model output is run through the same secret scanner before it is recorded, so a key the model happens to echo never becomes permanent history.

## Documentation

Apart from this page and the getting-started guide, the docs are in English.

- Just arrived and want to know what this is: this page is enough; one level deeper is [`docs/glossary.md`](docs/glossary.md).
- Want to put it to work: [`docs/getting-started.zh-CN.md`](docs/getting-started.zh-CN.md) → [`docs/operating.md`](docs/operating.md).
- Want to change it: [`ARCHITECTURE.md`](ARCHITECTURE.md) → [`AGENTS.md`](AGENTS.md) → the code and tests of the neighboring modules.

Also available: [`docs/logging.md`](docs/logging.md) (why logs are not history), [`docs/third-party.md`](docs/third-party.md) (whose shoulders we stand on, and the license obligations). [`docs/City.md`](docs/City.md) and [`docs/templates/`](docs/templates/) are the documents the city writes into buildings—agents read them, and so can you.

## Contributing

Start with [`AGENTS.md`](AGENTS.md). The thirty-second version:

```bash
cargo install just cargo-nextest --locked
just check
```

When that is green, a change is considered finished. **PR bodies, issues, and review comments may be written in your native language.** If you can, attach a parallel translation (English if your native language is not English, Chinese if it is)—a side-by-side version lets both humans and agents read faster and keeps meaning from being lost in translation.

## Standing on the shoulders of others

Logging into a provider requires a small set of endpoints and parameters. Rather than stare at those API docs myself, I follow two actively maintained projects:

| Project | License | What is followed |
|---|---|---|
| [openai/codex](https://github.com/openai/codex) | Apache-2.0 | OpenAI subscription login: endpoints, client id, scope, device-code flow |
| [earendil-works/pi](https://github.com/earendil-works/pi) | MIT | The equivalent intel for Anthropic and the other subscription providers |

**What is followed is intelligence, not code.** Endpoints and parameters are facts; the flow and credential custody are implemented here.

Connections to external applications are likewise outsourced: the city speaks MCP to any MCP server; Composio is one of them. This repository carries no one’s keys, pays for no one, and acts as no proxy. The full list, how to re-verify, and how licenses are handled live in [`docs/third-party.md`](docs/third-party.md). Licenses of code dependencies are checked one by one by `cargo deny`; the allow-list is [`deny.toml`](deny.toml).

## License

MPL-2.0 — see [`LICENSE`](LICENSE).

---

Questions, bug reports and disagreements are all welcome—open an issue, or write to the address on my profile.
