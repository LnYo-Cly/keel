use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) fn generate_run_id() -> String {
    format!("run-{}-{}", unix_millis(), std::process::id())
}

pub(crate) fn now_timestamp() -> String {
    unix_millis().to_string()
}

pub(crate) fn unix_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}
