# Roadmap — <building name>

> This table is the single denominator for progress in this building. There is no second roadmap and no todo tool.
> Status is one of five words. Free text such as "nearly there" cannot be rendered and cannot be reconciled, so the parser rejects it and names the row.
> Scope moves through **KEEP / ADD / DROP**. Requirements that replace each other are recorded as a replacement, not stacked into a larger project.

| # | Item | Status | Evidence |
|---|------|--------|----------|
| 1 |      | Not started |    |
| 2 |      | Not started |    |

**The five status words**: `Not started` | `In progress` | `Done` | `Blocked` | `Awaiting approval`

Case is not part of the contract: `done` reads as `Done`. Everything else outside the five is a malformed row.

**How to fill the evidence column**: a `Done` row carries a Locator — `cas:<hash>` or `file:<path>@<oid>` — that a reader can retrieve. **A `Done` row with an empty evidence cell stays visible and stays out of the numerator**, so the completion figure on screen means what it says.

**The four columns are fixed.** When a plan needs non-linear dependencies, parallel joins, or retry counts, write a separate diagram file and reference it from one row of this table.
