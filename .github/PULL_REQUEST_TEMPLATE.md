<!--
  A pull request is judged by one question: does this change hold?
  The checklist below is the evidence a reviewer needs; the diff shows
  what you did, so the description should say what you found and why.
  Describe in your own language (CONTRIBUTING §4); a parallel translation
  in the other language is welcome.
-->

**What this is** — one sentence: the problem it closes or the capability it adds.

**What I found** — the finding worth writing down: a gate that changed the design, a red-to-green that exposed a real defect, a choice between two approaches whose reason is not visible from the result (CONTRIBUTING §5).

## Checklist

- [ ] **`just check` green** — the closing condition (CONTRIBUTING §0). Paste the last lines of your run:
  `1208 tests passed / all gates green`（照实填，不复制粘贴示例）
- [ ] **Does this touch a protected path?** — `xtask/`, root `Cargo.toml`, `deny.toml`, `clippy.toml`, `justfile`, `.github/`, or a module-table row in `ARCHITECTURE.md`. If yes, the **merge commit** carries a `Verdict:` trailer quoting the person's ruling (CONTRIBUTING §3, `xtask guard`). State that ruling here:
- [ ] **SPEC in step with the code** — the crate's `<crate>-SPEC.md` updated in the same change-set, and in the same **commit** as any public-surface change (`xtask apisync`).
- [ ] **Module map in step** — a new file registered in `ARCHITECTURE.md` before or with it (`xtask modmap`).
- [ ] **Red-to-green visible** — for a defect fix, the failing test sits in the history before the fix (CONTRIBUTING §2 steps 3–4).

## Notes

- `deps:` is the bot's family and carries no card number (CONTRIBUTING §5); a person's change uses `card-<stage>.<index>: <what>`.
- A change that widens a gate instead of fixing the cause is the one thing `guard` exists to refuse; if you believe a gate must move, say why here rather than in the code.
