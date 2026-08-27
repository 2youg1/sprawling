// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Copyright (c) 2026 2youg1

// IdemKey is derived, never minted: no From<Uuid>, no public fields —
// a human-readable identity cannot impersonate a dedup key, or the
// double-payment defense dies on the resume path.

fn main() {
    let run = kernel::RunId::CITY;
    let _key: kernel::IdemKey = kernel::IdemKey::from(run);
}
