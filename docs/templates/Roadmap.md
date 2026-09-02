# Roadmap — <building name>

> This table is the single denominator for progress in this building. There is no second roadmap and no todo tool.
> **Do not edit this table by hand.** The `plan` tool is its only writer: it keeps the tree legal, and an edit that bypasses it can leave the whole building without a progress figure.
> Status is one of five words. Free text such as "nearly there" cannot be rendered and cannot be reconciled, so the parser rejects it and names the row.
> Scope moves through **KEEP / ADD / DROP**. Requirements that replace each other are recorded as a replacement, not stacked into a larger project.

| # | Item | Weight | Needs | Status | Evidence |
|---|------|--------|-------|--------|----------|
| 1 |      | 1      |       | Not started |    |
| 2 |      | 1      |       | Not started |    |

**The index is a path.** `2.3.1` hangs under `2.3`, which hangs under `2`. Splitting a node with the `plan` tool writes the children for you and numbers them on from the last one, so an index a reader saw yesterday still points at the same work.

**Weight is a ratio among the rows that share a parent, never a quantity.** `1` and `3` beside each other mean one quarter and three quarters of what the parent is worth, and doubling every number on a level changes nothing. A branch hands its whole share to its children, so dividing your own branch generously takes nothing from anybody else's — **the total is always the whole plan, and there is no way to ask for more than you were given.** An empty cell reads as 1.

**Needs names the rows that must finish first**, comma separated. A child waits for what its branch waits for. A dependency that runs in a circle is refused where it is written, because nothing in a circle can ever start.

**The five status words**: `Not started` | `In progress` | `Done` | `Blocked` | `Awaiting approval`

Case is not part of the contract: `done` reads as `Done`. Everything else outside the five is a malformed row.

**How to fill the evidence column**: a `Done` row carries a Locator — `cas:<hash>` or `file:<path>@<oid>` — that a reader can retrieve. **A `Done` row with an empty evidence cell stays visible and stays out of the numerator**, so the completion figure on screen means what it says.

**Only leaves count.** A branch has no work of its own; its children are its work, and counting both would count the same effort twice. A branch that says `Done` while a child does not is refused.

**The six columns are fixed.**
