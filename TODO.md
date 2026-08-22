# TODO

Future work, highest priority first. A row names an outcome; its design lands in the SPEC of the crate that owns it before any code is written. A row leaves this file when the work is done, not when it is planned.

## P0 — Redo the front end

Rebuild the client's appearance from the ground up rather than adjusting it. Layout, type, spacing, density, motion, colour, and the isometric city view are all in scope; the ten pages keep what they mean and lose how they look.

The client is correct today and does not yet look like something a person wants to keep open all day. Everything else in this project is judged through it, so this is not cosmetic.

- Lands in `crates/web/`
- Colour comes from `web::theme` and nowhere else — the `color` gate holds this
- Decisions go into `crates/web/web-SPEC.md` first

## P1 — Ship a Linux archive

`release.yml` verifies on Windows and builds archives for Windows and macOS. Linux is on hold by the person's ruling; bringing it back means adding `ubuntu-latest` to both the `verify` and `archive` matrices, once somebody has run the result on a Linux desktop and seen `start.sh` open a browser there.

The first CI run on Linux found `xtask/src/mem.rs` importing `std::process::Command` where nothing used it — the class of defect a host-only build never sees. Expect a few more of those before Linux is shippable.
