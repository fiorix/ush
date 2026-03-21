use std::collections::HashMap;
use std::io::{self, BufRead, Write};
use std::time::Duration;

use crate::exec::ExecResult;
use crate::time::{format_duration, parse_duration};

#[derive(Debug, Clone)]
pub struct Item {
    pub freq: f64,
    pub value: String,
    pub targets: Vec<String>,
}

fn to_fixed(num: f64, precision: u32) -> f64 {
    let factor = 10f64.powi(precision as i32);
    (num * factor + 0.5).floor() / factor
}

fn read_results(
    reader: impl io::Read,
    key_fn: impl Fn(&ExecResult) -> String,
    sorter: fn(&mut Vec<Item>),
) -> Result<Vec<Item>, Box<dyn std::error::Error>> {
    let buf = io::BufReader::new(reader);
    let mut m: HashMap<String, Vec<String>> = HashMap::new();
    let mut total = 0usize;

    for line in buf.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }

        let result: ExecResult = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(_) => continue,
        };

        total += 1;
        let key = key_fn(&result);
        m.entry(key).or_default().push(result.target);
    }

    if m.is_empty() {
        return Ok(Vec::new());
    }

    let mut items: Vec<Item> = m
        .into_iter()
        .map(|(value, targets)| Item {
            freq: to_fixed(targets.len() as f64 * 100.0 / total as f64, 2),
            value,
            targets,
        })
        .collect();

    sorter(&mut items);

    Ok(items)
}

pub fn duration(
    reader: impl io::Read,
    truncate: Duration,
) -> Result<Vec<Item>, Box<dyn std::error::Error>> {
    read_results(
        reader,
        |result| {
            let d = parse_duration(&result.duration).unwrap_or_default();
            let truncate_nanos = truncate.as_nanos();
            if truncate_nanos == 0 {
                return format_duration(d);
            }
            let d_nanos = d.as_nanos();
            let truncated = (d_nanos / truncate_nanos) * truncate_nanos + truncate_nanos;
            format_duration(Duration::from_nanos(truncated as u64))
        },
        |items| {
            items.sort_by(|a, b| {
                let da = parse_duration(&a.value).unwrap_or_default();
                let db = parse_duration(&b.value).unwrap_or_default();
                da.cmp(&db)
            });
        },
    )
}

pub fn exit_status(reader: impl io::Read) -> Result<Vec<Item>, Box<dyn std::error::Error>> {
    read_results(
        reader,
        |result| result.exit_status.to_string(),
        |items| {
            items.sort_by(|a, b| {
                let va: i32 = a.value.parse().unwrap_or(0);
                let vb: i32 = b.value.parse().unwrap_or(0);
                va.cmp(&vb)
            });
        },
    )
}

pub fn stdout(reader: impl io::Read) -> Result<Vec<Item>, Box<dyn std::error::Error>> {
    read_results(
        reader,
        |result| result.stdout.clone(),
        |items| items.sort_by(|a, b| a.targets.len().cmp(&b.targets.len())),
    )
}

pub fn stderr(reader: impl io::Read) -> Result<Vec<Item>, Box<dyn std::error::Error>> {
    read_results(
        reader,
        |result| result.stderr.clone(),
        |items| items.sort_by(|a, b| a.targets.len().cmp(&b.targets.len())),
    )
}

pub fn encode_json(w: &mut dyn Write, items: &[Item]) -> io::Result<()> {
    for item in items {
        write!(
            w,
            "{{\"freq\":{},\"value\":{},\"targets\":[",
            format_freq(item.freq),
            serde_json::to_string(&item.value).unwrap_or_default(),
        )?;
        for (i, t) in item.targets.iter().enumerate() {
            if i > 0 {
                write!(w, ",")?;
            }
            write!(w, "{}", serde_json::to_string(t).unwrap_or_default())?;
        }
        writeln!(w, "]}}")?;
    }
    Ok(())
}

/// Format frequency: integers as "100", floats as "33.33".
fn format_freq(f: f64) -> String {
    if f == f.trunc() {
        format!("{}", f as i64)
    } else {
        format!("{}", f)
    }
}

pub fn encode_wide(w: &mut dyn Write, items: &[Item]) -> io::Result<()> {
    writeln!(w, "{:<8} {:<8} {:<8} value", "count", "targets", "freq %")?;
    for (i, item) in items.iter().enumerate() {
        let mut v = item.value.clone();
        if v.len() > 50 {
            v.truncate(50);
            v.push_str("[...]");
        }
        let quoted = shell_quote(&v);
        writeln!(
            w,
            "{:<8} {:<8} {:<6.2}   {}",
            i + 1,
            item.targets.len(),
            item.freq,
            quoted
        )?;
    }
    Ok(())
}

/// Quotes and escapes a string for display, similar to shell quoting.
fn shell_quote(s: &str) -> String {
    let mut result = String::with_capacity(s.len() + 2);
    result.push('"');
    for c in s.chars() {
        match c {
            '"' => result.push_str("\\\""),
            '\\' => result.push_str("\\\\"),
            '\n' => result.push_str("\\n"),
            '\r' => result.push_str("\\r"),
            '\t' => result.push_str("\\t"),
            '\x07' => result.push_str("\\a"),
            '\x08' => result.push_str("\\b"),
            '\x0C' => result.push_str("\\f"),
            '\x0B' => result.push_str("\\v"),
            c if c.is_ascii_graphic() || c == ' ' => result.push(c),
            c if (c as u32) < 0x100 => {
                result.push_str(&format!("\\x{:02x}", c as u32));
            }
            c => {
                result.push_str(&format!("\\u{:04x}", c as u32));
            }
        }
    }
    result.push('"');
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_result_json(
        target: &str,
        duration: &str,
        exit_status: i32,
        stdout: &str,
        stderr: &str,
    ) -> String {
        serde_json::to_string(&ExecResult {
            target: target.to_string(),
            duration: duration.to_string(),
            start_time: "2024-01-01T00:00:00Z".to_string(),
            end_time: "2024-01-01T00:00:00Z".to_string(),
            exit_status,
            stdout: stdout.to_string(),
            stderr: stderr.to_string(),
            error: String::new(),
        })
        .unwrap()
    }

    #[test]
    fn test_duration() {
        let input = format!(
            "{}\n{}\n{}\n",
            make_result_json("a", "8ms", 0, "", ""),
            make_result_json("b", "6ms", 0, "", ""),
            make_result_json("c", "2ms", 0, "", ""),
        );

        let items = duration(input.as_bytes(), Duration::from_millis(5)).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].targets, vec!["c"]);
        let mut t1 = items[1].targets.clone();
        t1.sort();
        assert_eq!(t1, vec!["a", "b"]);
    }

    #[test]
    fn test_exit_status() {
        let input = format!(
            "{}\n{}\n{}\n",
            make_result_json("a", "0s", 255, "", ""),
            make_result_json("b", "0s", 255, "", ""),
            make_result_json("c", "0s", 1, "", ""),
        );

        let items = exit_status(input.as_bytes()).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].targets, vec!["c"]);
        let mut t1 = items[1].targets.clone();
        t1.sort();
        assert_eq!(t1, vec!["a", "b"]);
    }

    #[test]
    fn test_stdout_stderr() {
        let input = format!(
            "{}\n{}\n{}\n",
            make_result_json("a", "0s", 0, "hello", "foobar"),
            make_result_json("b", "0s", 0, "hello", "foobar"),
            make_result_json("c", "0s", 0, "world", ""),
        );

        let items = stdout(input.as_bytes()).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].targets, vec!["c"]);
        let mut t1 = items[1].targets.clone();
        t1.sort();
        assert_eq!(t1, vec!["a", "b"]);

        let items = stderr(input.as_bytes()).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].targets, vec!["c"]);
        let mut t1 = items[1].targets.clone();
        t1.sort();
        assert_eq!(t1, vec!["a", "b"]);
    }

    #[test]
    fn test_encode_json() {
        let items = vec![Item {
            freq: 100.0,
            value: "hello".to_string(),
            targets: vec!["a".to_string(), "b".to_string()],
        }];

        let mut buf = Vec::new();
        encode_json(&mut buf, &items).unwrap();

        let have = String::from_utf8(buf).unwrap();
        let want = "{\"freq\":100,\"value\":\"hello\",\"targets\":[\"a\",\"b\"]}\n";
        assert_eq!(have, want);
    }

    #[test]
    fn test_encode_wide() {
        let items = vec![Item {
            freq: 100.0,
            value: "hello".to_string(),
            targets: vec!["a".to_string(), "b".to_string()],
        }];

        let mut buf = Vec::new();
        encode_wide(&mut buf, &items).unwrap();

        let have = String::from_utf8(buf).unwrap();
        let want = "count    targets  freq %   value\n1        2        100.00   \"hello\"\n";
        assert_eq!(have, want);
    }
}
