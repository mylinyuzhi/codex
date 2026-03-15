//! JSON parsing utilities.

use serde::de::DeserializeOwned;
use serde_json::Value;
use std::io::BufRead;
use std::io::BufReader;

/// Parse JSON from bytes.
pub fn parse_json<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, serde_json::Error> {
    serde_json::from_slice(bytes)
}

/// Parse JSON from a string.
pub fn parse_json_str<T: DeserializeOwned>(s: &str) -> Result<T, serde_json::Error> {
    serde_json::from_str(s)
}

/// Parse JSON from a reader.
pub fn parse_json_reader<R: std::io::Read, T: DeserializeOwned>(
    reader: R,
) -> Result<T, serde_json::Error> {
    serde_json::from_reader(reader)
}

/// Parse a JSON event stream (Server-Sent Events format).
///
/// Each line starting with "data: " is parsed as JSON.
pub fn parse_json_event_stream<R: std::io::Read>(
    reader: R,
) -> impl Iterator<Item = Result<Value, JsonEventStreamError>> {
    let reader = BufReader::new(reader);
    reader.lines().filter_map(move |line| match line {
        Ok(line) => {
            let line = line.trim();
            if line.is_empty() || line == "data: [DONE]" {
                return None;
            }
            if let Some(data) = line.strip_prefix("data: ") {
                match serde_json::from_str(data) {
                    Ok(value) => Some(Ok(value)),
                    Err(e) => Some(Err(JsonEventStreamError::Parse(e))),
                }
            } else {
                None
            }
        }
        Err(e) => Some(Err(JsonEventStreamError::Io(e))),
    })
}

/// Error type for JSON event stream parsing.
#[derive(Debug, thiserror::Error)]
pub enum JsonEventStreamError {
    #[error("JSON parse error: {0}")]
    Parse(#[from] serde_json::Error),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// Securely parse JSON, limiting recursion depth to prevent stack overflow.
pub fn parse_json_secure<T: DeserializeOwned>(
    bytes: &[u8],
    max_depth: usize,
) -> Result<T, SecureJsonError> {
    let value: Value = serde_json::from_slice(bytes)?;
    check_depth(&value, max_depth)?;
    serde_json::from_value(value).map_err(SecureJsonError::from)
}

fn check_depth(value: &Value, max_depth: usize) -> Result<(), SecureJsonError> {
    if max_depth == 0 {
        return Err(SecureJsonError::DepthExceeded);
    }
    match value {
        Value::Object(map) => {
            for v in map.values() {
                check_depth(v, max_depth - 1)?;
            }
        }
        Value::Array(arr) => {
            for v in arr {
                check_depth(v, max_depth - 1)?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// Error type for secure JSON parsing.
#[derive(Debug, thiserror::Error)]
pub enum SecureJsonError {
    #[error("JSON depth exceeded maximum allowed")]
    DepthExceeded,
    #[error("JSON parse error: {0}")]
    Parse(#[from] serde_json::Error),
}

#[cfg(test)]
#[path = "json.test.rs"]
mod tests;
