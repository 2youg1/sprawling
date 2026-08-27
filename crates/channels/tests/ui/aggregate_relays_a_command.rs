// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Copyright (c) 2026 2youg1

// Constitution 13.6: "the aggregation layer forwards Queries and Events and
// never forwards a Command." Not a check inside `ask` - `ask` accepts a
// Query, and there is no sibling that accepts a Command, so relaying one
// has no spelling.

fn main() {
    let aggregate = channels::Aggregate::new();
    let elsewhere = channels::CityLabel::parse("attic").unwrap();
    let run = kernel::RunId::from_bytes([1u8; 16]);
    let stop_their_work = channels::Command::Cancel {
        run,
        idem: kernel::IdemKey::derive(&run, kernel::Seq::new(1), b"relay"),
    };
    let _ = aggregate.ask(&elsewhere, stop_their_work);
}
