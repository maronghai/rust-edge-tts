//! Synthesize text and play the resulting audio using the shared CLI.
#[tokio::main]
async fn main() {
    std::process::exit(edge_tts::cli::main_entry(true).await);
}
