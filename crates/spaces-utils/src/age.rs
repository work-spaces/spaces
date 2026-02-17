use bincode::{Decode, Encode};
use serde::{Deserialize, Serialize};

pub fn get_now() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0)
}

#[derive(Clone, Copy, Serialize, Deserialize, Encode, Decode, Debug)]
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

    pub fn get_age(&self) -> u128 {
        (get_now() - self.value) / (24 * 60 * 60 * 1000)
    }

    pub fn update(&mut self) {
        self.value = get_now();
    }
}
