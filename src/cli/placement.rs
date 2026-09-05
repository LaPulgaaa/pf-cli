use clap::{Args, Subcommand, ValueEnum};

use super::common::{PageArgs, Status, csv};
use crate::client::{Client, Request, Result, paginate};
use crate::output::fmt::{parse_date_arg, parse_datetime_arg};
use crate::output::{Output, View};

#[derive(Debug, Args)]
pub struct PlacementCmd {
    #[command(subcommand)]
    command: PlacementSub,
}

#[derive(Debug, Subcommand)]
enum PlacementSub {
    /// List placements.
    #[command(visible_alias = "ls")]
    List(ListArgs),
}

/// Related objects the placements endpoint can expand.
#[derive(Debug, Clone, Copy, ValueEnum)]
enum Include {
    #[value(name = "creator")]
    Creator,
    #[value(name = "creator.channels")]
    CreatorChannels,
    #[value(name = "collaboration")]
    Collaboration,
    #[value(name = "collaboration.campaign")]
    CollaborationCampaign,
}

#[derive(Debug, Args)]
struct ListArgs {
    /// Only placements on or after this date.
    #[arg(long, value_name = "YYYY-MM-DD", value_parser = parse_date_arg)]
    start_date: Option<String>,

    /// Only placements on or before this date.
    #[arg(long, value_name = "YYYY-MM-DD", value_parser = parse_date_arg)]
    end_date: Option<String>,

    /// Only placements whose metrics were updated on or after this point.
    #[arg(long, value_name = "WHEN", value_parser = parse_datetime_arg)]
    metrics_updated_after: Option<String>,

    /// Only placements whose metrics were updated on or before this point.
    #[arg(long, value_name = "WHEN", value_parser = parse_datetime_arg)]
    metrics_updated_before: Option<String>,

    /// Only placements belonging to this collaboration.
    #[arg(long = "collab", visible_alias = "collaboration", value_name = "ID")]
    collaboration_id: Option<String>,

    /// Filter by status. Repeatable, or comma-separated.
    #[arg(long, value_enum, value_delimiter = ',', num_args = 1..)]
    status: Vec<Status>,

    /// Expand related objects. Repeatable, or comma-separated.
    #[arg(long, value_enum, value_delimiter = ',', num_args = 1..)]
    include: Vec<Include>,

    #[command(flatten)]
    page: PageArgs,
}

impl PlacementCmd {
    pub async fn run(&self, client: &Client) -> Result<Output> {
        match &self.command {
            PlacementSub::List(args) => {
                let req = Request::get("/placements")
                    .query_opt("startDate", args.start_date.clone())
                    .query_opt("endDate", args.end_date.clone())
                    .query_opt("metricsUpdatedAfter", args.metrics_updated_after.clone())
                    .query_opt("metricsUpdatedBefore", args.metrics_updated_before.clone())
                    .query_opt("collaborationId", args.collaboration_id.clone())
                    .query_opt("status", csv(&args.status))
                    .query_opt("include", csv(&args.include))
                    .query_opt("limit", args.page.limit.map(|l| l.to_string()))
                    .query_opt("cursor", args.page.cursor.clone());

                let value = paginate::collect(client, req, args.page.paginate).await?;
                Ok(Output::new(value, View::Placements))
            }
        }
    }
}
