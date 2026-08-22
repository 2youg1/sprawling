# TODO

Future work, highest priority first. A row names an outcome; its design lands in the SPEC of the crate that owns it before any code is written. A row leaves this file when the work is done, not when it is planned.

## P0 — Redo the front end

Rebuild the client's appearance from the ground up rather than adjusting it. Layout, type, spacing, density, motion, colour, and the isometric city view are all in scope; the ten pages keep what they mean and lose how they look.

The client is correct today and does not yet look like something a person wants to keep open all day. Everything else in this project is judged through it, so this is not cosmetic.

- Lands in `crates/web/`
- Colour comes from `web::theme` and nowhere else — the `color` gate holds this
- Decisions go into `crates/web/web-SPEC.md` first
