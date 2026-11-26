// 第三方许可见 ../LICENSE.symfony。
use super::{ConsoleTreeError, TreeChildren, TreeNode, TreeStyle, TreeValue};
use crate::{
    console::{AsOutput, ConsoleError, OutputOptions, OutputReference},
    support::ArrayKey,
};
use std::collections::HashMap;

struct Cursor {
    children: TreeChildren,
    current: Option<TreeNode>,
    key: Option<ArrayKey>,
    advanced: bool,
}
impl Cursor {
    fn new(node: &TreeNode) -> Result<Self, ConsoleError> {
        let mut cursor = Self {
            children: node.get_children(),
            current: None,
            key: None,
            advanced: false,
        };
        cursor.load()?;
        Ok(cursor)
    }
    fn load(&mut self) -> Result<(), ConsoleError> {
        self.current = None;
        self.key = None;
        if let Some(node) = self.children.next() {
            self.current = Some(node?);
            self.key = self.children.key().cloned();
        }
        Ok(())
    }
    fn current(&self) -> Result<&TreeNode, ConsoleError> {
        self.current
            .as_ref()
            .ok_or_else(|| ConsoleError::from_source(ConsoleTreeError::InvalidCurrent))
    }
    fn key(&self) -> Result<i64, ConsoleError> {
        match &self.key {
            Some(ArrayKey::Integer(key)) => Ok(*key),
            Some(ArrayKey::String(key)) => <i64 as crate::support::EnumBacking>::from_input(
                &serde_json::Value::String(key.clone()),
            )
            .map_err(|_| ConsoleError::from_source(ConsoleTreeError::InvalidKey)),
            None => Err(ConsoleError::from_source(ConsoleTreeError::InvalidKey)),
        }
    }
    fn advance(&mut self) -> Result<(), ConsoleError> {
        self.advanced |= self.current.is_some();
        self.load()
    }
    fn rewind(&mut self) -> Result<(), ConsoleError> {
        if self.advanced {
            self.key = None;
            Err(ConsoleError::from_source(ConsoleTreeError::CannotRewind))
        } else {
            Ok(())
        }
    }
    fn has_children(&self) -> Result<bool, ConsoleError> {
        self.current()?
            .get_children()
            .next()
            .transpose()
            .map(|item| item.is_some())
    }
    fn child(&self) -> Result<Self, ConsoleError> {
        Self::new(self.current()?)
    }
}
/// 借用所选输出的树助手；游标保持单次生成器的状态。
/// ```compile_fail,E0499
/// use openrust::console::{Output, TreeHelper, TreeNode, TreeValues};
/// let mut output = Output::default();
/// let node = TreeNode::with_children("Root", [TreeNode::with_children("A", ["leaf"])]);
/// let mut tree = TreeHelper::create_tree(&mut output, node, TreeValues::new(), None).unwrap();
/// let mut child = tree.get_children().unwrap();
/// tree.render().unwrap();
/// child.render().unwrap();
/// ```
pub struct TreeHelper<'a> {
    output: OutputReference<'a>,
    node: TreeNode,
    style: TreeStyle,
    cursor: Cursor,
}
impl<'a> TreeHelper<'a> {
    pub fn create_tree<K: Into<ArrayKey>, V: Into<TreeValue>>(
        output: &'a mut (impl AsOutput + ?Sized),
        root: impl Into<TreeNode>,
        values: impl IntoIterator<Item = (K, V)>,
        style: Option<TreeStyle>,
    ) -> Result<Self, ConsoleError> {
        Self::from_reference(
            output.as_output(),
            root.into(),
            values,
            style.unwrap_or_default(),
        )
    }
    pub(in crate::console) fn from_reference<K: Into<ArrayKey>, V: Into<TreeValue>>(
        output: OutputReference<'a>,
        root: TreeNode,
        values: impl IntoIterator<Item = (K, V)>,
        style: TreeStyle,
    ) -> Result<Self, ConsoleError> {
        let node = TreeNode::from_values(values, Some(root))?;
        let cursor = Cursor::new(&node)?;
        Ok(Self {
            output,
            node,
            style,
            cursor,
        })
    }
    pub fn current(&self) -> Result<TreeNode, ConsoleError> {
        self.cursor.current().cloned()
    }
    pub fn key(&self) -> Result<i64, ConsoleError> {
        self.cursor.key()
    }
    pub fn advance(&mut self) -> Result<(), ConsoleError> {
        self.cursor.advance()
    }
    pub fn rewind(&mut self) -> Result<(), ConsoleError> {
        self.cursor.rewind()
    }
    pub fn valid(&self) -> bool {
        self.cursor.current.is_some()
    }
    pub fn has_children(&self) -> Result<bool, ConsoleError> {
        self.cursor.has_children()
    }
    pub fn get_children(&mut self) -> Result<TreeHelper<'_>, ConsoleError> {
        let node = self.current()?;
        let cursor = Cursor::new(&node)?;
        Ok(TreeHelper {
            output: self.output.reborrow(),
            node,
            style: self.style.clone(),
            cursor,
        })
    }
    pub fn recursive_iterator(&mut self) -> TreeIterator<'_> {
        TreeIterator::new(&mut self.cursor, self.style.clone())
    }
    pub fn render(&mut self) -> Result<(), ConsoleError> {
        self.output
            .writeln(self.node.get_value(), OutputOptions::default())?;
        let mut visited = HashMap::new();
        for entry in TreeIterator::new(&mut self.cursor, self.style.clone()) {
            let entry = entry?;
            if visited
                .insert(entry.node.identity(), entry.node.clone())
                .is_some()
            {
                return Err(ConsoleError::from_source(ConsoleTreeError::Cycle(
                    entry.node.to_string(),
                )));
            }
            self.output
                .writeln(entry.line.as_str(), OutputOptions::default())?;
        }
        Ok(())
    }
}
impl From<&str> for TreeNode {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}
impl From<String> for TreeNode {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}
impl From<Option<TreeNode>> for TreeNode {
    fn from(value: Option<TreeNode>) -> Self {
        value.unwrap_or_default()
    }
}
/// 原生迭代适配；游标的 `advance()` 仅负责推进。
pub struct TreeHelperIterator<'a> {
    cursor: &'a mut Cursor,
    started: bool,
    failed: bool,
}
impl<'a, 'o> IntoIterator for &'a mut TreeHelper<'o> {
    type Item = Result<TreeNode, ConsoleError>;
    type IntoIter = TreeHelperIterator<'a>;
    fn into_iter(self) -> Self::IntoIter {
        TreeHelperIterator {
            cursor: &mut self.cursor,
            started: false,
            failed: false,
        }
    }
}
impl Iterator for TreeHelperIterator<'_> {
    type Item = Result<TreeNode, ConsoleError>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.failed {
            return None;
        }
        let status = if self.started {
            self.cursor.advance()
        } else {
            self.started = true;
            self.cursor.rewind()
        };
        if let Err(error) = status {
            self.failed = true;
            return Some(Err(error));
        }
        self.cursor.current.clone().map(Ok)
    }
}
/// 已应用连接线的树条目；保留节点身份供渲染端检测重复对象。
pub struct TreeEntry {
    pub node: TreeNode,
    pub line: String,
}
enum CursorRef<'a> {
    Root(&'a mut Cursor),
    Child(Cursor),
}
impl std::ops::Deref for CursorRef<'_> {
    type Target = Cursor;
    fn deref(&self) -> &Cursor {
        match self {
            Self::Root(cursor) => cursor,
            Self::Child(cursor) => cursor,
        }
    }
}
impl std::ops::DerefMut for CursorRef<'_> {
    fn deref_mut(&mut self) -> &mut Cursor {
        match self {
            Self::Root(cursor) => cursor,
            Self::Child(cursor) => cursor,
        }
    }
}
struct Frame<'a> {
    cursor: CursorRef<'a>,
    pending: Option<Cursor>,
}
/// 逐项预读兄弟节点的深度优先遍历，保持子节点探测和错误发生的阶段。
pub struct TreeIterator<'a> {
    stack: Vec<Frame<'a>>,
    pub(super) style: TreeStyle,
    started: bool,
    failed: bool,
}
impl<'a> TreeIterator<'a> {
    fn new(cursor: &'a mut Cursor, style: TreeStyle) -> Self {
        Self {
            stack: vec![Frame {
                cursor: CursorRef::Root(cursor),
                pending: None,
            }],
            style,
            started: false,
            failed: false,
        }
    }
}
impl Iterator for TreeIterator<'_> {
    type Item = Result<TreeEntry, ConsoleError>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.failed {
            return None;
        }
        let result = (|| {
            if !self.started {
                self.started = true;
                self.stack[0].cursor.rewind()?;
            }
            loop {
                let frame = match self.stack.last_mut() {
                    Some(frame) => frame,
                    None => return Ok(None),
                };
                if let Some(mut child) = frame.pending.take() {
                    child.rewind()?;
                    self.stack.push(Frame {
                        cursor: CursorRef::Child(child),
                        pending: None,
                    });
                    continue;
                }
                if frame.cursor.current.is_none() {
                    self.stack.pop();
                    continue;
                }
                frame.cursor.key()?;
                let node = frame.cursor.current()?.clone();
                // 固定遍历器只在子树探测阶段捕获异常，预读下一项的失败仍传播。
                let child = if frame.cursor.has_children().unwrap_or(false) {
                    frame.cursor.child().ok()
                } else {
                    None
                };
                frame.cursor.advance()?;
                frame.pending = child;
                let mut line = self.style.prefixes[2].to_string();
                for parent in &self.stack[..self.stack.len() - 1] {
                    line.push_str(
                        &self.style.prefixes[if parent.cursor.current.is_some() {
                            3
                        } else {
                            4
                        }],
                    );
                }
                line.push_str(
                    &self.style.prefixes[if self
                        .stack
                        .last()
                        .expect("当前树帧")
                        .cursor
                        .current
                        .is_some()
                    {
                        0
                    } else {
                        1
                    }],
                );
                line.push_str(&self.style.prefixes[5]);
                line.push_str(node.get_value());
                return Ok(Some(TreeEntry { node, line }));
            }
        })();
        match result {
            Ok(item) => item.map(Ok),
            Err(error) => {
                self.failed = true;
                Some(Err(error))
            }
        }
    }
}
