// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// The taint ring has no exit: no field access, no into_inner — external
// content cannot be washed clean and passed along bare.

fn main() {
    let source = kernel::TaintSource::new("web:x").unwrap();
    let tainted = kernel::Tainted::new("payload".to_owned(), source);
    let bare: String = tainted.value;
}
