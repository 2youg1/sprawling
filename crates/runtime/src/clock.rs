// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Copyright (c) 2026 2youg1

//! ClockStamp: pure formatting of an injected instant against
//! already-resolved zone offsets. Nothing here samples
//! a clock; the calendar is integer arithmetic so replay and live share
//! one algorithm and no tz database.
//!
//! Emission (`StampGate`): `Off` emits never (A18 zero-byte); the first
//! result of a run emits once; `Timestamped` tools emit every result;
//! `Timeless` tools emit only when the granularity bucket changed.

use kernel::consts_policy::CLOCK_ZONES_MAX;
use kernel::{AxCode, AxError, ClockStampGranularity, ClockZone, Temporal, TimeMs};

/// One rendered zone row: the offset it was computed from and the local
/// wall time "YYYY-MM-DD HH:MM".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZoneEntry {
    pub id: String,
    pub offset_min: i32,
    pub local: String,
}

/// The stamp value shared by `status.now` and the result envelope.
/// `utc_ms` is truncated to the granularity bucket, so two stamps inside
/// one bucket are byte-identical (which is what makes Timeless dedup mean
/// something).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClockStamp {
    pub utc_ms: TimeMs,
    pub zones: Vec<ZoneEntry>,
}

impl ClockStamp {
    /// The one text form both envelope and human surfaces use.
    pub fn render(&self) -> String {
        let mut out = String::from("clock:");
        for zone in &self.zones {
            out.push(' ');
            out.push_str(&zone.id);
            out.push(' ');
            out.push_str(&zone.local);
            out.push(';');
        }
        out
    }
}

fn bucket_ms(granularity: ClockStampGranularity) -> Option<u64> {
    match granularity {
        ClockStampGranularity::Off => None,
        ClockStampGranularity::Minute => Some(60_000),
        ClockStampGranularity::FiveMinute => Some(300_000),
        ClockStampGranularity::Hour => Some(3_600_000),
        _ => None,
    }
}

/// Civil date from days since 1970-01-01 (era-based integer algorithm).
#[expect(
    clippy::arithmetic_side_effects,
    reason = "era arithmetic on i64: inputs are bounded by u64 milliseconds / 86_400_000 \
              (about 2e14 days), far inside i64; every divisor is a positive constant, so \
              neither overflow nor division by zero is reachable"
)]
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 }.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    (year, m, d)
}

#[expect(
    clippy::arithmetic_side_effects,
    reason = "div_euclid/rem_euclid by positive constants on i128 values bounded by \
              u64 ms plus an i32 offset in minutes; i128::MIN is unreachable"
)]
fn format_local(utc_ms: u64, offset_min: i32) -> Result<String, AxError> {
    let offset_ms = i128::from(offset_min) * 60_000;
    let shifted = i128::from(utc_ms) + offset_ms;
    let minutes_total = shifted.div_euclid(60_000);
    let days = minutes_total.div_euclid(1_440);
    let minute_of_day = minutes_total.rem_euclid(1_440);
    let day_i64 = i64::try_from(days).map_err(|_| {
        AxError::failure(
            AxCode::InvalidArgs,
            "format local time",
            "date out of range",
        )
    })?;
    let (year, month, day) = civil_from_days(day_i64);
    let hour = minute_of_day.div_euclid(60);
    let minute = minute_of_day.rem_euclid(60);
    Ok(format!(
        "{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}"
    ))
}

/// Formats one stamp: UTC row first (always), then every configured zone.
/// Zones beyond `CLOCK_ZONES_MAX` are refused, not silently dropped.
pub fn stamp(now: TimeMs, zones: &[ClockZone]) -> Result<ClockStamp, AxError> {
    let max = u64::from(CLOCK_ZONES_MAX);
    if u64::try_from(zones.len()).unwrap_or(u64::MAX) > max {
        return Err(AxError::failure(
            AxCode::InvalidArgs,
            "format clock stamp",
            format!("{} zones exceed CLOCK_ZONES_MAX={max}", zones.len()),
        ));
    }
    let mut rows = Vec::with_capacity(zones.len().saturating_add(1));
    rows.push(ZoneEntry {
        id: "utc".to_owned(),
        offset_min: 0,
        local: format_local(now.value(), 0)?,
    });
    for zone in zones {
        rows.push(ZoneEntry {
            id: zone.id.clone(),
            offset_min: zone.offset_min,
            local: format_local(now.value(), zone.offset_min)?,
        });
    }
    Ok(ClockStamp {
        utc_ms: now,
        zones: rows,
    })
}

/// Decides, per tool result, whether a stamp rides the envelope.
#[derive(Debug)]
pub struct StampGate {
    granularity: ClockStampGranularity,
    last_bucket: Option<u64>,
}

impl StampGate {
    pub fn new(granularity: ClockStampGranularity) -> StampGate {
        StampGate {
            granularity,
            last_bucket: None,
        }
    }

    /// The emission rule, in one place.
    pub fn observe(
        &mut self,
        now: TimeMs,
        temporal: Temporal,
        zones: &[ClockZone],
    ) -> Result<Option<ClockStamp>, AxError> {
        let Some(width) = bucket_ms(self.granularity) else {
            return Ok(None);
        };
        let bucket = now.value().checked_div(width).ok_or_else(|| {
            AxError::failure(
                AxCode::InvalidArgs,
                "bucket clock stamp",
                "zero bucket width",
            )
        })?;
        let due = match temporal {
            Temporal::Timestamped => true,
            Temporal::Timeless => self.last_bucket != Some(bucket),
            _ => false,
        } || self.last_bucket.is_none();
        if !due {
            return Ok(None);
        }
        self.last_bucket = Some(bucket);
        let truncated = bucket.checked_mul(width).ok_or_else(|| {
            AxError::failure(AxCode::InvalidArgs, "bucket clock stamp", "bucket overflow")
        })?;
        stamp(TimeMs::new(truncated), zones).map(Some)
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "test code"
)]
mod tests {
    use super::*;

    fn tokyo() -> ClockZone {
        ClockZone {
            id: "tokyo".to_owned(),
            offset_min: 540,
        }
    }

    #[test]
    fn the_integer_calendar_matches_known_instants() {
        assert_eq!(format_local(0, 0).unwrap(), "1970-01-01 00:00");
        assert_eq!(
            format_local(1_709_251_140_000, 0).unwrap(),
            "2024-02-29 23:59"
        );
        assert_eq!(
            format_local(1_785_585_600_000, 0).unwrap(),
            "2026-08-01 12:00"
        );
        assert_eq!(
            format_local(978_287_400_000, 0).unwrap(),
            "2000-12-31 18:30"
        );
    }

    #[test]
    fn offsets_shift_across_midnight_both_ways() {
        // 2026-08-01 12:00 UTC at +540 is 21:00 the same day; at -780 it
        // crosses back to 23:00 of July 31.
        assert_eq!(
            format_local(1_785_585_600_000, 540).unwrap(),
            "2026-08-01 21:00"
        );
        assert_eq!(
            format_local(1_785_585_600_000, -780).unwrap(),
            "2026-07-31 23:00"
        );
    }

    #[test]
    fn utc_row_is_always_first_and_empty_zones_report_utc_only() {
        let s = stamp(TimeMs::new(0), &[]).unwrap();
        assert_eq!(s.zones.len(), 1);
        assert_eq!(s.zones[0].id, "utc");
        let s = stamp(TimeMs::new(0), &[tokyo()]).unwrap();
        assert_eq!(s.zones.len(), 2);
        assert_eq!(s.zones[1].local, "1970-01-01 09:00");
        assert_eq!(
            s.render(),
            "clock: utc 1970-01-01 00:00; tokyo 1970-01-01 09:00;"
        );
    }

    #[test]
    fn zones_beyond_the_cap_are_refused_not_dropped() {
        let too_many: Vec<ClockZone> = (0..5)
            .map(|i| ClockZone {
                id: format!("z{i}"),
                offset_min: 0,
            })
            .collect();
        let err = stamp(TimeMs::new(0), &too_many).unwrap_err();
        assert_eq!(*err.code(), AxCode::InvalidArgs);
    }

    #[test]
    fn off_emits_never_even_for_timestamped_tools() {
        let mut gate = StampGate::new(ClockStampGranularity::Off);
        for t in [1u64, 60_001, 3_600_001] {
            let out = gate
                .observe(TimeMs::new(t), Temporal::Timestamped, &[])
                .unwrap();
            assert!(out.is_none(), "A18: Off must cost zero bytes");
        }
    }

    #[test]
    fn first_result_emits_once_then_timeless_deduplicates_within_a_bucket() {
        let mut gate = StampGate::new(ClockStampGranularity::Minute);
        // First result of the run always stamps (even Timeless).
        assert!(
            gate.observe(TimeMs::new(1_000), Temporal::Timeless, &[])
                .unwrap()
                .is_some()
        );
        // Same minute bucket: no stamp.
        assert!(
            gate.observe(TimeMs::new(30_000), Temporal::Timeless, &[])
                .unwrap()
                .is_none()
        );
        // Next minute: stamps again, truncated to the bucket start.
        let s = gate
            .observe(TimeMs::new(61_000), Temporal::Timeless, &[])
            .unwrap()
            .unwrap();
        assert_eq!(s.utc_ms, TimeMs::new(60_000));
    }

    #[test]
    fn timestamped_emits_every_result_with_bucket_truncation() {
        let mut gate = StampGate::new(ClockStampGranularity::Hour);
        let a = gate
            .observe(TimeMs::new(10), Temporal::Timestamped, &[])
            .unwrap()
            .unwrap();
        let b = gate
            .observe(TimeMs::new(20), Temporal::Timestamped, &[])
            .unwrap()
            .unwrap();
        assert_eq!(a.utc_ms, TimeMs::new(0));
        assert_eq!(b.utc_ms, TimeMs::new(0));
        assert_eq!(a.render(), b.render());
    }
}
