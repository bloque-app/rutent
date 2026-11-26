//! Just enough HTTP/1.1 for the demo's API on one thread: a request read whole off a connection, and a response
//! written back with `Connection: close`. No keep-alive, no chunked bodies, no TLS: the server listens on the loopback
//! address by default, for the person running it.

use std::collections::BTreeMap;
use std::io::{self, BufRead as _, BufReader, Read as _, Write as _};
use std::net::TcpStream;
use std::time::Duration;

/// The most a request's body may hold.
const MOST_BODY: usize = 1 << 20;
/// The most a request's line and headers may hold together.
const MOST_HEAD: usize = 64 << 10;

/// A request, read whole.
#[derive(Debug)]
pub struct Request {
    /// `GET`, `POST`, …
    pub method: String,
    /// The path, without its query, percent-decoded.
    pub path: String,
    /// The query's parameters, percent-decoded.
    pub query: BTreeMap<String, String>,
    /// The body, as sent.
    pub body: Vec<u8>,
}

impl Request {
    /// The path's segments, without empty ones: `/orders/o-1/run` is `["orders", "o-1", "run"]`.
    pub fn segments(&self) -> Vec<&str> {
        self.path.split('/').filter(|s| !s.is_empty()).collect()
    }
}

/// A response.
pub struct Response {
    /// Its status code.
    pub status: u16,
    /// Its content type.
    pub content_type: &'static str,
    /// Its body.
    pub body: Vec<u8>,
}

impl Response {
    /// A JSON response.
    pub fn json(status: u16, value: &serde_json::Value) -> Self {
        Self {
            status,
            content_type: "application/json; charset=utf-8",
            body: serde_json::to_vec_pretty(value).unwrap_or_default(),
        }
    }
}

/// Reads one request off `stream`; `None` where the client closed it before sending one.
///
/// # Errors
///
/// On a read that fails or times out, or a request that isn't HTTP/1.x or is too large.
pub fn read(stream: &TcpStream) -> io::Result<Option<Request>> {
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    if reader.read_line(&mut line)? == 0 {
        return Ok(None);
    }
    let mut parts = line.split_whitespace();
    let (Some(method), Some(target), Some(version)) = (parts.next(), parts.next(), parts.next())
    else {
        return Err(bad("a request line names a method, a target and a version"));
    };
    if !version.starts_with("HTTP/1.") {
        return Err(bad("only HTTP/1.x"));
    }
    let method = method.to_owned();
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let path = decode(path);
    let query = query
        .split('&')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let (k, v) = p.split_once('=').unwrap_or((p, ""));
            (decode(k), decode(v))
        })
        .collect();
    let mut length = 0usize;
    let mut head = line.len();
    loop {
        let mut header = String::new();
        let read = reader.read_line(&mut header)?;
        head += read;
        if read == 0 || head > MOST_HEAD {
            return Err(bad("the headers end with an empty line, within 64 KiB"));
        }
        let header = header.trim_end();
        if header.is_empty() {
            break;
        }
        if let Some((name, value)) = header.split_once(':')
            && name.trim().eq_ignore_ascii_case("content-length")
        {
            length = value
                .trim()
                .parse()
                .map_err(|_| bad("Content-Length is a number"))?;
        }
    }
    if length > MOST_BODY {
        return Err(bad("a body holds at most 1 MiB"));
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body)?;
    Ok(Some(Request {
        method,
        path,
        query,
        body,
    }))
}

/// Writes `response` to `stream` and closes the connection.
///
/// # Errors
///
/// On a write that fails.
pub fn write(mut stream: &TcpStream, response: &Response) -> io::Result<()> {
    let reason = match response.status {
        200 => "OK",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        422 => "Unprocessable Content",
        503 => "Service Unavailable",
        _ => "Internal Server Error",
    };
    let head = format!(
        "HTTP/1.1 {} {reason}\r\nContent-Type: {}\r\nContent-Length: {}\r\nCache-Control: no-store\r\n\
         X-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n",
        response.status,
        response.content_type,
        response.body.len()
    );
    stream.write_all(head.as_bytes())?;
    stream.write_all(&response.body)?;
    stream.flush()
}

fn bad(why: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, why)
}

/// Percent-decodes `text`, with `+` as a space; a malformed escape stays as it is.
fn decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < bytes.len() => {
                let escaped = std::str::from_utf8(&bytes[i + 1..i + 3])
                    .ok()
                    .and_then(|h| u8::from_str_radix(h, 16).ok());
                match escaped {
                    Some(b) => {
                        out.push(b);
                        i += 2;
                    }
                    None => out.push(b'%'),
                }
            }
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::decode;

    #[test]
    fn decodes_escapes_and_leaves_malformed_ones() {
        assert_eq!(decode("a%20b+c"), "a b c");
        assert_eq!(decode("100%"), "100%");
        assert_eq!(decode("%zz"), "%zz");
    }
}
