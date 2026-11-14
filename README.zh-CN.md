# edge-tts 使用说明

`edge-tts` 是一个原生 Rust 实现的 Microsoft Edge 在线文本转语音客户端，提供可复用的 `edge_tts` 库，以及 `edge-tts`、`edge-playback` 两个命令行程序。

命名约定：项目名和 Cargo 包名统一为 `edge-tts`；Rust 代码使用 `edge_tts` 导入库，因为 Rust 标识符使用下划线。主合成命令为 `edge-tts`，配套播放命令为 `edge-playback`。

## 功能

- 通过 Edge 语音 WebSocket 服务在线合成语音。
- 输出 MP3，服务格式为 24 kHz、单声道、48 kbit/s。
- 查询音色，并按语言、区域和性别筛选。
- 调整语速、音量和音调。
- 生成句子级或单词级边界事件。
- 生成 SRT 字幕和 JSON Lines 元数据。
- 自动拆分长文本，并保护 Unicode 字符和 XML 实体。
- 支持 HTTP、SOCKS5、SOCKS5H 代理及代理环境变量。
- 提供异步 API 和阻塞式 API。
- Windows 使用原生播放接口；其他平台可以使用 `mpv` 播放。

## 环境要求

- Rust stable 工具链。
- 能访问 Edge 语音服务的网络环境。
- 在非 Windows 系统上，或使用 `--mpv` 时，需要安装 `mpv` 并确保它位于 PATH 中。

## 编译

在项目根目录执行：

```powershell
cargo build --release
```

可执行文件位于 `target/release/`：

- `edge-tts`
- `edge-playback`

查看当前版本实际支持的全部参数：

```powershell
cargo run --bin edge-tts -- --help
cargo run --bin edge-playback -- --help
```

## `edge-tts` 命令

### 合成文本并保存 MP3

```powershell
.\target\release\edge-tts.exe `
  --text "你好，世界。" `
  --voice zh-CN-XiaoxiaoNeural `
  --write-media hello.mp3
```

如果不指定 `--write-media`，MP3 数据会写入标准输出。通常应使用 `--write-media` 保存到文件；当标准输出连接到终端时，程序会先提示，避免直接把二进制数据写到终端。

### 从 UTF-8 文件读取文本

```powershell
.\target\release\edge-tts.exe `
  --file input.txt `
  --voice zh-CN-XiaoxiaoNeural `
  --write-media output.mp3
```

`--file -` 表示从标准输入读取 UTF-8 文本。例如：

```powershell
Get-Content .\input.txt -Raw | .\target\release\edge-tts.exe `
  --file - `
  --write-media output.mp3
```

输入文件不能与输出文件使用同一路径，程序会拒绝覆盖输入文件。

### 调整语速、音量和音调

语速和音量使用带符号的百分比，音调使用带符号的 Hz：

```powershell
.\target\release\edge-tts.exe `
  --text "这是一段慢速、低音量的语音。" `
  --rate=-20% `
  --volume=-10% `
  --pitch=+5Hz `
  --write-media prosody.mp3
```

默认值如下：

| 参数 | 默认值 | 示例 |
|---|---:|---|
| `--rate` | `+0%` | `-20%`、`+15%` |
| `--volume` | `+0%` | `-10%`、`+10%` |
| `--pitch` | `+0Hz` | `-5Hz`、`+5Hz` |

负数参数建议使用等号形式，例如 `--rate=-10%`、`--pitch=-5Hz`，这样可以避免被命令行解析器当成新的选项。

### 生成字幕和元数据

默认生成句子级边界事件：

```powershell
.\target\release\edge-tts.exe `
  --text "第一句。第二句。" `
  --boundary SentenceBoundary `
  --write-media speech.mp3 `
  --write-subtitles speech.srt `
  --write-metadata speech.jsonl
```

生成单词级边界事件：

```powershell
.\target\release\edge-tts.exe `
  --text "Rust generates word timing." `
  --boundary WordBoundary `
  --write-media speech.mp3 `
  --write-subtitles words.srt
```

`--write-subtitles` 输出标准 SRT 字幕。`--write-metadata` 输出 JSON Lines，每行一个边界事件，例如：

```json
{"type":"SentenceBoundary","offset":0,"duration":10000000,"text":"第一句。"}
```

音频块不会写入 JSON Lines 文件。输出文件必须使用不同路径；程序会拒绝重复的输出路径。

将 `-` 作为字幕或元数据路径时，会写入标准错误：

```powershell
.\target\release\edge-tts.exe `
  --text "字幕输出测试" `
  --write-media speech.mp3 `
  --write-subtitles -
```

字幕和元数据不能同时使用 `-`，因为两者都会占用标准错误输出。

### 查询音色

显示表格：

```powershell
.\target\release\edge-tts.exe --list-voices
```

按完整区域筛选：

```powershell
.\target\release\edge-tts.exe --list-voices --locale zh-CN
```

按语言筛选：

```powershell
.\target\release\edge-tts.exe --list-voices --locale zh
```

按性别筛选：

```powershell
.\target\release\edge-tts.exe --list-voices --gender Female
```

组合筛选并输出 JSON：

```powershell
.\target\release\edge-tts.exe `
  --list-voices `
  --locale zh-CN `
  --gender Female `
  --json > voices.json
```

`--locale` 和 `--gender` 都不区分大小写。`--locale` 可以是完整区域，例如 `zh-CN`，也可以是语言代码，例如 `zh`。音色列表中的常用字段包括短名称、性别、内容类别和音色性格。

## `edge-playback` 命令

`edge-playback` 合成后立即播放语音。

### Windows 原生播放

Windows 默认使用系统多媒体接口：

```powershell
.\target\release\edge-playback.exe `
  --text "这是一段播放测试。" `
  --voice zh-CN-XiaoxiaoNeural
```

### 使用 `mpv` 播放

```powershell
.\target\release\edge-playback.exe `
  --text "This is an mpv playback test." `
  --mpv
```

在非 Windows 系统上，播放程序使用 `mpv`。Windows 也可以通过 `--mpv` 强制使用 `mpv`。

播放时可以同时保存音频和字幕：

```powershell
.\target\release\edge-playback.exe `
  --text "保存并播放" `
  --write-media saved.mp3 `
  --write-subtitles saved.srt
```

播放程序支持以下环境变量：

| 环境变量 | 作用 |
|---|---|
| `EDGE_PLAYBACK_MP3_FILE` | 未指定 `--write-media` 时使用的 MP3 路径 |
| `EDGE_PLAYBACK_SRT_FILE` | 未指定 `--write-subtitles` 时使用的字幕路径 |
| `EDGE_PLAYBACK_KEEP_TEMP` | 保留程序自动创建的临时文件 |
| `EDGE_PLAYBACK_DEBUG` | 输出实际使用的临时文件和输出路径 |

## 代理和超时

使用 `--proxy` 指定代理：

```powershell
.\target\release\edge-tts.exe `
  --text "代理测试" `
  --proxy http://127.0.0.1:8080 `
  --write-media proxy.mp3
```

支持的代理协议：

- `http://`
- `socks5://`
- `socks5h://`

HTTP 代理 URL 可以包含基本认证信息。没有指定 `--proxy` 时，程序会依次检查 `HTTPS_PROXY`、`https_proxy`、`ALL_PROXY` 和 `all_proxy`。如果目标地址匹配 `NO_PROXY` 或 `no_proxy`，则绕过环境代理。

连接和接收超时单位都是秒：

```powershell
.\target\release\edge-tts.exe `
  --text "超时参数测试" `
  --connect-timeout 15 `
  --receive-timeout 120 `
  --write-media timeout.mp3
```

参数说明：

| 参数 | 默认值 | 说明 |
|---|---:|---|
| `--connect-timeout` | `10` | 建立连接的最长等待时间 |
| `--receive-timeout` | `60` | 发送或接收服务数据的最长等待时间 |
| `--no-progress` | 关闭 | 不在交互式终端的标准错误上显示进度 |

## Rust 库 API

在另一个 Rust 项目中使用本地库：

```toml
[dependencies]
edge-tts = { path = ".." }
tokio = { version = "1", features = ["full"] }
futures-util = "0.3"
serde_json = "1"
```

### API 参考与示例验证

执行 `cargo doc --no-deps` 生成完整 Rust API 参考；需要由 Cargo 打开时使用
`cargo doc --no-deps --open`，库入口名称为 `edge_tts`。
执行 `cargo test --doc` 会编译 rustdoc 中的全部示例并运行离线示例。
联网和输出文件的示例标记为 `no_run`，只编译，不会在文档测试时请求服务或写文件。

API 参考覆盖公开类型和方法、配置字段、事件单位与 JSON 格式、错误枚举、
文本辅助函数和签名辅助函数。建议从库首页开始，依次阅读 `Communicate`、
`TtsConfig`、`TtsChunk`、`SubMaker` 和 `VoicesManager`。

| 操作 | 异步 API | 阻塞 API |
|---|---|---|
| 保存 MP3 与可选 JSONL | `Communicate::save` | `Communicate::save_sync` |
| 消费音频与边界事件 | `Communicate::stream` | `Communicate::stream_sync` |
| 获取音色目录 | `list_voices`、`list_voices_with_timeout` | `list_voices_sync` |
| 筛选内存音色 | `VoicesManager::find` | 同一本地方法 |
| 生成 SRT | `SubMaker::feed`、`get_srt` | 同一本地方法 |

库的保存方法会创建或截断文件，不自动创建父目录，也不检查路径冲突，必须提供不同的输出路径。
失败时可能保留部分文件；复用实例时，也可能先截断文件再报告实例已经使用。
所有合成入口共享单次消费状态，重试需要新建实例。异步流首次轮询时才开始工作；
丢弃阻塞迭代器会断开通道，后台线程可能需要等正在进行的网络操作完成或超时才退出。

合成的 `receive_timeout` 对每次发送或接收单独计时；
`list_voices_with_timeout` 的 `receive` 参数是每次 HTTP 请求的整体时限。
阻塞方法即使在异步代码中调用也会阻塞当前线程。
`VoicesManager::find` 按 JSON 字段精确且区分大小写地匹配，与 CLI 筛选的规则不同。

按错误类型进行处理：

```rust,no_run
use edge_tts::{Communicate, Error};

fn main() -> edge_tts::Result<()> {
    let mut tts = Communicate::new("处理语音合成错误。")?;
    match tts.save_sync("speech.mp3", None) {
        Ok(()) => Ok(()),
        Err(Error::Timeout(stage)) => {
            eprintln!("合成阶段超时：{stage}");
            Err(Error::Timeout(stage))
        }
        Err(error) => Err(error),
    }
}
```

### 可运行示例

项目自带 `examples/` 目录，示例可以直接通过 Cargo 运行。示例会访问在线 Edge 语音服务，因此运行时需要网络连接：

```powershell
# 异步合成，生成 speech.mp3 和 speech.jsonl
cargo run --example async_save -- "你好，Rust。" speech.mp3 speech.jsonl

# 消费流式音频，生成 stream.mp3 和 stream.srt
cargo run --example stream_to_files -- "第一句。第二句。"

# 查询并筛选中文女声音色
cargo run --example list_voices -- zh-CN Female
```

也可以只编译示例而不运行网络请求：

```powershell
cargo check --examples
```

`async_save` 演示 `Communicate::save` 和 JSONL 元数据；`stream_to_files` 演示逐块处理 `TtsChunk`、写入 MP3 和用 `SubMaker` 生成 SRT；`list_voices` 演示 `VoicesManager` 的精确字段筛选。

### 异步合成并保存

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

`Communicate::save` 会把音频写入指定路径，并可选地把边界事件写入 JSON Lines 文件。一个 `Communicate` 实例只能调用一次 `stream`、`save` 或 `stream_sync`。

### 消费异步流

```rust
use edge_tts::{Communicate, TtsChunk};
use futures_util::StreamExt;

#[tokio::main]
async fn main() -> edge_tts::Result<()> {
    let mut tts = Communicate::new("流式语音合成")?;
    let mut stream = tts.stream();

    while let Some(chunk) = stream.next().await {
        match chunk? {
            TtsChunk::Audio { data } => {
                // 写入文件、解码器或应用自己的缓冲区。
                println!("收到 {} 字节音频", data.len());
            }
            event => println!("边界事件：{event:?}"),
        }
    }
    Ok(())
}
```

同步程序可以使用 `save_sync` 或 `stream_sync`。`stream_sync` 返回一个阻塞式迭代器，释放迭代器后后台生产任务会停止发送数据。

### 查询和筛选音色

```rust
use edge_tts::VoicesManager;
use serde_json::json;
use std::collections::BTreeMap;

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

`Voice` 包含 `name`、`short_name`、`gender`、`locale`、`friendly_name`、`suggested_codec`、`status`、`voice_tag` 以及上游返回的其他字段。`Voice::language()` 返回 locale 中的语言部分，例如 `zh-CN` 对应 `zh`。

### 生成 SRT 字幕

`SubMaker` 接收单词级或句子级边界事件，并生成标准 SRT 文本：

```rust
use edge_tts::{SubMaker, TtsChunk};

fn make_subtitle() -> edge_tts::Result<String> {
    let mut subtitles = SubMaker::new();
    subtitles.feed(&TtsChunk::SentenceBoundary {
        offset: 0,
        duration: 10_000_000,
        text: "你好".into(),
    })?;
    Ok(subtitles.get_srt())
}
```

偏移量和时长使用 100 纳秒为单位，与服务协议一致。一个 `SubMaker` 实例不能混用单词级和句子级事件。

## 输出和协议说明

语音以 MP3 数据块的形式流式返回，服务输出格式为 24 kHz、单声道、48 kbit/s。长文本会按服务请求限制拆分，同时避免切断 Unicode 字符和 XML 实体。字幕时间戳会根据已经收到的音频字节数跨文本分段进行补偿。

客户端会计算 `Sec-MS-GEC` v2 请求签名。服务返回带有有效 `Date` 响应头的 HTTP 403 时，客户端会根据服务端时间修正时钟偏差并自动重试一次。

## 常见问题

### 返回 403 或签名错误

检查系统时间和网络连接。客户端在响应包含 `Date` 头时会自动做一次时钟偏差校正。如果仍然失败，检查代理配置和服务访问情况。

### 没有收到音频

确认音色名称正确，并检查当前网络或代理是否可以访问 Edge 语音服务。

### 语速、音量或音调参数无效

使用带符号的格式，例如 `-10%`、`+20%` 或 `-5Hz`。命令行中建议写成 `--rate=-10%`，避免负值被解析成新的命令行选项。

### 播放失败

Windows 优先检查系统多媒体播放接口和 MP3 文件；其他系统请安装 `mpv`。Windows 也可以使用 `--mpv` 测试外部播放器。

### 输出覆盖输入

更换输出路径。程序会拒绝把输入文件和输出文件设置为同一路径，也会拒绝多个输出参数使用同一路径。

## 已知限制

- 服务依赖 Microsoft Edge 在线语音接口，需要网络连接。
- 播放能力依赖 Windows 系统接口或外部 `mpv` 程序。
- 接口使用的是服务当前公开协议；上游服务参数变化时可能需要同步调整签名版本和请求字段。
- 文档示例默认使用 PowerShell；Linux 和 macOS 使用对应 shell 的换行和可执行文件写法即可。

## 许可证

项目代码采用 MIT 许可证。项目中加入许可证文件后，请以该文件为准。
