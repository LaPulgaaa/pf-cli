use clap::{Args, Subcommand};
use serde_json::json;

use crate::client::{Client, Request, Result};
use crate::output::{Output, View};

#[derive(Debug, Args)]
pub struct ProposalCmd {
    #[command(subcommand)]
    command: ProposalSub,
}

#[derive(Debug, Subcommand)]
enum ProposalSub {
    /// Accept a creator proposal, confirming the collaboration.
    ///
    /// Idempotent: accepting an already-accepted proposal returns its current
    /// state. Only creator-authored proposals can be acted on.
    Accept(AcceptArgs),

    /// Reject a creator proposal.
    ///
    /// Terminal: the collaboration is cancelled and its dates are released.
    Reject(RejectArgs),
}

#[derive(Debug, Args)]
struct AcceptArgs {
    /// Proposal ID, from `proposal.proposalId` in a conversation timeline.
    #[arg(value_name = "PROPOSAL-ID")]
    id: String,
}

#[derive(Debug, Args)]
struct RejectArgs {
    /// Proposal ID, from `proposal.proposalId` in a conversation timeline.
    #[arg(value_name = "PROPOSAL-ID")]
    id: String,

    /// Reason shown to the creator in the conversation.
    #[arg(long, value_name = "TEXT")]
    comment: Option<String>,
}

const MAX_COMMENT_CHARS: usize = 10_000;

impl ProposalCmd {
    pub async fn run(&self, client: &Client) -> Result<Output> {
        let req = match &self.command {
            ProposalSub::Accept(args) => Request::post(format!("/proposals/{}/accept", args.id)),
            ProposalSub::Reject(args) => {
                if let Some(comment) = &args.comment {
                    let chars = comment.chars().count();
                    if chars > MAX_COMMENT_CHARS {
                        return Err(crate::client::Error::usage(format!(
                            "--comment is {chars} characters; the API accepts at most {MAX_COMMENT_CHARS}."
                        )));
                    }
                }
                let req = Request::post(format!("/proposals/{}/reject", args.id));
                match &args.comment {
                    Some(comment) => req.body(json!({ "comment": comment })),
                    None => req,
                }
            }
        };

        let value = client.send(req).await?;
        Ok(Output::new(value, View::ProposalDetail))
    }
}
