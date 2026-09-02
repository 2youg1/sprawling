// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

// V3.17: weight is conserved because it cannot be minted. A share
// exists only as the whole plan or as one part of a share that was
// divided, so there is no way to give a branch more than its parent
// had.

fn main() {
    let forged = kernel::Share(2_000_000_000);
    let _ = forged.ppb();
}
