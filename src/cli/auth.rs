use clap::{Args, Subcommand};
use serde_json::{Value, json};

use crate::client::{Client, Request, Result};
use crate::output::{Output, View};

#[derive(Debug, Args)]
pub struct AuthCmd {
    #[command(subcommand)]
    command: AuthSub,
}

#[derive(Debug, Subcommand)]
enum AuthSub {
    /// Check that the configured token works, and against which workspace.
    Status,
}

impl AuthCmd {
    pub async fn run(&self, client: &Client) -> Result<Output> {
        match self.command {
            AuthSub::Status => status(client).await,
        }
    }
}

/// There is no `/me` endpoint, so the cheapest honest probe is the smallest
/// unpaginated read the API offers.
async fn status(client: &Client) -> Result<Output> {
    let response = client.send(Request::get("/labels")).await?;
    let labels = response.get("data").and_then(Value::as_array).map(Vec::len).unwrap_or(0);

    let value = json!({
        "data": {
            "ok": true,
            "baseUrl": client.base_url(),
            "tokenSource": client.token_source(),
            "token": client.masked_token(),
            "labelCount": labels,
        }
    });

    if client.is_dry_run() {
        return Ok(Output::new(value, View::Object));
    }

    // The note below says everything the object would; drawing both is noise.
    Ok(Output::new(value, View::Silent).note(format!(
        "\u{2713} {}\n  token {} (env {})\n  {labels} label{} readable",
        client.base_url(),
        client.masked_token(),
        client.token_source(),
        if labels == 1 { "" } else { "s" },
    )))
}
