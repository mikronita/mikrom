use serde::Serialize;

pub fn print_json<T: Serialize>(value: &T) {
    match serde_json::to_string_pretty(value) {
        Ok(json) => println!("{}", json),
        Err(err) => eprintln!("Error: Failed to serialize response to JSON: {}", err),
    }
}

pub fn format_timestamp(ts: i64) -> String {
    if ts == 0 {
        return "N/A".to_string();
    }
    chrono::DateTime::from_timestamp(ts, 0)
        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_else(|| "Invalid".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_timestamp_returns_na_for_zero() {
        assert_eq!(format_timestamp(0), "N/A");
    }

    #[test]
    fn format_timestamp_formats_unix_timestamp() {
        assert_eq!(format_timestamp(1_700_000_000), "2023-11-14 22:13:20");
    }

    #[test]
    fn format_timestamp_returns_invalid_for_out_of_range_values() {
        assert_eq!(format_timestamp(i64::MAX), "Invalid");
    }
}
