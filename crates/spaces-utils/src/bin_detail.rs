use serde::{Deserialize, Serialize};

pub type Blake3Hash = [u8; blake3::OUT_LEN];

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct BinDetail {
    pub hash: Blake3Hash,
    pub modified: Option<std::time::SystemTime>,
}
