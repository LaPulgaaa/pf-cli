pub mod error;
pub mod paginate;
pub mod ratelimit;

use std::sync::Mutex;
use std::time::Duration;

use reqwest::header::{HeaderName, HeaderValue};
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

pub use error::{Error, Result, exit};
use ratelimit::RateLimiter;

pub const DEFAULT_BASE_URL: &str = "https://workspace.passionfroot.me/api/v1";
pub const TOKEN_ENVS: [&str; 2] = ["PASSIONFROOT_API_TOKEN", "PF_API_TOKEN"];

/// One outbound API call, before it has been turned into a URL.
#[derive(Debug, Clone)]
pub struct Request {
    pub method: Method,
    pub path: String,
    pub query: Vec<(String, String)>,
    pub body: Option<Value>,
    pub headers: Vec<(String, String)>,
    /// Set on writes. Also reused verbatim across this client's own retries,
    /// which is the whole reason a retry cannot double-send.
    pub idempotency_key: Option<String>,
}

impl Request {
    pub fn get(path: impl Into<String>) -> Self {
        Self::new(Method::GET, path)
    }

    pub fn post(path: impl Into<String>) -> Self {
        Self::new(Method::POST, path)
    }

    pub fn new(method: Method, path: impl Into<String>) -> Self {
        Self {
            method,
            path: path.into(),
            query: Vec::new(),
            body: None,
            headers: Vec::new(),
            idempotency_key: None,
        }
    }

    pub fn query(mut self, key: &str, value: impl Into<String>) -> Self {
        self.query.push((key.to_string(), value.into()));
        self
    }

    pub fn query_opt(self, key: &str, value: Option<impl Into<String>>) -> Self {
        match value {
            Some(v) => self.query(key, v),
            None => self,
        }
    }

    pub fn body(mut self, body: Value) -> Self {
        self.body = Some(body);
        self
    }

    /// Attach an idempotency key, generating one when the caller has none.
    ///
    /// An auto-generated key only protects this process's own retries -- a
    /// second `pf` invocation gets a fresh key and sends again. Cross-invocation
    /// deduplication needs an explicit `--idempotency-key`, and the help text
    /// for every write command says so.
    pub fn idempotent(mut self, key: Option<String>) -> Self {
        self.idempotency_key =
            Some(key.unwrap_or_else(|| uuid::Uuid::new_v4().hyphenated().to_string()));
        self
    }
}

pub struct Client {
    http: reqwest::Client,
    base_url: String,
    token: String,
    token_source: String,
    limiter: RateLimiter,
    max_retries: u32,
    dry_run: bool,
    verbose: bool,
    dry_run_log: Mutex<Vec<Value>>,
}

impl Client {
    /// Credentials are resolved before the client exists, so the precedence
    /// between flag, environment and config file lives in one place rather
    /// than being rediscovered here.
    pub fn new(
        credentials: crate::config::Credentials,
        timeout: u64,
        max_retries: u32,
        dry_run: bool,
        verbose: bool,
    ) -> Result<Self> {
        let crate::config::Credentials {
            token,
            source: token_source,
            base_url,
        } = credentials;
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(timeout))
            .user_agent(concat!("pf-cli/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| Error::other(format!("Could not build the HTTP client: {e}")))?;

        let base_url = base_url.trim_end_matches('/').to_string();
        Ok(Self {
            http,
            base_url: base_url.clone(),
            token: token.clone(),
            token_source,
            // The budget is per key and per host, so the throttle is scoped the
            // same way.
            limiter: RateLimiter::new(&format!("{base_url}|{token}")),
            max_retries,
            dry_run,
            verbose,
            dry_run_log: Mutex::new(Vec::new()),
        })
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn token_source(&self) -> &str {
        &self.token_source
    }

    pub fn masked_token(&self) -> String {
        mask(&self.token)
    }

    pub fn is_dry_run(&self) -> bool {
        self.dry_run
    }

    /// The requests `--dry-run` withheld, in the order they would have been sent.
    pub fn take_dry_run_log(&self) -> Vec<Value> {
        std::mem::take(&mut self.dry_run_log.lock().unwrap())
    }

    pub fn record_dry_run(&self, entry: Value) {
        self.dry_run_log.lock().unwrap().push(entry);
    }

    fn url(&self, path: &str) -> String {
        if path.starts_with("http://") || path.starts_with("https://") {
            return path.to_string();
        }
        format!("{}/{}", self.base_url, path.trim_start_matches('/'))
    }

    /// Send one request, retrying only what is safe to retry.
    ///
    /// 4xx responses are never retried: they are the caller's problem, and
    /// re-sending a rejected write would just burn the rate-limit budget.
    pub async fn send(&self, req: Request) -> Result<Value> {
        if self.dry_run {
            self.record_dry_run(self.describe(&req, None));
            return Ok(json!({ "data": Value::Null }));
        }

        let mut attempt = 0u32;
        loop {
            self.limiter.acquire().await;

            let started = std::time::Instant::now();
            let built = self.build(&req)?;
            let result = self.http.execute(built).await;

            let response = match result {
                Ok(r) => r,
                Err(e) => {
                    // A dropped connection may or may not have applied a write.
                    // Retrying is only safe because writes carry a stable key.
                    if attempt < self.max_retries && (e.is_timeout() || e.is_connect()) {
                        attempt += 1;
                        self.backoff(attempt).await;
                        continue;
                    }
                    return Err(e.into());
                }
            };

            let status = response.status();
            let headers = response.headers().clone();
            self.limiter.observe(&headers);

            if self.verbose {
                let ms = started.elapsed().as_millis();
                eprintln!(
                    "{} {} -> {} ({ms}ms){}",
                    req.method,
                    self.url(&req.path),
                    status.as_u16(),
                    headers
                        .get("ratelimit")
                        .and_then(|v| v.to_str().ok())
                        .map(|v| format!("  RateLimit: {v}"))
                        .unwrap_or_default()
                );
                if headers.contains_key("idempotency-replayed") {
                    eprintln!("  (replayed from a stored idempotent response -- nothing was sent)");
                }
            }

            if status == StatusCode::TOO_MANY_REQUESTS && attempt < self.max_retries {
                let wait = ratelimit::retry_after(&headers)
                    .unwrap_or_else(|| Duration::from_millis(500 * (1 << attempt.min(4))));
                self.limiter.pause_for(wait);
                attempt += 1;
                if self.verbose {
                    eprintln!(
                        "  429 -- waiting {:.1}s before retry {attempt}",
                        wait.as_secs_f64()
                    );
                }
                tokio::time::sleep(wait).await;
                continue;
            }

            if status.is_server_error() && attempt < self.max_retries {
                attempt += 1;
                self.backoff(attempt).await;
                continue;
            }

            let text = response.text().await.unwrap_or_default();

            if !status.is_success() {
                let mut path = req.path.clone();
                if !req.query.is_empty() {
                    path.push('?');
                    path.push_str(&encode_query(&req.query));
                }
                return Err(Error::from_status(status.as_u16(), &text, &path));
            }

            if text.trim().is_empty() {
                return Ok(json!({ "data": Value::Null }));
            }
            return Ok(serde_json::from_str(&text)?);
        }
    }

    async fn backoff(&self, attempt: u32) {
        let base = Duration::from_millis(300 * (1 << attempt.min(4)));
        // Deterministic-enough jitter; this does not need to be random-quality.
        let jitter = Duration::from_millis((std::process::id() as u64 % 120) + 30);
        let wait = base + jitter;
        if self.verbose {
            eprintln!(
                "  retrying in {:.1}s (attempt {attempt})",
                wait.as_secs_f64()
            );
        }
        tokio::time::sleep(wait).await;
    }

    fn build(&self, req: &Request) -> Result<reqwest::Request> {
        let mut builder = self
            .http
            .request(req.method.clone(), self.url(&req.path))
            .bearer_auth(&self.token)
            .header("accept", "application/json");

        if !req.query.is_empty() {
            builder = builder.query(&req.query);
        }
        if let Some(key) = &req.idempotency_key {
            builder = builder.header("idempotency-key", key);
        }
        for (name, value) in &req.headers {
            let name = HeaderName::from_bytes(name.as_bytes())
                .map_err(|_| Error::usage(format!("Invalid header name: {name}")))?;
            let value = HeaderValue::from_str(value)
                .map_err(|_| Error::usage(format!("Invalid header value for {name}")))?;
            builder = builder.header(name, value);
        }
        if let Some(body) = &req.body {
            builder = builder.json(body);
        }

        builder.build().map_err(Into::into)
    }

    /// The `--dry-run` rendering of a request. The token is masked here rather
    /// than at print time so it cannot leak into `--json` output either.
    pub fn describe(&self, req: &Request, note: Option<&str>) -> Value {
        let mut url = self.url(&req.path);
        if !req.query.is_empty() {
            url.push('?');
            url.push_str(&encode_query(&req.query));
        }

        let mut headers = serde_json::Map::new();
        headers.insert(
            "authorization".into(),
            json!(format!("Bearer {}", mask(&self.token))),
        );
        headers.insert("accept".into(), json!("application/json"));
        if req.body.is_some() {
            headers.insert("content-type".into(), json!("application/json"));
        }
        if let Some(key) = &req.idempotency_key {
            headers.insert("idempotency-key".into(), json!(key));
        }
        for (name, value) in &req.headers {
            headers.insert(name.to_lowercase(), json!(value));
        }

        let mut entry = json!({
            "method": req.method.as_str(),
            "url": url,
            "headers": Value::Object(headers),
        });
        if let Some(body) = &req.body {
            entry["body"] = body.clone();
        }
        if let Some(note) = note {
            entry["note"] = json!(note);
        }
        entry
    }
}

fn encode_query(query: &[(String, String)]) -> String {
    query
        .iter()
        .map(|(k, v)| format!("{}={}", percent_encode(k), percent_encode(v)))
        .collect::<Vec<_>>()
        .join("&")
}

/// Enough of `application/x-www-form-urlencoded` for display purposes; the real
/// encoding is reqwest's job.
fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            b' ' => out.push_str("%20"),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

pub fn mask(token: &str) -> String {
    let chars: Vec<char> = token.chars().collect();
    match chars.len() {
        0..=8 => "…".to_string(),
        // Keep any `pf_live_`-style prefix, which identifies the key's kind
        // without revealing the secret.
        _ => {
            let head: String = chars[..8.min(chars.len())].iter().collect();
            let tail: String = chars[chars.len() - 4..].iter().collect();
            format!("{head}…{tail}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_tokens() {
        assert_eq!(mask("pf_live_abcdefgh8fa2"), "pf_live_…8fa2");
        assert_eq!(mask("short"), "…");
    }

    #[test]
    fn encodes_query_for_display() {
        let q = vec![("q".to_string(), "hello world".to_string())];
        assert_eq!(encode_query(&q), "q=hello%20world");
    }
}
