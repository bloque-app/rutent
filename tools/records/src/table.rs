//! A small reader for the Markdown tables and headings the records use.

/// A Markdown table with the heading it sits under.
pub(crate) struct Table {
    pub heading3: String,
    pub header: Vec<String>,
    /// Each row with its 1-based line number.
    pub rows: Vec<(usize, Vec<String>)>,
}

/// Splits one table line into trimmed cells; a `|` inside backticks or escaped with `\` stays in its cell.
pub(crate) fn split_row(line: &str) -> Vec<String> {
    let line = line.trim();
    let line = line.strip_prefix('|').unwrap_or(line);
    let line = line.strip_suffix('|').unwrap_or(line);
    let mut cells = Vec::new();
    let mut cell = String::new();
    let mut in_code = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '`' => {
                in_code = !in_code;
                cell.push(c);
            }
            '\\' if chars.peek() == Some(&'|') => {
                cell.push('|');
                chars.next();
            }
            '|' if !in_code => cells.push(std::mem::take(&mut cell).trim().to_string()),
            _ => cell.push(c),
        }
    }
    cells.push(cell.trim().to_string());
    cells
}

fn is_separator(cells: &[String]) -> bool {
    !cells.is_empty()
        && cells
            .iter()
            .all(|c| !c.is_empty() && c.chars().all(|ch| matches!(ch, '-' | ':' | ' ')))
}

/// Every table outside a fenced code block, in document order.
pub(crate) fn tables(text: &str) -> Vec<Table> {
    let mut out: Vec<Table> = Vec::new();
    let mut h3 = String::new();
    let mut fenced = false;
    let mut open: Option<Table> = None;
    for (index, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            fenced = !fenced;
        }
        if fenced || trimmed.starts_with("```") {
            continue;
        }
        if trimmed.starts_with('|') {
            let cells = split_row(trimmed);
            match open.as_mut() {
                None => {
                    open = Some(Table {
                        heading3: h3.clone(),
                        header: cells,
                        rows: Vec::new(),
                    });
                }
                Some(t) if t.rows.is_empty() && is_separator(&cells) => {}
                Some(t) => t.rows.push((index + 1, cells)),
            }
            continue;
        }
        if let Some(t) = open.take() {
            out.push(t);
        }
        if let Some(rest) = line.strip_prefix("### ") {
            h3 = rest.to_string();
        } else if line.starts_with("## ") {
            h3.clear();
        }
    }
    if let Some(t) = open.take() {
        out.push(t);
    }
    out
}
