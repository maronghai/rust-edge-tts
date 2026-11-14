//! Shared command-line implementation for edge-tts and edge-playback.
use crate::*;
use clap::{ArgGroup, Parser};
use std::io::{IsTerminal, Read, Write};
#[derive(Debug, Parser)]
#[command(version,about="Text-to-speech using Microsoft Edge's online TTS service",group(ArgGroup::new("input").required(true).args(["text","file","list_voices"])))]
/// Parsed CLI options shared by the synthesis and playback executables.
/// Use `clap::Parser::parse` or `try_parse_from` to enforce input-group rules.
pub struct Args {
    #[arg(short = 't', long)]
    /// Plain text input; mutually exclusive with file input and voice listing.
    pub text: Option<String>,
    #[arg(short = 'f', long)]
    /// UTF-8 input file; `-` and `/dev/stdin` read standard input.
    pub file: Option<String>,
    #[arg(short = 'l', long)]
    /// List voices instead of synthesizing text.
    pub list_voices: bool,
    #[arg(short='v',long,default_value=DEFAULT_VOICE)]
    /// Voice name; defaults to the library default voice.
    pub voice: String,
    #[arg(long, default_value = "+0%", allow_hyphen_values = true)]
    /// Signed percentage rate adjustment.
    pub rate: String,
    #[arg(long, default_value = "+0%", allow_hyphen_values = true)]
    /// Signed percentage volume adjustment.
    pub volume: String,
    #[arg(long, default_value = "+0Hz", allow_hyphen_values = true)]
    /// Signed hertz pitch adjustment.
    pub pitch: String,
    #[arg(long)]
    /// MP3 output path; omitted or `-` selects stdout outside playback.
    pub write_media: Option<String>,
    #[arg(long)]
    /// SRT output path; `-` selects stderr outside playback.
    pub write_subtitles: Option<String>,
    #[arg(long)]
    /// JSON Lines boundary output path; `-` selects stderr.
    pub write_metadata: Option<String>,
    #[arg(long)]
    /// Explicit network connection URL.
    pub proxy: Option<String>,
    #[arg(long)]
    /// Disable interactive progress on stderr.
    pub no_progress: bool,
    #[arg(long, value_enum, default_value = "SentenceBoundary")]
    /// Requested boundary granularity.
    pub boundary: Boundary,
    #[arg(long, default_value_t = 10)]
    /// Positive connection timeout in seconds.
    pub connect_timeout: u64,
    #[arg(long, default_value_t = 60)]
    /// Positive operation timeout in seconds; total request timeout for voice listing.
    pub receive_timeout: u64,
    #[arg(long, help = "Print voice list as JSON")]
    /// Serialize listed voices as pretty JSON instead of a table.
    pub json: bool,
    #[arg(long, help = "Filter voice list by locale or language")]
    /// Case-insensitive full locale or language filter for voice listing.
    pub locale: Option<String>,
    #[arg(long, help = "Filter voice list by gender")]
    /// Case-insensitive gender filter for voice listing.
    pub gender: Option<String>,
    #[arg(long, help = "Use mpv for edge-playback")]
    /// Force external mpv playback on Windows; other platforms already use it.
    pub mpv: bool,
}
/// Run voice discovery, synthesis, or playback with already parsed arguments.
///
/// `playback` selects synthesis followed by native Windows or mpv playback.
/// This function can write files, stdout/stderr, and invoke a player. Parse
/// arguments with Clap first: direct struct construction bypasses input groups.
/// Cancellation handling is provided by [`main_entry`], not by this function.
///
/// # Errors
/// Returns validation, transport, file, subtitle, or playback errors.
pub async fn run(mut args: Args, playback: bool) -> Result<()> {
    if args.connect_timeout == 0 || args.receive_timeout == 0 {
        return Err(Error::InvalidConfig("timeouts must be positive".into()));
    }
    if args.list_voices {
        let mut voices = list_voices_with_timeout(
            args.proxy.as_deref(),
            Duration::from_secs(args.connect_timeout),
            Duration::from_secs(args.receive_timeout),
        )
        .await?;
        voices.retain(|v| {
            args.locale.as_ref().is_none_or(|l| {
                v.locale.eq_ignore_ascii_case(l) || v.language().eq_ignore_ascii_case(l)
            }) && args
                .gender
                .as_ref()
                .is_none_or(|g| v.gender.eq_ignore_ascii_case(g))
        });
        voices.sort_by(|a, b| a.short_name.cmp(&b.short_name));
        if args.json {
            println!("{}", serde_json::to_string_pretty(&voices)?);
        } else {
            let width = voices
                .iter()
                .map(|v| v.short_name.len())
                .max()
                .unwrap_or(4)
                .max(4);
            println!(
                "{:<width$}  {:<8}  {:<24}  VoicePersonalities",
                "Name", "Gender", "ContentCategories"
            );
            for v in voices {
                println!(
                    "{:<width$}  {:<8}  {:<24}  {}",
                    v.short_name,
                    v.gender,
                    v.voice_tag.content_categories.join(", "),
                    v.voice_tag.voice_personalities.join(", ")
                );
            }
        }
        return Ok(());
    }
    let text = if let Some(file) = args.file.as_deref() {
        if file == "-" || file == "/dev/stdin" {
            let mut text = String::new();
            std::io::stdin().read_to_string(&mut text)?;
            text
        } else {
            tokio::fs::read_to_string(file).await?
        }
    } else {
        args.text.take().unwrap_or_default()
    };
    let config = TtsConfig {
        voice: args.voice.clone(),
        rate: args.rate.clone(),
        volume: args.volume.clone(),
        pitch: args.pitch.clone(),
        boundary: args.boundary,
        proxy: args.proxy.clone(),
        connect_timeout: Duration::from_secs(args.connect_timeout),
        receive_timeout: Duration::from_secs(args.receive_timeout),
    };
    let mut communicate = Communicate::with_config(text.clone(), config)?;
    let use_mpv = !cfg!(windows) || args.mpv;
    let mut temporary = Vec::new();
    if playback {
        if args.write_media.is_none() {
            args.write_media = std::env::var("EDGE_PLAYBACK_MP3_FILE").ok();
        }
        if args.write_subtitles.is_none() {
            args.write_subtitles = std::env::var("EDGE_PLAYBACK_SRT_FILE").ok();
        }
        if args.write_media.as_deref() == Some("-") || args.write_subtitles.as_deref() == Some("-")
        {
            return Err(Error::InvalidConfig(
                "playback requires file outputs".into(),
            ));
        }
        if args.write_media.is_none() {
            let f = tempfile::Builder::new().suffix(".mp3").tempfile()?;
            args.write_media = Some(f.path().to_string_lossy().into_owned());
            temporary.push(f);
        }
        if use_mpv && args.write_subtitles.is_none() {
            let f = tempfile::Builder::new().suffix(".srt").tempfile()?;
            args.write_subtitles = Some(f.path().to_string_lossy().into_owned());
            temporary.push(f);
        }
        if std::env::var_os("EDGE_PLAYBACK_DEBUG").is_some() {
            eprintln!(
                "Media: {:?}; subtitles: {:?}",
                args.write_media, args.write_subtitles
            );
        }
    }
    let audio_path = args.write_media.as_deref().filter(|p| *p != "-");
    let mut outputs = Vec::new();
    for p in [
        audio_path,
        args.write_subtitles.as_deref().filter(|p| *p != "-"),
        args.write_metadata.as_deref().filter(|p| *p != "-"),
    ]
    .into_iter()
    .flatten()
    {
        let absolute = std::path::absolute(p)?;
        if outputs.contains(&absolute) {
            return Err(Error::InvalidConfig("output files must be distinct".into()));
        }
        outputs.push(absolute);
    }
    if let Some(input) = args
        .file
        .as_deref()
        .filter(|p| *p != "-" && *p != "/dev/stdin")
    {
        let input = std::path::absolute(input)?;
        if outputs.contains(&input) {
            return Err(Error::InvalidConfig(
                "output must not overwrite input file".into(),
            ));
        }
    }
    if args.write_metadata.as_deref() == Some("-") && args.write_subtitles.as_deref() == Some("-") {
        return Err(Error::InvalidConfig(
            "metadata and subtitles cannot both use stderr".into(),
        ));
    }
    if audio_path.is_none() && std::io::stdout().is_terminal() {
        eprintln!(
            "Warning: MP3 data will be written to the terminal. Use --write-media to save it. Press Enter to continue."
        );
        let mut line = String::new();
        std::io::stdin().read_line(&mut line)?;
    }
    let mut audio: Box<dyn tokio::io::AsyncWrite + Unpin + Send> = match audio_path {
        Some(p) => Box::new(tokio::fs::File::create(p).await?),
        None => Box::new(tokio::io::stdout()),
    };
    let mut metadata: Option<Box<dyn tokio::io::AsyncWrite + Unpin + Send>> =
        match args.write_metadata.as_deref() {
            Some("-") => Some(Box::new(tokio::io::stderr())),
            Some(p) => Some(Box::new(tokio::fs::File::create(p).await?)),
            None => None,
        };
    let show = !args.no_progress
        && args.write_subtitles.as_deref() != Some("-")
        && args.write_metadata.as_deref() != Some("-")
        && std::io::stderr().is_terminal();
    let mut sub = SubMaker::new();
    let mut chars = 0usize;
    let count = text.chars().filter(|c| !c.is_whitespace()).count().max(1);
    let mut stream = communicate.stream();
    let synthesis: Result<()> = async {
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            match &chunk {
                TtsChunk::Audio { data } => audio.write_all(data).await?,
                event => {
                    sub.feed(event)?;
                    if let Some(file) = metadata.as_mut() {
                        let mut line = serde_json::to_vec(event)?;
                        line.push(b'\n');
                        file.write_all(&line).await?;
                    }
                    if let Some((_, _, _, s)) = event.boundary() {
                        chars += s.chars().filter(|c| !c.is_whitespace()).count();
                    }
                }
            }
            if show {
                eprint!("\rSynthesizing {:3}%", (chars * 100 / count).min(99));
                std::io::stderr().flush()?;
            }
        }
        audio.flush().await?;
        if let Some(file) = metadata.as_mut() {
            file.flush().await?;
        }
        Ok(())
    }
    .await;
    if show {
        if synthesis.is_ok() {
            eprintln!("\rSynthesizing 100%");
        } else {
            eprintln!();
        }
    }
    synthesis?;
    drop(audio);
    drop(metadata);
    if let Some(path) = args.write_subtitles.as_deref() {
        let srt = sub.get_srt();
        if path == "-" {
            tokio::io::stderr().write_all(srt.as_bytes()).await?;
        } else {
            tokio::fs::write(path, srt).await?;
        }
    }
    if playback {
        let media = args.write_media.as_deref().unwrap();
        if use_mpv {
            let mut cmd = tokio::process::Command::new("mpv");
            cmd.kill_on_drop(true)
                .arg("--msg-level=all=error,statusline=status");
            if cfg!(target_os = "macos") {
                cmd.args(["--audio-channels=stereo", "--audio-format=float"]);
            }
            if let Some(sub) = args.write_subtitles.as_deref() {
                cmd.arg(format!("--sub-file={sub}"));
            }
            let status = cmd.arg("--").arg(media).status().await?;
            if !status.success() {
                return Err(Error::Protocol(format!("mpv exited with {status}")));
            }
        } else {
            let path = media.to_owned();
            tokio::task::spawn_blocking(move || play_native(&path))
                .await
                .map_err(|e| Error::Protocol(e.to_string()))??;
        }
        if std::env::var_os("EDGE_PLAYBACK_KEEP_TEMP").is_some() {
            for file in temporary {
                let (_, path) = file.keep().map_err(|e| Error::Io(e.error))?;
                eprintln!("Keeping {}", path.display());
            }
        }
    }
    Ok(())
}
#[cfg(windows)]
fn play_native(path: &str) -> Result<()> {
    #[link(name = "winmm")]
    unsafe extern "system" {
        fn mciSendStringW(command: *const u16, result: *mut u16, size: u32, window: usize) -> u32;
        fn mciGetErrorStringW(error: u32, text: *mut u16, size: u32) -> i32;
    }
    fn send(command: &str) -> Result<()> {
        let wide: Vec<u16> = command.encode_utf16().chain(Some(0)).collect();
        let code = unsafe { mciSendStringW(wide.as_ptr(), std::ptr::null_mut(), 0, 0) };
        if code != 0 {
            let mut text = [0u16; 256];
            unsafe {
                mciGetErrorStringW(code, text.as_mut_ptr(), 256);
            };
            let end = text.iter().position(|v| *v == 0).unwrap_or(text.len());
            return Err(Error::Protocol(format!(
                "audio playback: {} ({code})",
                String::from_utf16_lossy(&text[..end])
            )));
        }
        Ok(())
    }
    let path = std::path::absolute(path)?;
    let path = path.to_string_lossy();
    if path.contains('"') {
        return Err(Error::InvalidConfig("invalid playback filename".into()));
    }
    let alias = format!("tts{}", uuid::Uuid::new_v4().simple());
    send(&format!("open \"{path}\" type mpegvideo alias {alias}"))?;
    let result = send(&format!("play {alias} wait"));
    let close = send(&format!("close {alias}"));
    result.and(close)
}
#[cfg(not(windows))]
fn play_native(_: &str) -> Result<()> {
    Err(Error::InvalidConfig(
        "native playback is only available on Windows; use mpv".into(),
    ))
}
/// Parse process arguments, run the selected CLI, and handle Ctrl+C.
///
/// Returns exit status 0 on success, 1 on operation failure, or 130 on Ctrl+C.
/// Clap handles help/version and argument parse failures itself, potentially
/// exiting the process. Intended for binary entry points, not embedded callers.
/// Cancellation may leave partially written output files.
pub async fn main_entry(playback: bool) -> i32 {
    let args = Args::parse();
    tokio::select! {result=run(args,playback)=>match result{Ok(())=>0,Err(e)=>{eprintln!("Error: {e}");1}},_=tokio::signal::ctrl_c()=>{eprintln!("\nOperation canceled.");130}}
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cli_compatibility() {
        assert!(Args::try_parse_from(["edge-tts"]).is_err());
        assert!(Args::try_parse_from(["edge-tts", "-t", "a", "-l"]).is_err());
        let args = Args::try_parse_from([
            "edge-tts",
            "-t",
            "你好",
            "--rate=-10%",
            "--pitch",
            "-20Hz",
            "--write-media",
            "out.mp3",
        ])
        .unwrap();
        assert_eq!(args.rate, "-10%");
        assert_eq!(args.pitch, "-20Hz");
    }
}
