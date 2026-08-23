# Concepts and vocabulary

> **For anyone who has to say what this thing does** — someone deciding whether to use it, someone about to operate it, someone about to change it. It answers what the words mean and how the pieces explain each other.
>
> It does not tell you how to install anything ([`getting-started.md`](getting-started.md)), how to run work day to day ([`operating.md`](operating.md)), or why the code has the shape it has ([`../ARCHITECTURE.md`](../ARCHITECTURE.md)).
>
> **This file says which word to use. Which words to avoid has its own single authority** — `xtask/lexicon.toml`, consumed directly by the `lexicon` gate in CI. Read that file to check whether a phrasing is retired; keeping the list in one place is what stops a second authority from forming.

## How the pieces explain each other

A **city** is one directory on one machine, and everything else is inside it. It has one Ledger, which is its complete history, and it never references another city. You can copy the directory to another machine and it is the same city; you can delete it and nothing outside it changes.

Inside a city are **buildings**, and inside a building are floors and rooms. That hierarchy is the directory tree — not a model of it, the tree itself. So an **address** like `lab/room1` names a place on disk, and naming that place settles three separate questions at once: which files an agent may write, which documents it starts with, and who it reports to. A design that separated those three would need rules to keep them agreeing; here they cannot disagree, because they are one fact.

Work happens as a **run**. A run has a start and an end, an address it works at, and a task. It is not a person: a **resident** is the standing identity that survives across runs, with a file of its own that says who it is; a run is the expensive thing that happens when that identity is put to work. Confusing the two is how a design ends up paying for a hundred idle personalities.

Everything a run does becomes an **EventRecord** in the **Ledger** before it becomes anything else — before a file changes, before a model is called, before money is spent. Every other view of the city is a **projection** of that stream: the pages in the browser, the cost report, the recycle bin. Projections are disposable by construction, and the test that keeps them honest is to delete one and rebuild it from the Ledger and require the same bytes.

That is also what makes the interface trustworthy in a specific, narrow way: it can be wrong about what it has not been told, and it cannot be wrong in a way the Ledger does not also record.

Two more relations are worth stating because they are easy to invert. A **Gate** decides one action inside the city, and it answers with an exhaustive verdict rather than a yes or no — so "this needs a person" is a real answer rather than a failure. And a **Discard** is a deletion that carries its own way back: the type has no constructor without one, so a deletion with no restoration is not something the code refuses, it is something the code cannot express.

> The tables below are the vocabulary itself. The order is reading order rather than alphabetical order, because alphabetical order helps nobody on a first pass.

## 1 Space and identity

| Name | What it is |
|---|---|
| **City** | One city on one machine: one Ledger, one complete history. Two cities never reference each other. |
| **Building** | A building within a city. The scope unit for configuration, Archive, and Policy. |
| **Floor** / **Room** | Floors and rooms inside a building. The directory tree is the space. |
| **Build Floor** / **Workshop** | A floor given to one piece of collaborative work, and the node graph that routes work across it. |
| **Utilities** | The shared services of a city, in the reserved subtree: nothing a resident writes to. |
| **Resident** | A standing identity with an `URBANITE.md` and a dossier, surviving across runs. |
| **Ephemeral** | A derived worker discarded after use, with no standing identity. |
| **Run** | One piece of work with a start and an end. **A resident is an identity; an active run is the cost** — the two numbers differ by two orders of magnitude. |
| **Fork** | A new run branched from a point in another run's history. It records a lineage; it does not start driving by itself. |
| **resume** | Reopening a city after the process died: the chain is verified, tool calls whose outcome was lost are closed as unknown, and what waits for a person is reported. |
| **Address** | A path newtype relative to the city root. It sets the write domain, the default context, and who the work reports to. |
| **reserved prefix** | A `.sprawling/` directory and its subtree, at any depth, always outside every write domain. Each scope keeps what governs it there — the city, and from F2.09 each building. An agent cannot edit its own accounting, its own configuration, or its own building's rules. |

## 2 History and content

| Name | What it is |
|---|---|
| **Ledger** | The only history. Every effect becomes an EventRecord first. |
| **EventRecord** | One line of the Ledger, carrying `seq`, `prev`, `t`, `kind`, and a payload. Payloads hold integers. |
| **EventDraft** | An event that has not yet been given a `seq` and a `prev`. Only the Ledger port turns one into an EventRecord. |
| **EventRef** | A reference to an event. **Privately minted**: no public constructor and no serde, so a forged reference cannot be spelled. |
| **Locator** | The retrieval grammar, `cas:` or `file:`. Fail-closed: a shape that does not match is refused rather than guessed. |
| **CAS** | Content-addressed store (BLAKE3). Identical content is stored once for its lifetime. |
| **projection** | A view rebuilt from the event stream. **Disposable**: deleting the table and rebuilding from the Ledger gives byte-identical results. |
| **Snapshot** | The same idea inside the browser (`web::app`): equally disposable, equally forward-only. |

## 3 Context and turns

| Name | What it is |
|---|---|
| **frozen prefix** | The frozen prefix in four segments — city, Building, Resident, Run. Assembling it is itself an event. |
| **Handoff** | The five-section artifact that carries a session across a freeze or a resume. |
| **turn** | The turn state machine: four phases, four cancellation-safe points. An interruption inside a phase cannot be spelled. |
| **Steer** | A redirection: add an instruction to a run **without interrupting it**. It lands at the end of the next tool result. |
| **Cancel** | Stop this run. When Cancel and Steer meet on the same boundary, Cancel wins. |
| **result envelope** | The envelope around a tool result, carrying three attachments: clock stamp, network reminder, and Steer. |
| **ClockStamp** | The clock stamp. With the feature off, output is byte-identical to a build that never had it. |

## 4 Decisions and safety

| Name | What it is |
|---|---|
| **Gate** | Five doors plus idempotent deduplication. A decision returns an exhaustive verdict rather than a bool. |
| **three-part refusal** | A refusal states what was refused, why, and an alternative that can be acted on. |
| **Taint** | External content is data. Taint joins on the union, rises through doors, and has no unwrapping surface. |
| **WriteDomain** | The set of prefixes a resident may write. The decision primitive is `Address::is_within`. |
| **Escalate** | Handing a decision up to a person, as an ApprovalItem, rather than deciding it. |
| **Sealed\<T\>** | A sealed value: no Debug, no Display, no Serialize, no Clone. |
| **SecretRef** | `secret:<realm>/<name>`. Configuration holds the reference; plaintext reaches the Vault only. |
| **Custody** | Credential custody: capture, replace in place with a reference, redeem at the wire. |
| **Discard** | Deletion. A Discard without a Restoration cannot be constructed. |
| **Restoration** | The way back: `Tracked` (committed), `Interred` (in the store), `Rebuildable` (reproducible). |
| **Recycle Bin** | The view over discarded things, where every row can state its own way back. |
| **ApprovalItem** | An item awaiting an answer. Two sources (Gate, agent), carrying a cluster key and a tainted flag. |
| **Policy** | An exemption rule settled from answered ApprovalItems. It expires. |
| **Reading Room** | The list of skills a building admits. A name on it that is not on the shelves is left out rather than promised. |
| **Autonomy** | Who answers: `Owner`, `Delegate`, or `Deferred`. |

## 5 Tools and the outside

| Name | What it is |
|---|---|
| **exec** | The tool that runs a program, a Python artifact, or a shell line, inside the sandbox the frozen configuration allows. |
| **edit** | The tool that changes a file, against a base version, inside the write domain. |
| **status** | The tool that answers what a run's own situation is: turns, budget, what waits for it. |
| **Endpoint** | One provider's chat URL, dialect, credential and headers. The city reaches an **external provider** only through one. |
| **Connector** | An external tool server a building configured, reached over MCP. Its tools carry a `Connector` effect, so the egress door knows where they go without a model naming a host. |
| **subscription login** | Signing in to a provider with a subscription instead of an API key: begin, approve in a browser, bring back the code the provider shows. |

## 6 Interface

| Name | What it is |
|---|---|
| **WebUI** | One page in a desktop browser. There is no second client. |
| **control surface** | The intervention surface at the bottom: five verbs plus the steer input. |
| **Approval Inbox** | The queue of pending answers, grouped by cluster key. A tainted item is never grouped. |
| **progress bar** | The progress bar. |
| **ACCENT** | Jing blue, `H=264`, meaning "something is happening here". |
| **ALERT** | Champagne gold, `H=84`, meaning "a person is needed here". |
| **single-hue language** | The single-hue visual language. |

## 7 Construction vocabulary (not product concepts)

| Name | What it is |
|---|---|
| **SPEC** | A crate's construction authority, `crates/<crate>/<crate>-SPEC.md`, written before its code. It ships with the crate it governs, so a reader of the code has the reasons for it. |
| **shape** | Which of the seven module shapes a file instantiates — decision, value, port, adapter, typestate, data, projection. A module that cannot name its shape usually holds two things. |
| **seam** | A trait declared in the inner layer and implemented in the outer one. One adapter is a supposed seam; two make it real. |
| **conformance** | A generic assertion suite run against any implementation of a port. |
| **gate** (lower case) | A machine guard in CI, run by `cargo xtask gates`. **Deliberately a homonym of the product's Gate above, and the boundary is this line**: a Gate decides one action inside the city, a gate decides whether a change may merge. Neither counts the other, and neither is written down as a number here — `xtask` counts its own. |
