// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

// Volatile isolation (15.3-4): TimeMs has no conversion into a frozen
// segment — "now" cannot be spelled into the frozen prefix.

fn main() {
    let now = kernel::TimeMs::new(1);
    let _segment: runtime::prefix::FrozenSegment = now.into();
}
