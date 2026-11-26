use std::borrow::Cow;
use unicode_segmentation::UnicodeSegmentation;

fn starts(text: &str) -> Vec<usize> {
    text.grapheme_indices(true)
        .map(|(position, _)| position)
        .collect()
}

pub(in crate::console) fn length(text: &str) -> usize {
    text.graphemes(true).count()
}

/// 只为可见片段建立规范化字素索引，解析样式仍使用原始标签。
pub(in crate::console) struct Slices<'a> {
    text: Cow<'a, str>,
    original: Vec<usize>,
    normalized: Vec<usize>,
}
impl<'a> Slices<'a> {
    pub fn new(text: &'a str) -> Self {
        let normalized = icu_normalizer::ComposingNormalizer::new_nfc().normalize(text);
        let original = starts(text);
        let mut bounds = if matches!(normalized, Cow::Borrowed(_)) {
            original.clone()
        } else {
            starts(normalized.as_ref())
        };
        bounds.push(normalized.len());
        Self {
            text: normalized,
            original,
            normalized: bounds,
        }
    }
    pub fn prefix_length(&self, byte: usize) -> usize {
        self.original.partition_point(|position| *position < byte)
    }
    pub fn slice(&self, start: usize, end: Option<usize>) -> &str {
        let count = self.normalized.len() - 1;
        let end = match end {
            Some(end) if end < start => count.saturating_sub(start - end),
            Some(end) => end.min(count),
            None => count,
        };
        if start >= count || start > end {
            return "";
        }
        &self.text[self.normalized[start]..self.normalized[end]]
    }
}
