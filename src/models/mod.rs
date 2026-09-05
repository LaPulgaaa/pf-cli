//! Serde mirrors of the documented API objects.
//!
//! These exist purely to render tables. `--json` always emits the upstream
//! response byte-for-byte, so nothing here can drop a field the API adds. Every
//! field is optional and unknown keys are ignored, because the public API is in
//! alpha and is documented as changing without notice -- a new field must never
//! turn into a parse failure on a read the user asked for.

use serde::Deserialize;

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Placement {
    pub id: Option<String>,
    pub creator_id: Option<String>,
    pub creator_name: Option<String>,
    pub collaboration_id: Option<String>,
    pub date: Option<String>,
    pub name: Option<String>,
    pub status: Option<String>,
    pub platform: Option<String>,
    pub post_type: Option<String>,
    pub url: Option<String>,
    pub view_count: Option<i64>,
    pub like_count: Option<i64>,
    pub comment_count: Option<i64>,
    pub share_count: Option<i64>,
    pub metrics_last_updated_at: Option<String>,
    pub price_cents: Option<i64>,
    pub usd_price_cents: Option<i64>,
    pub currency: Option<String>,
    pub creator: Option<Creator>,
    pub collaboration: Option<Collaboration>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Creator {
    pub id: Option<String>,
    pub display_name: Option<String>,
    pub country: Option<String>,
    pub note: Option<String>,
    pub labels: Vec<Label>,
    pub channels: Vec<Channel>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Label {
    pub id: Option<String>,
    pub name: Option<String>,
    pub color: Option<String>,
    pub position: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Channel {
    pub id: Option<String>,
    pub title: Option<String>,
    pub url: Option<String>,
    pub platform_type: Option<String>,
    pub metrics: Option<Metrics>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Metrics {
    pub reach: Option<i64>,
    pub views: Option<i64>,
    pub engagement: Option<i64>,
    pub engagement_rate: Option<f64>,
    pub metrics_updated_at: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Collaboration {
    pub id: Option<String>,
    pub name: Option<String>,
    pub creator_id: Option<String>,
    pub campaign_id: Option<String>,
    pub status: Option<String>,
    pub price_cents: Option<i64>,
    pub usd_price_cents: Option<i64>,
    pub currency: Option<String>,
    pub created_at: Option<String>,
    pub status_updated_at: Option<String>,
    pub campaign: Option<Campaign>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Campaign {
    pub id: Option<String>,
    pub name: Option<String>,
    pub description: Option<String>,
    pub budget_amount_cents: Option<i64>,
    pub budget_currency: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Conversation {
    pub id: Option<String>,
    pub creator_id: Option<String>,
    pub is_unread: Option<bool>,
    pub is_archived: Option<bool>,
    pub is_blocked: Option<bool>,
    pub last_activity_at: Option<String>,
    pub created_at: Option<String>,
    pub creator: Option<Creator>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Message {
    pub id: Option<String>,
    #[serde(rename = "type")]
    pub kind: Option<String>,
    pub actor: Option<String>,
    pub collaboration_id: Option<String>,
    pub created_at: Option<String>,
    pub text: Option<String>,
    pub comment: Option<String>,
    pub sender: Option<Sender>,
    pub attachments: Vec<Attachment>,
    pub proposal: Option<Proposal>,
    pub stats: Option<Stats>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Sender {
    pub id: Option<String>,
    pub name: Option<String>,
    pub email: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Attachment {
    pub file_name: Option<String>,
    pub file_size: Option<i64>,
    pub mime_type: Option<String>,
    pub download_url: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Proposal {
    pub proposal_id: Option<String>,
    pub collaboration_id: Option<String>,
    pub request_id: Option<String>,
    pub status: Option<String>,
    pub created_by: Option<String>,
    pub price_cents: Option<i64>,
    pub currency: Option<String>,
    pub package_name: Option<String>,
    pub comment: Option<String>,
    pub placements: Vec<ProposalPlacement>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ProposalPlacement {
    pub date: Option<String>,
    pub post_type: Option<String>,
    pub platform: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Stats {
    pub view_count: Option<i64>,
    pub like_count: Option<i64>,
    pub comment_count: Option<i64>,
    pub share_count: Option<i64>,
    pub content_url: Option<String>,
    pub content_title: Option<String>,
    pub platform: Option<String>,
}
