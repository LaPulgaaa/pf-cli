use clap::Args;

use super::common::PageArgs;
use crate::client::{Client, Request, Result, paginate};
use crate::output::fmt::parse_datetime_arg;
use crate::output::{Output, View};

/// Sugar over `conv list`, for the one query an agent working a queue actually
/// wants: what still needs answering, with creator names already attached.
#[derive(Debug, Args)]
pub struct InboxCmd {
    /// Only conversations with activity on or after this point.
    #[arg(long, value_name = "WHEN", value_parser = parse_datetime_arg)]
    updated_after: Option<String>,

    /// Include archived and blocked conversations, which are hidden by default.
    #[arg(long)]
    all: bool,

    #[command(flatten)]
    page: PageArgs,
}

impl InboxCmd {
    pub async fn run(&self, client: &Client) -> Result<Output> {
        let mut req = Request::get("/conversations")
            .query("isUnread", "true")
            .query("include", "creator")
            .query_opt("updatedAfter", self.updated_after.clone())
            .query_opt("limit", self.page.limit.map(|l| l.to_string()))
            .query_opt("cursor", self.page.cursor.clone());

        // An inbox is what is left to handle; archived and blocked threads are
        // deliberately not that. `conv list` still shows them by default.
        if !self.all {
            req = req.query("isArchived", "false").query("isBlocked", "false");
        }

        let value = paginate::collect(client, req, self.page.paginate).await?;
        Ok(Output::new(value, View::Conversations))
    }
}
