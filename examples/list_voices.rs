//! Fetch and filter the live voice catalog.
//!
//! Run with:
//! `cargo run --example list_voices -- zh-CN Female`

use edge_tts::VoicesManager;
use serde_json::json;
use std::{collections::BTreeMap, env};

#[tokio::main]
async fn main() -> edge_tts::Result<()> {
    let language = env::args().nth(1).unwrap_or_else(|| "zh".to_owned());
    let gender = env::args().nth(2).unwrap_or_else(|| "Female".to_owned());

    let manager = VoicesManager::create().await?;
    let criteria = BTreeMap::from([
        ("Language".to_owned(), json!(language)),
        ("Gender".to_owned(), json!(gender)),
    ]);

    for voice in manager.find(&criteria) {
        println!("{} | {} | {}", voice.short_name, voice.locale, voice.gender);
    }
    Ok(())
}
