use chrono::{DateTime, Utc};
use colored::Colorize;

pub fn get_online_colored(serial_number: &str, last_seen: &Option<DateTime<Utc>>) -> String {
    use chrono_humanize::HumanTime;
    let now = chrono::Utc::now();

    match last_seen {
        Some(parsed_time) => {
            let duration = now.signed_duration_since(parsed_time.with_timezone(&chrono::Utc));

            if duration.num_minutes() < 5 {
                serial_number.bright_green().to_string()
            } else {
                let human_time = HumanTime::from(parsed_time.with_timezone(&chrono::Utc));
                format!("{} ({})", serial_number, human_time)
                    .red()
                    .to_string()
            }
        }
        None => format!("{} (Unknown)", serial_number).yellow().to_string(),
    }
}
