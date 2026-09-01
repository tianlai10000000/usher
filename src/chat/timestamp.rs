use chrono::{DateTime, Datelike, FixedOffset, Local, Weekday};

pub(super) fn current_timestamp() -> String {
    Local::now().to_rfc3339()
}

pub(super) fn format_for_display(value: &str) -> String {
    format_for_display_at(
        value,
        Local::now().fixed_offset(),
        usher_i18n::active_locale().id,
    )
}

fn format_for_display_at(value: &str, now: DateTime<FixedOffset>, locale: &str) -> String {
    let Ok(parsed) = DateTime::parse_from_rfc3339(value.trim()) else {
        // Legacy history stored only HH:mm, so its calendar date cannot be recovered.
        return value.to_string();
    };
    let message = parsed.with_timezone(now.offset());
    let day_age = now
        .date_naive()
        .signed_duration_since(message.date_naive())
        .num_days();
    let time = message.format("%H:%M").to_string();

    match day_age {
        0 => time,
        1 => relative_with_clock(locale, "time.yesterday", time.as_str()),
        2 => relative_with_clock(locale, "time.two_days_ago", time.as_str()),
        3..=6 => relative_with_clock(locale, weekday_key(message.weekday()), time.as_str()),
        _ if message.year() == now.year() => usher_i18n::translate_for_with(
            locale,
            "time.month_day_with_clock",
            &[
                ("month", month_label(locale, message.month()).as_str()),
                ("day", message.day().to_string().as_str()),
                ("time", time.as_str()),
            ],
        ),
        _ => usher_i18n::translate_for_with(
            locale,
            "time.month_day_year_with_clock",
            &[
                ("month", month_label(locale, message.month()).as_str()),
                ("day", message.day().to_string().as_str()),
                ("year", message.year().to_string().as_str()),
                ("time", time.as_str()),
            ],
        ),
    }
}

fn relative_with_clock(locale: &str, relative_key: &str, time: &str) -> String {
    let relative = usher_i18n::translate_for(locale, relative_key);
    usher_i18n::translate_for_with(
        locale,
        "time.relative_with_clock",
        &[("relative", relative.as_ref()), ("time", time)],
    )
}

fn weekday_key(weekday: Weekday) -> &'static str {
    match weekday {
        Weekday::Mon => "time.weekday.mon",
        Weekday::Tue => "time.weekday.tue",
        Weekday::Wed => "time.weekday.wed",
        Weekday::Thu => "time.weekday.thu",
        Weekday::Fri => "time.weekday.fri",
        Weekday::Sat => "time.weekday.sat",
        Weekday::Sun => "time.weekday.sun",
    }
}

fn month_label(locale: &str, month: u32) -> String {
    let key = match month {
        1 => "time.month.jan",
        2 => "time.month.feb",
        3 => "time.month.mar",
        4 => "time.month.apr",
        5 => "time.month.may",
        6 => "time.month.jun",
        7 => "time.month.jul",
        8 => "time.month.aug",
        9 => "time.month.sep",
        10 => "time.month.oct",
        11 => "time.month.nov",
        12 => "time.month.dec",
        _ => return month.to_string(),
    };
    usher_i18n::translate_for(locale, key).into_owned()
}

#[cfg(test)]
fn expected_locale_timestamp(locale: &str, timestamp: &str) -> String {
    let now = DateTime::parse_from_rfc3339("2026-08-13T15:30:00+08:00").unwrap();
    format_for_display_at(timestamp, now, locale)
}

#[cfg(test)]
fn assert_locale_timestamps(locale: &str, expected: [&str; 6]) {
    for (timestamp, expected) in [
        "2026-08-13T09:24:00+08:00",
        "2026-08-12T23:50:00+08:00",
        "2026-08-11T08:05:00+08:00",
        "2026-08-10T08:05:00+08:00",
        "2026-08-06T08:05:00+08:00",
        "2025-12-31T23:59:00+08:00",
    ]
    .into_iter()
    .zip(expected)
    {
        assert_eq!(expected_locale_timestamp(locale, timestamp), expected);
    }
}

#[cfg(test)]
mod tests {
    use super::{assert_locale_timestamps, expected_locale_timestamp};

    #[test]
    fn formats_chat_timestamps_for_english() {
        assert_locale_timestamps(
            "en",
            [
                "09:24",
                "yesterday 23:50",
                "two days ago 08:05",
                "Monday 08:05",
                "Aug 6 08:05",
                "Dec 31, 2025 23:59",
            ],
        );
    }

    #[test]
    fn formats_chat_timestamps_for_simplified_chinese() {
        assert_locale_timestamps(
            "zh-CN",
            [
                "09:24",
                "昨天 23:50",
                "前天 08:05",
                "星期一 08:05",
                "8月6日 08:05",
                "2025年12月31日 23:59",
            ],
        );
    }

    #[test]
    fn converts_the_stored_offset_to_the_viewers_local_offset() {
        assert_eq!(
            expected_locale_timestamp("en", "2026-08-12T16:30:00Z"),
            "00:30"
        );
        assert_eq!(
            expected_locale_timestamp("en", "2026-08-11T16:30:00Z"),
            "yesterday 00:30"
        );
    }

    #[test]
    fn preserves_legacy_time_only_values() {
        assert_eq!(expected_locale_timestamp("en", "12:00"), "12:00");
        assert_eq!(expected_locale_timestamp("zh-CN", "12:00"), "12:00");
    }
}
