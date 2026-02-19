use serde::{Deserialize, Serialize};

fn system_time_to_millis_since_epoch(time: std::time::SystemTime) -> u128 {
    time.duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0)
}

pub fn get_now() -> u128 {
    system_time_to_millis_since_epoch(std::time::SystemTime::now())
}

#[derive(Clone, Copy, Serialize, Deserialize, Debug)]
pub struct LastUsed {
    value: u128,
}

impl Default for LastUsed {
    fn default() -> Self {
        Self { value: get_now() }
    }
}

impl LastUsed {
    pub fn new(last_used: u128) -> Self {
        Self { value: last_used }
    }

    pub fn new_from_file(path: &std::path::Path) -> Option<Self> {
        if path.exists() {
            let metadata = std::fs::metadata(path).ok()?;
            let created = metadata.created().ok()?;
            let value = system_time_to_millis_since_epoch(created);
            Some(Self { value })
        } else {
            None
        }
    }

    pub fn get_current_age(&self) -> u128 {
        get_now().saturating_sub(self.value) / (24 * 60 * 60 * 1000)
    }

    pub fn get_age(&self, reference_time: u128) -> u128 {
        reference_time.saturating_sub(self.value) / (24 * 60 * 60 * 1000)
    }

    pub fn update(&mut self) {
        self.value = get_now();
    }
}
