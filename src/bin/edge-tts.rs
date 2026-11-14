//! Synthesize text or list service voices using the shared CLI.
#[tokio::main]
async fn main() {
    std::process::exit(edge_tts::cli::main_entry(false).await);
}
