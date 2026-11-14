//! Synthesize text asynchronously and save MP3 plus JSONL metadata.
//!
//! Run with:
//! `cargo run --example async_save -- "你好，Rust。" speech.mp3 speech.jsonl`

use edge_tts::{Communicate, TtsConfig};
use std::{env, path::Path};

#[tokio::main]
async fn main() -> edge_tts::Result<()> {
    let text = env::args()
        .nth(1)
        .unwrap_or_else(|| "Hello from the Rust Edge TTS client.".to_owned());
    let audio = env::args()
        .nth(2)
        .unwrap_or_else(|| "speech.mp3".to_owned());
    let metadata = env::args()
        .nth(3)
        .unwrap_or_else(|| "speech.jsonl".to_owned());

    let config = TtsConfig {
        voice: "zh-CN-XiaoxiaoNeural".into(),
        rate: "+10%".into(),
        ..TtsConfig::default()
    };
    let mut tts = Communicate::with_config(text, config)?;
    tts.save(&audio, Some(Path::new(&metadata))).await?;

    println!("audio: {audio}");
    println!("metadata: {metadata}");
    Ok(())
}
