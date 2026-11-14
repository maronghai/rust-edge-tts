//! Consume the async stream and write MP3 plus SRT subtitles.
//!
//! Run with:
//! `cargo run --example stream_to_files -- "第一句。第二句。"`

use edge_tts::{Communicate, SubMaker, TtsChunk};
use futures_util::StreamExt;
use std::env;
use tokio::io::AsyncWriteExt;

#[tokio::main]
async fn main() -> edge_tts::Result<()> {
    let text = env::args()
        .nth(1)
        .unwrap_or_else(|| "第一句。第二句。".to_owned());
    let mut tts = Communicate::new(text)?;
    let mut stream = tts.stream();
    let mut audio = tokio::fs::File::create("stream.mp3").await?;
    let mut subtitles = SubMaker::new();
    let mut audio_bytes = 0usize;

    while let Some(item) = stream.next().await {
        match item? {
            TtsChunk::Audio { data } => {
                audio_bytes += data.len();
                audio.write_all(&data).await?;
            }
            event => subtitles.feed(&event)?,
        }
    }
    audio.flush().await?;
    tokio::fs::write("stream.srt", subtitles.get_srt()).await?;

    println!("wrote {audio_bytes} MP3 bytes to stream.mp3");
    println!("wrote subtitles to stream.srt");
    Ok(())
}
