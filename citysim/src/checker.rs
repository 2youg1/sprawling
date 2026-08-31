// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! Invariant checker, first entry: chain intact and seq contiguous
//!. A thin veneer over runtime::replay
//! on purpose — one verification authority, never a second one.

use kernel::AxError;

/// Invariant 1. Errors carry the 1-based failing line in `subject`.
pub fn check_chain(lines: Vec<Vec<u8>>) -> Result<(), AxError> {
    runtime::replay::verify_lines(lines).map(|_| ())
}
