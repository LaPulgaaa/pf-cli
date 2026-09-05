use clap::{Args, Subcommand, ValueEnum};
use serde_json::{Value, json};

use super::common::{PageArgs, TextInput, csv, tristate};
use crate::client::{Client, Request, Result, paginate};
use crate::output::fmt::parse_datetime_arg;
use crate::output::{Output, View};

#[derive(Debug, Args)]
pub struct ConvCmd {
    #[command(subcommand)]
    command: ConvSub,
}

#[derive(Debug, Subcommand)]
enum ConvSub {
    /// List conversations, newest activity first.
    #[command(visible_alias = "ls")]
    List(ListArgs),

    /// Print a conversation's timeline, newest first.
    #[command(visible_alias = "log")]
    Messages(MessagesArgs),

    /// Send a message into a conversation.
    Reply(ReplyArgs),

    /// Clear a conversation's unread flag.
    ///
    /// Only needed when you process a conversation without answering it:
    /// sending a message already marks it read.
    Read(ReadArgs),
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum Include {
    #[value(name = "creator")]
    Creator,
}

#[derive(Debug, Args)]
struct ListArgs {
    /// Only conversations with activity on or after this point.
    ///
    /// Tracks message activity only: archiving, blocking and read-state changes
    /// do not bump it.
    #[arg(long, value_name = "WHEN", value_parser = parse_datetime_arg)]
    updated_after: Option<String>,

    /// The conversation with one specific creator.
    #[arg(long = "creator", value_name = "ID")]
    creator_id: Option<String>,

    /// Keyword search over creator names and message text (min 2 characters).
    ///
    /// This is the API's `q` parameter; `-q` is --quiet here.
    #[arg(long, value_name = "TEXT")]
    search: Option<String>,

    /// Only unread conversations.
    #[arg(long)]
    unread: bool,
    /// Only conversations that are not unread.
    #[arg(long = "no-unread", conflicts_with = "unread")]
    no_unread: bool,

    /// Only archived conversations.
    #[arg(long)]
    archived: bool,
    /// Exclude archived conversations.
    #[arg(long = "no-archived", conflicts_with = "archived")]
    no_archived: bool,

    /// Only blocked conversations.
    #[arg(long)]
    blocked: bool,
    /// Exclude blocked conversations.
    #[arg(long = "no-blocked", conflicts_with = "blocked")]
    no_blocked: bool,

    /// Expand related objects.
    #[arg(long, value_enum, value_delimiter = ',', num_args = 1..)]
    include: Vec<Include>,

    #[command(flatten)]
    page: PageArgs,
}

#[derive(Debug, Args)]
struct MessagesArgs {
    /// Conversation ID.
    #[arg(value_name = "CONVERSATION-ID")]
    id: String,

    /// Only messages on or after this point.
    #[arg(long, value_name = "WHEN", value_parser = parse_datetime_arg)]
    created_after: Option<String>,

    /// Keep only these message types.
    ///
    /// Filtered locally after the fetch -- the endpoint has no type parameter,
    /// so this narrows the output but not the request.
    #[arg(long = "type", value_name = "TYPE")]
    types: Vec<String>,

    /// Print oldest first instead of newest first.
    #[arg(long)]
    reverse: bool,

    #[command(flatten)]
    page: PageArgs,
}

#[derive(Debug, Args)]
struct ReplyArgs {
    /// Conversation ID.
    #[arg(value_name = "CONVERSATION-ID")]
    id: String,

    #[command(flatten)]
    text: TextInput,

    /// Reuse a key so a retry cannot send twice.
    ///
    /// pf generates one per invocation, which makes its own retries safe. It
    /// does nothing across separate `pf` runs: pass your own key to make
    /// re-running this command idempotent.
    #[arg(long = "idempotency-key", value_name = "KEY")]
    idempotency_key: Option<String>,
}

#[derive(Debug, Args)]
struct ReadArgs {
    /// Conversation ID.
    #[arg(value_name = "CONVERSATION-ID")]
    id: String,
}

impl ConvCmd {
    pub async fn run(&self, client: &Client) -> Result<Output> {
        match &self.command {
            ConvSub::List(args) => list(client, args).await,
            ConvSub::Messages(args) => messages(client, args).await,
            ConvSub::Reply(args) => reply(client, args).await,
            ConvSub::Read(args) => {
                let value =
                    client.send(Request::post(format!("/conversations/{}/read", args.id))).await?;
                Ok(Output::new(value, View::ConversationDetail))
            }
        }
    }
}

async fn list(client: &Client, args: &ListArgs) -> Result<Output> {
    if let Some(search) = &args.search {
        if search.chars().count() < 2 {
            return Err(crate::client::Error::usage(
                "--search needs at least 2 characters.",
            ));
        }
    }

    let req = Request::get("/conversations")
        .query_opt("updatedAfter", args.updated_after.clone())
        .query_opt("creatorId", args.creator_id.clone())
        .query_opt("q", args.search.clone())
        .query_opt("isUnread", tristate(args.unread, args.no_unread).map(|b| b.to_string()))
        .query_opt("isArchived", tristate(args.archived, args.no_archived).map(|b| b.to_string()))
        .query_opt("isBlocked", tristate(args.blocked, args.no_blocked).map(|b| b.to_string()))
        .query_opt("include", csv(&args.include))
        .query_opt("limit", args.page.limit.map(|l| l.to_string()))
        .query_opt("cursor", args.page.cursor.clone());

    let value = paginate::collect(client, req, args.page.paginate).await?;
    Ok(Output::new(value, View::Conversations))
}

async fn messages(client: &Client, args: &MessagesArgs) -> Result<Output> {
    let req = Request::get(format!("/conversations/{}/messages", args.id))
        .query_opt("createdAfter", args.created_after.clone())
        .query_opt("limit", args.page.limit.map(|l| l.to_string()))
        .query_opt("cursor", args.page.cursor.clone());

    let mut value = paginate::collect(client, req, args.page.paginate).await?;
    let mut note = None;

    if let Some(items) = value.get_mut("data").and_then(Value::as_array_mut) {
        if !args.types.is_empty() {
            let before = items.len();
            let present: Vec<String> = items
                .iter()
                .filter_map(|m| m.get("type")?.as_str().map(str::to_owned))
                .collect();
            items.retain(|m| {
                m.get("type")
                    .and_then(Value::as_str)
                    .is_some_and(|t| args.types.iter().any(|want| want == t))
            });
            // An unknown --type silently returns nothing otherwise, and the API
            // adds types over time, so name what was actually there.
            if items.is_empty() && before > 0 {
                let mut kinds: Vec<&str> = present.iter().map(String::as_str).collect();
                kinds.sort_unstable();
                kinds.dedup();
                note = Some(format!(
                    "No messages matched --type. Types present in this page: {}",
                    kinds.join(", ")
                ));
            }
        }
        if args.reverse {
            items.reverse();
        }
    }

    let out = Output::new(value, View::Messages);
    Ok(match note {
        Some(n) => out.note(n),
        None => out,
    })
}

async fn reply(client: &Client, args: &ReplyArgs) -> Result<Output> {
    let text = args.text.resolve()?;
    let req = Request::post(format!("/conversations/{}/messages", args.id))
        .body(json!({ "text": text }))
        .idempotent(args.idempotency_key.clone());

    let value = client.send(req).await?;
    Ok(Output::new(value, View::Object))
}
