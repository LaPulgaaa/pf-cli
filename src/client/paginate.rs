use serde_json::{Value, json};

use super::{Client, Error, Request, Result};

/// Run a list request, optionally walking the cursor to exhaustion.
///
/// Cursor handling is deliberately hidden from callers: the API treats cursors
/// as opaque and rejects any it did not issue with a `400` rather than quietly
/// restarting at page one, so a cursor must never be constructed, cached across
/// runs, or replayed into a retry.
pub async fn collect(client: &Client, req: Request, paginate: bool) -> Result<Value> {
    if !paginate {
        return client.send(req).await;
    }

    let mut merged: Vec<Value> = Vec::new();
    let mut cursor: Option<String> = None;
    let mut pages = 0usize;

    loop {
        let mut page_req = req.clone();
        // The caller may have supplied a starting cursor; ours replaces it on
        // every subsequent page.
        if let Some(c) = &cursor {
            page_req.query.retain(|(k, _)| k != "cursor");
            page_req = page_req.query("cursor", c.clone());
        }

        let page = client.send(page_req).await?;
        pages += 1;

        match page.get("data") {
            Some(Value::Array(items)) => merged.extend(items.iter().cloned()),
            // A dry run returns no data; there is nothing to walk.
            Some(Value::Null) | None => break,
            Some(other) => {
                // Not a list endpoint -- hand it back untouched rather than
                // pretending pagination applied.
                return Ok(json!({ "data": other.clone() }));
            }
        }

        let pagination = page.get("pagination");
        let has_more = pagination.and_then(|p| p.get("hasMore")).and_then(Value::as_bool);
        let next = pagination
            .and_then(|p| p.get("nextCursor"))
            .and_then(Value::as_str)
            .map(str::to_owned);

        if has_more != Some(true) {
            break;
        }

        match next {
            // `hasMore` without a cursor would loop forever on page one.
            None => {
                return Err(Error::other(
                    "The API reported more pages but returned no nextCursor.".to_string(),
                )
                .with_hint("Re-run without --paginate and page manually with --cursor."));
            }
            Some(next) if Some(&next) == cursor.as_ref() => {
                return Err(Error::other(format!(
                    "Pagination stalled: the API returned the same cursor twice after {pages} pages."
                )));
            }
            Some(next) => cursor = Some(next),
        }
    }

    Ok(json!({
        "data": merged,
        "pagination": { "nextCursor": Value::Null, "hasMore": false },
    }))
}
