# The front-end method

**How a screen in this repository goes from a design to shipped pixels.** Settled 2026-09-01, after a measurement of how much a hand-transcribed client had drifted from its own prototype. Everything below was measured on this tree; the one place a claim is not yet verified says so.

`crates/web` is exempt from SPEC-first and from red-before-green (AGENTS.md, "The view layer is exempt from the ceremony"). This document is what replaces that ceremony: it is the method the view layer is held to instead.

## 1 What went wrong, and why it was not a matter of skill

The client was transcribed by hand. A designer settled a screen in HTML, and a person or a model then rewrote that screen as Dioxus RSX. Every loss happened in the rewriting step, and none of it happened in the browser: the same DOM produces the same pixels.

Measured on 2026-09-01, comparing the prototype `prototype.html`, the shipped `assets/index.html`, and the RSX that claimed to implement them:

| Quantity | Value |
|---|---|
| Class names shared by the prototype and the shipped stylesheet | 60 of 262 |
| Class names the RSX emitted for which **no CSS rule existed at all** | 31 |

Thirty-one class names rendered at browser defaults, across approvals, settings, the ledger and buildings — the screens that matter most. The cause was not taste and not effort. **Delete the transcription step and the loss goes with it.**

## 2 The four steps

### Step 1 — settle the screen in HTML

One screen, one `.html` file in `crates/web/screens/`. Link the stylesheet the product actually ships; do not carry a `<style>` block.

```html
<link rel="stylesheet" href="../assets/app.css">
```

A model writes it, a person opens it in a browser, and they iterate until it is right. **Every design judgement is spent here.**

The rule about the stylesheet is the load-bearing one. A prototype that carries its own styles cannot physically look like the product, and that is where "60 of 262" came from. Because the prototype now links the same bytes the product ships, the prototype cannot be prettier than the product and cannot be uglier than it: they are the same interface.

Design tokens are injected at run time by `web::theme`, which a static file cannot reach, so `theme`'s own test writes the same table out to `target/screens/tokens.css` for the prototype to link. The authority is still the table in `theme.rs`; a generated file that drifts turns the test red and is rewritten in place. **Run `cargo test -p web` once before opening a screen file.**

### Step 2 — translate; never transcribe

```bash
dx translate --file crates/web/screens/board.html --output crates/web/src/board.rs
```

Section 3 reports what this tool gets right and what it gets wrong. The part that caused the drift — structure and class names — is exact.

### Step 3 — add bindings, and nothing else

Insert `{state.x}`, `for`, and `if` into the translated output.

> **Replace text nodes and add control flow. Do not change tag structure. Do not change class names.**

This is the wall the whole method rests on. The moment "improve the structure while I am in here" is allowed, the precision bought in step 2 is gone in that edit.

### Step 4 — accept against the accessibility tree

`cargo xtask ax` compares the affordances a settled screen authored — roles, accessible names, current-page marks, landmark elements — against the ones the shipped client emits. It reads both sides from source, so neither keeps a copy of the other.

**What this gate is and is not.** It compares *authored* affordances, not a computed accessibility tree. A computed tree needs a browser, a browser needs a binary this build does not ship, and a gate that cannot run offline is a gate that stops running. Comparing the computed tree against a running client is still worth doing, and it is a person's job with a browser open. The gate catches the drift that actually happened: a `role`, an `aria-label` or an `aria-current` dropped during step 3, which is invisible because the page still renders and the pixels still match, and the only thing lost is what a person who cannot see the pixels was going to be told.

### Step 5 — open the screen in an engine and measure it

```bash
cargo xtask render
```

**This step exists because the four above all read source, and a stylesheet's rules do not collide in source.** They collide in the cascade. Two rules that each read correctly where they were written laid the composer's four panel parts out as a row — title mid-line, the box the work is written in floated to the top right — and put a second left edge on every page, 31px from the first. The tree was green throughout: every gate, every test. The defect was visible to anyone who opened the page and invisible to everything that read the files.

The gate renders each settled screen in whatever Chromium-family browser the machine has, and asserts three properties of where the boxes landed: a page has one left edge, a panel's head is the top of its own panel, and nothing is wider than the region holding it. Properties rather than a screenshot comparison, because a screenshot diff fails on a font hint and passes on a page nobody photographed.

No browser on the machine means the gate prints that it skipped and judges nothing — `SPRAWLING_BROWSER` names one explicitly. A person still looks at the result; what the gate removes is the class of defect that survives *because* nobody looked.

## 3 What `dx translate` does, measured

`dx` 0.7.x, measured 2026-09-01 against real slices of `prototype.html` plus five corner cases.

**Exact, and therefore trustworthy:**

| Item | Result |
|---|---|
| Elements, nesting, order | exact |
| Class names, including multi-class `"step now"` | exact |
| Chinese text | intact |
| `data-*` attributes | correct; hyphens kept and quoted |
| Rust keyword collisions, `type=` | correct, becomes `r#type` |
| `aria-current` → `aria_current`, `viewBox` → `view_box` | correct |
| HTML entities `&amp;` `&lt;` `&#8594;` `&nbsp;` | decoded correctly |
| Self-closing tags `<hr/>` `<br>` `<img/>` | correct |
| Quote escaping | correct |

**One defect you must catch every time. Bare boolean attributes are inverted — 4 of 4 in the sample:**

| Input | Output | Correct |
|---|---|---|
| `<button disabled>` | `disabled: "false"` | `disabled: true` |
| `<input required>` | `required: "false"` | `required: true` |
| `<input autofocus>` | `autofocus: "false"` | `autofocus: true` |
| `<details open>` | `open: "false"` | `open: true` |

In HTML a bare attribute is true by its presence; `dx` translates it to the string `"false"`. The RSX in this repository writes `disabled: <bool expression>`, so the type is wrong as well as the value. **Run this immediately after step 2:**

```bash
dx translate --file "$1" --output "$2"
grep -n ': "false"\|: "true"' "$2" && echo "bare boolean attributes were inverted; write real booleans" && exit 1
```

**Three further deviations, none fatal:**

- Attributes are reordered alphabetically. Harmless to render, noisy in a diff.
- A literal `{}` in text is not escaped to `{{}}`. RSX strings interpolate, so this **fails to compile** — a safe failure, caught by the compiler.
- HTML comments are dropped. Design intent belongs somewhere other than the markup.

## 4 The rules this method replaced, and why

### Rescinded: no JavaScript

The rule bought a real thing: no npm supply chain, no node toolchain, no build step, reproducible builds. It cost 18,867 lines of RSX and a translation gap.

It was probably right while the front end was small. **It became wrong the moment the front end reached 19k lines and design iteration became the bottleneck, and no mechanism existed to announce that moment.** `xtask/src/zerojs.rs` is deleted.

> Should a client runtime ever be wanted again, the suggested boundary is: **no npm dependency and no node build step; a vendored, dependency-free, auditable single file of 20 KB or less is allowed.** That keeps what the rule was buying and releases what it was accidentally blocking.

This repository ships a Rust-to-wasm client, and `crates/channels` is the seam. **Writing a second client in any language is supported**, and nothing here forbids TypeScript.

### The rule that came out of it

> **Every rule that excludes an architecture carries a re-pricing condition.** A rule that was right when it was written is not thereby right now. Write the parameter that made it right beside it, and when that parameter moves, re-argue the rule instead of obeying it.
>
> **Rules that exclude a defect** — the panic bans, the arithmetic bans, the determinism rules — **carry no such condition, because nothing about them expires.**

The test is one question: does this rule exclude an architecture, or a defect?

| | Example | If you deleted it |
|---|---|---|
| Excludes an architecture | no JS, no TypeScript | you may have excluded the right answer |
| Excludes a defect | no `unwrap`, no panic, no silent overflow | you buy no design freedom, only bugs |

The sharper form, learned from this incident:

> **A rule that pushes work into a medium your executor is bad at compounds its cost, and stays invisible almost the whole time.**

Models are strong HTML designers and mediocre RSX designers; the training corpora differ by orders of magnitude. The no-JavaScript rule was not fatal by itself. What was fatal is that it pushed the work into RSX.

### Also re-priced

- **`apisync` relaxed** from every commit to every release. Its necessity could not be shown, and it taxed every change.
- **`guard` narrowed** to the one shape nobody may take without a ruling: a gate loosened while carrying the work it would otherwise have to pass. A commit whose whole diff is gate machinery is a re-pricing, and re-pricing is ordinary work. The old width is the recorded mechanical reason the no-JavaScript rule outlived its argument by a year.

### Kept, because necessity is demonstrable

The panic, arithmetic and `unsafe` bans (crash prevention); the determinism rules — time as a parameter, `BTreeMap` on decision paths, no floats in ledger payloads, one spawn point — because a replayable ledger is the product's own claim; `secret` credential handling; `release` privacy.

### Kept as taste, exempt by definition

Module layout, `pub(crate)` by default, the glossary, comment discipline, the language table, colour from `web::theme`.
