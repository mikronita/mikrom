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
