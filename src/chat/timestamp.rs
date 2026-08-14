use chrono::{DateTime, Datelike, FixedOffset, Local, Weekday};

pub(super) fn current_timestamp() -> String {
    Local::now().to_rfc3339()
}

pub(super) fn format_for_display(value: &str) -> String {
    format_for_display_at(value, Local::now().fixed_offset())
}

fn format_for_display_at(value: &str, now: DateTime<FixedOffset>) -> String {
    let Ok(parsed) = DateTime::parse_from_rfc3339(value.trim()) else {
        // Legacy history stored only HH:mm, so its calendar date cannot be recovered.
        return value.to_string();
    };
    let message = parsed.with_timezone(now.offset());
    let day_age = now
        .date_naive()
        .signed_duration_since(message.date_naive())
        .num_days();
    let time = message.format("%H:%M");

    match day_age {
        0 => time.to_string(),
        1 => format!("昨天 {time}"),
        2 => format!("前天 {time}"),
        3..=6 => format!("{} {time}", weekday_label(message.weekday())),
        _ if message.year() == now.year() => {
            format!("{}月{}日 {time}", message.month(), message.day())
        }
        _ => format!(
            "{}年{}月{}日 {time}",
            message.year(),
            message.month(),
            message.day()
        ),
    }
}

fn weekday_label(weekday: Weekday) -> &'static str {
    match weekday {
        Weekday::Mon => "星期一",
        Weekday::Tue => "星期二",
        Weekday::Wed => "星期三",
        Weekday::Thu => "星期四",
        Weekday::Fri => "星期五",
        Weekday::Sat => "星期六",
        Weekday::Sun => "星期日",
    }
}

#[cfg(test)]
mod tests {
    use super::format_for_display_at;
    use chrono::DateTime;

    fn display(timestamp: &str) -> String {
        let now = DateTime::parse_from_rfc3339("2026-08-13T15:30:00+08:00").unwrap();
        format_for_display_at(timestamp, now)
    }

    #[test]
    fn formats_chat_timestamps_by_calendar_age() {
        assert_eq!(display("2026-08-13T09:24:00+08:00"), "09:24");
        assert_eq!(display("2026-08-12T23:50:00+08:00"), "昨天 23:50");
        assert_eq!(display("2026-08-11T08:05:00+08:00"), "前天 08:05");
        assert_eq!(display("2026-08-10T08:05:00+08:00"), "星期一 08:05");
        assert_eq!(display("2026-08-06T08:05:00+08:00"), "8月6日 08:05");
        assert_eq!(display("2025-12-31T23:59:00+08:00"), "2025年12月31日 23:59");
    }

    #[test]
    fn converts_the_stored_offset_to_the_viewers_local_offset() {
        assert_eq!(display("2026-08-12T16:30:00Z"), "00:30");
        assert_eq!(display("2026-08-11T16:30:00Z"), "昨天 00:30");
    }

    #[test]
    fn preserves_legacy_time_only_values() {
        assert_eq!(display("12:00"), "12:00");
    }
}
