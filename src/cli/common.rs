use std::io::{IsTerminal, Read};
use std::path::PathBuf;

use clap::{Args, ValueEnum};

use crate::client::{Error, Result};

/// The simplified status vocabulary shared by placements and collaborations.
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum Status {
    Pending,
    Confirmed,
    Cancelled,
}

#[derive(Debug, Args, Clone)]
pub struct PageArgs {
    /// Results per page.
    #[arg(long, value_name = "N")]
    pub limit: Option<u32>,

    /// Resume from a cursor returned by an earlier call.
    ///
    /// Cursors are opaque and single-origin: one the API did not issue is
    /// rejected with a 400 rather than silently restarting at page one.
    #[arg(long, value_name = "CURSOR")]
    pub cursor: Option<String>,

    /// Follow cursors to the end and emit every page as one result.
    #[arg(long)]
    pub paginate: bool,
}

/// Message body, from a flag, a file, or a pipe.
#[derive(Debug, Args, Clone)]
pub struct TextInput {
    /// Message text. Basic HTML (`<p>`, `<br>`, links) renders; everything else
    /// is escaped and shown literally. Sent verbatim -- a bare newline is not a
    /// line break.
    #[arg(long, value_name = "TEXT", conflicts_with = "text_file")]
    pub text: Option<String>,

    /// Read the message text from a file, or from stdin with `-`.
    #[arg(long = "text-file", value_name = "PATH")]
    pub text_file: Option<PathBuf>,
}

const MAX_TEXT_CHARS: usize = 10_000;

impl TextInput {
    /// Resolve the body, falling back to piped stdin when neither flag is given
    /// so `echo ... | pf conv reply <id>` works without ceremony.
    pub fn resolve(&self) -> Result<String> {
        let raw = if let Some(text) = &self.text {
            text.clone()
        } else if let Some(path) = &self.text_file {
            if path.as_os_str() == "-" {
                read_stdin()?
            } else {
                std::fs::read_to_string(path).map_err(|e| {
                    Error::usage(format!("Could not read {}: {e}", path.display()))
                })?
            }
        } else if !std::io::stdin().is_terminal() {
            read_stdin()?
        } else {
            return Err(Error::usage(
                "No message text. Pass --text, --text-file <path>, or pipe the text on stdin.",
            ));
        };

        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err(Error::usage("Message text is empty."));
        }

        // Checked here so an over-long body fails instantly instead of spending
        // one of the two requests per second the API allows.
        let chars = trimmed.chars().count();
        if chars > MAX_TEXT_CHARS {
            return Err(Error::usage(format!(
                "Message text is {chars} characters; the API accepts at most {MAX_TEXT_CHARS}."
            )));
        }

        Ok(trimmed.to_string())
    }
}

fn read_stdin() -> Result<String> {
    let mut buf = String::new();
    std::io::stdin()
        .read_to_string(&mut buf)
        .map_err(|e| Error::usage(format!("Could not read stdin: {e}")))?;
    Ok(buf)
}

/// Join `ValueEnum` selections into the comma-separated form the API expects.
///
/// The variant names are the API's own values, so this cannot drift from what
/// the flag accepts.
pub fn csv<T: ValueEnum>(values: &[T]) -> Option<String> {
    if values.is_empty() {
        return None;
    }
    let joined = values
        .iter()
        .filter_map(|v| v.to_possible_value())
        .map(|p| p.get_name().to_string())
        .collect::<Vec<_>>()
        .join(",");
    Some(joined)
}

/// Collapse a `--flag` / `--no-flag` pair into the tri-state the API wants:
/// omitting the parameter is meaningfully different from sending `false`.
pub fn tristate(yes: bool, no: bool) -> Option<bool> {
    match (yes, no) {
        (true, false) => Some(true),
        (false, true) => Some(false),
        _ => None,
    }
}
