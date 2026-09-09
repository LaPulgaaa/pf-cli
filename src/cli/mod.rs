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
use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

use crate::client::{Client, Result};
use crate::config::{self, Config, TokenArgs};
use crate::output::{Output, Printer};

const ABOUT: &str = "Work with the Passionfroot public API from the command line.";

const LONG_ABOUT: &str = "\
Work with the Passionfroot public API from the command line.

Mint a key at Settings > API Keys in your dashboard, then save it once with
`pf auth login`. It is written to ~/.config/pf/config.toml, readable only by
you. A token can also come from --token, --token-file, or the environment
variable PASSIONFROOT_API_TOKEN, in that order of precedence.

Several workspaces are handled with named profiles: `pf auth login --profile
agency-b`, then `pf --profile agency-b conv list`.

Output is a table on a terminal and JSON everywhere else, so piping a command
into jq needs no extra flag. pf throttles itself to the documented 2 requests
per second, retries 429 and 5xx responses with backoff, and attaches an
Idempotency-Key to every write so a retry cannot send twice.";

const AFTER_HELP: &str = "\
Examples:
  pf auth login
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
    ///
    /// Falls back to the active profile's, then to
    /// https://workspace.passionfroot.me/api/v1. Passed to `auth login`, it is
    /// saved alongside the token.
    #[arg(long, global = true, value_name = "URL")]
    pub base_url: Option<String>,

    /// API token for this invocation.
    ///
    /// Convenient in a pipeline, but it lands in shell history and is visible
    /// to `ps` while the command runs. Prefer --token-file or `pf auth login`.
    #[arg(long, global = true, value_name = "TOKEN")]
    pub token: Option<String>,

    /// Read the API token from a file, or from stdin with `-`.
    #[arg(long, global = true, value_name = "PATH", conflicts_with = "token")]
    pub token_file: Option<PathBuf>,

    /// Use this named profile from the config file.
    #[arg(long, global = true, value_name = "NAME")]
    pub profile: Option<String>,

    /// Read configuration from this file instead of ~/.config/pf/config.toml.
    #[arg(long, global = true, value_name = "PATH")]
    pub config: Option<PathBuf>,

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

    pub fn config_path(&self) -> Option<PathBuf> {
        Config::path(self.config.as_deref())
    }

    pub fn load_config(&self) -> Result<Config> {
        Config::load(self.config_path().as_deref())
    }

    pub fn credentials(&self, config: &Config) -> Result<config::Credentials> {
        config::resolve(
            &TokenArgs {
                token: self.token.as_deref(),
                token_file: self.token_file.as_deref(),
                profile: self.profile.as_deref(),
                base_url: self.base_url.as_deref(),
            },
            config,
        )
    }

    pub fn client(&self, config: &Config) -> Result<Client> {
        Client::new(
            self.credentials(config)?,
            self.timeout,
            self.max_retries,
            self.dry_run,
            self.verbose,
        )
    }
}

#[derive(Debug, Subcommand)]
pub enum Resource {
    /// Save, inspect and remove the API key pf uses.
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
            // Handled in main before a client exists: `auth login` has to run
            // when there is no usable token yet.
            Resource::Auth(_) => Err(crate::client::Error::other(
                "auth commands are dispatched before the client is built",
            )),
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
