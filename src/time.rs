use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Formats a Duration matching Go's time.Duration.String() output.
pub fn format_go_duration(d: Duration) -> String {
    let total_nanos = d.as_nanos();

    if total_nanos == 0 {
        return "0s".to_string();
    }

    let mut result = String::new();
    let mut remaining = total_nanos;

    // Hours
    let hours = remaining / 3_600_000_000_000;
    if hours > 0 {
        result.push_str(&format!("{}h", hours));
        remaining %= 3_600_000_000_000;
    }

    // Minutes
    let minutes = remaining / 60_000_000_000;
    if minutes > 0 {
        result.push_str(&format!("{}m", minutes));
        remaining %= 60_000_000_000;
    }

    // Seconds and sub-second units
    if remaining > 0 {
        if remaining >= 1_000_000_000 {
            let secs = remaining / 1_000_000_000;
            let frac = remaining % 1_000_000_000;
            if frac == 0 {
                result.push_str(&format!("{}s", secs));
            } else {
                let frac_str = format!("{:09}", frac);
                let trimmed = frac_str.trim_end_matches('0');
                result.push_str(&format!("{}.{}s", secs, trimmed));
            }
        } else if remaining >= 1_000_000 {
            let ms = remaining / 1_000_000;
            let frac = remaining % 1_000_000;
            if frac == 0 {
                result.push_str(&format!("{}ms", ms));
            } else {
                let frac_str = format!("{:06}", frac);
                let trimmed = frac_str.trim_end_matches('0');
                result.push_str(&format!("{}.{}ms", ms, trimmed));
            }
        } else if remaining >= 1_000 {
            let us = remaining / 1_000;
            let frac = remaining % 1_000;
            if frac == 0 {
                result.push_str(&format!("{}µs", us));
            } else {
                let frac_str = format!("{:03}", frac);
                let trimmed = frac_str.trim_end_matches('0');
                result.push_str(&format!("{}.{}µs", us, trimmed));
            }
        } else {
            result.push_str(&format!("{}ns", remaining));
        }
    }

    result
}

/// Parses a Go-style duration string (e.g., "1h2m3.5s", "100ms", "1.5µs").
pub fn parse_go_duration(s: &str) -> Option<Duration> {
    if s == "0s" {
        return Some(Duration::ZERO);
    }

    let mut total_nanos: u128 = 0;
    let mut chars = s.chars().peekable();
    let mut num_str = String::new();

    while chars.peek().is_some() {
        num_str.clear();

        // Parse number (possibly with decimal point)
        while let Some(&c) = chars.peek() {
            if c.is_ascii_digit() || c == '.' {
                num_str.push(c);
                chars.next();
            } else {
                break;
            }
        }

        if num_str.is_empty() {
            return None;
        }

        // Parse unit
        let mut unit = String::new();
        while let Some(&c) = chars.peek() {
            if c.is_ascii_digit() || c == '.' {
                break;
            }
            unit.push(c);
            chars.next();
        }

        let value: f64 = num_str.parse().ok()?;
        let nanos_per_unit: f64 = match unit.as_str() {
            "ns" => 1.0,
            "us" | "µs" => 1_000.0,
            "ms" => 1_000_000.0,
            "s" => 1_000_000_000.0,
            "m" => 60_000_000_000.0,
            "h" => 3_600_000_000_000.0,
            _ => return None,
        };

        total_nanos += (value * nanos_per_unit) as u128;
    }

    Some(Duration::from_nanos(total_nanos as u64))
}

/// Formats a SystemTime as RFC3339 with nanosecond precision (matching Go's time.Time JSON).
pub fn format_rfc3339_nano(t: SystemTime) -> String {
    let dur = t.duration_since(UNIX_EPOCH).unwrap_or_default();
    let secs = dur.as_secs();
    let nanos = dur.subsec_nanos();

    // Convert to calendar date/time
    let days_since_epoch = (secs / 86400) as i64;
    let time_of_day = secs % 86400;

    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    // Calculate year, month, day from days since epoch (1970-01-01)
    let (year, month, day) = days_to_date(days_since_epoch);

    if nanos == 0 {
        format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
            year, month, day, hours, minutes, seconds
        )
    } else {
        // Trim trailing zeros from nanoseconds like Go does
        let nanos_str = format!("{:09}", nanos);
        let trimmed = nanos_str.trim_end_matches('0');
        format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{}Z",
            year, month, day, hours, minutes, seconds, trimmed
        )
    }
}

fn days_to_date(days: i64) -> (i32, u32, u32) {
    // Algorithm from https://howardhinnant.github.io/date_algorithms.html
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    (year as i32, m, d)
}

/// Parses an RFC3339 timestamp string to SystemTime.
#[allow(dead_code)]
pub fn parse_rfc3339(s: &str) -> Option<SystemTime> {
    // Format: 2006-01-02T15:04:05.999999999Z or 2006-01-02T15:04:05Z
    if s.len() < 20 {
        return None;
    }

    let year: i32 = s[0..4].parse().ok()?;
    let month: u32 = s[5..7].parse().ok()?;
    let day: u32 = s[8..10].parse().ok()?;
    let hour: u64 = s[11..13].parse().ok()?;
    let minute: u64 = s[14..16].parse().ok()?;
    let second: u64 = s[17..19].parse().ok()?;

    let nanos: u32 = if s.len() > 20 && s.as_bytes()[19] == b'.' {
        let end = s.len() - 1; // strip 'Z'
        let frac = &s[20..end];
        let padded = format!("{:0<9}", frac);
        padded[..9].parse().ok()?
    } else {
        0
    };

    let days = date_to_days(year, month, day);
    let secs = days as u64 * 86400 + hour * 3600 + minute * 60 + second;

    Some(UNIX_EPOCH + Duration::new(secs, nanos))
}

#[allow(dead_code)]
fn date_to_days(year: i32, month: u32, day: u32) -> i64 {
    let y = if month <= 2 { year as i64 - 1 } else { year as i64 };
    let m = if month <= 2 { month + 9 } else { month - 3 };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u32;
    let doy = (153 * m + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe as i64 - 719468
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_go_duration() {
        assert_eq!(format_go_duration(Duration::from_secs(0)), "0s");
        assert_eq!(format_go_duration(Duration::from_secs(1)), "1s");
        assert_eq!(format_go_duration(Duration::from_millis(100)), "100ms");
        assert_eq!(format_go_duration(Duration::from_millis(1500)), "1.5s");
        assert_eq!(
            format_go_duration(Duration::from_nanos(1_234_567)),
            "1.234567ms"
        );
        assert_eq!(format_go_duration(Duration::from_secs(90)), "1m30s");
        assert_eq!(format_go_duration(Duration::from_secs(3661)), "1h1m1s");
        assert_eq!(format_go_duration(Duration::from_micros(1)), "1µs");
        assert_eq!(format_go_duration(Duration::from_nanos(1)), "1ns");
        assert_eq!(format_go_duration(Duration::from_nanos(1500)), "1.5µs");
    }

    #[test]
    fn test_parse_go_duration() {
        assert_eq!(parse_go_duration("0s"), Some(Duration::ZERO));
        assert_eq!(parse_go_duration("1s"), Some(Duration::from_secs(1)));
        assert_eq!(parse_go_duration("100ms"), Some(Duration::from_millis(100)));
        assert_eq!(parse_go_duration("1.5s"), Some(Duration::from_millis(1500)));
        assert_eq!(parse_go_duration("1m30s"), Some(Duration::from_secs(90)));
        assert_eq!(parse_go_duration("1h1m1s"), Some(Duration::from_secs(3661)));
        assert_eq!(parse_go_duration("5ms"), Some(Duration::from_millis(5)));
    }

    #[test]
    fn test_format_rfc3339_nano() {
        let t = UNIX_EPOCH + Duration::new(1_000_000_000, 123_456_789);
        let s = format_rfc3339_nano(t);
        assert_eq!(s, "2001-09-09T01:46:40.123456789Z");

        let t = UNIX_EPOCH + Duration::new(0, 0);
        let s = format_rfc3339_nano(t);
        assert_eq!(s, "1970-01-01T00:00:00Z");
    }

    #[test]
    fn test_parse_rfc3339() {
        let t = parse_rfc3339("2001-09-09T01:46:40.123456789Z").unwrap();
        let expected = UNIX_EPOCH + Duration::new(1_000_000_000, 123_456_789);
        assert_eq!(t, expected);

        let t = parse_rfc3339("1970-01-01T00:00:00Z").unwrap();
        assert_eq!(t, UNIX_EPOCH);
    }

    #[test]
    fn test_roundtrip_rfc3339() {
        let original = UNIX_EPOCH + Duration::new(1_700_000_000, 500_000_000);
        let formatted = format_rfc3339_nano(original);
        let parsed = parse_rfc3339(&formatted).unwrap();
        assert_eq!(original, parsed);
    }
}
