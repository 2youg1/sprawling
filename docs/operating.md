# Operating — running work, answering it, and what to do when something breaks

> **For someone using this city every day.** It covers dispatching and steering work, the five ways a person intervenes, what each page answers, and the routes out of the situations that actually occur.
>
> It does not cover installation ([`getting-started.md`](getting-started.md)) or the design ([`../ARCHITECTURE.md`](../ARCHITECTURE.md)). The diagnostic log has its own file ([`logging.md`](logging.md)) because it is a thing to understand once rather than a step to follow.

## The five verbs

Everything a person does to running work is one of five, and each is recorded as an event like anything else. Three of them require a handoff to be written, because they change what a run was told and the next session has to know that.

| Verb | What it does | Where |
|---|---|---|
| **Steer** | adds an instruction to a run **without stopping it**; it lands at the end of the next tool result | live page |
| **Cancel** | stops one run. When Cancel and Steer meet on the same boundary, Cancel wins — stopping is the one that cannot be taken back | live page |
| **Halt** | stops the whole city; every run reads as halted until you let it go on | control surface |
| **Release** | lets a halted city go on | control surface |
| **Approve** | answers what a Gate or an agent escalated | approvals page |

Steering is not interruption. The instruction is folded into the next assembly, so a request already on the wire is never rewritten mid-flight; what a person said arrives at the next safe point, and the run continues from where it is rather than from where it was.

## What each page answers

The nav is grouped by the question, not by the module.

**Happening now.** *city* is the whole city as a drawing, one prism per building, lit where work is running; the strip above it carries the three numbers no other page can state — how long the Ledger is, how many signals wait in rooms, how much was discarded and never taken back. *live* is one session, line by line, with a bounded window that says what it dropped. *approvals* is what needs a person, grouped so that forty identical questions are one decision — except a tainted item, which stands alone because grouping it would let one answer cover a question nobody read.

**The record.** *ledger* is the event stream with filters that always say how many rows they hid. *archive* searches every building's shelves at the moment you ask, and lists what was filed lately from the record — two sources, never merged, each saying which it is. *recycle bin* is every discarded thing with the instruction that brings it back. *cost* is money and tokens, cut five ways, each summing exactly to what was billed.

**Setup.** *settings* is providers, models and subscription logins.

A building's own page is not in the nav — a city may hold fifty buildings — so the way in is to select one on the city page and press **read it**. There you get its documents, its archive index, and one leaf per room showing what waits in that room's mailbox. Looking at a mailbox does not empty it: the queue is folded from the Ledger, and only a run pulling a signal removes it.

## Approving

An item states what is being asked, who asked, and what it wants to do. Three answers:

- **allow** — this once.
- **refuse** — the run is told, in three parts: what was refused, why, and what it can do instead.
- **and stop asking me this** — a standing policy, offered only where a policy is admissible. Where it is not, the button is absent rather than offered and then refused, because an interface that offers what the far side will reject teaches people to ignore refusals.

Approving does not merely unblock: the work continues from the answer. What was blocked is dispatched again with the cluster you allowed already granted, so answering a group of five is one action rather than five rounds of the same question.

A **tainted** item is one that began with text from outside — a web page, an inbound request, a tool result. It is never grouped, and no policy can waive it.

## Reading cost honestly

The cost page shows shares against the authoritative total rather than normalising its own rows, so an unattributed remainder stays visible instead of being divided away. Where a call came back with tokens and no amount, the page says how many calls that was and why there is no figure. A subscription reports no price at all, and a page that rendered that as `$0.00` would be inventing a fact.

`by_skill` is honestly one bucket. A tool call does not happen "under" a skill — a skill is a line of disclosure in the prefix, not a calling context — so attributing spend to skills by call would be inventing a basis. The dimension stays, reporting what it can defend.

## When something breaks

**A run stopped and nothing is waiting.** Look at the live page for its phase. `frozen` means budget, watchdog or a limit stopped it, and the reason is in the stream. A frozen run does not restart itself; dispatch again, or fork from the step before the problem.

**A provider is slow or silent.** The right-hand status says `degraded` or `lost`. Calls carry a deadline, so a silent provider ends as a timeout rather than hanging a run forever. If it recurs, check the endpoint on the settings page: a provider that stops listing models is usually a key that expired.

**A file is gone.** The recycle bin has a row for it, with the instruction to bring it back — a checkpoint commit to restore from, a stored copy to retrieve, or a way to rebuild it. There is no restore button: nothing on the wire performs a restoration, and a button that does nothing when pressed is worse than a sentence you can act on. If a row says only that the record names a scheme this build cannot read, the Ledger has the plan.

**The whole city is behaving oddly.** Halt it. Halting is recorded, every run reads as halted, and nothing new starts until you release it. Then read the ledger page from before the trouble.

**The process died mid-call.** `sprawling resume <city>` verifies the chain, closes tool calls whose outcome was lost as unknown rather than as failed, and reports what waits for a person.

**Something looks wrong with the history itself.** `sprawling replay <ledger-dir>` verifies the chain offline, read-only. If a byte was changed, it names the line and refuses to go on. This is the check to run before trusting an exported city from somewhere else.

**You need more detail than the pages give.** Raise the log level: `sprawling serve <city> --log decide` explains why verdicts came out the way they did, `--log trace` adds phase changes and retries, `--log wire` adds the bytes of frames. Every line carries the Ledger position it happened at, so a surprising log line and a surprising event meet on one integer. See [`logging.md`](logging.md).

## Moving a city

```bash
sprawling export ~/cities/first city.bundle
sprawling restore city.bundle ~/cities/copy
sprawling replay ~/cities/copy
```

The manifest is the integrity test: a short copy is refused at restore rather than becoming a city that quietly lost an hour. Verifying the chain afterwards is the step that makes the copy trustworthy rather than merely present.

## Two habits worth having

**Give a building a plan.** `Roadmap.md` is the only task table, and it is the denominator for every progress reading you will see. A building without one is not broken — the pages say "no plan" rather than inventing a percentage — but nothing can report how far along it is either.

**Let the agents keep their own notes.** `Memo.md` for decisions and corrections, `Handoff.md` for the next session, the archive for what was worth keeping. They are ordinary files: readable in the browser, editable in your editor, and the same bytes either way.
