use super::MessageBag;
use crate::contracts::support::MessageProvider;
use std::borrow::Cow;

/// 现有消息包保持借用；数组输入只在接入边界构造成消息包。
pub struct MessageSource<'a>(Cow<'a, MessageBag>);
impl AsRef<MessageBag> for MessageSource<'_> {
    fn as_ref(&self) -> &MessageBag {
        &self.0
    }
}
impl<'a, T: MessageProvider + ?Sized> From<&'a T> for MessageSource<'a> {
    fn from(provider: &'a T) -> Self {
        Self(Cow::Borrowed(provider.get_message_bag()))
    }
}
impl From<MessageBag> for MessageSource<'_> {
    fn from(messages: MessageBag) -> Self {
        Self(Cow::Owned(messages))
    }
}
impl<K: Into<String> + Ord, V: IntoMessageLines> From<std::collections::BTreeMap<K, V>>
    for MessageSource<'_>
{
    fn from(messages: std::collections::BTreeMap<K, V>) -> Self {
        Self(Cow::Owned(MessageBag::new(
            messages
                .into_iter()
                .map(|(key, value)| (key, value.into_message_lines())),
        )))
    }
}
impl<K: Into<String>, V: IntoMessageLines, const N: usize> From<[(K, V); N]> for MessageSource<'_> {
    fn from(messages: [(K, V); N]) -> Self {
        Self(Cow::Owned(MessageBag::new(
            messages
                .into_iter()
                .map(|(key, value)| (key, value.into_message_lines())),
        )))
    }
}

/// 错误数组的单条或多条消息，保留字符串输入的原意。
pub trait IntoMessageLines {
    fn into_message_lines(self) -> Vec<String>;
}
impl IntoMessageLines for String {
    fn into_message_lines(self) -> Vec<String> {
        vec![self]
    }
}
impl IntoMessageLines for &str {
    fn into_message_lines(self) -> Vec<String> {
        vec![self.into()]
    }
}
impl<S: Into<String>> IntoMessageLines for Vec<S> {
    fn into_message_lines(self) -> Vec<String> {
        self.into_iter().map(Into::into).collect()
    }
}
impl<S: Into<String>, const N: usize> IntoMessageLines for [S; N] {
    fn into_message_lines(self) -> Vec<String> {
        self.into_iter().map(Into::into).collect()
    }
}
