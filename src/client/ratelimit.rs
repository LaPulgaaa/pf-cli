use std::fs::{File, OpenOptions};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tokio::time::Instant;

/// Client-side throttle for the documented budget: 2 requests/second per key,
/// behind a 20/second per-IP cap.
///
/// Staying under the limit locally is most of the point of this tool -- an
/// agent looping over `pf` calls should never be the reason a 429 happens.
/// The bucket starts at the documented rate and then follows whatever the
/// `RateLimit` headers actually report, so a change to the alpha limits
/// tightens us automatically instead of after a release.
pub struct RateLimiter {
    state: Mutex<State>,
    /// Slot file shared with every other `pf` process using the same key.
    shared: Option<PathBuf>,
}

struct State {
    /// Earliest instant at which the next request may leave.
    next_at: Option<Instant>,
    interval: Duration,
}

impl RateLimiter {
    /// `scope` identifies the budget being shared: the API key and the host it
    /// is used against. Different keys throttle independently, matching the
    /// per-key limit.
    pub fn new(scope: &str) -> Self {
        Self {
            state: Mutex::new(State { next_at: None, interval: Duration::from_millis(500) }),
            shared: shared_slot_path(scope),
        }
    }

    /// Block until this caller's slot comes up, then claim the one after it.
    ///
    /// Two reservations are made: one in-process, and one in a lock file shared
    /// with concurrent `pf` processes. The in-process bucket alone would leave
    /// an agent driving `pf` from a shell loop -- the most likely way this tool
    /// is used -- with no throttling whatsoever, since each invocation would
    /// start with an empty bucket.
    pub async fn acquire(&self) {
        let interval = self.state.lock().unwrap().interval;

        let local = {
            let mut st = self.state.lock().unwrap();
            let now = Instant::now();
            let slot = match st.next_at {
                Some(t) if t > now => t,
                _ => now,
            };
            st.next_at = Some(slot + st.interval);
            slot.saturating_duration_since(now)
        };

        let shared = self.reserve_shared(interval).unwrap_or_default();
        let wait = local.max(shared);
        if !wait.is_zero() {
            tokio::time::sleep(wait).await;
        }
    }

    /// Claim the next slot in the cross-process file, returning how long to wait
    /// for it. Any failure here is non-fatal: throttling degrades to
    /// in-process only rather than blocking the request.
    fn reserve_shared(&self, interval: Duration) -> Option<Duration> {
        let path = self.shared.as_ref()?;
        let mut file = open_slot_file(path)?;
        file.lock().ok()?;

        let now_ms = now_millis();
        let stored = read_slot(&mut file);
        // A slot further out than any legitimate Retry-After means a stale or
        // corrupt file -- most likely a process killed mid-backoff. Do not let
        // it wedge every later run.
        let slot = match stored {
            Some(t) if t > now_ms && t - now_ms <= MAX_SHARED_WAIT_MS => t,
            _ => now_ms,
        };
        write_slot(&mut file, slot + interval.as_millis() as u64);

        let _ = file.unlock();
        Some(Duration::from_millis(slot.saturating_sub(now_ms)))
    }

    fn push_shared(&self, wait: Duration) {
        let Some(path) = self.shared.as_ref() else { return };
        let Some(mut file) = open_slot_file(path) else { return };
        if file.lock().is_err() {
            return;
        }
        let until = now_millis() + wait.as_millis() as u64;
        if read_slot(&mut file).is_none_or(|t| t < until) {
            write_slot(&mut file, until);
        }
        let _ = file.unlock();
    }

    /// Fold a response's rate-limit headers back into the bucket.
    pub fn observe(&self, headers: &reqwest::header::HeaderMap) {
        let combined = header_str(headers, "ratelimit");

        // `RateLimit-Policy: 2;w=1` -- derive the spacing from the real policy.
        if let Some(policy) = header_str(headers, "ratelimit-policy") {
            if let Some((limit, window)) = parse_policy(&policy) {
                if limit > 0 && window > 0.0 {
                    let per = Duration::from_secs_f64(window / limit as f64);
                    let mut st = self.state.lock().unwrap();
                    st.interval = per;
                }
            }
        }

        // `RateLimit: limit=2, remaining=0, reset=1` -- budget is spent, hold
        // everything until the window rolls over.
        if let Some(combined) = combined {
            let remaining = parse_kv(&combined, "remaining");
            let reset = parse_kv(&combined, "reset");
            if let (Some(0.0), Some(reset)) = (remaining, reset) {
                let mut st = self.state.lock().unwrap();
                let until = Instant::now() + Duration::from_secs_f64(reset.max(0.0));
                if st.next_at.is_none_or(|t| t < until) {
                    st.next_at = Some(until);
                }
            }
        }
    }

    /// Push the next slot out past a `Retry-After` we were explicitly given, so
    /// a 429 does not just delay the retry but every request queued behind it.
    pub fn pause_for(&self, wait: Duration) {
        {
            let mut st = self.state.lock().unwrap();
            let until = Instant::now() + wait;
            if st.next_at.is_none_or(|t| t < until) {
                st.next_at = Some(until);
            }
        }
        // A 429 is a property of the key, not of this process: hold every
        // concurrent `pf` back, not just this one.
        self.push_shared(wait);
    }
}

/// The longest a shared slot may sit in the future before it is treated as
/// stale. Matches the cap applied to `Retry-After`.
const MAX_SHARED_WAIT_MS: u64 = 300_000;

fn now_millis() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

fn open_slot_file(path: &PathBuf) -> Option<File> {
    OpenOptions::new().read(true).write(true).create(true).open(path).ok()
}

fn read_slot(file: &mut File) -> Option<u64> {
    file.seek(SeekFrom::Start(0)).ok()?;
    let mut buf = String::new();
    file.read_to_string(&mut buf).ok()?;
    buf.trim().parse::<u64>().ok()
}

fn write_slot(file: &mut File, millis: u64) {
    let _ = file.seek(SeekFrom::Start(0));
    let _ = file.set_len(0);
    let _ = write!(file, "{millis}");
    let _ = file.flush();
}

/// One slot file per key, in the user's cache directory. The key itself is
/// hashed rather than stored, so the filename cannot leak a secret to anyone
/// listing the directory.
fn shared_slot_path(scope: &str) -> Option<PathBuf> {
    if std::env::var_os("PF_NO_SHARED_THROTTLE").is_some_and(|v| !v.is_empty()) {
        return None;
    }

    let mut hasher = DefaultHasher::new();
    scope.hash(&mut hasher);
    let digest = hasher.finish();

    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::home_dir().map(|h| h.join(".cache")))
        .unwrap_or_else(std::env::temp_dir);

    let dir = base.join("pf");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir.join(format!("throttle-{digest:016x}")))
}

fn header_str(headers: &reqwest::header::HeaderMap, name: &str) -> Option<String> {
    headers.get(name)?.to_str().ok().map(str::to_owned)
}

/// `2;w=1` -> (2, 1.0)
fn parse_policy(s: &str) -> Option<(u32, f64)> {
    let mut parts = s.split(';');
    let limit: u32 = parts.next()?.trim().parse().ok()?;
    let window = parts
        .find_map(|p| p.trim().strip_prefix("w=").and_then(|v| v.trim().parse::<f64>().ok()))?;
    Some((limit, window))
}

/// Pull `key=value` out of a comma-separated structured header.
fn parse_kv(s: &str, key: &str) -> Option<f64> {
    s.split(',').find_map(|part| {
        let (k, v) = part.split_once('=')?;
        (k.trim().eq_ignore_ascii_case(key)).then(|| v.trim().parse::<f64>().ok())?
    })
}

/// `Retry-After` is seconds in every response the API documents, but the header
/// also permits an HTTP date; treat anything unparseable as a short wait rather
/// than giving up on the retry.
pub fn retry_after(headers: &reqwest::header::HeaderMap) -> Option<Duration> {
    let raw = header_str(headers, "retry-after")?;
    raw.trim().parse::<f64>().ok().map(|secs| Duration::from_secs_f64(secs.clamp(0.0, 300.0)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_policy_header() {
        assert_eq!(parse_policy("2;w=1"), Some((2, 1.0)));
        assert_eq!(parse_policy("20;w=1.5"), Some((20, 1.5)));
        assert_eq!(parse_policy("garbage"), None);
    }

    #[test]
    fn parses_combined_header() {
        let h = "limit=2, remaining=0, reset=1";
        assert_eq!(parse_kv(h, "limit"), Some(2.0));
        assert_eq!(parse_kv(h, "remaining"), Some(0.0));
        assert_eq!(parse_kv(h, "reset"), Some(1.0));
        assert_eq!(parse_kv(h, "absent"), None);
    }
}
