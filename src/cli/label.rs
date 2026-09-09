use clap::{Args, Subcommand};
use serde_json::Value;

use crate::client::{Client, Error, Request, Result};
use crate::output::{Output, View};

#[derive(Debug, Args)]
pub struct LabelCmd {
    #[command(subcommand)]
    command: LabelSub,
}

#[derive(Debug, Subcommand)]
enum LabelSub {
    /// List your workspace's creator-label catalog.
    #[command(visible_alias = "ls")]
    List,
}

impl LabelCmd {
    pub async fn run(&self, client: &Client) -> Result<Output> {
        match self.command {
            // Not paginated: label names are unique per workspace and catalogs
            // are small.
            LabelSub::List => {
                let value = client.send(Request::get("/labels")).await?;
                Ok(Output::new(value, View::Labels))
            }
        }
    }
}

/// Turn `--label` values into label IDs, accepting either IDs or names.
///
/// The API rejects a label ID it does not own with a 400 rather than an empty
/// list, so a typo is never silently read as "a label nobody carries" -- this
/// keeps that property for names too, by failing with the real catalog rather
/// than passing an unknown name through.
pub async fn resolve_ids(client: &Client, values: &[String]) -> Result<Vec<String>> {
    if values.is_empty() {
        return Ok(Vec::new());
    }

    // Skip the extra request when every value is already ID-shaped.
    if values.iter().all(|v| is_id_shaped(v)) {
        return Ok(values.to_vec());
    }

    let catalog = client.send(Request::get("/labels")).await?;
    let labels: Vec<(String, String)> = catalog
        .get("data")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|l| {
                    Some((
                        l.get("id")?.as_str()?.to_string(),
                        l.get("name")?.as_str()?.to_string(),
                    ))
                })
                .collect()
        })
        .unwrap_or_default();

    let mut resolved = Vec::with_capacity(values.len());
    for value in values {
        if let Some((id, _)) = labels.iter().find(|(id, _)| id == value) {
            resolved.push(id.clone());
            continue;
        }
        match labels
            .iter()
            .find(|(_, name)| name.eq_ignore_ascii_case(value))
        {
            Some((id, _)) => resolved.push(id.clone()),
            None => {
                let known: Vec<&str> = labels.iter().map(|(_, n)| n.as_str()).collect();
                return Err(
                    Error::usage(format!("No label named `{value}` in this workspace.")).with_hint(
                        if known.is_empty() {
                            "This workspace has no labels yet.".to_string()
                        } else {
                            format!("Known labels: {}", known.join(", "))
                        },
                    ),
                );
            }
        }
    }
    Ok(resolved)
}

/// Label IDs come in two shapes: a bare UUID, and `clb_abc123` -- a prefix, an
/// underscore, then an opaque body. Either is passed straight through; only a
/// value matching neither is worth spending a catalog request to resolve.
fn is_id_shaped(value: &str) -> bool {
    if crate::output::fmt::is_uuid(value) {
        return true;
    }
    let Some((prefix, rest)) = value.split_once('_') else {
        return false;
    };
    !prefix.is_empty()
        && prefix.chars().all(|c| c.is_ascii_lowercase())
        && rest.len() >= 6
        && rest.chars().all(|c| c.is_ascii_alphanumeric())
}

#[cfg(test)]
mod tests {
    use super::is_id_shaped;

    #[test]
    fn tells_ids_from_names() {
        assert!(is_id_shaped("clb_abc123"));
        assert!(is_id_shaped("5c54cbd4-dc5e-42c1-92a3-6b1435b8032b"));
        assert!(!is_id_shaped("High performer"));
        assert!(!is_id_shaped("Brand fit"));
        assert!(!is_id_shaped("clb_"));
    }
}
