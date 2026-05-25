use yansi::Paint;

// Emojis for better UX
pub const SUCCESS: &str = "✅";
pub const ERROR: &str = "❌";
pub const INFO: &str = "ℹ️";
pub const WAIT: &str = "⏳";
pub const ROCKET: &str = "🚀";
pub const PAUSE: &str = "⏸️";
pub const RESUME: &str = "▶️";
pub const KEY: &str = "🔑";
pub const APP: &str = "📦";
pub const DEP: &str = "🚢";
pub const SYS: &str = "⚙️";
pub const WATCH: &str = "👀";
pub const CLOCK: &str = "🕒";
pub const PORT: &str = "🔌";
pub const JSON: &str = "🧾";
pub const TABLE: &str = "📊";
pub const LIVE: &str = "🟢";
pub const IDLE: &str = "⚪";
pub const WARN: &str = "🟡";

pub fn bold_cyan(s: &str) -> String {
    Paint::new(s).cyan().bold().to_string()
}

pub fn green_label(s: &str) -> String {
    Paint::new(s).green().to_string()
}

pub fn red_label(s: &str) -> String {
    Paint::new(s).red().bold().to_string()
}

pub fn cyan_label(s: &str) -> String {
    Paint::new(s).cyan().to_string()
}

pub fn yellow_label(s: &str) -> String {
    Paint::new(s).yellow().to_string()
}

pub fn info(msg: &str) {
    println!("{} {}", INFO, msg);
}

pub fn success(msg: &str) {
    println!("{} {} {}", SUCCESS, green_label("Success:"), msg);
}

pub fn error(msg: &str) {
    println!("{} {} {}", ERROR, red_label("Error:"), msg);
}

pub fn step(emoji: &str, msg: &str) {
    println!("{} {}", emoji, msg);
}

pub fn label_value(emoji: &str, label: &str, value: &str) {
    println!("  {} {:<12} {}", emoji, label, value);
}

pub fn table(title: &str, headers: &[&str], rows: &[Vec<String>]) {
    if rows.is_empty() {
        info("No rows to display.");
        return;
    }

    step(TABLE, &bold_cyan(title));

    let widths = column_widths(headers, rows);
    println!("{}", table_border(&widths, '┌', '┬', '┐'));
    print_row(
        headers
            .iter()
            .map(|h| Paint::new(*h).bold().to_string())
            .collect(),
        &widths,
    );
    println!("{}", table_border(&widths, '├', '┼', '┤'));
    for row in rows {
        print_row(row.clone(), &widths);
    }
    println!("{}", table_border(&widths, '└', '┴', '┘'));
}

fn column_widths(headers: &[&str], rows: &[Vec<String>]) -> Vec<usize> {
    let mut widths: Vec<usize> = headers.iter().map(|h| h.chars().count()).collect();
    for row in rows {
        for (index, cell) in row.iter().enumerate() {
            if let Some(width) = widths.get_mut(index) {
                *width = (*width).max(visible_width(cell));
            }
        }
    }
    widths.into_iter().map(|width| width.clamp(3, 42)).collect()
}

fn visible_width(value: &str) -> usize {
    let mut width = 0;
    let mut chars = value.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' {
            for next in chars.by_ref() {
                if next == 'm' {
                    break;
                }
            }
        } else {
            width += 1;
        }
    }
    width
}

fn table_border(widths: &[usize], left: char, join: char, right: char) -> String {
    let mut border = String::from(left);
    for (index, width) in widths.iter().enumerate() {
        border.push_str(&"─".repeat(width + 2));
        border.push(if index == widths.len() - 1 {
            right
        } else {
            join
        });
    }
    border
}

fn print_row(cells: Vec<String>, widths: &[usize]) {
    print!("│");
    for (index, width) in widths.iter().enumerate() {
        let cell = cells.get(index).map(String::as_str).unwrap_or("");
        let truncated = truncate(cell, *width);
        let padding = width.saturating_sub(visible_width(&truncated));
        print!(" {}{} │", truncated, " ".repeat(padding));
    }
    println!();
}

fn truncate(value: &str, max: usize) -> String {
    if visible_width(value) <= max {
        return value.to_string();
    }

    let mut result = String::new();
    let mut width = 0;
    let mut chars = value.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' {
            result.push(ch);
            for next in chars.by_ref() {
                result.push(next);
                if next == 'm' {
                    break;
                }
            }
            continue;
        }

        if width + 1 >= max {
            result.push('…');
            break;
        }
        result.push(ch);
        width += 1;
    }
    result
}

pub fn status_label(status: &str) -> String {
    match status.to_ascii_lowercase().as_str() {
        "online" | "running" | "run" | "succeeded" | "success" | "active" => {
            format!("{} {}", LIVE, green_label(status))
        },
        "pending" | "building" | "scheduled" | "starting" | "paused" => {
            format!("{} {}", WARN, yellow_label(status))
        },
        "failed" | "cancelled" | "error" | "offline" => format!("{} {}", ERROR, red_label(status)),
        _ => format!("{} {}", IDLE, status),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visible_width_ignores_ansi_escape_sequences() {
        let painted = Paint::new("hello").green().bold().to_string();
        assert_eq!(visible_width(&painted), 5);
    }

    #[test]
    fn visible_width_counts_emoji_as_one_display_cell_for_table_math() {
        assert_eq!(visible_width("🚀 app"), 5);
    }

    #[test]
    fn truncate_returns_original_when_value_fits() {
        assert_eq!(truncate("short", 10), "short");
    }

    #[test]
    fn truncate_adds_ellipsis_when_value_exceeds_width() {
        assert_eq!(truncate("deployment-abcdef", 8), "deploym…");
    }

    #[test]
    fn truncate_preserves_ansi_sequences() {
        let painted = Paint::new("deployment-abcdef").red().to_string();
        let truncated = truncate(&painted, 8);
        assert!(truncated.contains('…'));
        assert!(truncated.contains("\u{1b}["));
    }

    #[test]
    fn column_widths_use_headers_and_rows() {
        let rows = vec![vec!["api".to_string(), "running".to_string()]];
        assert_eq!(column_widths(&["Name", "Status"], &rows), vec![4, 7]);
    }

    #[test]
    fn column_widths_are_clamped_to_maximum() {
        let rows = vec![vec!["x".repeat(100)]];
        assert_eq!(column_widths(&["Name"], &rows), vec![42]);
    }

    #[test]
    fn column_widths_are_clamped_to_minimum() {
        let rows = vec![vec!["x".to_string()]];
        assert_eq!(column_widths(&["N"], &rows), vec![3]);
    }

    #[test]
    fn table_border_uses_requested_glyphs() {
        assert_eq!(table_border(&[3, 5], '┌', '┬', '┐'), "┌─────┬───────┐");
        assert_eq!(table_border(&[3, 5], '└', '┴', '┘'), "└─────┴───────┘");
    }

    #[test]
    fn status_label_marks_success_states_green() {
        let label = status_label("RUNNING");
        assert!(label.contains(LIVE));
        assert!(label.contains("RUNNING"));
    }

    #[test]
    fn status_label_marks_pending_states_yellow() {
        let label = status_label("SCHEDULED");
        assert!(label.contains(WARN));
        assert!(label.contains("SCHEDULED"));
    }

    #[test]
    fn status_label_marks_failure_states_red() {
        let label = status_label("FAILED");
        assert!(label.contains(ERROR));
        assert!(label.contains("FAILED"));
    }

    #[test]
    fn status_label_marks_unknown_states_idle() {
        let label = status_label("mystery");
        assert!(label.contains(IDLE));
        assert!(label.contains("mystery"));
    }
}
