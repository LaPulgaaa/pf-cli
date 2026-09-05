use std::io::Read;
use std::path::PathBuf;

use clap::Args;
use reqwest::Method;
use serde_json::{Value, json};

use crate::client::{Client, Error, Request, Result, paginate};
use crate::output::{Output, View};

/// Direct access to any endpoint, including ones this CLI does not model.
#[derive(Debug, Args)]
pub struct ApiCmd {
    /// `<PATH>` or `<METHOD> <PATH>`, e.g. `/placements` or `POST /inquiries`.
    ///
    /// A path may be absolute, relative, or a full URL; anything relative is
    /// resolved against --base-url.
    #[arg(value_name = "[METHOD] PATH", num_args = 1..=2, required = true)]
    target: Vec<String>,

    /// HTTP method. Defaults to GET, or POST when --input supplies a body.
    #[arg(short = 'X', long, value_name = "METHOD")]
    method: Option<String>,

    /// Extra header, as `Name: value`. Repeatable.
    #[arg(short = 'H', long = "header", value_name = "NAME: VALUE")]
    headers: Vec<String>,

    /// Typed parameter: `true`, `false`, `null` and numbers are sent as such.
    ///
    /// Becomes a query parameter on the default GET, and a JSON body field once
    /// a method that takes a body is named. Fields alone never promote a read
    /// into a write -- pass `-X POST` (or `pf api POST <path>`) for that.
    #[arg(short = 'f', long = "field", value_name = "KEY=VALUE")]
    fields: Vec<String>,

    /// Parameter always sent as a string, however it looks.
    #[arg(short = 'F', long = "raw-field", value_name = "KEY=VALUE")]
    raw_fields: Vec<String>,

    /// Read the whole request body from a JSON file, or from stdin with `-`.
    #[arg(long, value_name = "FILE", conflicts_with_all = ["fields", "raw_fields"])]
    input: Option<PathBuf>,

    /// Follow cursors, for any endpoint returning `{ data, pagination }`.
    #[arg(long)]
    paginate: bool,

    /// Send this Idempotency-Key.
    #[arg(long = "idempotency-key", value_name = "KEY")]
    idempotency_key: Option<String>,
}

impl ApiCmd {
    pub async fn run(&self, client: &Client) -> Result<Output> {
        let (method_from_target, path) = split_target(&self.target)?;

        let body = self.body()?;
        let method_name = self
            .method
            .clone()
            .or(method_from_target)
            // Deliberately not gh's rule, where fields alone imply POST. Most
            // of this API is reads, and an unintended POST to one of them is a
            // write nobody asked for; an unintended GET is harmless.
            .unwrap_or_else(|| if self.input.is_some() { "POST".into() } else { "GET".into() })
            .to_uppercase();

        let method = Method::from_bytes(method_name.as_bytes())
            .map_err(|_| Error::usage(format!("Not an HTTP method: {method_name}")))?;

        // The cursor is pf's to manage during a walk; one already in the path
        // would be sent alongside ours on every page after the first.
        if self.paginate && path.contains("cursor=") {
            return Err(Error::usage(
                "--paginate manages the cursor itself, but this path already sets one.",
            )
            .with_hint("Drop `cursor=` from the path, or page manually without --paginate."));
        }

        let sends_body = !matches!(method, Method::GET | Method::HEAD | Method::DELETE);
        let mut req = Request::new(method, path);

        if let Some(body) = body {
            if sends_body {
                req = req.body(body);
            } else {
                // A GET has nowhere to put a body, so the fields become query
                // parameters rather than being dropped.
                for (key, value) in body.as_object().into_iter().flatten() {
                    let rendered = match value {
                        Value::String(s) => s.clone(),
                        other => other.to_string(),
                    };
                    req = req.query(key, rendered);
                }
            }
        }

        for header in &self.headers {
            let (name, value) = header.split_once(':').ok_or_else(|| {
                Error::usage(format!("Header must be `Name: value`, got `{header}`"))
            })?;
            req.headers.push((name.trim().to_string(), value.trim().to_string()));
        }

        if let Some(key) = &self.idempotency_key {
            req = req.idempotent(Some(key.clone()));
        }

        let value = paginate::collect(client, req, self.paginate).await?;
        Ok(Output::new(value, View::Raw))
    }

    fn body(&self) -> Result<Option<Value>> {
        if let Some(path) = &self.input {
            let raw = if path.as_os_str() == "-" {
                let mut buf = String::new();
                std::io::stdin()
                    .read_to_string(&mut buf)
                    .map_err(|e| Error::usage(format!("Could not read stdin: {e}")))?;
                buf
            } else {
                std::fs::read_to_string(path)
                    .map_err(|e| Error::usage(format!("Could not read {}: {e}", path.display())))?
            };
            let parsed: Value = serde_json::from_str(&raw)
                .map_err(|e| Error::usage(format!("--input is not valid JSON: {e}")))?;
            return Ok(Some(parsed));
        }

        if self.fields.is_empty() && self.raw_fields.is_empty() {
            return Ok(None);
        }

        let mut object = serde_json::Map::new();
        for field in &self.fields {
            let (key, value) = split_field(field)?;
            object.insert(key, infer(value));
        }
        for field in &self.raw_fields {
            let (key, value) = split_field(field)?;
            object.insert(key, json!(value));
        }
        Ok(Some(Value::Object(object)))
    }
}

fn split_target(target: &[String]) -> Result<(Option<String>, String)> {
    match target {
        [path] => Ok((None, path.clone())),
        [method, path] => Ok((Some(method.clone()), path.clone())),
        _ => Err(Error::usage("Expected `pf api <PATH>` or `pf api <METHOD> <PATH>`.")),
    }
}

fn split_field(field: &str) -> Result<(String, &str)> {
    field
        .split_once('=')
        .map(|(k, v)| (k.to_string(), v))
        .ok_or_else(|| Error::usage(format!("Field must be `key=value`, got `{field}`")))
}

/// Give `-f` values their obvious JSON type, the way `gh api` does.
fn infer(value: &str) -> Value {
    match value {
        "true" => json!(true),
        "false" => json!(false),
        "null" => Value::Null,
        _ => {
            if let Ok(n) = value.parse::<i64>() {
                json!(n)
            } else if let Ok(n) = value.parse::<f64>() {
                json!(n)
            } else {
                json!(value)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn infers_field_types() {
        assert_eq!(infer("true"), json!(true));
        assert_eq!(infer("42"), json!(42));
        assert_eq!(infer("1.5"), json!(1.5));
        assert_eq!(infer("hello"), json!("hello"));
        assert_eq!(infer("null"), Value::Null);
    }

    #[test]
    fn splits_method_and_path() {
        let one = vec!["/placements".to_string()];
        assert_eq!(split_target(&one).unwrap(), (None, "/placements".into()));
        let two = vec!["POST".to_string(), "/inquiries".to_string()];
        assert_eq!(split_target(&two).unwrap(), (Some("POST".into()), "/inquiries".into()));
    }
}
