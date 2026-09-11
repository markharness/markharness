//! UTC timestamp formatting for identity events.
//!
//! ADR 0026 §6 moved this out of `execution`: an `ExecutionBinding` records
//! no timestamp (ADR 0025 §1), so a time function exported from that module
//! would suggest the opposite. Identity events do carry `recorded_at`, so
//! the function itself is still needed.

use std::time::{SystemTime, UNIX_EPOCH};

/// Converts a count of days since the Unix epoch into a proleptic Gregorian
/// date, per Howard Hinnant's `civil_from_days` algorithm (public domain,
/// http://howardhinnant.github.io/date_algorithms.html). Avoids pulling in
/// a date/time crate for a single UTC timestamp field.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

/// Crate-visible so `identity::feature_ops` (and future identity-event
/// producers) can stamp `recorded_at` without duplicating this date math
/// or pulling in a date/time crate.
pub(crate) fn iso8601_utc_now() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock is before the Unix epoch")
        .as_secs() as i64;
    let days = secs.div_euclid(86400);
    let time_of_day = secs.rem_euclid(86400);
    let (y, m, d) = civil_from_days(days);
    let (h, mi, s) = (
        time_of_day / 3600,
        (time_of_day % 3600) / 60,
        time_of_day % 60,
    );
    format!("{y:04}-{m:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_from_days_maps_the_epoch_to_1970_01_01() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
    }

    #[test]
    fn civil_from_days_handles_a_leap_day() {
        // 2020-02-29 is 18321 days after the epoch.
        assert_eq!(civil_from_days(18321), (2020, 2, 29));
    }

    #[test]
    fn iso8601_utc_now_is_a_fixed_width_utc_timestamp() {
        let stamp = iso8601_utc_now();
        assert_eq!(stamp.len(), 20, "{stamp}");
        assert!(stamp.ends_with('Z'), "{stamp}");
        assert_eq!(&stamp[4..5], "-", "{stamp}");
        assert_eq!(&stamp[10..11], "T", "{stamp}");
    }
}
