use crate::{Boundary, Error, Result, TtsChunk};
#[derive(Debug, Clone, PartialEq, Eq)]
/// One subtitle cue. All timestamps use 100-nanosecond ticks.
pub struct Subtitle {
    /// One-based insertion index; SRT rendering renumbers nonempty valid cues.
    pub index: usize,
    /// Inclusive start offset from the combined audio origin, in ticks.
    pub start: u64,
    /// End offset in ticks, computed as offset plus duration when fed.
    pub end: u64,
    /// Plain subtitle text.
    pub content: String,
}
#[derive(Debug, Default, Clone)]
/// Collect timing events of a single kind and render SRT.
///
/// Cues retain arrival order; they are not sorted, merged, or de-overlapped.
///
/// ```
/// use edge_tts::{SubMaker, TtsChunk};
/// let mut maker = SubMaker::new();
/// maker.feed(&TtsChunk::SentenceBoundary {
///     offset: 10_000_000,
///     duration: 5_000_000,
///     text: "Hello".into(),
/// })?;
/// assert_eq!(maker.get_srt(), "1\n00:00:01,000 --> 00:00:01,500\nHello\n\n");
/// # Ok::<(), edge_tts::Error>(())
/// ```
pub struct SubMaker {
    /// Collected cues. Direct edits bypass event validation and boundary-kind tracking.
    pub cues: Vec<Subtitle>,
    kind: Option<Boundary>,
}
impl SubMaker {
    /// Create an empty subtitle collector; equivalent to `default()`.
    pub fn new() -> Self {
        Self::default()
    }
    /// Append a word or sentence event; the first accepted event fixes the kind.
    ///
    /// # Errors
    /// Returns [`Error::InvalidConfig`] for audio or mixed boundary kinds, and
    /// [`Error::Protocol`] if offset plus duration overflows `u64`.
    pub fn feed(&mut self, event: &TtsChunk) -> Result<()> {
        let (kind, offset, duration, text) = event
            .boundary()
            .ok_or_else(|| Error::InvalidConfig("expected a boundary event".into()))?;
        if self.kind.is_some_and(|k| k != kind) {
            return Err(Error::InvalidConfig(
                "cannot mix sentence and word boundaries".into(),
            ));
        }
        let end = offset
            .checked_add(duration)
            .ok_or_else(|| Error::Protocol("subtitle timestamp overflow".into()))?;
        self.kind = Some(kind);
        self.cues.push(Subtitle {
            index: self.cues.len() + 1,
            start: offset,
            end,
            content: text.to_owned(),
        });
        Ok(())
    }
    /// Render cues as SRT with LF newlines and a blank line after each cue.
    ///
    /// Drops blank text and cues whose end is not after start, trims text, normalizes
    /// line endings, and renumbers retained cues from one. Ticks are truncated to
    /// milliseconds. Very short positive cues may render equal start/end times.
    /// An empty collector produces an empty string. `Display` uses the same renderer.
    pub fn get_srt(&self) -> String {
        let mut out = String::new();
        let mut index = 1;
        for cue in &self.cues {
            if cue.end <= cue.start || cue.content.trim().is_empty() {
                continue;
            }
            out.push_str(&format!(
                "{index}\n{} --> {}\n{}\n\n",
                stamp(cue.start),
                stamp(cue.end),
                cue.content.replace("\r\n", "\n").replace('\r', "\n").trim()
            ));
            index += 1;
        }
        out
    }
}
impl std::fmt::Display for SubMaker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.get_srt())
    }
}
fn stamp(ticks: u64) -> String {
    let ms = ticks / 10000;
    format!(
        "{:02}:{:02}:{:02},{:03}",
        ms / 3600000,
        ms / 60000 % 60,
        ms / 1000 % 60,
        ms % 1000
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn timestamps_and_types() {
        let mut s = SubMaker::new();
        s.feed(&TtsChunk::WordBoundary {
            offset: 10_000_000,
            duration: 5_000_000,
            text: "你好".into(),
        })
        .unwrap();
        assert_eq!(s.get_srt(), "1\n00:00:01,000 --> 00:00:01,500\n你好\n\n");
        assert!(
            s.feed(&TtsChunk::SentenceBoundary {
                offset: 0,
                duration: 1,
                text: "x".into()
            })
            .is_err()
        );
        assert!(s.feed(&TtsChunk::Audio { data: vec![] }).is_err());
    }
}
