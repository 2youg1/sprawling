# sprawling

**Organise a crowd of agents into a city, on your own machine. One Rust binary, and one page in a browser.**

![binary](docs/badges/release_binary.svg) ![client](docs/badges/frontend_artifact.svg)

The binary those badges weigh is attached to the [latest release](../../releases/latest); the numbers are rendered from the same measurement the build gate takes, so nobody types a size into a document.

> **Status: pre-alpha, under research and development.** The main loop is connected: register a provider in the browser, raise a building, dispatch a piece of work, and the model really calls tools and writes files into that building. Several agents work at the same time, each in its own room.
>
> What is not connected yet is in [what works today](#what-works-today). Read that section before you put real work through it.
>
> 中文：[README.zh-CN.md](README.zh-CN.md)

**Strengths**: small; concepts that are genuinely cool; built for many agents rather than for one agent with extensions bolted on.

**Countless weaknesses**: a student project, funded by no API reseller and maintained by no lab; no anime mascot; I want the WebUI to be good and I am not quite good enough at it yet; stability and usability both still need debugging.

---

## Why I built this

> The author's own words, in Chinese, kept verbatim. An English translation waits for his review rather than being made on his behalf.

我不想24/7守在电脑面前，直到5h额度撞墙再去睡觉，你也不想。

我换过很多Harness，有些理念落后，有些超出实际：就以RSI来说，在LLM本身脱离无状态之前，Harness能做的只是不断地针对最新的模型做适配和学习公司的现有业务流程并更高速地运行，前者的趋势是消融实验，后者则需要隐私。

越来越多的小规模公司正在出现，它们是有着大量Agent开发在线服务的小型团队，其中99%就是Markdown集+几个天才。

因而我想要制作一个跟上新生的多Agent（Graph engineering）又同时能务实地处理RSI和记忆相关概念风潮的Harness，我将务实的可拓展、节省用户精力、实验性的agent规模化的成本控制、隐私与可靠性、长时运行能力这些放在了设计的核心，并结合了一些城市学与社会学的内容设计了sprawling。

Agent的能力和规模越强，人的注意力就越贵，我不想sprawling成为无数希望劫持你注意力应用中的一个。sprawling区别于常规Hanress聚焦于编写Prompt，最佳使用实践应是转向Loop，安排工作流，让Agent开发sprawling，接管你的固定工作……让你本人专注于新业务的设计，新技能的学习，偶尔回来看一眼跑的怎么样。

诚实地说目前还没有一种多Agent方案提升的性能对得起规模化提升的成本，但探索这项技术在自动化业务，社会模拟以及AI对齐方面的研究刚刚起步，我们还需要花费很多精力和资源探索Agent集群场景下模型的交互行为、协作效率与社会性表现。

Agent记忆的确是实现RSI很重要的途径，但不是依靠Harness做注入，你的文件、代码，文档库就是记忆，Agent真正和你一起成长的尝试在LLM脱离无状态之前大多是对模型的拖累。

如果你想要用自己喜欢的Harness可以试一下RefRain。sprawling主要面向为小团队持久化运营和学术（无论计算机还是人文社科）研究平台，目前还处于研究与开发阶段，欢迎一起开发，也欢迎和我联系/讨论。

除了迁移必要的业务skill/MCP/ACP之外，推荐暂时保持精简，在使用中遇到问题时再手动追加内容，即使是相同的模型搭配不同的Harness都会有完全不同的行为。

我用的电脑不好，所以我不会放任多Agent产生性能开销指数增长的问题，也适合部署在你的旧电脑或云电脑上。

我不卖API也买不起装你信息的硬盘，因而数据都留在本地，我设计了专门的保密楼，配上本地模型完全可以用于处理隐私数据，但这也意味着我没法运行巨大规模的测试。

---

## What it is

One binary, one page in a browser. The client is Rust compiled to WebAssembly and embedded in that binary; there is no second client, and no npm or node appears in the build chain, which a gate checks rather than a convention.

The directory tree on disk is the space: a **City** is a tree, a project is a **Building**, and an agent's workplace is a **Room**.

**One address settles four things at once.** `lab/room1` says where the files are, which files that agent may write, what context it starts with, and who it reports to. Nothing has to keep those four in agreement, because they are one fact.

**The Ledger is the only history.** Every effect becomes an event before it becomes an effect. Every view in the interface is a projection of that stream: throw one away, rebuild it from the Ledger, get the same bytes. Change one byte of the log and verification names the line and refuses to go on.

**A deletion carries its own way back.** The type that represents discarding a file has no constructor without a Restoration — "deleted with no way back" is not refused at runtime, it cannot be written at all. Every row in the recycle bin carries the sentence that brings that file back.

**Cost is attributed five ways**, and each dimension sums exactly to the amount the provider actually billed. Where a provider reports no price — a subscription, for instance — the interface says so instead of printing `$0.00`, because zero and unknown are different facts.

**The interface is built to leave you alone.** No red dots, no unread counts, no infinite feed, no animation on a progress bar. One thing interrupts you: something that needs a person. Everything else waits where you will find it.

**Some states are unrepresentable rather than validated.** Forging an event reference, deserialising a finished-with-evidence, entering a credential over the network, drawing a percentage with no denominator — none of these can be written in the type system. Each has a compile-failure counterexample in the test suite, because "cannot be expressed" is itself a claim that needs proving.

## Running it

The single binary is enough. No npm, no node, no runtime to install.

```bash
sprawling init  <city-dir>          # raise a city; its name goes into the genesis record
sprawling serve <city-dir> [addr]   # start the control surface, loopback only by default
# then open http://127.0.0.1:8787
```

Four steps in the page, about ten seconds:

1. **settings** — the provider's base URL, its dialect (OpenAI or Anthropic) and the key. The key goes straight into the platform credential service; from then on the page only ever shows a `secret:realm/name` reference.
2. Same page, choose a model per tag: `main` thinks, `digest` reads long documents on its behalf.
3. **city** — raise a building.
4. The control surface at the bottom — an address, what to produce, what counts as done. **It does not ask for a budget**: nobody can price a piece of work before it runs, and a subscription has no unit price at all; what it cost is reported afterwards from the record.

The rest of the commands:

```bash
sprawling resume <city-dir>         # after a restart: verify the chain, close tool calls whose
                                    # outcome was lost, report what waits for a person
sprawling fork <city> <run> <seq>   # branch a lineage from one step of a run
sprawling adopt <city> <dir>        # take an existing directory in as a building, overwriting nothing
sprawling replay <ledger-dir>       # verify a chain offline, read-only
sprawling export <city-dir> <file>  # pack a whole city; the manifest is the integrity test
sprawling restore <file> <city-dir> # unpack it on another machine
sprawling status [--deps]           # this machine; --deps lists what is compiled in
```

The whole path from an empty directory to a finished run, skipping no step, is [`docs/getting-started.md`](docs/getting-started.md).

## Five words

| Word | What it is |
|---|---|
| **City** | One city on one machine: one directory tree, one Ledger, one complete history. Two cities never reference each other. |
| **Building** | A building in a city, one line of business. Configuration, archive and write domains are scoped to it. |
| **Room** | A room in a building, which is a subdirectory. One agent works in one room. |
| **Run** | One piece of work with a start and an end. **A resident is an identity; a run is the cost** — the two numbers differ by two orders of magnitude. |
| **Ledger** | The only history. One line per event, append-only, verifiable offline. |

The rest of the vocabulary is [`docs/glossary.md`](docs/glossary.md).

## What works today

**Works**, each with an end-to-end assertion or a real measurement behind it: registering a provider and choosing models; raising buildings and dispatching work; a model really calling tools and writing files into that building; giving a building an outside MCP server; several agents at once, each with its own git worktree, and nothing merging into a building without somebody other than its author verifying it (a compile error, not a rule); ten pages (city, live, approvals, recycle bin, archive, cost, ledger, building, room mailbox, settings); halting a city and letting it go on; verifying a chain offline; exporting a city and restoring it on another machine.

**Built, but never met the real thing**: a hosted MCP server, a real subscription login, a real inbound ACP request over HTTP. All three chains are proven against servers written for the purpose, which proves the chain and not the far end. One real call in the Anthropic dialect hung with no return and the cause is not established; the OpenAI dialect is clear.

**Not built, and why**:

| Not built | Why |
|---|---|
| An OS-level sandbox | It binds per platform and this machine can verify one of three. Unverified isolation is worse than none, because it gets treated as a defence. So today's sentence is "a deletion can be undone", not "a deletion cannot happen" |
| A browser end-to-end run in CI | The loop is a command a developer runs, not a gate |
| A reproducible build | The fixture is written; the compiler flag that makes two builds byte-identical is not set |
| Attributing spend to skills | A decision rather than a gap: a tool call does not happen "under" a skill — a skill is a line of disclosure in the prefix, not a calling context — so attributing spend by call would be inventing a basis |

## Parts you can replace

Nothing here is hosted and no account is held on your behalf, so everything that reaches outside sits on a seam and can be swapped without touching anything else:

| Part | Where it lives | How to replace it |
|---|---|---|
| Subscription-login intelligence (followed from openai/codex and earendil-works/pi) | `gateway::oauth_profiles` (data only, zero branches), `gateway::credential` (flow and renewal) | add a profile row. **Credential custody is never delegated**: plaintext reaches the platform vault and nothing else |
| Model endpoints and dialects | `gateway::endpoint`, `gateway::dialect`; local inference goes through `gateway::native` | fill in a base URL and a dialect on the settings page; a local model is reached directly, not through the outbound gateway |
| SaaS and outside tools ([Composio](https://composio.dev) is one MCP server among them) | the `Outbound` seam in `protocol::mcp`, `bin::mcp_stdio` and `bin::mcp_http`, a building's `CONFIG.toml` | change one URL or one command. A confidential building starts none of them |
| The sandbox | the `runtime::sandbox` seam (today's adapter is wasmtime with fuel) | implement the seam and pass its conformance suite |
| The client | `channels::wire` is the whole API surface | write a second client against that wire |

Where each of these sits, and what stays fixed, is in [`ARCHITECTURE.md`](ARCHITECTURE.md).

## Where it listens, where credentials live

**Loopback only, by default.** To let another machine on your network connect, bind a non-loopback address and set `SPRAWLING_PAIRING_TOKEN`; with no token configured it **refuses to start** rather than starting and refusing connections later. Beyond your network this repository ships no tunnel and no relay: each carries its own trust model, and bundling one would be making a security decision that is yours.

**Plaintext credentials reach no file, no event and no log.** Keys go into the platform credential service and configuration holds only a `secret:realm/name` reference. Model output passes the same secret scan on its way into history, so a key a model repeated does not become permanent.

## Documentation

Everything except the Chinese README and the Chinese getting-started is in English.

- **Just arrived**: this page is enough; one layer deeper is [`docs/glossary.md`](docs/glossary.md).
- **Going to use it**: [`docs/getting-started.md`](docs/getting-started.md) → [`docs/operating.md`](docs/operating.md).
- **Going to change it**: [`ARCHITECTURE.md`](ARCHITECTURE.md) → [`AGENTS.md`](AGENTS.md) → the code and tests next to what you are touching.

Also: [`docs/logging.md`](docs/logging.md) (why the log is not history) and [`docs/third-party.md`](docs/third-party.md) (whose shoulders this stands on, and the licence obligations). [`docs/City.md`](docs/City.md) and [`docs/templates/`](docs/templates/) are the documents the city writes into a building — the agents read them, and so can you.

## Contributing

Read [`AGENTS.md`](AGENTS.md); the thirty-second version:

```bash
cargo install just cargo-nextest --locked
just check
```

A change is finished when that is green. **Write pull requests, issues and review comments in your own language.** A parallel translation — English if you wrote another language, Chinese if you wrote English — is welcome rather than required: with both side by side a reader is faster, and a mistranslation is visible instead of silent.

## Whose shoulders

Signing in to a provider requires knowing a handful of endpoints and parameters. Rather than watch those API docs myself, I follow two actively maintained projects:

| Project | Licence | What is followed |
|---|---|---|
| [openai/codex](https://github.com/openai/codex) | Apache-2.0 | OpenAI's subscription login: endpoints, client id, scopes, device-code flow |
| [earendil-works/pi](https://github.com/earendil-works/pi) | MIT | the same intelligence for Anthropic and the other subscription providers |

**What is followed is intelligence, not code.** Endpoints and parameters are facts; the flow and the credential custody are implemented here.

Connections to outside applications are outsourced the same way: the city speaks **MCP** to any server, Composio among them, and this repository bundles nobody's key, pays for nothing, and proxies nothing. The full list, how to re-check it, and the licence handling are in [`docs/third-party.md`](docs/third-party.md). Licences of code dependencies are checked one by one by `cargo deny`; the allowlist is [`deny.toml`](deny.toml).

## Licence

MPL-2.0, see [`LICENSE`](LICENSE).
