# Changelog

Every release is a tag of the form `v<version>-Pre-alpha-<YYMMDD>`. The date is
part of the name because a pre-alpha version number says almost nothing about
how old the tree is, and how old the tree is, is what a reader of a pre-alpha
release most needs to know.

Each entry records what changed and, where a number is claimed, the machine that
produced it. Wall-clock figures are readings from one developer machine
(i5-1340P, 16 GB, Samsung MZVL41T0HBLB NVMe) and never gates — a slow runner is
not a defect. Byte counts are gated, because a byte count does not depend on how
busy the machine was.

The three releases before this file existed are reconstructed here from their
release notes and their commits.

---

## v0.0.3-Pre-alpha-260903

Build cards V3.01 through V3.55, plus ten lettered follow-ups. The shape of the
work was: make the ledger fast enough
that the interface could be judged, give the city a plan it can walk on its own,
then open the client in a real browser and look at it.

### The city can now walk a plan without being spoken to

- `kernel::plan` — a plan is a tree of addressed nodes, and five malformed
  shapes are refused at the point of construction rather than reported later. A
  cycle is refused as a walk, not as a set, because the person fixing it needs
  to see which edge to cut.
- `kernel::share` — weight is conserved by construction. `Share` has no
  constructor, no arithmetic and no `Deserialize`; the only way to make one is
  `split`, which consumes its input. Conservation is not a rule that is checked,
  it is the only thing the type can express.
- `kernel::pursuit` — a standing goal. The stopping condition is exactly one
  thing: the ready set is empty **and** nothing is in flight. Money is
  deliberately not part of it.
- `kernel::blockage` — a red node propagates up the tree and forward along
  dependency edges, and comes out as one sentence naming the source rather than
  as seventeen red dots.
- The claim tool gained `split` and `block`, reaching six actions, and both new
  ones must carry a reason. The whole six-action schema costs 547 bytes of
  prompt, one byte less than the four-action version did.
- The board page — ready, waiting on dependencies, in progress, blocked, done.
  It holds no state of its own and has no dragging: a node moves because a run
  reported that it moved.

### The interface

- The stylesheet became a file the client and the settled screens both link, so
  a screen is settled against the stylesheet the product ships rather than
  against a copy of it.
- Dispatch collapsed to a single box. Everything else is inferred, the inference
  is one sentence, and every word in that sentence can be clicked and changed —
  so a wrong inference costs one click rather than one re-entry.
- Streaming, end to end. `gateway::endpoint` reads server-sent events; a token
  delta travels as its own frame class and never enters the event stream,
  because a token increment is not an effect. When the settled text and the
  streamed text disagree, the page draws the settled text.
- A session page now answers what a person actually asks: what this session
  cost, which gate it is waiting at, how much context is left, and when the
  handoff was written.
- The prompt the city sent is visible, segment by segment, with the hash of each
  segment — read out of the `prompt_assembled` event rather than recomputed, so
  the page is not a second authority on its own contents.
- A skill whose bytes moved since the last run that mentioned it says so. It
  says *this changed*; it never says *this is safe*.
- Typography and layout: one left edge per page, a spine that is a column rather
  than a margin each child sets for itself, and a sans stack led by open-licensed
  faces. Lato was measured and dropped — a stack entry that changes what a reader
  sees without proving it is better is worse than none.
- The cost page, the approval page and the record table speak the reader's
  language. Eleven phrases in two languages, 1.2 KB, and the gate that now
  refuses the defect they fixed.

### Speed, all measured on one machine

| Reading | Before | After |
|---|---:|---:|
| One `Query::RunHistory`, 50,000-record ledger, end to end | 2,823.5 ms | **1.1 ms** |
| The run-history read itself | 27.6 ms | **3.311 ms** |
| Reading one ledger line at a random offset | 742 µs | **~7 µs** |
| Ledger append, p50 / p99 | 1.047 / 1.879 ms | **0.562 / 0.838 ms** |
| A tool wave that changed one file, whole cycle | 262.8 ms of scan alone | **37.8 ms** |

The four causes were each measured before they were fixed: a byte-at-a-time
read, an index rebuilt under every question, a scan proportional to the worktree
instead of to the change, and a segment file reopened for every single write.
None of them was a guess, and the biggest of them was on the write path where
nobody had looked.

### Four new gates, and one deleted

`cargo xtask gates` now runs sixteen.

- **`ax`** — every role, accessible name and landmark a settled screen wrote
  down is offered by the client too.
- **`wiring`** — a verb the city can carry out is reachable from the client, or
  it is classified on the wire seam with a reason a person may not ask for it.
  It caught the direction nobody watches: `Pursue` and `SetAutonomy` were on the
  wire, matched by the worker, covered by tests, and **no control could reach
  them**. A drawn button that fails is loud; a button that does not exist is
  silent.
- **`render`** — the settled screens are opened in a real engine and the boxes
  are measured where they landed. A conflict inside the cascade exists in
  neither source file, so no gate that reads source can see it.
- **`wording`** — every word a reader is given comes from `web::lang`. Judged by
  position: a literal sitting in a text node or a spoken attribute is an
  English sentence on a Chinese page.
- **`zerojs` deleted.** It asserted that this repository's own commands never
  invoke npm or node. That excluded an architecture rather than a defect: the
  client shipped here is WebAssembly, and a second client in any language is
  supported. Three documents that read as a ban on JavaScript were corrected in
  the same change.

`length` gained two more units — a file may not pass 1,000 lines including its
tests, and a function may not take more than four parameters. `guard` learned to
tell a loosening from a tightening, so striking an exemption no longer costs a
ruling.

### Maintainability, with the debt written down

Eleven files stood above the new 1,000-line limit; all eleven are back under it
and the register of what predates the rule is **empty**, which is that rule's
completed state. Every cut was taken at a seam the architecture or the crate's
own SPEC had already named, never at a line count: `crates/sprawling/src/assembly.rs`
went from 11,461 lines to a module tree, and moving `impl RunWorker` into sixteen
submodules changed the visibility of not one field and added not one public item.

Forty-two over-long signatures are down to twenty-four. An exemption is keyed by
path and function name and **may not travel with the function**, so a signature
being moved is either fixed or left where it is.

### V10: an adversarial property checker, outside the tree

`adversary/` is a third client — Haskell, outside the workspace, driving the
shipped binary over the wire, written to attack rather than to use. Thirteen
properties in 30.8 seconds. It is never a gate: on a machine without GHC,
`just check` behaves byte for byte as it does where the directory is absent.

It has found three things a specific trace would not have:

1. `sprawling call` exits 0 when a refusal does not arrive inside the quiet
   window, while its own documentation promises that exit 1 means the city
   refused. An agent branching on that code reads a failure as a success.
   **Open — the ruling belongs to the Rust side.**
2. A dispatch the city was going to refuse wrote `JOB.md` to disk first.
   **Fixed** (card V3.51): every refusable judgement now runs before the first
   byte is written, in one new phase, and no caller sees a different error code.
3. The wire makes all twenty-three state-changing commands carry an `IdemKey`,
   `kernel::gate::dedup` implements the check as a pure function, and nothing
   calls it — so the same command replayed under the same key happens twice.
   **Open.** A key that must be carried and is never read is a promise the door
   makes and does not keep.

Getting the suite from *does not finish in 1,800 seconds* to *30.8 seconds* was
four measured causes, of which the instructive one is that parallelism was
swapping cities: when two runs contended for a port, the loser's city exited and
the loser's own liveness probe reached the winner's city on the same port, so a
whole trace ran against somebody else's history — **and it reported a defect that
was open at the time as green.** Ports are now lent from a pool and the tree runs
serially.

### Repository

Renamed to `sprawling-agents`. Both READMEs now cross-link
[kusanagi](https://github.com/2youg1/kusanagi) and state the division: one chain
inside a city, one chain per pair between cities.

### Fixed

- A dispatch to an address that was never raised left a directory tree behind
  and nothing in the ledger (found by V10; card V3.51).
- A dispatch into the reserved subtree wrote `JOB.md` before being refused. The
  checker's generator always supplied a session name, so this had never been hit.
- Startup no longer treats an unreadable plan as a plan somebody emptied.
- `#3` — `replay` on a directory holding no ledger reported success. It now asks
  whether there are segments at all, and answers `E_PATH_NOT_FOUND`.
- `#5` — one gate's internal failure discarded the verdicts of every gate after
  it, including `guard`. All sixteen now run to the end and the exit code takes
  the heaviest state, because *not judged* and *judged and clean* were reading
  the same.
- `#2` — `kernel::trybuild` failed on any machine with the `rust-src` component
  installed.

### Known and unfixed

Issue [#1](https://github.com/2youg1/sprawling-agents/issues/1) stays open. The
front end is better and it is not yet worth a working day.

Three defects found by opening the client in a real browser against a real
provider are recorded in `crates/web/web-SPEC.md` sections 8-65 and 8-66 and are
not all fixed: a turn panel whose heading denied the eight turns listed
underneath it, a fold labelled with the wrong speaker, and a page that showed
every part of the answer except the question the person typed. **None of the
three was caught by a gate**, and the reason is the same for all of them: a gate
compares what two sides wrote down, and nothing a page leaves out leaves a trace.

`#/setup` and `#/cost` have no settled screen, which is why the defects keep
landing there.

---

## v0.0.2-Pre-alpha-260827

Cut twice on the same day under the same version number, deliberately: the
second cut changed nothing about what the program does, only whether a person
could watch it do it.

- **A turn says what happened in it.** The client folded three of fifty-eight
  event kinds and discarded the rest, so a refusal and a successful file read
  reached the screen as the same grey line. The message, the token usage, the
  stop reason and the billed amount had been on the wire the whole time.
- **Opening yesterday's session showed an empty page.** History could not be
  asked for by session, so four concurrent sessions split one slice of five
  hundred records.
- **A session lists the files it changed**, with `+` and `−` counts taken from
  git between two real checkpoints. The list cannot contain a file an agent
  merely read, because the fence is the write domain.
- **Dropping a file on the composer** now reaches the composer and never presses
  the button, and a drop target lights up during the drag.
- **A building's documents** come apart by weight and slant rather than by
  colour. This interface has two colours; a syntax-highlighted document would
  have spent both on something that means neither.
- Three features existed and could not be reached: pairing a browser from
  another machine (a four-link chain with three links cut), the console
  answering a query, and the `exec` refusal naming a build that did not exist.
- Four failures reported as something else: a run under review writing to the
  building's shelf, an unreadable file reported as an empty one, a change
  outrunning its own record, and a ceiling lost when work was handed on.
- The dispatch path went from one 1,069-line function to 158 lines across named
  phases, and a gate started failing the build on any production function past
  200 lines.

## v0.0.1-Pre-alpha-260824

The first cut. Two capabilities landed just before it, both verified end to end.

- **Residents find each other.** A run asks for its neighbours and gets every
  address it can reach inside its building, each carrying the line that
  resident's own `URBANITE.md` offers about what to bring them. Detail decays
  with distance: the rest of the city comes back as building names.
- **A message reaches its reader whether or not they are working.** Speaking to
  a busy resident slips the message under the door, where it lands at the end of
  their next tool result; speaking to an idle one starts a run for them. Either
  way it arrives labelled with the sender's address, which is also the address
  that answers it — and a resident cannot render as you, which is a property of
  the type rather than a convention.

Two residents in one building negotiated six hours of kiln time to a written
agreement against a real provider. That is the evidence behind both claims.

---

## Thanks

- **[@Ameshika](https://github.com/Ameshika)** — PR #4, which brought dependabot
  to both ecosystems this repository has.
