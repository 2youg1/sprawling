// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

// IdemKey is derived, never minted: no From<Uuid>, no From<RunId>, no
// public fields — a human-readable identity cannot impersonate a dedup
// key, or the double-payment defense dies on the resume path.
//
// The bound shape is deliberate. `IdemKey::from(run)` used to be the
// case, and its E0308 pointed at the blanket `From` in `core`, whose note
// carries a source snippet only when `rust-src` is installed — the
// snapshot could not be the same on both machines. An unsatisfied bound
// is reported against the fixture's own lines, so the diagnostic no
// longer depends on what is installed alongside the toolchain.

fn main() {
    fn minting<T: From<kernel::RunId> + From<uuid::Uuid>>() {}
    minting::<kernel::IdemKey>();
}
