use clap::{Args, Subcommand};
use serde_json::json;

use super::common::TextInput;
use crate::client::{Client, Request, Result};
use crate::output::{Output, View};

#[derive(Debug, Args)]
pub struct InquiryCmd {
    #[command(subcommand)]
    command: InquirySub,
}

#[derive(Debug, Subcommand)]
enum InquirySub {
    /// Send an inquiry to a creator, creating the conversation.
    ///
    /// Use this for a creator you have not talked to before. `pf creator
    /// message` picks between this and a plain reply for you.
    Send(SendArgs),
}

#[derive(Debug, Args)]
struct SendArgs {
    /// Creator to send the inquiry to.
    #[arg(long = "creator", value_name = "ID")]
    creator_id: String,

    #[command(flatten)]
    text: TextInput,

    /// Link the resulting collaboration to one of your campaigns.
    #[arg(long = "campaign", value_name = "ID")]
    campaign_id: Option<String>,

    /// Reuse a key so a retry cannot send twice.
    ///
    /// pf generates one per invocation, which makes its own retries safe. It
    /// does nothing across separate `pf` runs: pass your own key to make
    /// re-running this command idempotent.
    #[arg(long = "idempotency-key", value_name = "KEY")]
    idempotency_key: Option<String>,
}

impl InquiryCmd {
    pub async fn run(&self, client: &Client) -> Result<Output> {
        match &self.command {
            InquirySub::Send(args) => {
                let text = args.text.resolve()?;
                let mut body = json!({ "creatorId": args.creator_id, "text": text });
                if let Some(campaign) = &args.campaign_id {
                    body["campaignId"] = json!(campaign);
                }

                let req = Request::post("/inquiries")
                    .body(body)
                    .idempotent(args.idempotency_key.clone());

                let value = client.send(req).await?;
                Ok(Output::new(value, View::Object))
            }
        }
    }
}
