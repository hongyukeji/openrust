// 第三方许可见 ../LICENSE.renderer。
use crate::{console::ConsoleError, support::input_scalars};
/// 类分隔符只在右侧下一个左括号之前没有右括号时生效。
fn parts(input: &str) -> Vec<&str> {
    let mut opening = None;
    let mut closing = None;
    let mut boundaries = vec![];
    for (index, byte) in input.bytes().enumerate().rev() {
        match byte {
            b'[' => opening = Some(index),
            b']' => closing = Some(index),
            _ => {}
        }
        if byte == b'-' && closing.is_none_or(|end| opening.is_some_and(|start| end >= start)) {
            boundaries.push(index);
        }
    }
    let mut start = 0;
    let mut result = vec![];
    for end in boundaries
        .into_iter()
        .rev()
        .chain(std::iter::once(input.len()))
    {
        result.push(&input[start..end]);
        start = end + 1;
    }
    result
}
/// 终端样式回调的字符串或整数参数。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StyleArgument<'a> {
    Text(&'a str),
    Integer(i64),
}
impl<'a> StyleArgument<'a> {
    fn new(value: &'a str) -> Self {
        if input_scalars::numeric_prefix(value) == Some(value) {
            Self::Integer(input_scalars::integer_string(value))
        } else {
            Self::Text(value)
        }
    }
    pub(super) fn text(&self) -> Result<&'a str, ConsoleError> {
        match self {
            Self::Text(v) => Ok(v),
            _ => Err(ConsoleError::new("组件样式需要文本参数")),
        }
    }
    pub(super) fn integer(&self) -> Result<i64, ConsoleError> {
        match self {
            Self::Integer(v) => Ok(*v),
            _ => Err(ConsoleError::new("组件样式需要整数参数")),
        }
    }
}
pub(super) struct Method<'a> {
    pub name: String,
    pub args: Vec<StyleArgument<'a>>,
}
pub(super) struct MethodCall<'a> {
    pub name: &'a str,
    pub args: &'a [StyleArgument<'a>],
}

impl Method<'_> {
    pub(super) fn as_call(&self) -> MethodCall<'_> {
        MethodCall {
            name: &self.name,
            args: &self.args,
        }
    }
}

impl<'a> MethodCall<'a> {
    pub fn argument(&self, n: usize) -> Result<&StyleArgument<'a>, ConsoleError> {
        self.args
            .get(n)
            .ok_or_else(|| ConsoleError::new("组件样式缺少参数"))
    }
    pub(super) fn text(&self, n: usize) -> Result<&'a str, ConsoleError> {
        self.argument(n)?.text()
    }
    pub(super) fn integer(&self, n: usize) -> Result<i64, ConsoleError> {
        self.argument(n)?.integer()
    }
}
pub(super) fn resolve(input: &str) -> Result<Method<'_>, ConsoleError> {
    let pieces = parts(input);
    // 固定方法表保留重载解析边界；方法名忽略 ASCII 大小写，原始标记另行保存。
    for n in (1..=pieces.len()).rev() {
        let method = pieces[..n].join("").to_ascii_lowercase().replace(' ', "");
        if is_known(&method) {
            return Ok(Method {
                name: method,
                args: pieces[n..].iter().map(|p| StyleArgument::new(p)).collect(),
            });
        }
    }
    Err(ConsoleError::new(format!(
        "控制台组件暂不支持此终端样式：{input}"
    )))
}
pub(super) fn is_known(name: &str) -> bool {
    METHODS.contains(&name)
}

const METHODS: &[&str] = &[
    "__construct",
    "setelement",
    "defaultstyles",
    "getproperties",
    "setproperties",
    "setstyle",
    "hasstyle",
    "addstyle",
    "inheritfromstyles",
    "bg",
    "fontbold",
    "fontnormal",
    "strong",
    "italic",
    "underline",
    "ml",
    "mr",
    "mb",
    "mt",
    "mx",
    "my",
    "m",
    "pl",
    "pr",
    "px",
    "pt",
    "pb",
    "py",
    "p",
    "spacey",
    "spacex",
    "bordert",
    "text",
    "truncate",
    "w",
    "wfull",
    "wauto",
    "minw",
    "maxw",
    "uppercase",
    "lowercase",
    "capitalize",
    "snakecase",
    "linethrough",
    "invisible",
    "hidden",
    "block",
    "flex",
    "flex1",
    "justifybetween",
    "justifyaround",
    "justifyevenly",
    "justifycenter",
    "contentrepeat",
    "prepend",
    "append",
    "list",
    "with",
    "href",
    "format",
    "getformatstring",
    "getmargins",
    "getpaddings",
    "applywidth",
    "applystyling",
    "getlength",
    "getinnerwidth",
    "getcolorvariant",
    "calcwidthfromfraction",
    "getparentwidth",
    "trimtext",
];
