# edge-tts

**Project name: `edge-tts`**

A native Rust client and command-line interface for Microsoft's Edge online text-to-speech service. The package provides the `edge_tts` library together with `edge-tts` and `edge-playback` binaries.

Naming: the project, repository, and Cargo package are `edge-tts`; Rust code imports the library as `edge_tts` because Rust identifiers use underscores. The main synthesis command is `edge-tts`, and the companion playback command is `edge-playback`.

## Features

- Online synthesis through the Edge speech WebSocket service.
- MP3 output in the service's `audio-24khz-48kbitrate-mono-mp3` format.
- Voice listing with locale, language, and gender filters.
- Rate, volume, and pitch control.
- Sentence-boundary and word-boundary metadata.
- SRT subtitle generation and JSON Lines metadata output.
- Long-text splitting with Unicode and XML-entity safety.
- HTTP, SOCKS5, and SOCKS5H proxy support, including proxy environment variables.
- Async and blocking Rust APIs.
- Windows native playback and optional `mpv` playback on other platforms.

## Requirements

- Rust stable toolchain.
- Network access to the Edge speech service.
- An `mpv` executable when `edge-playback --mpv` is used or when playback is required on non-Windows systems.

## Build

```text
cargo build --release
```

The binaries are produced in `target/release/`:

- `edge-tts`
- `edge-playback`

Run the command help page for the authoritative option list:

```text
cargo run --bin edge-tts -- --help
cargo run --bin edge-playback -- --help
```

## Command-line usage

### Synthesize text to an MP3 file

```text
edge-tts --text "Hello from Rust" \
  --voice en-US-EmmaMultilingualNeural \
  --write-media hello.mp3
```

On Windows PowerShell, the same command can be written on one line or continued with a backtick. The program writes MP3 bytes to standard output when `--write-media` is omitted. Save the result to a file for normal use.

### Read UTF-8 text from a file

```text
edge-tts --file input.txt \
  --voice zh-CN-XiaoxiaoNeural \
  --write-media output.mp3
```

Use `--file -` to read UTF-8 text from standard input:

```text
type input.txt | edge-tts --file - --write-media output.mp3
```

### Set prosody

The accepted forms are signed percentages for rate and volume, and signed hertz for pitch:

```text
edge-tts --text "A slower, quieter voice" \
  --rate=-20% \
  --volume=-10% \
  --pitch=+5Hz \
  --write-media prosody.mp3
```

The default values are `+0%`, `+0%`, and `+0Hz`.

### Generate subtitles and metadata

Sentence boundaries are selected by default:

```text
edge-tts --text "第一句。第二句。" \
  --boundary SentenceBoundary \
  --write-media speech.mp3 \
  --write-subtitles speech.srt \
  --write-metadata speech.jsonl
```

For word-level events:

```text
edge-tts --text "Generate word timing." \
  --boundary WordBoundary \
  --write-media speech.mp3 \
  --write-subtitles words.srt
```

The JSON Lines file contains one event per line. Audio chunks are not written to this file; boundary events are serialized as JSON objects with `type`, `offset`, `duration`, and `text` fields.

Use `--write-subtitles -` to write SRT to standard error. Use `--write-metadata -` to write JSON Lines to standard error. These two outputs cannot both use `-` at the same time.

### List voices

Human-readable output:

```text
edge-tts --list-voices
edge-tts --list-voices --locale zh-CN
edge-tts --list-voices --locale en --gender Female
```

JSON output:

```text
edge-tts --list-voices --locale zh-CN --json > voices.json
```

`--locale` accepts a complete locale such as `zh-CN` or a language such as `zh`. Filtering is case-insensitive.

### Playback

`edge-playback` synthesizes and immediately plays the result. Windows uses the native multimedia API by default:

```text
edge-playback --text "This is a playback test." \
  --voice en-US-EmmaMultilingualNeural
```

Use `--mpv` to force playback through `mpv`:

```text
edge-playback --text "播放测试" --mpv
```

On non-Windows platforms, `mpv` is used. Optional output files can be kept with `--write-media` and `--write-subtitles`.

The playback binary also recognizes these environment variables:

- `EDGE_PLAYBACK_MP3_FILE`: default MP3 output path.
- `EDGE_PLAYBACK_SRT_FILE`: default subtitle output path.
- `EDGE_PLAYBACK_KEEP_TEMP`: keep automatically created temporary files.
- `EDGE_PLAYBACK_DEBUG`: print selected temporary and output paths.

### Proxy and timeouts

An explicit proxy can be supplied with `--proxy`:

```text
edge-tts --text "Proxy test" \
  --proxy http://127.0.0.1:8080 \
  --write-media proxy.mp3
```

Supported explicit proxy schemes are `http`, `socks5`, and `socks5h`. HTTP proxy URLs may include basic-auth credentials. If `--proxy` is omitted, the client checks `HTTPS_PROXY`, `https_proxy`, `ALL_PROXY`, and `all_proxy`, unless the target is covered by `NO_PROXY` or `no_proxy`.

Timeouts are in seconds:

```text
edge-tts --text "Timeout test" \
  --connect-timeout 15 \
  --receive-timeout 120 \
  --write-media timeout.mp3
```

`--no-progress` disables the interactive progress indicator on stderr.

## Library API

Add the local crate as a dependency when embedding the library in another Rust application:

```toml
[dependencies]
edge-tts = { path = ".." }
tokio = { version = "1", features = ["full"] }
futures-util = "0.3"
serde_json = "1"
```

### API reference and verified examples

Build the Rust API reference with `cargo doc --no-deps`. To open it with Cargo, run `cargo doc --no-deps --open`; the library entry is `edge_tts`. Run `cargo test --doc` to compile all rustdoc examples and execute offline ones. Network and file-output examples are marked `no_run`: they are compiled, not executed by documentation tests.

The reference documents all exported library types and methods, configuration fields, event units, JSON representation, error variants, text helpers, and signing helpers. Start with the crate overview, then `Communicate`, `TtsConfig`, `TtsChunk`, `SubMaker`, and `VoicesManager`.

| Operation | Async API | Blocking API |
|---|---|---|
| Save MP3 and optional JSONL | `Communicate::save` | `Communicate::save_sync` |
| Consume audio/timing events | `Communicate::stream` | `Communicate::stream_sync` |
| Fetch voice catalog | `list_voices`, `list_voices_with_timeout` | `list_voices_sync` |
| Filter cached records | `VoicesManager::find` | Same local method |
| Render SRT | `SubMaker::feed`, `get_srt` | Same local methods |

Library save methods create or truncate files, do not create parent directories, and do not check for path collisions. Use distinct output paths. Failures can leave partial files, even when a reused instance fails after file creation. All synthesis entry points share one single-use state; construct a new request for a retry. Async streams begin on first poll. Dropping a blocking iterator disconnects its channel; worker exit may wait for an in-flight network operation.

Synthesis `receive_timeout` applies to individual sends/receives, while `list_voices_with_timeout` uses a total HTTP request deadline for its `receive` argument. Blocking methods block their caller, even when called from async code. `VoicesManager::find` uses case-sensitive exact JSON matching, unlike CLI filters.

Example of handling errors without discarding their type:

```rust,no_run
use edge_tts::{Communicate, Error};

fn main() -> edge_tts::Result<()> {
    let mut tts = Communicate::new("Handle errors explicitly.")?;
    match tts.save_sync("speech.mp3", None) {
        Ok(()) => Ok(()),
        Err(Error::Timeout(stage)) => {
            eprintln!("Synthesis timed out during {stage}");
            Err(Error::Timeout(stage))
        }
        Err(error) => Err(error),
    }
}
```

### Runnable examples

The repository includes three executable examples under `examples/`. They call the online Edge speech service, so running them requires network access:

```text
# Async synthesis, writing speech.mp3 and speech.jsonl
cargo run --example async_save -- "Hello from Rust." speech.mp3 speech.jsonl

# Stream audio and write stream.mp3 plus stream.srt
cargo run --example stream_to_files -- "First sentence. Second sentence."

# Fetch and filter Chinese female voices
cargo run --example list_voices -- zh-CN Female
```

Compile every example without making network requests with:

```text
cargo check --examples
```

`async_save` demonstrates `Communicate::save` and JSONL metadata. `stream_to_files` demonstrates processing `TtsChunk` values, writing MP3 chunks, and creating SRT with `SubMaker`. `list_voices` demonstrates exact field filtering with `VoicesManager`.

### Async synthesis and save

```rust
use edge_tts::{Communicate, TtsConfig};

#[tokio::main]
async fn main() -> edge_tts::Result<()> {
    let config = TtsConfig {
        voice: "zh-CN-XiaoxiaoNeural".into(),
        rate: "+10%".into(),
        ..TtsConfig::default()
    };

    let mut tts = Communicate::with_config("你好，Rust。", config)?;
    tts.save("speech.mp3", Some(std::path::Path::new("speech.jsonl"))).await?;
    Ok(())
}
```

`Communicate::save` writes audio to the selected path and optionally writes boundary events as JSON Lines. A `Communicate` value can be streamed only once.

### Consume a stream

```rust
use futures_util::StreamExt;
use edge_tts::{Communicate, TtsChunk};

#[tokio::main]
async fn main() -> edge_tts::Result<()> {
    let mut tts = Communicate::new("Streaming synthesis")?;
    let mut stream = tts.stream();

    while let Some(chunk) = stream.next().await {
        match chunk? {
            TtsChunk::Audio { data } => {
                // Send data to a file, decoder, or application buffer.
                println!("received {} bytes", data.len());
            }
            event => println!("boundary event: {event:?}"),
        }
    }
    Ok(())
}
```

For synchronous applications, use `stream_sync` or `save_sync`.

### List and filter voices

```rust
use std::collections::BTreeMap;
use serde_json::json;
use edge_tts::VoicesManager;

#[tokio::main]
async fn main() -> edge_tts::Result<()> {
    let manager = VoicesManager::create().await?;
    let mut criteria = BTreeMap::new();
    criteria.insert("Language".into(), json!("zh"));
    criteria.insert("Gender".into(), json!("Female"));

    for voice in manager.find(&criteria) {
        println!("{} ({})", voice.short_name, voice.locale);
    }
    Ok(())
}
```

`Voice` includes `name`, `short_name`, `gender`, `locale`, `friendly_name`, `suggested_codec`, `status`, `voice_tag`, and additional upstream fields. `Voice::language()` returns the language portion of the locale.

### Generate SRT from boundary events

`SubMaker` accepts word or sentence boundary events of one kind and produces standard SRT text:

```rust
use edge_tts::{SubMaker, TtsChunk};

let mut subtitles = SubMaker::new();
subtitles.feed(&TtsChunk::SentenceBoundary {
    offset: 0,
    duration: 10_000_000,
    text: "Hello".into(),
})?;
println!("{}", subtitles.get_srt());
# Ok::<(), edge_tts::Error>(())
```

Offsets and durations use 100-nanosecond ticks, matching the service protocol.

## Output and protocol details

Audio is streamed in MP3 chunks. The service output format is 24 kHz, mono, 48 kbit/s. Text is split into safe chunks for the service request limit, and subtitle timestamps are compensated across chunks according to the received audio size.

The client computes the current `Sec-MS-GEC` v2 request signature. If the service returns HTTP 403 with a valid `Date` header, it corrects the local clock skew and retries once.

## Troubleshooting

- **403 or signature errors:** check the system clock and network connectivity. The client performs one automatic clock-skew correction when the response includes a `Date` header.
- **No audio:** verify that the selected voice exists and that the service is reachable through the configured network or proxy.
- **Invalid rate, volume, or pitch:** use values such as `-10%`, `+20%`, or `-5Hz`.
- **Playback fails on non-Windows:** install `mpv` and pass `--mpv`.
- **Output overwrites input:** choose a different output path; the CLI rejects identical input and output paths.

## License

This project is distributed under the MIT license. See the repository license file when one is added to the project.
