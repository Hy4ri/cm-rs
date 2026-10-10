//! Lenient JSON for `ui_*.json` files.
//!
//! Kunos and modders ship JSON with raw newlines inside strings, trailing
//! commas, comments and BOMs. This repairs those before handing off to
//! serde_json.

use anyhow::{Context, Result};
use serde_json::Value;

pub fn sanitize(input: &str) -> String {
    let src: Vec<char> = input.trim_start_matches('\u{feff}').chars().collect();
    let mut out = String::with_capacity(src.len());
    let mut i = 0;
    let mut in_string = false;
    while i < src.len() {
        let c = src[i];
        if in_string {
            match c {
                '\\' if i + 1 < src.len() => {
                    out.push(c);
                    out.push(src[i + 1]);
                    i += 2;
                    continue;
                }
                '"' => in_string = false,
                '\n' => {
                    out.push_str("\\n");
                    i += 1;
                    continue;
                }
                '\r' => {
                    i += 1;
                    continue;
                }
                '\t' => {
                    out.push_str("\\t");
                    i += 1;
                    continue;
                }
                c if (c as u32) < 0x20 => {
                    i += 1;
                    continue;
                }
                _ => {}
            }
            out.push(c);
            i += 1;
            continue;
        }

        match c {
            '"' => {
                in_string = true;
                out.push(c);
            }
            '/' if src.get(i + 1) == Some(&'/') => {
                while i < src.len() && src[i] != '\n' {
                    i += 1;
                }
                continue;
            }
            '/' if src.get(i + 1) == Some(&'*') => {
                i += 2;
                while i + 1 < src.len() && !(src[i] == '*' && src[i + 1] == '/') {
                    i += 1;
                }
                i += 2;
                continue;
            }
            ',' => {
                // Drop the comma if the next significant char closes a container.
                let mut j = i + 1;
                while j < src.len() && src[j].is_whitespace() {
                    j += 1;
                }
                if !matches!(src.get(j), Some('}') | Some(']')) {
                    out.push(c);
                }
            }
            _ => out.push(c),
        }
        i += 1;
    }
    out
}

pub fn parse(input: &str) -> Result<Value> {
    serde_json::from_str(&sanitize(input)).context("parsing lenient JSON")
}

pub fn parse_file(path: &std::path::Path) -> Result<Value> {
    let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    parse(&String::from_utf8_lossy(&bytes)).with_context(|| format!("in {}", path.display()))
}

/// Reads a field as display text whether it is a string or a number.
pub fn text(v: &Value, key: &str) -> Option<String> {
    match v.get(key)? {
        Value::String(s) => Some(s.trim().to_owned()).filter(|s| !s.is_empty()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repairs_kunos_json() {
        let v = parse("{\n\"description\": \"line one<br/>\n\tline two\",\n\"tags\": [\"a\", \"b\",],\n}").unwrap();
        assert_eq!(v["tags"].as_array().unwrap().len(), 2);
        assert!(v["description"].as_str().unwrap().contains("line two"));
    }
}
