// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// The first line of the double defence (docs/logging.md section 5): a
// sealed value cannot enter a log at the type level. `Sealed` has
// neither Debug nor Display, so neither of the two ways to put a value
// in a message compiles. The scan is the second line, for the plaintext
// that arrives as an ordinary string.

fn main() {
    let sealed: kernel::Sealed<String> = kernel::Sealed::new(Box::new("value".to_owned()));
    let _by_display = format!("the credential is {sealed}");
    let _by_debug = format!("the credential is {sealed:?}");
}
