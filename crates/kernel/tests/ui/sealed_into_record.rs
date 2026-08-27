// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Copyright (c) 2026 2youg1

// Sealed values reach no sink: no Serialize, so a sealed credential can
// never ride an EventRecord payload; no Debug, so it cannot be logged.

fn main() {
    let sealed = kernel::Sealed::new(Box::new("hunter2".to_owned()));
    let _ = serde_json::to_value(&sealed);
    let _ = format!("{sealed:?}");
}
