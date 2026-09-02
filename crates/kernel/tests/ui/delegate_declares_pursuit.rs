// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

// V3.22: a pursuit is the person's. A sub-agent that could set
// the city working until the work runs out is a sub-agent that can
// spend the night on its own idea, so declaring one takes the
// depth-zero position and a `Delegate` is not one.

fn main() {
    let root = kernel::Delegator::root();
    let delegate = root.delegate(kernel::DelegateKind::Resident);
    let _ = kernel::Pursuit::declare(&delegate, "work all night".to_owned());
}
