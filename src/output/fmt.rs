use chrono::{DateTime, Datelike, Local, NaiveDate};

/// Render cents in the currency the API reported.
///
/// Only tables get this treatment. `--json` keeps the integer cents, because a
/// formatted string is lossy and anything downstream doing arithmetic wants the
/// original.
pub fn money(cents: Option<i64>, currency: Option<&str>) -> String {
    let Some(cents) = cents else {
        return "-".into();
    };
    let code = currency.unwrap_or("").to_uppercase();
    let negative = cents < 0;
    let abs = cents.unsigned_abs();
    let amount = format!("{}.{:02}", thousands(abs / 100), abs % 100);
    let sign = if negative { "-" } else { "" };

    match code.as_str() {
        "USD" => format!("{sign}${amount}"),
        "EUR" => format!("{sign}\u{20ac}{amount}"),
        "GBP" => format!("{sign}\u{a3}{amount}"),
        "" => format!("{sign}{amount}"),
        _ => format!("{sign}{amount} {code}"),
    }
}

pub fn count(n: Option<i64>) -> String {
    match n {
        Some(n) if n < 0 => format!("-{}", thousands(n.unsigned_abs())),
        Some(n) => thousands(n as u64),
        None => "-".into(),
    }
}

pub fn thousands(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// ISO 8601 -> a short local-time stamp. Anything unparseable is passed through
/// verbatim rather than swallowed, so an unexpected format stays visible.
pub fn datetime(raw: Option<&str>) -> String {
    let Some(raw) = raw else { return "-".into() };
    let Ok(parsed) = DateTime::parse_from_rfc3339(raw) else {
        return raw.split('T').next().unwrap_or(raw).to_string();
    };
    let local = parsed.with_timezone(&Local);
    if local.year() == Local::now().year() {
        local.format("%b %e %H:%M").to_string()
    } else {
        local.format("%b %e, %Y").to_string()
    }
}

/// Placement dates are scheduling dates, not timestamps; the time of day is
/// noise.
pub fn date(raw: Option<&str>) -> String {
    let Some(raw) = raw else { return "-".into() };
    if let Ok(parsed) = DateTime::parse_from_rfc3339(raw) {
        return parsed.with_timezone(&Local).format("%Y-%m-%d").to_string();
    }
    raw.split('T').next().unwrap_or(raw).to_string()
}

pub fn text(value: Option<&str>) -> String {
    value.map(str::to_owned).unwrap_or_else(|| "-".into())
}

/// Message text may carry composer HTML (`<p>`, `<br>`, links). Tables want the
/// reading copy; `--json` still gets the original markup.
pub fn strip_html(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut in_tag = false;
    let mut chars = input.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => {
                in_tag = false;
                // Block boundaries become spaces so words do not run together.
                if !out.ends_with(' ') && !out.is_empty() {
                    out.push(' ');
                }
            }
            _ if in_tag => {}
            '&' => {
                let mut entity = String::new();
                while let Some(&n) = chars.peek() {
                    if n == ';' || entity.len() > 8 {
                        break;
                    }
                    entity.push(n);
                    chars.next();
                }
                if chars.peek() == Some(&';') {
                    chars.next();
                }
                match decode_entity(&entity) {
                    Some(decoded) => out.push_str(&decoded),
                    // Never drop characters we do not recognise: show the
                    // entity as written rather than silently losing text.
                    None => {
                        out.push('&');
                        out.push_str(&entity);
                        out.push(';');
                    }
                }
            }
            '\n' | '\t' | '\r' => out.push(' '),
            _ => out.push(c),
        }
    }

    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn decode_entity(entity: &str) -> Option<String> {
    let named = match entity {
        "amp" => "&",
        "lt" => "<",
        "gt" => ">",
        "quot" => "\"",
        "apos" => "'",
        "nbsp" => " ",
        "mdash" => "\u{2014}",
        "ndash" => "\u{2013}",
        "hellip" => "\u{2026}",
        "lsquo" => "\u{2018}",
        "rsquo" => "\u{2019}",
        "ldquo" => "\u{201c}",
        "rdquo" => "\u{201d}",
        _ => {
            // Numeric forms: `&#39;` and `&#x27;`.
            let rest = entity.strip_prefix('#')?;
            let code = match rest.strip_prefix(['x', 'X']) {
                Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                None => rest.parse::<u32>().ok()?,
            };
            return char::from_u32(code).map(String::from);
        }
    };
    Some(named.to_string())
}

/// Whether a value is a bare UUID.
///
/// Identifiers are all-or-nothing: half a UUID cannot be copied, pasted back,
/// or recognised, whereas half a sentence still reads. Callers use this to
/// exempt identifiers from column truncation.
pub fn is_uuid(s: &str) -> bool {
    s.len() == 36
        && s.as_bytes().iter().enumerate().all(|(i, b)| match i {
            8 | 13 | 18 | 23 => *b == b'-',
            _ => b.is_ascii_hexdigit(),
        })
}

pub fn truncate(s: &str, max: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max {
        return s.to_string();
    }
    let cut: String = chars[..max.saturating_sub(1)].iter().collect();
    format!("{}\u{2026}", cut.trim_end())
}

/// Validate a `YYYY-MM-DD` argument before spending a request on it.
pub fn parse_date_arg(s: &str) -> Result<String, String> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .map(|_| s.to_string())
        .map_err(|_| format!("expected a date as YYYY-MM-DD, got `{s}`"))
}

/// The `*After` / `*Before` filters accept either a date or a full timestamp.
pub fn parse_datetime_arg(s: &str) -> Result<String, String> {
    if NaiveDate::parse_from_str(s, "%Y-%m-%d").is_ok() || DateTime::parse_from_rfc3339(s).is_ok() {
        return Ok(s.to_string());
    }
    Err(format!(
        "expected YYYY-MM-DD or an ISO 8601 datetime, got `{s}`"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_money_per_currency() {
        assert_eq!(money(Some(150000), Some("USD")), "$1,500.00");
        assert_eq!(money(Some(31500), Some("EUR")), "\u{20ac}315.00");
        assert_eq!(money(Some(999), Some("SEK")), "9.99 SEK");
        assert_eq!(money(None, Some("USD")), "-");
    }

    #[test]
    fn strips_composer_markup() {
        assert_eq!(strip_html("<p>Hi there!</p>"), "Hi there!");
        assert_eq!(strip_html("<p>One</p><p>Two</p>"), "One Two");
        assert_eq!(strip_html("A &amp; B &lt;3"), "A & B <3");
    }

    #[test]
    fn keeps_characters_it_cannot_decode() {
        assert_eq!(strip_html("draft &mdash; ready"), "draft \u{2014} ready");
        assert_eq!(strip_html("it&#39;s here"), "it's here");
        assert_eq!(strip_html("x &weird; y"), "x &weird; y");
    }

    #[test]
    fn recognises_uuids() {
        assert!(is_uuid("29851872-88ae-42b4-b69a-a096f1c2d3e4"));
        assert!(!is_uuid("29851872-88ae-42b4-b69a-a096f"));
        assert!(!is_uuid("clb_abc123"));
        assert!(!is_uuid("Anna | UGC Coach & Content Strategist"));
    }

    #[test]
    fn rejects_malformed_dates() {
        assert!(parse_date_arg("2026-07-01").is_ok());
        assert!(parse_date_arg("07/01/2026").is_err());
        assert!(parse_datetime_arg("2026-07-01T00:00:00Z").is_ok());
        assert!(parse_datetime_arg("yesterday").is_err());
    }
}
