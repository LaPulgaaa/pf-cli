pub mod api;
pub mod auth;
pub mod collab;
pub mod common;
pub mod conv;
pub mod creator;
pub mod inbox;
pub mod inquiry;
pub mod label;
pub mod placement;
pub mod proposal;

use std::io::IsTerminal;

use clap::{Args, Parser, Subcommand};

use crate::client::{Client, DEFAULT_BASE_URL, Result};
use crate::output::{Output, Printer};

const ABOUT: &str = "Work with the Passionfroot public API from the command line.";

const LONG_ABOUT: &str = "\
Work with the Passionfroot public API from the command line.

Authentication reads PASSIONFROOT_API_TOKEN (or PF_API_TOKEN) from the
environment. Mint a key at Settings > API Keys in your dashboard and export it
from your shell profile.

Output is a table on a terminal and JSON everywhere else, so piping a command
into jq needs no extra flag. pf throttles itself to the documented 2 requests
per second, retries 429 and 5xx responses with backoff, and attaches an
Idempotency-Key to every write so a retry cannot send twice.";

const AFTER_HELP: &str = "\
Examples:
  pf auth status
  pf inbox
  pf placement list --status confirmed --start-date 2026-01-01 --paginate
  pf creator list --label \"High performer\" --include channels
  pf conv messages conv_abc123 --type message --reverse
  pf creator message creator_abc123 --text \"Hi! Would you like to collaborate?\"
  pf proposal accept prop_123
  pf api /placements -f limit=5 --paginate

Exit codes:
  0 ok   2 usage   3 auth   4 not found   5 conflict
  6 rate limited   7 network   1 other";

#[derive(Debug, Parser)]
#[command(
    name = "pf",
    version,
    about = ABOUT,
    long_about = LONG_ABOUT,
    after_help = AFTER_HELP,
    disable_help_subcommand = true,
    propagate_version = true
)]
pub struct Cli {
    #[command(flatten)]
    pub global: GlobalArgs,

    #[command(subcommand)]
    pub command: Resource,
}

#[derive(Debug, Args)]
pub struct GlobalArgs {
    /// Emit JSON. Already the default when stdout is not a terminal.
    #[arg(long, global = true)]
    pub json: bool,

    /// API root.
    #[arg(long, global = true, env = "PF_BASE_URL", default_value = DEFAULT_BASE_URL, value_name = "URL")]
    pub base_url: String,

    /// Per-request timeout, in seconds.
    #[arg(long, global = true, default_value_t = 30, value_name = "SECS")]
    pub timeout: u64,

    /// How many times to retry 429 and 5xx responses. 4xx is never retried.
    #[arg(long, global = true, default_value_t = 3, value_name = "N")]
    pub max_retries: u32,

    /// Print the requests that would be sent, and send nothing.
    #[arg(long, global = true)]
    pub dry_run: bool,

    /// Trace requests, status codes and rate-limit headers to stderr.
    #[arg(short = 'v', long, global = true)]
    pub verbose: bool,

    /// Suppress the human-only notes on stderr.
    #[arg(short = 'q', long, global = true)]
    pub quiet: bool,

    /// Never colourise output.
    #[arg(long, global = true)]
    pub no_color: bool,
}

impl GlobalArgs {
    pub fn printer(&self) -> Printer {
        let tty = std::io::stdout().is_terminal();
        Printer {
            // Piped output is almost always being parsed, so JSON is the safer
            // default there than a table nobody promised to keep stable.
            json: self.json || !tty,
            color: tty
                && !self.no_color
                && std::env::var_os("NO_COLOR").is_none_or(|v| v.is_empty()),
            quiet: self.quiet,
        }
    }

    pub fn client(&self) -> Result<Client> {
        Client::new(
            self.base_url.clone(),
            self.timeout,
            self.max_retries,
            self.dry_run,
            self.verbose,
        )
    }
}

#[derive(Debug, Subcommand)]
pub enum Resource {
    /// Check the configured API token.
    Auth(auth::AuthCmd),

    /// Unread conversations that still need answering.
    Inbox(inbox::InboxCmd),

    /// Scheduled and published placements.
    #[command(visible_alias = "placements")]
    Placement(placement::PlacementCmd),

    /// Creators, their channels, and your workspace's notes and labels.
    #[command(visible_alias = "creators")]
    Creator(creator::CreatorCmd),

    /// Your workspace's creator-label catalog.
    #[command(visible_alias = "labels")]
    Label(label::LabelCmd),

    /// Collaborations and their campaigns.
    #[command(visible_alias = "collaboration")]
    Collab(collab::CollabCmd),

    /// Conversations with creators, and their message timelines.
    #[command(visible_alias = "conversation")]
    Conv(conv::ConvCmd),

    /// Outreach to creators you have not talked to yet.
    Inquiry(inquiry::InquiryCmd),

    /// Creator proposals awaiting your decision.
    Proposal(proposal::ProposalCmd),

    /// Call any endpoint directly.
    Api(api::ApiCmd),
}

impl Resource {
    pub async fn run(&self, client: &Client) -> Result<Output> {
        match self {
            Resource::Auth(cmd) => cmd.run(client).await,
            Resource::Inbox(cmd) => cmd.run(client).await,
            Resource::Placement(cmd) => cmd.run(client).await,
            Resource::Creator(cmd) => cmd.run(client).await,
            Resource::Label(cmd) => cmd.run(client).await,
            Resource::Collab(cmd) => cmd.run(client).await,
            Resource::Conv(cmd) => cmd.run(client).await,
            Resource::Inquiry(cmd) => cmd.run(client).await,
            Resource::Proposal(cmd) => cmd.run(client).await,
            Resource::Api(cmd) => cmd.run(client).await,
        }
    }
}
