use clap::{Args, Subcommand, ValueEnum};
use serde_json::{Value, json};

use super::common::{PageArgs, TextInput, csv};
use super::label;
use crate::client::{Client, Error, Request, Result, paginate};
use crate::output::{Output, View};

#[derive(Debug, Args)]
pub struct CreatorCmd {
    #[command(subcommand)]
    command: CreatorSub,
}

#[derive(Debug, Subcommand)]
enum CreatorSub {
    /// List creators your organization can see.
    #[command(visible_alias = "ls")]
    List(ListArgs),

    /// Retrieve a single creator.
    #[command(visible_alias = "view")]
    Get(GetArgs),

    /// Message a creator, whether or not you have talked before.
    ///
    /// Looks up the conversation with this creator -- there is at most one --
    /// and sends into it. When none exists yet, sends an inquiry instead, which
    /// creates the conversation. Which branch ran is reported on stderr, and as
    /// an `action` field under --json.
    Message(MessageArgs),
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum Include {
    #[value(name = "channels")]
    Channels,
}

#[derive(Debug, Args)]
struct ListArgs {
    /// Filter to creators carrying any of these labels, by ID or by name.
    ///
    /// Names are resolved against the label catalog, which costs one extra
    /// request; pass IDs to skip it.
    #[arg(long = "label", value_name = "ID|NAME")]
    labels: Vec<String>,

    /// Expand related objects.
    #[arg(long, value_enum, value_delimiter = ',', num_args = 1..)]
    include: Vec<Include>,

    #[command(flatten)]
    page: PageArgs,
}

#[derive(Debug, Args)]
struct GetArgs {
    /// Creator ID.
    #[arg(value_name = "CREATOR-ID")]
    id: String,

    /// Expand related objects.
    #[arg(long, value_enum, value_delimiter = ',', num_args = 1..)]
    include: Vec<Include>,
}

#[derive(Debug, Args)]
struct MessageArgs {
    /// Creator ID.
    #[arg(value_name = "CREATOR-ID")]
    id: String,

    #[command(flatten)]
    text: TextInput,

    /// Link the new collaboration to a campaign. Only valid when this becomes
    /// an inquiry -- an existing conversation cannot be reassigned.
    #[arg(long = "campaign", value_name = "ID")]
    campaign_id: Option<String>,

    /// Reuse a key so a retry cannot send twice.
    ///
    /// pf generates one per invocation, which makes its own retries safe. It
    /// does nothing across separate `pf` runs: pass your own key to make
    /// re-running this command idempotent.
    #[arg(long = "idempotency-key", value_name = "KEY")]
    idempotency_key: Option<String>,

    /// Send an inquiry without looking for an existing conversation.
    #[arg(long, conflicts_with = "require_existing")]
    force_inquiry: bool,

    /// Fail rather than falling back to an inquiry when no conversation exists.
    #[arg(long)]
    require_existing: bool,
}

impl CreatorCmd {
    pub async fn run(&self, client: &Client) -> Result<Output> {
        match &self.command {
            CreatorSub::List(args) => {
                let label_ids = label::resolve_ids(client, &args.labels).await?;
                let req = Request::get("/creators")
                    .query_opt(
                        "labelIds",
                        (!label_ids.is_empty()).then(|| label_ids.join(",")),
                    )
                    .query_opt("include", csv(&args.include))
                    .query_opt("limit", args.page.limit.map(|l| l.to_string()))
                    .query_opt("cursor", args.page.cursor.clone());

                let value = paginate::collect(client, req, args.page.paginate).await?;
                Ok(Output::new(value, View::Creators))
            }
            CreatorSub::Get(args) => {
                let req = Request::get(format!("/creators/{}", args.id))
                    .query_opt("include", csv(&args.include));
                let value = client.send(req).await?;
                Ok(Output::new(value, View::CreatorDetail))
            }
            CreatorSub::Message(args) => message(client, args).await,
        }
    }
}

async fn message(client: &Client, args: &MessageArgs) -> Result<Output> {
    let text = args.text.resolve()?;

    if client.is_dry_run() && !args.force_inquiry {
        return dry_run_message(client, args, &text);
    }

    let existing = if args.force_inquiry {
        None
    } else {
        find_conversation(client, &args.id).await?
    };

    match existing {
        Some(conversation_id) => {
            // A campaign link is set when the collaboration is created, which
            // already happened for an existing conversation. Failing here beats
            // dropping the argument silently.
            if args.campaign_id.is_some() {
                return Err(Error::usage(format!(
                    "--campaign cannot be applied to the existing conversation {conversation_id}."
                ))
                .with_hint(
                    "A campaign is linked when the inquiry creates the collaboration. Send this \
                     message without --campaign, or use --force-inquiry to open a new one.",
                ));
            }

            let req = Request::post(format!("/conversations/{conversation_id}/messages"))
                .body(json!({ "text": text }))
                .idempotent(args.idempotency_key.clone());
            let value = client.send(req).await?;

            Ok(Output::new(
                merge_action(value, "message", Some(&conversation_id)),
                View::Object,
            )
            .note(format!(
                "\u{2192} sent into the existing conversation {conversation_id}"
            )))
        }
        None => {
            if args.require_existing {
                return Err(Error {
                    code: "not_found",
                    status: None,
                    message: format!("No conversation exists with creator {}.", args.id),
                    hint: Some(
                        "Drop --require-existing to open one with an inquiry instead.".into(),
                    ),
                    exit: crate::client::exit::NOT_FOUND,
                });
            }

            let mut body = json!({ "creatorId": args.id, "text": text });
            if let Some(campaign) = &args.campaign_id {
                body["campaignId"] = json!(campaign);
            }
            let req = Request::post("/inquiries")
                .body(body)
                .idempotent(args.idempotency_key.clone());
            let value = client.send(req).await?;

            let created = value
                .get("data")
                .and_then(|d| d.get("conversationId"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();

            Ok(Output::new(merge_action(value, "inquiry", None), View::Object).note(format!(
                "\u{2192} no conversation with this creator yet; sent as an inquiry, opening {created}"
            )))
        }
    }
}

/// Under --dry-run the lookup does not run, so the branch is genuinely unknown.
/// Show the lookup and both possible follow-ups rather than guessing one.
fn dry_run_message(client: &Client, args: &MessageArgs, text: &str) -> Result<Output> {
    let lookup = Request::get("/conversations")
        .query("creatorId", args.id.clone())
        .query("limit", "1");
    client.record_dry_run(client.describe(
        &lookup,
        Some("step 1: decides which of the two branches below runs"),
    ));

    if !args.force_inquiry {
        let send = Request::post("/conversations/{conversationId}/messages")
            .body(json!({ "text": text }))
            .idempotent(args.idempotency_key.clone());
        client.record_dry_run(client.describe(
            &send,
            Some("step 2a: if a conversation with this creator already exists"),
        ));
    }

    if !args.require_existing {
        let mut body = json!({ "creatorId": args.id, "text": text });
        if let Some(campaign) = &args.campaign_id {
            body["campaignId"] = json!(campaign);
        }
        let inquiry = Request::post("/inquiries")
            .body(body)
            .idempotent(args.idempotency_key.clone());
        client.record_dry_run(
            client.describe(&inquiry, Some("step 2b: if there is no conversation yet")),
        );
    }

    Ok(Output::new(json!({ "data": Value::Null }), View::Object))
}

/// There is at most one conversation per creator, so a single-result lookup is
/// exact rather than a best guess.
async fn find_conversation(client: &Client, creator_id: &str) -> Result<Option<String>> {
    let req = Request::get("/conversations")
        .query("creatorId", creator_id)
        .query("limit", "1");
    let value = client.send(req).await?;
    Ok(value
        .get("data")
        .and_then(Value::as_array)
        .and_then(|items| items.first())
        .and_then(|c| c.get("id"))
        .and_then(Value::as_str)
        .map(str::to_owned))
}

/// Annotate a composite command's result with the branch it took. The upstream
/// `data` is left exactly as returned.
fn merge_action(mut value: Value, action: &str, conversation_id: Option<&str>) -> Value {
    if let Some(obj) = value.as_object_mut() {
        obj.insert("action".into(), json!(action));
        if let Some(id) = conversation_id {
            obj.insert("conversationId".into(), json!(id));
        }
    }
    value
}
