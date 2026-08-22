// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Constitution 8.3: "that command can only travel an in-process memory
// channel; a remote connection cannot spell this frame." Two ways to try,
// both refused by the compiler rather than by a check at run time.

fn main() {
    // 1. Serialize the in-process command: `Sealed<String>` has no
    //    `Serialize`, so neither does the enum carrying it.
    let local = channels::Command::PutSecret {
        realm: "anthropic".to_owned(),
        name: "api".to_owned(),
        value: kernel::Sealed::new(Box::new("sk-not-a-real-key".to_owned())),
    };
    let _bytes = serde_json::to_string(&local);

    // 2. Put a plaintext credential in the wire-side variant: its payload
    //    type is uninhabited, so no value - of any type - fits the field.
    let _remote = channels::WireCommand::PutSecret {
        realm: "anthropic".to_owned(),
        name: "api".to_owned(),
        value: "sk-not-a-real-key".to_owned(),
    };
}
