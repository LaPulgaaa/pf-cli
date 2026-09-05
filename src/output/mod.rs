pub mod fmt;
pub mod table;

use serde_json::Value;

use crate::models::*;
use table::{Cell, Table, detail, status_style};

/// How a command's payload should be drawn on a terminal. Irrelevant under
/// `--json`, which always emits the upstream response untouched.
#[derive(Debug, Clone, Copy)]
pub enum View {
    Placements,
    Creators,
    CreatorDetail,
    Labels,
    Collaborations,
    CollaborationDetail,
    Conversations,
    ConversationDetail,
    Messages,
    ProposalDetail,
    /// Generic key/value rendering of whatever object came back.
    Object,
    /// Already-formatted JSON with no table equivalent (`pf api`).
    Raw,
    /// Nothing to draw: the command's stderr note is the whole human answer.
    Silent,
}

pub struct Output {
    pub value: Value,
    pub view: View,
    /// A human-only aside, printed to stderr so it never contaminates stdout.
    pub note: Option<String>,
}

impl Output {
    pub fn new(value: Value, view: View) -> Self {
        Self { value, view, note: None }
    }

    pub fn note(mut self, note: impl Into<String>) -> Self {
        self.note = Some(note.into());
        self
    }
}

pub struct Printer {
    pub json: bool,
    pub color: bool,
    pub quiet: bool,
}

impl Printer {
    pub fn emit(&self, out: &Output) {
        if let Some(note) = &out.note {
            if !self.quiet {
                eprintln!("{note}");
            }
        }

        if self.json {
            // Pretty-printed, but still the upstream document: no field is
            // renamed, dropped, or reformatted on the way through.
            match serde_json::to_string_pretty(&out.value) {
                Ok(s) => println!("{s}"),
                Err(_) => println!("{}", out.value),
            }
            return;
        }

        let data = out.value.get("data").unwrap_or(&Value::Null);
        match out.view {
            View::Placements => self.placements(data),
            View::Creators => self.creators(data),
            View::CreatorDetail => self.creator_detail(data),
            View::Labels => self.labels(data),
            View::Collaborations => self.collaborations(data),
            View::CollaborationDetail => self.collaboration_detail(data),
            View::Conversations => self.conversations(data),
            View::ConversationDetail => self.conversation_detail(data),
            View::Messages => self.messages(data),
            View::ProposalDetail => self.proposal_detail(data),
            View::Object => self.object(data),
            View::Raw => self.raw(&out.value),
            View::Silent => {}
        }

        if let Some(pagination) = out.value.get("pagination") {
            self.pagination_hint(pagination);
        }
    }

    /// A cursor is only useful if the caller is told it exists.
    fn pagination_hint(&self, pagination: &Value) {
        if self.quiet || pagination.get("hasMore").and_then(Value::as_bool) != Some(true) {
            return;
        }
        let cursor = pagination.get("nextCursor").and_then(Value::as_str).unwrap_or("");
        eprintln!("\nMore results available. Re-run with --paginate, or --cursor {cursor}");
    }

    fn raw(&self, value: &Value) {
        match serde_json::to_string_pretty(value) {
            Ok(s) => println!("{s}"),
            Err(_) => println!("{value}"),
        }
    }

    fn placements(&self, data: &Value) {
        let Ok(items) = serde_json::from_value::<Vec<Placement>>(data.clone()) else {
            return self.raw(data);
        };
        let mut t = Table::new(
            &["date", "creator", "name", "platform", "type", "status", "views", "price"],
            self.color,
        )
        .max_widths(&[12, 24, 36, 12, 12, 10, 10, 14]);

        for p in &items {
            let creator = p
                .creator
                .as_ref()
                .and_then(|c| c.display_name.clone())
                .or_else(|| p.creator_name.clone())
                .or_else(|| p.creator_id.clone());
            let status = p.status.clone().unwrap_or_default();
            t.push(vec![
                Cell::plain(fmt::date(p.date.as_deref())),
                Cell::plain(creator.unwrap_or_else(|| "-".into())),
                Cell::plain(fmt::text(p.name.as_deref())),
                Cell::plain(fmt::text(p.platform.as_deref())),
                Cell::plain(fmt::text(p.post_type.as_deref())),
                Cell::styled(status.clone(), status_style(&status)),
                Cell::plain(fmt::count(p.view_count)),
                Cell::plain(fmt::money(p.price_cents, p.currency.as_deref())),
            ]);
        }
        t.print();
    }

    fn creators(&self, data: &Value) {
        let Ok(items) = serde_json::from_value::<Vec<Creator>>(data.clone()) else {
            return self.raw(data);
        };
        let mut t = Table::new(&["id", "name", "country", "labels", "channels"], self.color)
            .max_widths(&[38, 28, 8, 30, 34]);

        for c in &items {
            t.push(vec![
                Cell::plain(fmt::text(c.id.as_deref())),
                Cell::plain(fmt::text(c.display_name.as_deref())),
                Cell::plain(fmt::text(c.country.as_deref())),
                Cell::plain(join_labels(&c.labels)),
                Cell::plain(join_channels(&c.channels)),
            ]);
        }
        t.print();
    }

    fn creator_detail(&self, data: &Value) {
        let Ok(c) = serde_json::from_value::<Creator>(data.clone()) else {
            return self.raw(data);
        };
        detail(
            &[
                ("id", fmt::text(c.id.as_deref())),
                ("name", fmt::text(c.display_name.as_deref())),
                ("country", fmt::text(c.country.as_deref())),
                ("labels", join_labels(&c.labels)),
                ("note", c.note.as_deref().map(fmt::strip_html).unwrap_or_else(|| "-".into())),
            ],
            self.color,
        );

        if c.channels.is_empty() {
            return;
        }
        println!();
        let mut t = Table::new(&["channel", "platform", "reach", "views", "engagement rate"], self.color)
            .max_widths(&[34, 14, 12, 12, 16]);
        for ch in &c.channels {
            let m = ch.metrics.as_ref();
            t.push(vec![
                Cell::plain(fmt::text(ch.title.as_deref())),
                Cell::plain(fmt::text(ch.platform_type.as_deref())),
                Cell::plain(fmt::count(m.and_then(|m| m.reach))),
                Cell::plain(fmt::count(m.and_then(|m| m.views))),
                Cell::plain(
                    m.and_then(|m| m.engagement_rate)
                        .map(|r| format!("{r:.2}%"))
                        .unwrap_or_else(|| "-".into()),
                ),
            ]);
        }
        t.print();
    }

    fn labels(&self, data: &Value) {
        let Ok(items) = serde_json::from_value::<Vec<Label>>(data.clone()) else {
            return self.raw(data);
        };
        let mut t = Table::new(&["id", "name", "color"], self.color).max_widths(&[38, 40, 12]);
        for l in &items {
            t.push(vec![
                Cell::plain(fmt::text(l.id.as_deref())),
                Cell::plain(fmt::text(l.name.as_deref())),
                Cell::plain(fmt::text(l.color.as_deref())),
            ]);
        }
        t.print();
    }

    fn collaborations(&self, data: &Value) {
        let Ok(items) = serde_json::from_value::<Vec<Collaboration>>(data.clone()) else {
            return self.raw(data);
        };
        let mut t = Table::new(&["id", "name", "status", "price", "created"], self.color)
            .max_widths(&[38, 40, 10, 14, 16]);
        for c in &items {
            let status = c.status.clone().unwrap_or_default();
            t.push(vec![
                Cell::plain(fmt::text(c.id.as_deref())),
                Cell::plain(fmt::text(c.name.as_deref())),
                Cell::styled(status.clone(), status_style(&status)),
                Cell::plain(fmt::money(c.price_cents, c.currency.as_deref())),
                Cell::plain(fmt::datetime(c.created_at.as_deref())),
            ]);
        }
        t.print();
    }

    fn collaboration_detail(&self, data: &Value) {
        let Ok(c) = serde_json::from_value::<Collaboration>(data.clone()) else {
            return self.raw(data);
        };
        let mut rows = vec![
            ("id", fmt::text(c.id.as_deref())),
            ("name", fmt::text(c.name.as_deref())),
            ("creator", fmt::text(c.creator_id.as_deref())),
            ("status", fmt::text(c.status.as_deref())),
            ("price", fmt::money(c.price_cents, c.currency.as_deref())),
            ("price (usd)", fmt::money(c.usd_price_cents, Some("USD"))),
            ("created", fmt::datetime(c.created_at.as_deref())),
            ("status updated", fmt::datetime(c.status_updated_at.as_deref())),
        ];
        if let Some(campaign) = &c.campaign {
            rows.push(("campaign", fmt::text(campaign.name.as_deref())));
            rows.push((
                "campaign budget",
                fmt::money(campaign.budget_amount_cents, campaign.budget_currency.as_deref()),
            ));
        }
        detail(&rows, self.color);
    }

    fn conversations(&self, data: &Value) {
        let Ok(items) = serde_json::from_value::<Vec<Conversation>>(data.clone()) else {
            return self.raw(data);
        };
        let mut t = Table::new(&["id", "creator", "state", "last activity"], self.color)
            .max_widths(&[38, 30, 24, 16]);

        for c in &items {
            let creator = c
                .creator
                .as_ref()
                .and_then(|cr| cr.display_name.clone())
                .or_else(|| c.creator_id.clone());

            let mut state = Vec::new();
            if c.is_unread == Some(true) {
                state.push("unread");
            }
            if c.is_archived == Some(true) {
                state.push("archived");
            }
            if c.is_blocked == Some(true) {
                state.push("blocked");
            }
            let unread = c.is_unread == Some(true);
            let label = if state.is_empty() { "read".to_string() } else { state.join(", ") };

            t.push(vec![
                Cell::plain(fmt::text(c.id.as_deref())),
                Cell::plain(creator.unwrap_or_else(|| "-".into())),
                if unread {
                    Cell::styled(label, table::BOLD)
                } else {
                    Cell::styled(label, table::DIM)
                },
                Cell::plain(fmt::datetime(c.last_activity_at.as_deref())),
            ]);
        }
        t.print();
    }

    fn conversation_detail(&self, data: &Value) {
        let Ok(c) = serde_json::from_value::<Conversation>(data.clone()) else {
            return self.raw(data);
        };
        let mut state = Vec::new();
        if c.is_unread == Some(true) {
            state.push("unread");
        }
        if c.is_archived == Some(true) {
            state.push("archived");
        }
        if c.is_blocked == Some(true) {
            state.push("blocked");
        }
        detail(
            &[
                ("id", fmt::text(c.id.as_deref())),
                ("creator", fmt::text(c.creator_id.as_deref())),
                ("state", if state.is_empty() { "read".into() } else { state.join(", ") }),
                ("last activity", fmt::datetime(c.last_activity_at.as_deref())),
                ("created", fmt::datetime(c.created_at.as_deref())),
            ],
            self.color,
        );
    }

    fn messages(&self, data: &Value) {
        let Ok(items) = serde_json::from_value::<Vec<Message>>(data.clone()) else {
            return self.raw(data);
        };
        let mut t = Table::new(&["time", "actor", "type", "summary"], self.color)
            .max_widths(&[16, 8, 34, 72]);

        for m in &items {
            let actor = m.actor.clone().unwrap_or_default();
            let actor_style = match actor.as_str() {
                "creator" => table::CYAN,
                "brand" => table::GREEN,
                _ => table::DIM,
            };
            t.push(vec![
                Cell::plain(fmt::datetime(m.created_at.as_deref())),
                Cell::styled(actor, actor_style),
                Cell::plain(m.kind.clone().unwrap_or_else(|| "-".into())),
                Cell::plain(summarize(m)),
            ]);
        }
        t.print();
    }

    fn proposal_detail(&self, data: &Value) {
        let Ok(p) = serde_json::from_value::<Proposal>(data.clone()) else {
            return self.raw(data);
        };
        detail(
            &[
                ("proposal", fmt::text(p.proposal_id.as_deref())),
                ("status", fmt::text(p.status.as_deref())),
                ("authored by", fmt::text(p.created_by.as_deref())),
                ("package", fmt::text(p.package_name.as_deref())),
                ("price", fmt::money(p.price_cents, p.currency.as_deref())),
                ("collaboration", fmt::text(p.collaboration_id.as_deref())),
                ("request", fmt::text(p.request_id.as_deref())),
            ],
            self.color,
        );

        if p.placements.is_empty() {
            return;
        }
        println!();
        let mut t = Table::new(&["date", "platform", "type"], self.color);
        for pl in &p.placements {
            t.push(vec![
                Cell::plain(fmt::date(pl.date.as_deref())),
                Cell::plain(fmt::text(pl.platform.as_deref())),
                Cell::plain(fmt::text(pl.post_type.as_deref())),
            ]);
        }
        t.print();
    }

    /// Fallback for small write responses: print the object's own keys rather
    /// than inventing a schema for each one.
    fn object(&self, data: &Value) {
        let Some(map) = data.as_object() else {
            return self.raw(data);
        };
        let rows: Vec<(String, String)> = map
            .iter()
            .map(|(k, v)| {
                let rendered = match v {
                    Value::String(s) => s.clone(),
                    Value::Null => "-".to_string(),
                    other => other.to_string(),
                };
                (humanize_key(k), rendered)
            })
            .collect();
        let borrowed: Vec<(&str, String)> =
            rows.iter().map(|(k, v)| (k.as_str(), v.clone())).collect();
        detail(&borrowed, self.color);
    }
}

fn join_labels(labels: &[Label]) -> String {
    if labels.is_empty() {
        return "-".into();
    }
    labels.iter().filter_map(|l| l.name.clone()).collect::<Vec<_>>().join(", ")
}

fn join_channels(channels: &[Channel]) -> String {
    if channels.is_empty() {
        return "-".into();
    }
    channels.iter().filter_map(|c| c.platform_type.clone()).collect::<Vec<_>>().join(", ")
}

/// One line describing a timeline entry, chosen by which payload the type carries.
fn summarize(m: &Message) -> String {
    if let Some(text) = &m.text {
        let body = fmt::strip_html(text);
        if !body.is_empty() {
            let attachments = match m.attachments.len() {
                0 => String::new(),
                n => format!(" [{n} attachment{}]", if n == 1 { "" } else { "s" }),
            };
            return format!("{body}{attachments}");
        }
    }
    if let Some(p) = &m.proposal {
        let price = fmt::money(p.price_cents, p.currency.as_deref());
        let package = p.package_name.clone().unwrap_or_else(|| "proposal".into());
        let status = p.status.clone().unwrap_or_default();
        let comment = p
            .comment
            .as_deref()
            .filter(|c| !c.is_empty())
            .map(|c| format!(" -- {}", fmt::strip_html(c)))
            .unwrap_or_default();
        return format!("{package} at {price} (now {status}){comment}");
    }
    if let Some(s) = &m.stats {
        let title = s
            .content_title
            .clone()
            .or_else(|| s.content_url.clone())
            .unwrap_or_else(|| "results".into());
        return format!("{title} -- {} views", fmt::count(s.view_count));
    }
    if let Some(comment) = &m.comment {
        return fmt::strip_html(comment);
    }
    if !m.attachments.is_empty() {
        let names: Vec<String> =
            m.attachments.iter().filter_map(|a| a.file_name.clone()).collect();
        return names.join(", ");
    }
    "-".into()
}

/// `conversationId` -> `conversation id`
fn humanize_key(key: &str) -> String {
    let mut out = String::with_capacity(key.len() + 4);
    for (i, c) in key.chars().enumerate() {
        if c.is_ascii_uppercase() && i > 0 {
            out.push(' ');
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn humanizes_camel_case_keys() {
        assert_eq!(humanize_key("conversationId"), "conversation id");
        assert_eq!(humanize_key("id"), "id");
    }

    #[test]
    fn summarizes_a_text_message() {
        let m = Message {
            text: Some("<p>Here is the draft</p>".into()),
            attachments: vec![Attachment { file_name: Some("a.pdf".into()), ..Default::default() }],
            ..Default::default()
        };
        assert_eq!(summarize(&m), "Here is the draft [1 attachment]");
    }
}

impl Printer {
    /// Render the requests `--dry-run` withheld.
    pub fn emit_dry_run(&self, entries: &[Value]) {
        if self.json {
            let doc = serde_json::json!({ "dryRun": entries });
            match serde_json::to_string_pretty(&doc) {
                Ok(s) => println!("{s}"),
                Err(_) => println!("{doc}"),
            }
            return;
        }

        for (i, entry) in entries.iter().enumerate() {
            if i > 0 {
                println!();
            }
            if let Some(note) = entry.get("note").and_then(Value::as_str) {
                if self.color {
                    println!("{}# {note}{}", table::DIM, table::RESET);
                } else {
                    println!("# {note}");
                }
            }

            let method = entry.get("method").and_then(Value::as_str).unwrap_or("GET");
            let url = entry.get("url").and_then(Value::as_str).unwrap_or("");
            if self.color {
                println!("{}{method}{} {url}", table::BOLD, table::RESET);
            } else {
                println!("{method} {url}");
            }

            for (name, value) in entry.get("headers").and_then(Value::as_object).into_iter().flatten()
            {
                let rendered = value.as_str().unwrap_or_default();
                if self.color {
                    println!("  {}{name}:{} {rendered}", table::DIM, table::RESET);
                } else {
                    println!("  {name}: {rendered}");
                }
            }

            if let Some(body) = entry.get("body") {
                println!();
                match serde_json::to_string_pretty(body) {
                    Ok(s) => {
                        for line in s.lines() {
                            println!("  {line}");
                        }
                    }
                    Err(_) => println!("  {body}"),
                }
            }
        }

        if !self.quiet {
            eprintln!(
                "\nDry run: {} request{} shown, none sent.",
                entries.len(),
                if entries.len() == 1 { "" } else { "s" }
            );
        }
    }

    /// Errors go to stderr in the same shape the caller asked for output in, so
    /// a JSON consumer never has to parse prose.
    pub fn emit_error(&self, err: &crate::client::Error) {
        if self.json {
            let mut body = serde_json::Map::new();
            body.insert("code".into(), serde_json::json!(err.code));
            if let Some(status) = err.status {
                body.insert("status".into(), serde_json::json!(status));
            }
            body.insert("message".into(), serde_json::json!(err.message));
            if let Some(hint) = &err.hint {
                body.insert("hint".into(), serde_json::json!(hint));
            }
            let doc = serde_json::json!({ "error": Value::Object(body) });
            eprintln!("{}", serde_json::to_string_pretty(&doc).unwrap_or_else(|_| doc.to_string()));
            return;
        }

        let status = err.status.map(|s| format!(" ({s})")).unwrap_or_default();
        if self.color {
            eprintln!("{}error{}{status}: {}", table::RED, table::RESET, err.message);
        } else {
            eprintln!("error{status}: {}", err.message);
        }
        if let Some(hint) = &err.hint {
            if self.color {
                eprintln!("{}hint: {hint}{}", table::DIM, table::RESET);
            } else {
                eprintln!("hint: {hint}");
            }
        }
    }
}
