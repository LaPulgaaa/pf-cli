use clap::{Args, Subcommand, ValueEnum};

use super::common::{PageArgs, Status, csv};
use crate::client::{Client, Request, Result, paginate};
use crate::output::{Output, View};

#[derive(Debug, Args)]
pub struct CollabCmd {
    #[command(subcommand)]
    command: CollabSub,
}

#[derive(Debug, Subcommand)]
enum CollabSub {
    /// List collaborations.
    #[command(visible_alias = "ls")]
    List(ListArgs),

    /// Retrieve a single collaboration.
    #[command(visible_alias = "view")]
    Get(GetArgs),
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum Include {
    #[value(name = "campaign")]
    Campaign,
}

#[derive(Debug, Args)]
struct ListArgs {
    /// Filter by status. Repeatable, or comma-separated.
    #[arg(long, value_enum, value_delimiter = ',', num_args = 1..)]
    status: Vec<Status>,

    /// Only collaborations with this creator.
    #[arg(long = "creator", value_name = "ID")]
    creator_id: Option<String>,

    /// Expand related objects.
    #[arg(long, value_enum, value_delimiter = ',', num_args = 1..)]
    include: Vec<Include>,

    #[command(flatten)]
    page: PageArgs,
}

#[derive(Debug, Args)]
struct GetArgs {
    /// Collaboration ID.
    #[arg(value_name = "COLLABORATION-ID")]
    id: String,

    /// Expand related objects.
    #[arg(long, value_enum, value_delimiter = ',', num_args = 1..)]
    include: Vec<Include>,
}

impl CollabCmd {
    pub async fn run(&self, client: &Client) -> Result<Output> {
        match &self.command {
            CollabSub::List(args) => {
                let req = Request::get("/collaborations")
                    .query_opt("status", csv(&args.status))
                    .query_opt("creatorId", args.creator_id.clone())
                    .query_opt("include", csv(&args.include))
                    .query_opt("limit", args.page.limit.map(|l| l.to_string()))
                    .query_opt("cursor", args.page.cursor.clone());

                let value = paginate::collect(client, req, args.page.paginate).await?;
                Ok(Output::new(value, View::Collaborations))
            }
            CollabSub::Get(args) => {
                let req = Request::get(format!("/collaborations/{}", args.id))
                    .query_opt("include", csv(&args.include));
                let value = client.send(req).await?;
                Ok(Output::new(value, View::CollaborationDetail))
            }
        }
    }
}
