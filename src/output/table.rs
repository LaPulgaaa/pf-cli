//! A minimal column printer in the style of `gh`: uppercase headers, two-space
//! gutters, no box drawing. Tables are the TTY-only view -- anything piped gets
//! JSON -- so this never has to be machine-parseable.

pub const RESET: &str = "\x1b[0m";
pub const DIM: &str = "\x1b[2m";
pub const BOLD: &str = "\x1b[1m";
pub const GREEN: &str = "\x1b[32m";
pub const YELLOW: &str = "\x1b[33m";
pub const RED: &str = "\x1b[31m";
pub const CYAN: &str = "\x1b[36m";

const DEFAULT_MAX_WIDTH: usize = 44;

#[derive(Debug, Clone)]
pub struct Cell {
    pub text: String,
    pub style: Option<&'static str>,
}

impl Cell {
    pub fn plain(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            style: None,
        }
    }

    pub fn styled(text: impl Into<String>, style: &'static str) -> Self {
        Self {
            text: text.into(),
            style: Some(style),
        }
    }
}

impl From<String> for Cell {
    fn from(s: String) -> Self {
        Cell::plain(s)
    }
}

impl From<&str> for Cell {
    fn from(s: &str) -> Self {
        Cell::plain(s)
    }
}

pub struct Table {
    headers: Vec<String>,
    widths: Vec<usize>,
    rows: Vec<Vec<Cell>>,
    color: bool,
}

impl Table {
    pub fn new(headers: &[&str], color: bool) -> Self {
        Self {
            headers: headers.iter().map(|h| h.to_uppercase()).collect(),
            widths: vec![DEFAULT_MAX_WIDTH; headers.len()],
            rows: Vec::new(),
            color,
        }
    }

    /// Per-column truncation caps, for columns that hold free text.
    pub fn max_widths(mut self, widths: &[usize]) -> Self {
        self.widths = widths.to_vec();
        self
    }

    pub fn push(&mut self, row: Vec<Cell>) {
        self.rows.push(row);
    }

    pub fn print(&self) {
        if self.rows.is_empty() {
            eprintln!("No results.");
            return;
        }

        let cols = self.headers.len();
        let mut widths: Vec<usize> = self.headers.iter().map(|h| h.chars().count()).collect();

        let truncated: Vec<Vec<Cell>> = self
            .rows
            .iter()
            .map(|row| {
                row.iter()
                    .enumerate()
                    .map(|(i, cell)| {
                        let cap = self.widths.get(i).copied().unwrap_or(DEFAULT_MAX_WIDTH);
                        Cell {
                            text: super::fmt::truncate(&cell.text, cap),
                            style: cell.style,
                        }
                    })
                    .collect()
            })
            .collect();

        for row in &truncated {
            for (i, cell) in row.iter().enumerate().take(cols) {
                widths[i] = widths[i].max(cell.text.chars().count());
            }
        }

        let mut out = String::new();
        for (i, header) in self.headers.iter().enumerate() {
            let pad = if i + 1 == cols {
                0
            } else {
                widths[i] - header.chars().count() + 2
            };
            if self.color {
                out.push_str(DIM);
                out.push_str(header);
                out.push_str(RESET);
            } else {
                out.push_str(header);
            }
            out.push_str(&" ".repeat(pad));
        }
        out.push('\n');

        for row in &truncated {
            for (i, cell) in row.iter().enumerate().take(cols) {
                let pad = if i + 1 == cols {
                    0
                } else {
                    widths[i] - cell.text.chars().count() + 2
                };
                match (self.color, cell.style) {
                    (true, Some(style)) => {
                        out.push_str(style);
                        out.push_str(&cell.text);
                        out.push_str(RESET);
                    }
                    _ => out.push_str(&cell.text),
                }
                out.push_str(&" ".repeat(pad));
            }
            // Trailing padding on the last column would show up as invisible
            // whitespace in copied output.
            while out.ends_with(' ') {
                out.pop();
            }
            out.push('\n');
        }

        print!("{out}");
    }
}

/// Shared status colouring, so `pending` looks the same on a placement, a
/// collaboration and a proposal.
pub fn status_style(status: &str) -> &'static str {
    match status {
        "confirmed" | "accepted" => GREEN,
        "pending" | "offer_pending" => YELLOW,
        "cancelled" | "rejected" => RED,
        _ => DIM,
    }
}

/// Key/value blocks for single-object views, where a one-row table reads worse
/// than a plain list.
pub fn detail(rows: &[(&str, String)], color: bool) {
    let width = rows
        .iter()
        .map(|(k, _)| k.chars().count())
        .max()
        .unwrap_or(0);
    for (key, value) in rows {
        let pad = " ".repeat(width - key.chars().count());
        if color {
            println!("{DIM}{key}{RESET}{pad}  {value}");
        } else {
            println!("{key}{pad}  {value}");
        }
    }
}
