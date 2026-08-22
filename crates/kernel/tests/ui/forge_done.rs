// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// A6's type half: claiming done makes nothing done. Evidence has no
// public constructor path but `Evidence::new`, which validates.

fn main() {
    let forged = kernel::Evidence(vec![]);
    let _ = kernel::Completion::Done(forged);
}
