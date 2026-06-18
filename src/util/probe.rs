//! A reusable stdio JSON-RPC probe helper for LSP message framing.
//!
//! LSP frames a message as a `Content-Length: N\r\n\r\n` header followed by `N` bytes
//! of JSON body. This module encodes and decodes that framing over both async streams
//! (in-memory duplex pipes for unit tests) and blocking readers (a child process's
//! stdio in an integration probe), so the same wire logic backs every probe.

use std::io::{self, BufRead};

use serde_json::Value;
use tokio::io::{AsyncRead, AsyncReadExt};

/// Encodes a JSON-RPC `body` as a length-framed LSP message.
///
/// # Example
///
/// ```
/// use gradle_analyzer::util::probe::encode_frame;
/// use serde_json::json;
///
/// let bytes = encode_frame(&json!({"jsonrpc": "2.0", "id": 1, "method": "shutdown"}));
/// assert!(bytes.starts_with(b"Content-Length: "));
/// ```
pub fn encode_frame(body: &Value) -> Vec<u8> {
    let payload = serde_json::to_vec(body).expect("serializable JSON value");
    let mut framed = format!("Content-Length: {}\r\n\r\n", payload.len()).into_bytes();
    framed.extend_from_slice(&payload);
    framed
}

/// Builds a JSON-RPC 2.0 request value with the given `id`, `method`, and `params`.
///
/// A `Value::Null` params is omitted entirely, since methods like `shutdown` take no
/// params and a literal `"params": null` is rejected by strict servers.
pub fn request(id: i64, method: &str, params: Value) -> Value {
    let mut obj = serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": method,
    });
    if !params.is_null() {
        obj["params"] = params;
    }
    obj
}

/// Builds a JSON-RPC 2.0 notification value (no `id`).
///
/// A `Value::Null` params is omitted, matching the convention used by [`request`].
pub fn notification(method: &str, params: Value) -> Value {
    let mut obj = serde_json::json!({
        "jsonrpc": "2.0",
        "method": method,
    });
    if !params.is_null() {
        obj["params"] = params;
    }
    obj
}

/// Reads one length-framed LSP message from an async reader.
///
/// Parses the `Content-Length` header, consumes the blank separator line, then reads
/// exactly that many body bytes and decodes them as JSON. Returns an error on EOF, a
/// missing or malformed length, or invalid JSON.
pub async fn read_frame_async<R>(reader: &mut R) -> io::Result<Value>
where
    R: AsyncRead + Unpin,
{
    let content_length = read_content_length_async(reader).await?;
    let mut body = vec![0u8; content_length];
    reader.read_exact(&mut body).await?;
    serde_json::from_slice(&body).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

/// Reads header lines from an async reader and returns the parsed content length.
async fn read_content_length_async<R>(reader: &mut R) -> io::Result<usize>
where
    R: AsyncRead + Unpin,
{
    let mut headers = String::new();
    loop {
        let line = read_line_async(reader).await?;
        if line == "\r\n" || line == "\n" || line.is_empty() {
            break;
        }
        headers.push_str(&line);
    }
    parse_content_length(&headers)
}

/// Reads a single `\n`-terminated line (byte at a time) from an async reader.
async fn read_line_async<R>(reader: &mut R) -> io::Result<String>
where
    R: AsyncRead + Unpin,
{
    let mut line = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        let n = reader.read(&mut byte).await?;
        if n == 0 {
            break;
        }
        line.push(byte[0]);
        if byte[0] == b'\n' {
            break;
        }
    }
    if line.is_empty() {
        return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "eof reading header"));
    }
    Ok(String::from_utf8_lossy(&line).into_owned())
}

/// Reads one length-framed LSP message from a blocking buffered reader (child stdio).
pub fn read_frame_blocking<R>(reader: &mut R) -> io::Result<Value>
where
    R: BufRead,
{
    let mut headers = String::new();
    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line)?;
        if n == 0 {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "eof reading header"));
        }
        if line == "\r\n" || line == "\n" {
            break;
        }
        headers.push_str(&line);
    }
    let content_length = parse_content_length(&headers)?;
    let mut body = vec![0u8; content_length];
    reader.read_exact(&mut body)?;
    serde_json::from_slice(&body).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

/// Extracts the `Content-Length` value from collected header text.
fn parse_content_length(headers: &str) -> io::Result<usize> {
    for line in headers.lines() {
        if let Some(rest) = line
            .trim()
            .strip_prefix("Content-Length:")
            .or_else(|| line.trim().strip_prefix("content-length:"))
        {
            return rest
                .trim()
                .parse::<usize>()
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e));
        }
    }
    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "missing Content-Length header",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn encode_then_blocking_decode_roundtrips() {
        let value = request(7, "initialize", serde_json::json!({"capabilities": {}}));
        let framed = encode_frame(&value);
        let mut cursor = Cursor::new(framed);
        let decoded = read_frame_blocking(&mut cursor).unwrap();
        assert_eq!(decoded["id"], 7);
        assert_eq!(decoded["method"], "initialize");
    }

    #[tokio::test]
    async fn encode_then_async_decode_roundtrips() {
        let value = notification("initialized", serde_json::json!({}));
        let framed = encode_frame(&value);
        let mut reader = Cursor::new(framed);
        let decoded = read_frame_async(&mut reader).await.unwrap();
        assert_eq!(decoded["method"], "initialized");
    }

    #[test]
    fn missing_content_length_is_error_not_panic() {
        let mut cursor = Cursor::new(b"X-Other: 1\r\n\r\n{}".to_vec());
        assert!(read_frame_blocking(&mut cursor).is_err());
    }
}
