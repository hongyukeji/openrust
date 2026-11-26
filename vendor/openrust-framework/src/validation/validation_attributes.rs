use std::borrow::Cow;

/// 条件验证接受一个字段或有序字段列表；单字段借用不分配字符串。
pub enum ValidationAttributes<'a> {
    One(Cow<'a, str>),
    Many(Vec<Cow<'a, str>>),
}
impl<'a> From<&'a str> for ValidationAttributes<'a> {
    fn from(value: &'a str) -> Self {
        Self::One(Cow::Borrowed(value))
    }
}
impl<'a> From<&'a String> for ValidationAttributes<'a> {
    fn from(value: &'a String) -> Self {
        Self::One(Cow::Borrowed(value.as_str()))
    }
}
impl From<String> for ValidationAttributes<'_> {
    fn from(value: String) -> Self {
        Self::One(Cow::Owned(value))
    }
}
impl<'a> From<Cow<'a, str>> for ValidationAttributes<'a> {
    fn from(value: Cow<'a, str>) -> Self {
        Self::One(value)
    }
}
impl<'a, T: Into<Cow<'a, str>>> From<Vec<T>> for ValidationAttributes<'a> {
    fn from(values: Vec<T>) -> Self {
        Self::Many(values.into_iter().map(Into::into).collect())
    }
}
impl<'a, T: Into<Cow<'a, str>>, const N: usize> From<[T; N]> for ValidationAttributes<'a> {
    fn from(values: [T; N]) -> Self {
        Self::Many(values.into_iter().map(Into::into).collect())
    }
}
impl<'a, T: AsRef<str>> From<&'a [T]> for ValidationAttributes<'a> {
    fn from(values: &'a [T]) -> Self {
        Self::Many(
            values
                .iter()
                .map(|value| Cow::Borrowed(value.as_ref()))
                .collect(),
        )
    }
}
impl<'a> IntoIterator for ValidationAttributes<'a> {
    type Item = Cow<'a, str>;
    type IntoIter =
        std::iter::Chain<std::option::IntoIter<Self::Item>, std::vec::IntoIter<Self::Item>>;
    fn into_iter(self) -> Self::IntoIter {
        let (one, many) = match self {
            Self::One(value) => (Some(value), Vec::new()),
            Self::Many(values) => (None, values),
        };
        one.into_iter().chain(many)
    }
}
