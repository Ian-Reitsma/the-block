//! Minimal in-house URI and query-string helpers used across the HTTP stack.
//!
//! The implementations intentionally cover only the behaviour exercised by the
//! workspace today. They prefer explicit error surfaces and avoid pulling in
//! third-party parsers so that the networking crates can compile while the full
//! router lands. As functionality grows we can flesh the routines out and add
//! conformance tests alongside the production implementations.

use std::error::Error;
use std::fmt;

/// Percent-encode a single byte according to the unreserved character set.
fn percent_encode_byte(byte: u8, out: &mut String) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    out.push('%');
    out.push(HEX[(byte >> 4) as usize] as char);
    out.push(HEX[(byte & 0x0F) as usize] as char);
}

fn percent_encode_component(component: &str) -> String {
    let mut out = String::with_capacity(component.len());
    for &byte in component.as_bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(byte as char);
        } else {
            percent_encode_byte(byte, &mut out);
        }
    }
    out
}

fn percent_decode_component(component: &str) -> String {
    let bytes = component.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                let hi = bytes[i + 1];
                let lo = bytes[i + 2];
                let value = (decode_hex(hi), decode_hex(lo));
                if let (Some(hi), Some(lo)) = value {
                    out.push((hi << 4) | lo);
                    i += 3;
                } else {
                    // Invalid escape sequence; preserve the literal bytes.
                    out.push(bytes[i]);
                    i += 1;
                }
            }
            byte => {
                out.push(byte);
                i += 1;
            }
        }
    }
    String::from_utf8(out).unwrap_or_else(|_| component.to_string())
}

fn decode_hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// Helpers for form-style URL encoding/decoding.
pub mod form_urlencoded {
    use super::{percent_decode_component, percent_encode_component};

    /// Builder used to encode query-string pairs.
    pub struct Serializer {
        output: String,
        needs_separator: bool,
    }

    impl Serializer {
        /// Create a serializer seeded with an optional prefix.
        pub fn new(prefix: String) -> Self {
            let needs_separator = !prefix.is_empty();
            Self {
                output: prefix,
                needs_separator,
            }
        }

        /// Append a key/value pair using percent-encoding.
        pub fn append_pair(&mut self, key: &str, value: &str) {
            if self.needs_separator {
                self.output.push('&');
            }
            self.output.push_str(&percent_encode_component(key));
            self.output.push('=');
            self.output.push_str(&percent_encode_component(value));
            self.needs_separator = true;
        }

        /// Finalize the encoded string.
        pub fn finish(self) -> String {
            self.output
        }
    }

    /// Parse a URL-encoded query string into owned key/value pairs.
    pub fn parse(input: &[u8]) -> Vec<(String, String)> {
        let text = String::from_utf8_lossy(input);
        let mut pairs = Vec::new();
        for segment in text.split('&') {
            if segment.is_empty() {
                continue;
            }
            let (key, value) = match segment.split_once('=') {
                Some((key, value)) => (key, value),
                None => (segment, ""),
            };
            pairs.push((
                percent_decode_component(key),
                percent_decode_component(value),
            ));
        }
        pairs
    }
}

/// Simplified URI representation that captures the components required by the
/// current networking stack.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Uri {
    scheme: String,
    host: Option<String>,
    host_is_ipv6: bool,
    port: Option<u16>,
    path: String,
    query: Option<String>,
}

impl Uri {
    /// Parse a URI string into its components.
    pub fn parse(input: &str) -> Result<Self, UriError> {
        if input.is_empty() {
            return Err(UriError::Empty);
        }
        let (scheme, rest) = input.split_once("://").ok_or(UriError::MissingScheme)?;
        if scheme.is_empty() || !scheme.chars().all(is_scheme_char) {
            return Err(UriError::InvalidScheme);
        }
        let remainder = rest.split('#').next().unwrap_or("");
        let (authority, path_part) = match remainder.find('/') {
            Some(idx) => (&remainder[..idx], &remainder[idx..]),
            None => (remainder, ""),
        };
        let authority = authority.trim();
        if authority.is_empty() {
            return Err(UriError::MissingHost);
        }
        let (host, host_is_ipv6, port) = if let Some(rest) = authority.strip_prefix('[') {
            let end = rest.find(']').ok_or(UriError::InvalidAuthority)?;
            let host = &rest[..end];
            let trailing = &rest[end + 1..];
            let port = if let Some(stripped) = trailing.strip_prefix(':') {
                let (value, extra) = split_port(stripped)?;
                if !extra.is_empty() {
                    return Err(UriError::InvalidAuthority);
                }
                Some(value)
            } else {
                if !trailing.is_empty() {
                    return Err(UriError::InvalidAuthority);
                }
                None
            };
            (Some(host.to_string()), true, port)
        } else {
            let (host_part, port_part) = match authority.rsplit_once(':') {
                Some((host, _port)) if host.contains(':') => (authority, None),
                Some((host, port)) => {
                    let (value, extra) = split_port(port)?;
                    if !extra.is_empty() {
                        return Err(UriError::InvalidAuthority);
                    }
                    (host, Some(value))
                }
                None => (authority, None),
            };
            let host_part = host_part.trim();
            if host_part.is_empty() {
                return Err(UriError::MissingHost);
            }
            (Some(host_part.to_string()), false, port_part)
        };

        let (raw_path, raw_query) = match path_part.split_once('?') {
            Some((path, query)) => (path, Some(query)),
            None => (path_part, None),
        };
        let path = if raw_path.is_empty() {
            "/".to_string()
        } else if raw_path.starts_with('/') {
            raw_path.to_string()
        } else {
            format!("/{raw_path}")
        };
        let query = raw_query
            .map(|q| q.trim())
            .filter(|q| !q.is_empty())
            .map(|q| q.to_string());

        Ok(Self {
            scheme: scheme.to_ascii_lowercase(),
            host,
            host_is_ipv6,
            port,
            path,
            query,
        })
    }

    /// Return the URI scheme in lowercase form.
    pub fn scheme(&self) -> &str {
        &self.scheme
    }

    /// Host name or address without brackets.
    pub fn host_str(&self) -> Option<&str> {
        self.host.as_deref()
    }

    /// Returns true when the host represents an IPv6 literal.
    pub fn host_is_ipv6(&self) -> bool {
        self.host_is_ipv6
    }

    /// Explicit port component, if present.
    pub fn port(&self) -> Option<u16> {
        self.port
    }

    /// Port derived from the explicit value or the default for the scheme.
    pub fn port_or_known_default(&self) -> Option<u16> {
        self.port.or_else(|| match self.scheme.as_str() {
            "http" | "ws" => Some(80),
            "https" | "wss" => Some(443),
            _ => None,
        })
    }

    /// Normalised path component including a leading `/`.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Query string without the leading `?`.
    pub fn query(&self) -> Option<&str> {
        self.query.as_deref()
    }

    /// Render the path plus optional query suitable for HTTP requests.
    pub fn path_and_query(&self) -> String {
        let mut target = self.path.clone();
        if let Some(query) = &self.query {
            target.push('?');
            target.push_str(query);
        }
        target
    }

    /// Host portion formatted for Host headers (adds brackets for IPv6).
    pub fn host_header(&self) -> Option<String> {
        let host = self.host.as_ref()?;
        let formatted_host = if self.host_is_ipv6 {
            format!("[{host}]")
        } else {
            host.clone()
        };
        if let Some(port) = self.port {
            Some(format!("{formatted_host}:{port}"))
        } else {
            Some(formatted_host)
        }
    }

    /// Render `host:port` suitable for socket connections.
    pub fn socket_addr(&self) -> Option<String> {
        let host = self.host.as_ref()?;
        let port = self.port_or_known_default()?;
        if self.host_is_ipv6 {
            Some(format!("[{host}]:{port}"))
        } else {
            Some(format!("{host}:{port}"))
        }
    }

    /// Authority (host plus optional port) formatted for URI strings.
    pub fn authority(&self) -> Option<String> {
        let host = self.host.as_ref()?;
        let display_host = if self.host_is_ipv6 {
            format!("[{host}]")
        } else {
            host.clone()
        };
        if let Some(port) = self.port {
            Some(format!("{display_host}:{port}"))
        } else {
            Some(display_host)
        }
    }

    /// Build a new absolute URI string using the provided scheme and path.
    pub fn rebuild_with_path(&self, scheme: &str, path: &str) -> Result<String, UriError> {
        if self.host.is_none() {
            return Err(UriError::MissingHost);
        }
        if !scheme.chars().all(is_scheme_char) {
            return Err(UriError::InvalidScheme);
        }
        let mut out = format!("{}://", scheme.to_ascii_lowercase());
        if let Some(authority) = self.authority() {
            out.push_str(&authority);
        }
        if path.starts_with('/') {
            out.push_str(path);
        } else {
            out.push('/');
            out.push_str(path);
        }
        Ok(out)
    }
}

fn split_port(input: &str) -> Result<(u16, &str), UriError> {
    let mut digits = String::new();
    for (idx, ch) in input.char_indices() {
        if ch.is_ascii_digit() {
            digits.push(ch);
        } else {
            let value = digits.parse::<u16>().map_err(|_| UriError::InvalidPort)?;
            return Ok((value, &input[idx..]));
        }
    }
    if digits.is_empty() {
        Err(UriError::InvalidPort)
    } else {
        digits
            .parse::<u16>()
            .map(|value| (value, ""))
            .map_err(|_| UriError::InvalidPort)
    }
}

fn is_scheme_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '+' | '-' | '.')
}

/// Errors that can surface when parsing URIs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum UriError {
    Empty,
    MissingScheme,
    InvalidScheme,
    MissingHost,
    InvalidPort,
    InvalidAuthority,
}

impl fmt::Display for UriError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            UriError::Empty => write!(f, "uri is empty"),
            UriError::MissingScheme => write!(f, "uri missing scheme"),
            UriError::InvalidScheme => write!(f, "uri has invalid scheme"),
            UriError::MissingHost => write!(f, "uri missing host"),
            UriError::InvalidPort => write!(f, "uri has invalid port"),
            UriError::InvalidAuthority => write!(f, "uri has invalid authority"),
        }
    }
}

impl Error for UriError {}

/// Join a path segment onto an existing absolute path using the same semantics
/// as `url::Url::join` for simple relative additions.
pub fn join_path(base: &str, segment: &str) -> String {
    let mut path = if base.is_empty() {
        "/".to_string()
    } else {
        base.to_string()
    };
    if !path.ends_with('/') {
        if let Some(idx) = path.rfind('/') {
            path.truncate(idx + 1);
        } else {
            path.clear();
            path.push('/');
        }
    }
    path.push_str(segment);
    path
}
