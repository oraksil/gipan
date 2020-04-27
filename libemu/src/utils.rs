pub mod time {
    use std::time::{SystemTime, Duration, UNIX_EPOCH};

    pub fn now_utc() -> Duration {
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap()
    }
}
