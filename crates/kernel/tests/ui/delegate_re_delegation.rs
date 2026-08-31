// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

// One level deep: a Delegate value has no delegate
// method. The grandchild cannot be spelled.

fn main() {
    let root = kernel::Delegator::root();
    let child = root.delegate(kernel::DelegateKind::Ephemeral);
    let _grandchild = child.delegate(kernel::DelegateKind::Ephemeral);
}
