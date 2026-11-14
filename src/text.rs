//! Plain-text cleanup, XML escaping, safe chunk splitting, and SSML assembly.
//!
//! Most callers should pass plain text to [`crate::Communicate`] instead of
//! calling these helpers. Escaping an already escaped string escapes it again.
use crate::{Error, Result, TtsConfig};
/// Escape ampersand, angle brackets, apostrophe, and quote as XML entities.
///
/// ```
/// assert_eq!(edge_tts::text::escape("A&B <x>"), "A&amp;B &lt;x&gt;");
/// ```
pub fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('\'', "&apos;")
        .replace('"', "&quot;")
}
/// Decode the five named XML entities handled by [`escape`], once.
/// Numeric and unknown entities are left unchanged. This is not a full XML parser.
pub fn unescape(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&apos;", "'")
        .replace("&quot;", "\"")
        .replace("&amp;", "&")
}
/// Replace U+0000–U+0008, U+000B–U+000C, and U+000E–U+001F with spaces.
/// Tabs, line feeds, and carriage returns are preserved; no trimming is performed.
pub fn remove_incompatible_characters(text: &str) -> String {
    text.chars()
        .map(|c| {
            if matches!(c as u32,0..=8|11..=12|14..=31) {
                ' '
            } else {
                c
            }
        })
        .collect()
}
/// Split already XML-escaped text into chunks of at most `limit` UTF-8 bytes.
///
/// Prefer a newline, then a space, within the byte limit. Trim each chunk and
/// omit empty chunks; whitespace at split points is not preserved. Protects
/// Unicode scalar boundaries, not grapheme clusters. Input should contain
/// well-formed escaped entities; this function does not validate XML.
///
/// # Errors
/// Returns [`Error::InvalidConfig`] for a zero limit or when the next Unicode
/// scalar/entity cannot fit in a chunk.
///
/// ```
/// use edge_tts::text::{escape, split_text_by_byte_length};
/// let parts = split_text_by_byte_length(&escape("你好&世界"), 8)?;
/// assert!(parts.iter().all(|part| part.len() <= 8));
/// assert_eq!(parts.concat(), "你好&amp;世界");
/// # Ok::<(), edge_tts::Error>(())
/// ```
pub fn split_text_by_byte_length(mut text: &str, limit: usize) -> Result<Vec<String>> {
    if limit == 0 {
        return Err(Error::InvalidConfig("chunk size must be positive".into()));
    }
    let mut out = Vec::new();
    while text.len() > limit {
        let mut end = limit;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        if let Some(i) = text[..end].rfind('\n').or_else(|| text[..end].rfind(' '))
            && i > 0
        {
            end = i;
        }
        if let Some(i) = text[..end].rfind('&')
            && !text[i..end].contains(';')
        {
            end = i;
        }
        if end == 0 {
            return Err(Error::InvalidConfig(
                "chunk size is smaller than a character or XML entity".into(),
            ));
        }
        let part = text[..end].trim();
        if !part.is_empty() {
            out.push(part.to_owned());
        }
        text = &text[end..];
    }
    if !text.trim().is_empty() {
        out.push(text.trim().to_owned());
    }
    Ok(out)
}
/// Assemble the service SSML wrapper from a validated config and escaped text.
///
/// Call [`TtsConfig::validate`] first and [`escape`] on plain text. This helper
/// does not validate options or escape its text argument; voice names are escaped.
/// The wrapper uses `xml:lang='en-US'`, with the selected voice determining speech.
pub fn mkssml(config: &TtsConfig, escaped_text: &str) -> String {
    format!(
        "<speak version='1.0' xmlns='http://www.w3.org/2001/10/synthesis' xml:lang='en-US'><voice name='{}'><prosody pitch='{}' rate='{}' volume='{}'>{}</prosody></voice></speak>",
        escape(&config.voice),
        config.pitch,
        config.rate,
        config.volume,
        escaped_text
    )
}
pub(crate) fn validate(config: &TtsConfig) -> Result<TtsConfig> {
    let mut c = config.clone();
    let re = regex::Regex::new(r"^([a-z]{2,})-([A-Z]{2,})-(.+Neural)$").unwrap();
    if let Some(m) = re.captures(&c.voice) {
        let (lang, mut region, mut name) = (&m[1], m[2].to_string(), m[3].to_string());
        if let Some((r, n)) = name.split_once('-') {
            region = format!("{region}-{r}");
            name = n.to_string();
        }
        c.voice = format!("Microsoft Server Speech Text to Speech Voice ({lang}-{region}, {name})");
    }
    for (name, value, pattern) in [
        (
            "voice",
            c.voice.as_str(),
            r"^Microsoft Server Speech Text to Speech Voice \(.+,.+\)$",
        ),
        ("rate", &c.rate, r"^[+-][0-9]+%$"),
        ("volume", &c.volume, r"^[+-][0-9]+%$"),
        ("pitch", &c.pitch, r"^[+-][0-9]+Hz$"),
    ] {
        if !regex::Regex::new(pattern).unwrap().is_match(value) {
            return Err(Error::InvalidConfig(format!("invalid {name}: {value}")));
        }
    }
    if c.connect_timeout.is_zero() || c.receive_timeout.is_zero() {
        return Err(Error::InvalidConfig("timeouts must be positive".into()));
    }
    Ok(c)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unicode_entities() {
        let t = escape(&"你好<&😀".repeat(2000));
        let parts = split_text_by_byte_length(&t, 4096).unwrap();
        assert_eq!(parts.concat(), t);
        assert!(parts.iter().all(|p| p.len() <= 4096 && !p.ends_with('&')));
        assert!(split_text_by_byte_length("😀", 1).is_err());
        assert!(split_text_by_byte_length("&amp;", 2).is_err());
    }
    #[test]
    fn controls() {
        assert_eq!(remove_incompatible_characters("a\0\u{b}\t\n"), "a  \t\n");
    }
    #[test]
    fn voice_config() {
        let c = validate(&TtsConfig {
            voice: "zh-CN-liaoning-XiaobeiNeural".into(),
            ..Default::default()
        })
        .unwrap();
        assert!(c.voice.contains("(zh-CN-liaoning, XiaobeiNeural)"));
        assert!(
            validate(&TtsConfig {
                rate: "fast".into(),
                ..Default::default()
            })
            .is_err()
        );
    }
}
