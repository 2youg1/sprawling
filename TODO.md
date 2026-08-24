# TODO

Future work, highest priority first. A row names an outcome; its design lands in the SPEC of the crate that owns it before any code is written. A row leaves this file when the work is done, not when it is planned.

Where a row says **needs a ruling**, it touches something in the `guard` row of AGENTS.md and cannot start without the person's word, recorded as a `Verdict:` trailer.

Where a row says **evidence**, the claim was checked by running something or by a search whose empty result is the finding. Rows without it are judgements, and are marked as such.

## P4 — The ledger of numbers

1. **Two size readings have drifted past their slack.** Rebuilt on 2026-08-24 after the P0/P1/P2/P3 cards: `frontend_artifact` measures 558,419 B against a best of 490,594 B, and `release_binary` 8,605,696 B against 7,249,920 B. The badges state the new readings, so the badge rule is green and the slack rule is not. The causes are known and cumulative: `web::lang`'s two-language table (F2.14, F2.18), the four modules this session added to the client, and tokio's `signal` feature in the binary. Either recover the size or record the readings with the reason. **needs a ruling**: `xtask/budgets.toml`.

## P5 — Claims made by construction rather than by observation

Not known to be wrong; not watched.

- Nobody has unpacked the Linux archive on a Linux desktop and watched the binary open a browser. `release.yml` builds it, `ci.yml` tests the tree that goes into it, and no human has run the result. **Split it**: the ubuntu job can prove the archive unpacks and the binary serves — what stays unproven is a person seeing a browser window, and the release notes say so rather than implying otherwise.
- The Chinese interface has been read on a headless Edge at three window sizes and by nobody in a real browser. What a screenshot cannot show: whether the wording is right to somebody who did not write it.
