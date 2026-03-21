//! Minimal JSON serialization/deserialization for the specific types used in ush.

use std::collections::HashMap;

/// Escapes a string for JSON output (matching Go's encoding/json).
pub fn escape_json_string(s: &str) -> String {
    let mut result = String::with_capacity(s.len() + 2);
    result.push('"');
    for c in s.chars() {
        match c {
            '"' => result.push_str("\\\""),
            '\\' => result.push_str("\\\\"),
            '\n' => result.push_str("\\n"),
            '\r' => result.push_str("\\r"),
            '\t' => result.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                result.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => result.push(c),
        }
    }
    result.push('"');
    result
}

/// Parses a JSON string value, handling escape sequences.
fn parse_json_string(s: &str) -> Option<(String, &str)> {
    let s = s.trim_start();
    if !s.starts_with('"') {
        return None;
    }
    let s = &s[1..];
    let mut result = String::new();
    let mut chars = s.chars();
    let mut byte_count = 0;
    loop {
        match chars.next() {
            None => return None,
            Some('"') => {
                byte_count += 1;
                return Some((result, &s[byte_count..]));
            }
            Some('\\') => {
                byte_count += 1;
                match chars.next() {
                    Some('"') => {
                        result.push('"');
                        byte_count += 1;
                    }
                    Some('\\') => {
                        result.push('\\');
                        byte_count += 1;
                    }
                    Some('/') => {
                        result.push('/');
                        byte_count += 1;
                    }
                    Some('n') => {
                        result.push('\n');
                        byte_count += 1;
                    }
                    Some('r') => {
                        result.push('\r');
                        byte_count += 1;
                    }
                    Some('t') => {
                        result.push('\t');
                        byte_count += 1;
                    }
                    Some('b') => {
                        result.push('\u{0008}');
                        byte_count += 1;
                    }
                    Some('f') => {
                        result.push('\u{000C}');
                        byte_count += 1;
                    }
                    Some('u') => {
                        byte_count += 1;
                        let mut hex = String::new();
                        for _ in 0..4 {
                            match chars.next() {
                                Some(c) => {
                                    hex.push(c);
                                    byte_count += c.len_utf8();
                                }
                                None => return None,
                            }
                        }
                        if let Ok(cp) = u32::from_str_radix(&hex, 16) {
                            // Handle UTF-16 surrogate pairs
                            if (0xD800..=0xDBFF).contains(&cp) {
                                // High surrogate — peek for \uDCxx low surrogate.
                                // Clone the iterator to avoid consuming chars on mismatch.
                                let mut peek = chars.clone();
                                let is_surrogate_pair =
                                    peek.next() == Some('\\') && peek.next() == Some('u');
                                if is_surrogate_pair {
                                    let mut peek_bytes: usize = 2; // for \ and u
                                    let mut hex2 = String::new();
                                    let mut valid = true;
                                    for _ in 0..4 {
                                        match peek.next() {
                                            Some(c) => {
                                                hex2.push(c);
                                                peek_bytes += c.len_utf8();
                                            }
                                            None => {
                                                valid = false;
                                                break;
                                            }
                                        }
                                    }
                                    if valid {
                                        if let Ok(low) = u32::from_str_radix(&hex2, 16) {
                                            if (0xDC00..=0xDFFF).contains(&low) {
                                                let combined = 0x10000
                                                    + ((cp - 0xD800) << 10)
                                                    + (low - 0xDC00);
                                                if let Some(c) = char::from_u32(combined) {
                                                    result.push(c);
                                                    byte_count += peek_bytes;
                                                    chars = peek;
                                                }
                                            }
                                        }
                                    }
                                }
                                // If low surrogate doesn't follow, silently drop the lone surrogate
                            } else if let Some(c) = char::from_u32(cp) {
                                result.push(c);
                            }
                        }
                    }
                    _ => return None,
                }
            }
            Some(c) => {
                result.push(c);
                byte_count += c.len_utf8();
            }
        }
    }
}

/// A simple JSON value type for parsing exec results.
#[derive(Debug, Clone)]
pub enum JsonValue {
    String(String),
    Number(f64),
    #[allow(dead_code)]
    Array(Vec<JsonValue>),
    Object(Vec<(String, JsonValue)>),
    Null,
}

impl JsonValue {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            JsonValue::String(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_i32(&self) -> Option<i32> {
        match self {
            JsonValue::Number(n) => Some(*n as i32),
            _ => None,
        }
    }

    #[allow(dead_code)]
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            JsonValue::Number(n) => Some(*n),
            _ => None,
        }
    }
}

/// Parse a JSON value from a string, returning the value and remaining string.
pub fn parse_json_value(s: &str) -> Option<(JsonValue, &str)> {
    let s = s.trim_start();
    if s.is_empty() {
        return None;
    }

    match s.as_bytes()[0] {
        b'"' => {
            let (v, rest) = parse_json_string(s)?;
            Some((JsonValue::String(v), rest))
        }
        b'{' => parse_json_object(s),
        b'[' => parse_json_array(s),
        b'n' => {
            if let Some(rest) = s.strip_prefix("null") {
                Some((JsonValue::Null, rest))
            } else {
                None
            }
        }
        _ => {
            // number
            let mut end = 0;
            for c in s.chars() {
                if c == ',' || c == '}' || c == ']' || c.is_whitespace() {
                    break;
                }
                end += c.len_utf8();
            }
            let num: f64 = s[..end].parse().ok()?;
            Some((JsonValue::Number(num), &s[end..]))
        }
    }
}

fn parse_json_object(s: &str) -> Option<(JsonValue, &str)> {
    let s = s.trim_start();
    if !s.starts_with('{') {
        return None;
    }
    let mut s = &s[1..];
    let mut fields = Vec::new();

    loop {
        s = s.trim_start();
        if let Some(rest) = s.strip_prefix('}') {
            return Some((JsonValue::Object(fields), rest));
        }
        if !fields.is_empty() {
            if let Some(rest) = s.strip_prefix(',') {
                s = rest.trim_start();
            } else {
                return None;
            }
        }
        let (key, rest) = parse_json_string(s)?;
        s = rest.trim_start();
        if !s.starts_with(':') {
            return None;
        }
        s = &s[1..];
        let (value, rest) = parse_json_value(s)?;
        s = rest;
        fields.push((key, value));
    }
}

fn parse_json_array(s: &str) -> Option<(JsonValue, &str)> {
    let s = s.trim_start();
    if !s.starts_with('[') {
        return None;
    }
    let mut s = &s[1..];
    let mut items = Vec::new();

    loop {
        s = s.trim_start();
        if let Some(rest) = s.strip_prefix(']') {
            return Some((JsonValue::Array(items), rest));
        }
        if !items.is_empty() {
            if let Some(rest) = s.strip_prefix(',') {
                s = rest;
            } else {
                return None;
            }
        }
        let (value, rest) = parse_json_value(s)?;
        s = rest;
        items.push(value);
    }
}

/// Parse exec result fields from a JSON object.
pub fn parse_exec_result(obj: &JsonValue) -> Option<HashMap<String, JsonValue>> {
    match obj {
        JsonValue::Object(fields) => {
            let mut map = HashMap::new();
            for (k, v) in fields {
                map.insert(k.clone(), v.clone());
            }
            Some(map)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_escape_json_string() {
        assert_eq!(escape_json_string("hello"), "\"hello\"");
        assert_eq!(escape_json_string("he\"llo"), "\"he\\\"llo\"");
        assert_eq!(escape_json_string("he\nllo"), "\"he\\nllo\"");
    }

    #[test]
    fn test_parse_surrogate_pairs() {
        // U+1F600 (grinning face) encoded as surrogate pair: \uD83D\uDE00
        let (v, _) = parse_json_value("\"\\uD83D\\uDE00\"").unwrap();
        assert_eq!(v.as_str().unwrap(), "\u{1F600}");

        // U+1F4A9 (pile of poo) encoded as surrogate pair: \uD83D\uDCA9
        let (v, _) = parse_json_value("\"\\uD83D\\uDCA9\"").unwrap();
        assert_eq!(v.as_str().unwrap(), "\u{1F4A9}");

        // Regular BMP character should still work
        let (v, _) = parse_json_value("\"\\u0041\"").unwrap();
        assert_eq!(v.as_str().unwrap(), "A");
    }

    #[test]
    fn test_parse_json_value() {
        let (v, _) = parse_json_value("\"hello\"").unwrap();
        assert_eq!(v.as_str().unwrap(), "hello");

        let (v, _) = parse_json_value("42").unwrap();
        assert_eq!(v.as_i32().unwrap(), 42);

        let (v, _) = parse_json_value("{\"a\":1,\"b\":\"c\"}").unwrap();
        if let JsonValue::Object(fields) = v {
            assert_eq!(fields.len(), 2);
        } else {
            panic!("expected object");
        }
    }
}
