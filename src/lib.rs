//! Native Rust client for the Edge online text-to-speech service.
//!
//! Audio is streamed as 24 kHz, 48 kbit/s mono MP3. Synthesis and voice
//! discovery require network access; no Python runtime is required.
//!
//! # Start here
//!
//! - [`Communicate`]: synthesize once, save files, or consume a stream.
//! - [`TtsConfig`]: voice, prosody, boundary selection, and timeouts.
//! - [`TtsChunk`]: MP3 bytes or word/sentence timing events.
//! - [`SubMaker`]: build SRT subtitles from timing events.
//! - [`list_voices`] and [`VoicesManager`]: discover and filter voices.
//! - [`Error`]: configuration, transport, protocol, and file errors.
//!
//! # Async file output
//!
//! Add the local package under the library name used by these examples:
//!
//! ```toml
//! [dependencies]
//! edge_tts = { package = "rust-tts", path = "../rust-tts" }
//! tokio = { version = "1", features = ["full"] }
//! futures-util = "0.3"
//! serde_json = "1"
//! ```
//!
//! ```no_run
//! use edge_tts::{Communicate, TtsConfig};
//! use std::path::Path;
//!
//! #[tokio::main]
//! async fn main() -> edge_tts::Result<()> {
//!     let config = TtsConfig {
//!         voice: "zh-CN-XiaoxiaoNeural".into(),
//!         rate: "+10%".into(),
//!         ..Default::default()
//!     };
//!     let mut tts = Communicate::with_config("你好，Rust。", config)?;
//!     tts.save("speech.mp3", Some(Path::new("speech.jsonl"))).await?;
//!     Ok(())
//! }
//! ```
//!
//! # Execution and output contracts
//!
//! Async network methods need a Tokio runtime with I/O and time enabled.
//! Blocking alternatives create their own runtime on a worker thread; they
//! still block their caller and should not run directly on an async executor.
//! A [`Communicate`] instance permits one synthesis attempt across all stream
//! and save methods. Create a new instance to retry after failure or cancellation.
//! Async streams start on first poll; dropping an active stream cancels its work.
//!
//! Boundary offsets and durations use **100-nanosecond ticks**: 10,000,000 ticks
//! equal one second. Long-input offsets include compensation based on audio
//! bytes received from earlier segments. MP3 chunks are byte fragments and are
//! not guaranteed to be independently decodable files.
//!
//! Save methods create or truncate output files before synthesis and can leave
//! empty or partial files on failure. Use distinct paths; library save methods
//! do not perform the CLI's path-collision checks. Metadata files contain JSON
//! Lines boundary events, not SRT; use [`SubMaker`] for subtitles.
//!
//! A service HTTP 403 with a valid Date header triggers one clock-skew correction
//! and retry. This does not change the operating system clock. Other failures
//! are returned to the caller without a general retry policy.
pub mod cli;
pub mod drm;
mod subtitles;
pub mod text;
mod transport;
use futures_util::{SinkExt, Stream, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path, pin::Pin, time::Duration};
pub use subtitles::{SubMaker, Subtitle};
use tokio::io::AsyncWriteExt;
use tokio_tungstenite::tungstenite::{Message, client::IntoClientRequest};
/// Result alias used by all fallible library operations.
pub type Result<T> = std::result::Result<T, Error>;
/// Default voice short name used by [`TtsConfig::default`].
pub const DEFAULT_VOICE: &str = "en-US-EmmaMultilingualNeural";
const BASE: &str = "speech.platform.bing.com/consumer/speech/synthesize/readaloud";
const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/143.0.0.0 Safari/537.36 Edg/143.0.0.0";
#[derive(Debug, thiserror::Error)]
/// Errors returned by synthesis, voice discovery, subtitle handling, and CLI operations.
///
/// Transport errors preserve their underlying error via [`std::error::Error::source`].
/// A stream can yield successful chunks before yielding an error.
pub enum Error {
    #[error("invalid configuration: {0}")]
    /// An invalid configuration, empty input, or invalid operation.
    InvalidConfig(String),
    #[error("unexpected service response: {0}")]
    /// An unexpected service response, framing error, or worker/playback failure.
    Protocol(String),
    #[error("no audio received")]
    /// A synthesis segment ended without receiving audio bytes.
    NoAudioReceived,
    #[error("stream() can only be called once per Communicate instance")]
    /// The synthesis instance has already been consumed by a previous attempt.
    StreamAlreadyCalled,
    #[error("{0} timed out")]
    /// A synthesis operation exceeded its deadline; the label identifies the stage.
    Timeout(&'static str),
    #[error(transparent)]
    /// A filesystem, socket I/O, or runtime creation error.
    Io(#[from] std::io::Error),
    #[error(transparent)]
    /// A voice-list HTTP request, status, or response decoding error.
    Http(#[from] reqwest::Error),
    #[error(transparent)]
    /// A WebSocket handshake or transport error.
    WebSocket(#[from] tokio_tungstenite::tungstenite::Error),
    #[error(transparent)]
    /// A JSON metadata decoding or serialization error.
    Json(#[from] serde_json::Error),
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
/// Selects one timing granularity for a synthesis request.
///
/// Only one kind is requested at a time. The default configuration selects sentences.
pub enum Boundary {
    #[value(name = "WordBoundary", alias = "word")]
    /// Request word-level timing events.
    WordBoundary,
    #[value(name = "SentenceBoundary", alias = "sentence")]
    /// Request sentence-level timing events.
    SentenceBoundary,
}
#[derive(Debug, Clone)]
/// Synthesis options; constructors validate and normalize a copy.
///
/// Defaults: [`DEFAULT_VOICE`], neutral prosody, sentence boundaries,
/// 10-second connection timeout, and 60-second per-operation timeout.
/// Validation checks syntax, not voice availability or service-side numeric ranges.
///
/// ```
/// use edge_tts::TtsConfig;
/// let config = TtsConfig {
///     voice: "zh-CN-XiaoxiaoNeural".into(),
///     rate: "-10%".into(),
///     ..Default::default()
/// };
/// let normalized = config.validate()?;
/// assert!(normalized.voice.contains("zh-CN, XiaoxiaoNeural"));
/// assert_eq!(config.voice, "zh-CN-XiaoxiaoNeural");
/// # Ok::<(), edge_tts::Error>(())
/// ```
pub struct TtsConfig {
    /// Voice short name or full Microsoft Server Speech voice name.
    /// Short names are expanded during validation; availability is checked by the service.
    pub voice: String,
    /// Signed integer percentage, such as `+0%` or `-20%`.
    pub rate: String,
    /// Signed integer percentage, such as `+0%` or `-10%`.
    pub volume: String,
    /// Signed integer hertz, such as `+0Hz` or `+5Hz`.
    pub pitch: String,
    /// Timing events requested from the service; defaults to sentences.
    pub boundary: Boundary,
    /// Optional explicit connection URL. See the transport behavior in the README.
    /// `None` allows environment-based connection configuration.
    pub proxy: Option<String>,
    /// Nonzero deadline for each synthesis connection attempt, including handshake.
    pub connect_timeout: Duration,
    /// Nonzero deadline for each synthesis send, receive, or pong operation.
    /// This is not a total synthesis deadline.
    pub receive_timeout: Duration,
}
impl Default for TtsConfig {
    fn default() -> Self {
        Self {
            voice: DEFAULT_VOICE.into(),
            rate: "+0%".into(),
            volume: "+0%".into(),
            pitch: "+0Hz".into(),
            boundary: Boundary::SentenceBoundary,
            proxy: None,
            connect_timeout: Duration::from_secs(10),
            receive_timeout: Duration::from_secs(60),
        }
    }
}
impl TtsConfig {
    /// Return a normalized copy without modifying `self`.
    ///
    /// # Errors
    /// Returns [`Error::InvalidConfig`] for invalid voice/prosody syntax or zero timeouts.
    pub fn validate(&self) -> Result<Self> {
        text::validate(self)
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
/// One streamed audio fragment or boundary event.
///
/// Serde uses an internal `type` tag: `audio`, `WordBoundary`, or
/// `SentenceBoundary`. Boundary JSON includes `offset`, `duration`, and `text`.
/// Audio JSON contains a numeric byte array in `data`; save metadata omits audio.
pub enum TtsChunk {
    #[serde(rename = "audio")]
    /// A fragment of the MP3 byte stream, in service delivery order.
    Audio {
        /// Encoded MP3 bytes; concatenate fragments to produce the output stream.
        data: Vec<u8>,
    },
    /// Timing for a word.
    WordBoundary {
        /// Start from the beginning of the combined audio, in 100-nanosecond ticks.
        offset: u64,
        /// Duration in 100-nanosecond ticks.
        duration: u64,
        /// Boundary text with known XML entities decoded.
        text: String,
    },
    /// Timing for a sentence.
    SentenceBoundary {
        /// Start from the beginning of the combined audio, in 100-nanosecond ticks.
        offset: u64,
        /// Duration in 100-nanosecond ticks.
        duration: u64,
        /// Boundary text with known XML entities decoded.
        text: String,
    },
}
impl TtsChunk {
    /// Borrow timing as `(kind, offset_ticks, duration_ticks, text)`.
    /// Returns `None` for audio fragments.
    pub fn boundary(&self) -> Option<(Boundary, u64, u64, &str)> {
        match self {
            Self::Audio { .. } => None,
            Self::WordBoundary {
                offset,
                duration,
                text,
            } => Some((Boundary::WordBoundary, *offset, *duration, text)),
            Self::SentenceBoundary {
                offset,
                duration,
                text,
            } => Some((Boundary::SentenceBoundary, *offset, *duration, text)),
        }
    }
}
/// A pinned, sendable async stream borrowing its [`Communicate`] instance.
/// Each item is a chunk or an error; consume it using `futures_util::StreamExt`.
pub type TtsStream<'a> = Pin<Box<dyn Stream<Item = Result<TtsChunk>> + Send + 'a>>;
#[derive(Debug)]
/// A single-use synthesis request owning its text and validated configuration.
///
/// Construction performs local validation only. Network access starts when the
/// async stream is polled, a blocking stream is started, or a save method runs.
/// After synthesis starts, failures and early cancellation still consume the instance.
pub struct Communicate {
    text: String,
    config: TtsConfig,
    called: bool,
}
impl Communicate {
    /// Create a request using [`TtsConfig::default`].
    ///
    /// # Errors
    /// Returns [`Error::InvalidConfig`] if text is empty after control-character
    /// replacement and whitespace trimming.
    pub fn new(text: impl Into<String>) -> Result<Self> {
        Self::with_config(text, TtsConfig::default())
    }
    /// Create a request with explicit options, storing a normalized configuration.
    /// Pass plain text: XML escaping is performed internally, not treated as SSML.
    ///
    /// # Errors
    /// Returns [`Error::InvalidConfig`] for empty text or invalid configuration.
    pub fn with_config(text: impl Into<String>, config: TtsConfig) -> Result<Self> {
        let config = config.validate()?;
        let text = text.into();
        if text::remove_incompatible_characters(&text)
            .trim()
            .is_empty()
        {
            return Err(Error::InvalidConfig("text must not be empty".into()));
        }
        Ok(Self {
            text,
            config,
            called: false,
        })
    }
    /// Borrow the validated configuration, including the normalized full voice name.
    pub fn config(&self) -> &TtsConfig {
        &self.config
    }
    /// Start synthesis when the returned stream is first polled.
    ///
    /// Audio and timing events arrive interleaved. Long text is split automatically.
    /// Dropping a polled stream cancels it but does not reset the single-use state.
    ///
    /// # Errors
    /// Errors are stream items, including [`Error::StreamAlreadyCalled`] on reuse,
    /// transport/protocol failures, timeouts, and [`Error::NoAudioReceived`].
    ///
    /// ```no_run
    /// use edge_tts::{Communicate, SubMaker, TtsChunk};
    /// use futures_util::StreamExt;
    /// use tokio::io::AsyncWriteExt;
    /// # #[tokio::main]
    /// # async fn main() -> edge_tts::Result<()> {
    /// let mut tts = Communicate::new("Stream audio and subtitles.")?;
    /// let mut audio = tokio::fs::File::create("stream.mp3").await?;
    /// let mut subtitles = SubMaker::new();
    /// let mut stream = tts.stream();
    /// while let Some(item) = stream.next().await {
    ///     match item? {
    ///         TtsChunk::Audio { data } => audio.write_all(&data).await?,
    ///         event => subtitles.feed(&event)?,
    ///     }
    /// }
    /// audio.flush().await?;
    /// tokio::fs::write("stream.srt", subtitles.get_srt()).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn stream(&mut self) -> TtsStream<'_> {
        Box::pin(async_stream::try_stream! {
        if self.called{Err(Error::StreamAlreadyCalled)?;}self.called=true;
        let cleaned=text::escape(&text::remove_incompatible_characters(&self.text));let parts=text::split_text_by_byte_length(&cleaned,4096)?;let mut total_bytes=0u64;
        for part in parts{
        let mut socket=None;
        for attempt in 0..2{
        let url=format!("wss://{BASE}/edge/v1?TrustedClientToken={}&ConnectionId={}&Sec-MS-GEC={}&Sec-MS-GEC-Version={}",drm::TOKEN,id(),drm::token(),drm::VERSION);
        let mut request=url.into_client_request()?;
        for (k,v) in [("User-Agent",UA),("Origin","chrome-extension://jdiccldimpdaibmpdkjnbmckianbfold"),("Pragma","no-cache"),("Cache-Control","no-cache"),("Accept-Language","en-US,en;q=0.9")]{request.headers_mut().insert(tokio_tungstenite::tungstenite::http::header::HeaderName::from_bytes(k.as_bytes()).unwrap(),v.parse().unwrap());}
        request.headers_mut().insert("Cookie",format!("muid={};",id().to_uppercase()).parse().unwrap());
        let connection=tokio::time::timeout(self.config.connect_timeout,transport::connect(request,self.config.proxy.as_deref())).await.map_err(|_|Error::Timeout("connection"))?;
        match connection{Ok(ws)=>{socket=Some(ws);break},Err(Error::WebSocket(tokio_tungstenite::tungstenite::Error::Http(response))) if response.status().as_u16()==403 && attempt==0=>{drm::correct(response.headers().get("date").and_then(|v|v.to_str().ok()))?;},Err(e)=>Err(e)?}
        }
        let mut ws=socket.ok_or_else(||Error::Protocol("connection failed".into()))?;
        let word=self.config.boundary==Boundary::WordBoundary;
        let config=json!({"context":{"synthesis":{"audio":{"metadataoptions":{"sentenceBoundaryEnabled":if word{"false"}else{"true"},"wordBoundaryEnabled":if word{"true"}else{"false"}},"outputFormat":"audio-24khz-48kbitrate-mono-mp3"}}}});
        let command=format!("X-Timestamp:{}\r\nContent-Type:application/json; charset=utf-8\r\nPath:speech.config\r\n\r\n{config}\r\n",timestamp());
        let ssml=format!("X-RequestId:{}\r\nContent-Type:application/ssml+xml\r\nX-Timestamp:{}Z\r\nPath:ssml\r\n\r\n{}",id(),timestamp(),text::mkssml(&self.config,&part));
        for command in [command,ssml]{tokio::time::timeout(self.config.receive_timeout,ws.send(Message::Text(command.into()))).await.map_err(|_|Error::Timeout("send"))??;}
        let compensation=total_bytes*8*10_000_000/48_000;let mut bytes=0u64;let mut ended=false;
        loop{
        let next=tokio::time::timeout(self.config.receive_timeout,ws.next()).await.map_err(|_|Error::Timeout("receive"))?;
        let Some(frame)=next else{break};match frame?{
        Message::Text(message)=>{
        let (headers,body)=parse_text_frame(message.as_bytes())?;
        match headers.get("Path").map(String::as_str){Some("turn.end")=>{ended=true;break},Some("response"|"turn.start")=>{},Some("audio.metadata")=>{
        let payload:Value=serde_json::from_slice(body)?;let metadata=payload["Metadata"].as_array().ok_or_else(||Error::Protocol("missing Metadata array".into()))?;
        for entry in metadata{let kind=entry["Type"].as_str().ok_or_else(||Error::Protocol("missing metadata Type".into()))?;if kind=="SessionEnd"{continue;}let data=&entry["Data"];let offset=data["Offset"].as_u64().ok_or_else(||Error::Protocol("invalid boundary offset".into()))?.checked_add(compensation).ok_or_else(||Error::Protocol("boundary offset overflow".into()))?;let duration=data["Duration"].as_u64().ok_or_else(||Error::Protocol("invalid boundary duration".into()))?;let text=text::unescape(data["text"]["Text"].as_str().ok_or_else(||Error::Protocol("missing boundary text".into()))?);
        yield match kind{"WordBoundary"=>TtsChunk::WordBoundary{offset,duration,text},"SentenceBoundary"=>TtsChunk::SentenceBoundary{offset,duration,text},other=>Err(Error::Protocol(format!("unknown metadata: {other}")))?};}
        },other=>Err(Error::Protocol(format!("unknown path: {other:?}")))?}
        },
        Message::Binary(frame)=>{if let Some(audio)=parse_binary_frame(&frame)?{bytes+=audio.len() as u64;yield TtsChunk::Audio{data:audio.to_vec()};}},
        Message::Ping(payload)=>{tokio::time::timeout(self.config.receive_timeout,ws.send(Message::Pong(payload))).await.map_err(|_|Error::Timeout("pong"))??;},
        Message::Close(_)=>break,Message::Pong(_)|Message::Frame(_)=>{}
        }}
        if bytes==0{Err(Error::NoAudioReceived)?;}
        if !ended{Err(Error::Protocol("connection closed before turn.end".into()))?;}
        total_bytes+=bytes;
        }
        })
    }
    /// Save concatenated MP3 audio and optional JSON Lines boundary metadata.
    ///
    /// `metadata` is a filesystem path, not SRT output. `None` discards timing
    /// events. Files are created or truncated before consuming the stream;
    /// existing files are overwritten, parent directories are not created,
    /// and failed synthesis can leave partial files. Supply distinct paths:
    /// this method does not check for collisions or interpret `-` as stdio.
    /// Reusing an instance can truncate files before the stream reports reuse.
    ///
    /// # Errors
    /// Returns filesystem errors and any error from [`Self::stream`].
    ///
    /// ```no_run
    /// # async fn example() -> edge_tts::Result<()> {
    /// use edge_tts::Communicate;
    /// use std::path::Path;
    /// let mut tts = Communicate::new("Save with metadata.")?;
    /// tts.save("speech.mp3", Some(Path::new("speech.jsonl"))).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn save(&mut self, audio: impl AsRef<Path>, metadata: Option<&Path>) -> Result<()> {
        let mut audio = tokio::fs::File::create(audio).await?;
        let mut metadata = match metadata {
            Some(p) => Some(tokio::fs::File::create(p).await?),
            None => None,
        };
        let mut stream = self.stream();
        while let Some(chunk) = stream.next().await {
            match chunk? {
                TtsChunk::Audio { data } => audio.write_all(&data).await?,
                boundary => {
                    if let Some(file) = metadata.as_mut() {
                        let mut line = serde_json::to_vec(&boundary)?;
                        line.push(b'\n');
                        file.write_all(&line).await?;
                    }
                }
            }
        }
        audio.flush().await?;
        if let Some(file) = metadata.as_mut() {
            file.flush().await?;
        }
        Ok(())
    }
    /// Blocking counterpart to [`Self::save`], using a worker thread and Tokio runtime.
    ///
    /// The caller blocks until completion; file and single-use semantics match `save`.
    ///
    /// # Errors
    /// Returns save errors, runtime creation errors, or a worker panic as a protocol error.
    ///
    /// ```no_run
    /// use edge_tts::Communicate;
    /// let mut tts = Communicate::new("Blocking synthesis.")?;
    /// tts.save_sync("speech.mp3", None)?;
    /// # Ok::<(), edge_tts::Error>(())
    /// ```
    pub fn save_sync(&mut self, audio: impl AsRef<Path>, metadata: Option<&Path>) -> Result<()> {
        let audio = audio.as_ref().to_owned();
        let metadata = metadata.map(Path::to_owned);
        std::thread::scope(|scope| {
            scope
                .spawn(|| {
                    tokio::runtime::Runtime::new()?.block_on(self.save(audio, metadata.as_deref()))
                })
                .join()
                .map_err(|_| Error::Protocol("blocking worker panicked".into()))?
        })
    }
    /// Start a background producer and return a blocking iterator with capacity one.
    ///
    /// Marks this instance consumed immediately. Each `next()` blocks until an
    /// item arrives or the producer exits. Dropping the iterator disconnects the
    /// channel; the worker stops on its next attempted send, which can be delayed
    /// by an in-flight network operation until its configured timeout.
    ///
    /// # Errors
    /// Returns [`Error::StreamAlreadyCalled`] immediately on reuse. Network and
    /// runtime errors are delivered as iterator items.
    ///
    /// ```no_run
    /// use edge_tts::{Communicate, TtsChunk};
    /// let mut tts = Communicate::new("Blocking stream.")?;
    /// for item in tts.stream_sync()? {
    ///     if let TtsChunk::Audio { data } = item? {
    ///         println!("{} MP3 bytes", data.len());
    ///     }
    /// }
    /// # Ok::<(), edge_tts::Error>(())
    /// ```
    pub fn stream_sync(&mut self) -> Result<BlockingStream> {
        if self.called {
            return Err(Error::StreamAlreadyCalled);
        }
        self.called = true;
        let mut inner = Self::with_config(self.text.clone(), self.config.clone())?;
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let runtime = match tokio::runtime::Runtime::new() {
                Ok(r) => r,
                Err(e) => {
                    let _ = tx.send(Err(Error::Io(e)));
                    return;
                }
            };
            runtime.block_on(async move {
                let mut stream = inner.stream();
                while let Some(item) = stream.next().await {
                    if tx.send(item).is_err() {
                        break;
                    }
                }
            });
        });
        Ok(BlockingStream { rx })
    }
}
/// Blocking iterator of synthesis chunks returned by [`Communicate::stream_sync`].
///
/// Dropping it disconnects its bounded channel. It does not join the worker thread.
pub struct BlockingStream {
    rx: std::sync::mpsc::Receiver<Result<TtsChunk>>,
}
impl Iterator for BlockingStream {
    type Item = Result<TtsChunk>;
    fn next(&mut self) -> Option<Self::Item> {
        self.rx.recv().ok()
    }
}
fn id() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}
fn timestamp() -> String {
    chrono::Utc::now()
        .format("%a %b %d %Y %H:%M:%S GMT+0000 (Coordinated Universal Time)")
        .to_string()
}
fn headers(bytes: &[u8]) -> Result<BTreeMap<String, String>> {
    let text = std::str::from_utf8(bytes).map_err(|e| Error::Protocol(e.to_string()))?;
    let mut result = BTreeMap::new();
    for line in text.split("\r\n").filter(|s| !s.is_empty()) {
        let (k, v) = line
            .split_once(':')
            .ok_or_else(|| Error::Protocol("malformed frame header".into()))?;
        result.insert(k.to_owned(), v.trim().to_owned());
    }
    Ok(result)
}
fn parse_text_frame(bytes: &[u8]) -> Result<(BTreeMap<String, String>, &[u8])> {
    let end = bytes
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or_else(|| Error::Protocol("missing frame separator".into()))?;
    Ok((headers(&bytes[..end])?, &bytes[end + 4..]))
}
fn parse_binary_frame(bytes: &[u8]) -> Result<Option<&[u8]>> {
    if bytes.len() < 2 {
        return Err(Error::Protocol("missing binary header length".into()));
    }
    let end = 2 + u16::from_be_bytes([bytes[0], bytes[1]]) as usize;
    if end > bytes.len() {
        return Err(Error::Protocol("binary header exceeds frame".into()));
    }
    let h = headers(&bytes[2..end])?;
    if h.get("Path").map(String::as_str) != Some("audio") {
        return Err(Error::Protocol("binary path is not audio".into()));
    }
    let audio = &bytes[end..];
    match h.get("Content-Type").map(String::as_str) {
        None if audio.is_empty() => Ok(None),
        Some("audio/mpeg") if !audio.is_empty() => Ok(Some(audio)),
        _ => Err(Error::Protocol(
            "invalid audio content type or empty payload".into(),
        )),
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "PascalCase")]
/// Service-provided categories and personality labels; missing arrays default to empty.
/// Serialized field names use PascalCase.
pub struct VoiceTag {
    #[serde(default)]
    /// Categories such as the types of content the voice is suited for.
    pub content_categories: Vec<String>,
    #[serde(default)]
    /// Service-provided descriptive voice labels.
    pub voice_personalities: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
/// One service voice record with PascalCase JSON field names.
/// Unknown top-level fields are preserved in [`Self::extra`].
pub struct Voice {
    /// Full service voice name.
    pub name: String,
    /// Short name accepted by [`TtsConfig::voice`].
    pub short_name: String,
    /// Service gender label, commonly `Female` or `Male`.
    pub gender: String,
    /// Locale such as `zh-CN` or `en-US`.
    pub locale: String,
    #[serde(default)]
    /// Display name, or an empty string if absent.
    pub friendly_name: String,
    #[serde(default)]
    /// Service-suggested codec; does not change the fixed synthesis MP3 format.
    pub suggested_codec: String,
    #[serde(default)]
    /// Service status string, or an empty string if absent.
    pub status: String,
    #[serde(default)]
    /// Content categories and personality labels; defaults if absent.
    pub voice_tag: VoiceTag,
    #[serde(flatten)]
    /// Additional top-level service fields, preserved by Serde flattening.
    pub extra: BTreeMap<String, Value>,
}
impl Voice {
    /// Return the locale prefix before the first hyphen, for example `zh` from `zh-CN`.
    pub fn language(&self) -> &str {
        self.locale.split('-').next().unwrap_or(&self.locale)
    }
}
/// Fetch the current voice catalog without sorting or caching.
/// Uses 10-second connection and 60-second total HTTP request timeouts.
///
/// # Errors
/// Returns HTTP/network, response decoding, or clock-correction protocol errors.
pub async fn list_voices(proxy: Option<&str>) -> Result<Vec<Voice>> {
    list_voices_with_timeout(proxy, Duration::from_secs(10), Duration::from_secs(60)).await
}
/// Fetch voices with explicit connection and total HTTP request deadlines.
///
/// Unlike synthesis `receive_timeout`, `receive` is the entire HTTP request
/// deadline per attempt. An eligible HTTP 403 is retried once after time correction.
///
/// # Errors
/// Returns HTTP/network or JSON response errors; missing or invalid Date headers
/// during 403 correction return [`Error::Protocol`].
pub async fn list_voices_with_timeout(
    proxy: Option<&str>,
    connect: Duration,
    receive: Duration,
) -> Result<Vec<Voice>> {
    let mut builder = reqwest::Client::builder()
        .user_agent(UA)
        .connect_timeout(connect)
        .timeout(receive);
    if let Some(proxy) = proxy {
        builder = builder.proxy(reqwest::Proxy::all(proxy)?);
    }
    let client = builder.build()?;
    for attempt in 0..2 {
        let url = format!(
            "https://{BASE}/voices/list?trustedclienttoken={}&Sec-MS-GEC={}&Sec-MS-GEC-Version={}",
            drm::TOKEN,
            drm::token(),
            drm::VERSION
        );
        let response = client
            .get(url)
            .header("Cookie", format!("muid={};", id().to_uppercase()))
            .header("Accept-Language", "en-US,en;q=0.9")
            .header(
                "Sec-CH-UA",
                "\" Not;A Brand\";v=\"99\", \"Microsoft Edge\";v=\"143\", \"Chromium\";v=\"143\"",
            )
            .header("Sec-CH-UA-Mobile", "?0")
            .header("Sec-Fetch-Site", "none")
            .header("Sec-Fetch-Mode", "cors")
            .header("Sec-Fetch-Dest", "empty")
            .send()
            .await?;
        if response.status().as_u16() == 403 && attempt == 0 {
            drm::correct(response.headers().get("date").and_then(|v| v.to_str().ok()))?;
            continue;
        }
        return Ok(response.error_for_status()?.json().await?);
    }
    Err(Error::Protocol("voice list retry exhausted".into()))
}
/// Blocking voice discovery using a worker thread and its own Tokio runtime.
///
/// # Errors
/// Returns discovery/runtime errors, or a protocol error if the worker panics.
///
/// ```no_run
/// let voices = edge_tts::list_voices_sync(None)?;
/// for voice in voices {
///     println!("{} ({})", voice.short_name, voice.locale);
/// }
/// # Ok::<(), edge_tts::Error>(())
/// ```
pub fn list_voices_sync(proxy: Option<&str>) -> Result<Vec<Voice>> {
    std::thread::scope(|s| {
        s.spawn(|| tokio::runtime::Runtime::new()?.block_on(list_voices(proxy)))
            .join()
            .map_err(|_| Error::Protocol("blocking worker panicked".into()))?
    })
}
#[derive(Debug, Clone)]
/// An in-memory voice catalog supporting exact field-based filtering.
/// No refresh, persistence, or automatic cache expiry is performed.
pub struct VoicesManager {
    /// Owned voice records; callers may update this catalog directly.
    pub voices: Vec<Voice>,
}
impl VoicesManager {
    /// Fetch a catalog using [`list_voices`] with default connection options.
    /// For explicit options, fetch separately and pass records to [`Self::from_voices`].
    ///
    /// # Errors
    /// Propagates errors from [`list_voices`].
    pub async fn create() -> Result<Self> {
        Ok(Self::from_voices(list_voices(None).await?))
    }
    /// Build a manager from caller-provided records without network access.
    pub fn from_voices(voices: Vec<Voice>) -> Self {
        Self { voices }
    }
    /// Return borrowed records matching every criterion, preserving catalog order.
    ///
    /// Keys use service JSON names, such as `Locale`, `Gender`, `ShortName`,
    /// or the computed `Language`. Values use exact, case-sensitive JSON
    /// equality. Unknown keys do not match; empty criteria match all records.
    /// Nested objects must match completely; dotted-path queries are unsupported.
    ///
    /// ```
    /// use edge_tts::{Voice, VoicesManager};
    /// use serde_json::json;
    /// use std::collections::BTreeMap;
    /// let voice: Voice = serde_json::from_value(json!({
    ///     "Name": "Example", "ShortName": "zh-CN-XiaoxiaoNeural",
    ///     "Gender": "Female", "Locale": "zh-CN"
    /// }))?;
    /// let manager = VoicesManager::from_voices(vec![voice]);
    /// let criteria = BTreeMap::from([
    ///     ("Language".into(), json!("zh")),
    ///     ("Gender".into(), json!("Female")),
    /// ]);
    /// assert_eq!(manager.find(&criteria).len(), 1);
    /// # Ok::<(), serde_json::Error>(())
    /// ```
    pub fn find(&self, criteria: &BTreeMap<String, Value>) -> Vec<&Voice> {
        self.voices
            .iter()
            .filter(|v| {
                let value = serde_json::to_value(v).unwrap_or_default();
                criteria.iter().all(|(key, want)| {
                    if key == "Language" {
                        want.as_str() == Some(v.language())
                    } else {
                        value.get(key) == Some(want)
                    }
                })
            })
            .collect()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn binary_frames() {
        assert!(parse_binary_frame(&[0]).is_err());
        assert!(parse_binary_frame(&[0, 100]).is_err());
        let h = b"Path:audio\r\nContent-Type:audio/mpeg\r\n";
        let mut f = (h.len() as u16).to_be_bytes().to_vec();
        f.extend(h);
        f.extend([1, 2, 3]);
        assert_eq!(parse_binary_frame(&f).unwrap(), Some(&[1, 2, 3][..]));
    }
    #[test]
    fn parse_headers() {
        let (h, b) = parse_text_frame(b"Path:audio.metadata\r\nX:one:two\r\n\r\n{}").unwrap();
        assert_eq!(h["X"], "one:two");
        assert_eq!(b, b"{}");
    }
    #[test]
    fn empty_and_reused() {
        assert!(Communicate::new(" \0 ").is_err());
        let mut c = Communicate::new("test").unwrap();
        c.called = true;
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            assert!(matches!(
                c.stream().next().await.unwrap(),
                Err(Error::StreamAlreadyCalled)
            ));
        });
    }
}
