use std::fmt;

/// Stable exit codes. Agents branch on these, so they are part of the contract
/// and must not be renumbered.
pub mod exit {
    pub const OK: u8 = 0;
    pub const OTHER: u8 = 1;
    pub const USAGE: u8 = 2;
    pub const AUTH: u8 = 3;
    pub const NOT_FOUND: u8 = 4;
    pub const CONFLICT: u8 = 5;
    pub const RATE_LIMITED: u8 = 6;
    pub const NETWORK: u8 = 7;
}

#[derive(Debug)]
pub struct Error {
    pub code: &'static str,
    pub status: Option<u16>,
    pub message: String,
    pub hint: Option<String>,
    pub exit: u8,
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    pub fn usage(message: impl Into<String>) -> Self {
        Self {
            code: "usage",
            status: None,
            message: message.into(),
            hint: None,
            exit: exit::USAGE,
        }
    }

    pub fn other(message: impl Into<String>) -> Self {
        Self {
            code: "error",
            status: None,
            message: message.into(),
            hint: None,
            exit: exit::OTHER,
        }
    }

    pub fn network(message: impl Into<String>) -> Self {
        Self {
            code: "network",
            status: None,
            message: message.into(),
            hint: None,
            exit: exit::NETWORK,
        }
    }

    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    /// Build an error from an HTTP status and whatever the API put in the body.
    ///
    /// The hints exist because a bare status is genuinely ambiguous on several
    /// of these paths -- a 409 on `proposal accept` means one of three quite
    /// different things, and the caller cannot tell which from the code alone.
    pub fn from_status(status: u16, body: &str, path: &str) -> Self {
        let api_message = extract_message(body);
        let (code, exit) = match status {
            401 => ("unauthorized", exit::AUTH),
            403 => ("forbidden", exit::AUTH),
            404 => ("not_found", exit::NOT_FOUND),
            409 => ("conflict", exit::CONFLICT),
            422 => ("unprocessable", exit::CONFLICT),
            429 => ("rate_limited", exit::RATE_LIMITED),
            400 => ("bad_request", exit::OTHER),
            s if s >= 500 => ("server_error", exit::OTHER),
            _ => ("http_error", exit::OTHER),
        };

        let message = api_message.unwrap_or_else(|| match status {
            401 => "Missing or invalid API key.".into(),
            403 => "Not authorized for this resource.".into(),
            404 => "Not found, or not accessible by your organization.".into(),
            429 => "Rate limit exceeded.".into(),
            _ => format!("HTTP {status} from {path}"),
        });

        let hint = hint_for(status, path);
        Self {
            code,
            status: Some(status),
            message,
            hint,
            exit,
        }
    }
}

fn hint_for(status: u16, path: &str) -> Option<String> {
    let hint = match status {
        401 | 403 => {
            "Set PASSIONFROOT_API_TOKEN to a key minted at Settings > API Keys, then run `pf auth status`."
        }
        404 => {
            "The public API only exposes objects owned by your organization; a valid ID from another workspace reads as not found."
        }
        409 if path.contains("/proposals/") => {
            "A proposal can only be accepted or rejected once, and only creator-authored proposals can be acted on at all. Re-read the timeline with `pf conv messages` and check `proposal.status` and `proposal.createdBy`."
        }
        409 => {
            "A request with this Idempotency-Key is still in flight, or stayed locked after an indeterminate failure (up to 24h). Re-read the resource to check whether the write landed, then retry with a fresh key only if it did not."
        }
        422 => {
            "This Idempotency-Key was already used with a different body. A key identifies one specific request -- use a fresh key for a different one."
        }
        429 => {
            "The API allows 2 requests/second per key and 20/second per source IP. pf throttles itself to stay under both; seeing this means retries were exhausted."
        }
        400 if path.contains("cursor=") => {
            "Cursors are opaque and single-origin. A cursor the API did not issue -- or one held from before the last format change -- is rejected. Start the walk again without one."
        }
        _ => return None,
    };
    Some(hint.to_string())
}

/// The API's error shape is not documented, so probe the usual suspects rather
/// than modelling it.
fn extract_message(body: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    for path in [&["error", "message"][..], &["error"][..], &["message"][..]] {
        let mut cur = &v;
        for key in path {
            cur = cur.get(key)?;
        }
        if let Some(s) = cur.as_str() {
            return Some(s.to_string());
        }
    }
    None
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for Error {}

impl From<reqwest::Error> for Error {
    fn from(e: reqwest::Error) -> Self {
        if e.is_timeout() {
            Error::network(format!("Request timed out: {e}"))
                .with_hint("Raise --timeout, or retry. Writes carry an Idempotency-Key, so a retry cannot double-send.")
        } else {
            Error::network(e.to_string())
        }
    }
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Error::other(format!("Could not parse the API response as JSON: {e}"))
    }
}
