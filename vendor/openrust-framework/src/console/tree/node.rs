// 第三方许可见 ../LICENSE.symfony。
use crate::{console::ConsoleError, support::ArrayKey};
use parking_lot::Mutex;
use std::{
    fmt,
    sync::{Arc, Weak},
};

type ChildrenIterator = Box<dyn Iterator<Item = Result<(ArrayKey, TreeNode), ConsoleError>> + Send>;
type ChildrenFactory = Arc<dyn Fn() -> ChildrenIterator + Send + Sync>;

/// 共享身份的树节点；标签只读，惰性子节点在每次遍历时重新求值。
#[derive(Clone, Default)]
pub struct TreeNode(Arc<Node>);
#[derive(Default)]
struct Node {
    value: String,
    children: Mutex<Vec<TreeChild>>,
}
/// 非持有性的节点引用，用于回调中的反向链接，避免强引用循环。
#[derive(Clone, Default)]
pub struct WeakTreeNode(Weak<Node>);
impl WeakTreeNode {
    pub fn upgrade(&self) -> Option<TreeNode> {
        self.0.upgrade().map(TreeNode)
    }
}
/// 直接节点与可重复调用的惰性子节点工厂。
#[derive(Clone)]
pub enum TreeChild {
    Node(TreeNode),
    Lazy(ChildrenFactory),
}
impl TreeChild {
    pub fn lazy<F, I>(factory: F) -> Self
    where
        F: Fn() -> I + Send + Sync + 'static,
        I: IntoIterator<Item = Result<TreeNode, ConsoleError>>,
        I::IntoIter: Send + 'static,
    {
        Self::lazy_keyed(move || {
            factory()
                .into_iter()
                .enumerate()
                .map(|(key, node)| node.map(|node| (ArrayKey::Integer(key as i64), node)))
        })
    }
    /// 保留工厂给出的键；普通工厂可使用 `lazy` 自动编号。
    pub fn lazy_keyed<F, I>(factory: F) -> Self
    where
        F: Fn() -> I + Send + Sync + 'static,
        I: IntoIterator<Item = Result<(ArrayKey, TreeNode), ConsoleError>>,
        I::IntoIter: Send + 'static,
    {
        Self::Lazy(Arc::new(move || Box::new(factory().into_iter())))
    }
}
impl From<TreeNode> for TreeChild {
    fn from(node: TreeNode) -> Self {
        Self::Node(node)
    }
}
impl From<&str> for TreeChild {
    fn from(value: &str) -> Self {
        TreeNode::new(value).into()
    }
}
impl From<String> for TreeChild {
    fn from(value: String) -> Self {
        TreeNode::new(value).into()
    }
}

pub type TreeValues = Vec<(ArrayKey, TreeValue)>;
/// 带键的嵌套值；节点值也按其可迭代的孩子展开，直接保留身份请用 `add_child`。
pub enum TreeValue {
    Text(String),
    Values(TreeValues),
    Node(TreeNode),
    Iterable(Box<dyn Iterator<Item = Result<(ArrayKey, TreeValue), ConsoleError>> + Send>),
}
impl TreeValue {
    /// 保存一次性嵌套生产者，不提前收集其输出。
    pub fn iterable<K: Into<ArrayKey>, V: Into<TreeValue>, I>(values: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        I::IntoIter: Send + 'static,
    {
        Self::Iterable(Box::new(
            values
                .into_iter()
                .map(|(key, value)| Ok((key.into(), value.into()))),
        ))
    }
    pub fn try_iterable<K: Into<ArrayKey>, V: Into<TreeValue>, I>(values: I) -> Self
    where
        I: IntoIterator<Item = Result<(K, V), ConsoleError>>,
        I::IntoIter: Send + 'static,
    {
        Self::Iterable(Box::new(
            values
                .into_iter()
                .map(|entry| entry.map(|(key, value)| (key.into(), value.into()))),
        ))
    }
    pub fn branch<K: Into<ArrayKey>, V: Into<TreeValue>>(
        values: impl IntoIterator<Item = (K, V)>,
    ) -> Self {
        Self::Values(
            values
                .into_iter()
                .map(|(key, value)| (key.into(), value.into()))
                .collect(),
        )
    }
}
impl From<&str> for TreeValue {
    fn from(value: &str) -> Self {
        Self::Text(value.into())
    }
}
impl From<String> for TreeValue {
    fn from(value: String) -> Self {
        Self::Text(value)
    }
}
impl From<TreeNode> for TreeValue {
    fn from(node: TreeNode) -> Self {
        Self::Node(node)
    }
}

impl TreeNode {
    pub fn new(value: impl Into<String>) -> Self {
        Self(Arc::new(Node {
            value: value.into(),
            children: Mutex::new(Vec::new()),
        }))
    }
    pub fn with_children(
        value: impl Into<String>,
        children: impl IntoIterator<Item = impl Into<TreeChild>>,
    ) -> Self {
        let node = Self::new(value);
        for child in children {
            node.add_child(child);
        }
        node
    }
    pub fn get_value(&self) -> &str {
        &self.0.value
    }
    pub fn add_child(&self, child: impl Into<TreeChild>) -> &Self {
        // 转换可执行调用方代码，须在锁外完成。
        let child = child.into();
        self.0.children.lock().push(child);
        self
    }
    pub fn get_children(&self) -> TreeChildren {
        TreeChildren {
            node: self.clone(),
            children: None,
            active: None,
            retained: None,
            key: None,
            next_key: 0,
            failed: false,
        }
    }
    pub fn get_iterator(&self) -> TreeChildren {
        self.get_children()
    }
    pub fn count(&self) -> Result<usize, ConsoleError> {
        self.get_children()
            .try_fold(0usize, |count, child| child.map(|_| count + 1))
    }
    pub fn downgrade(&self) -> WeakTreeNode {
        WeakTreeNode(Arc::downgrade(&self.0))
    }
    pub fn ptr_eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
    /// 显式解除子链接；对自行建立的强引用循环，调用方负责在不再使用时解除。
    pub fn clear_children(&self) {
        let children = std::mem::take(&mut *self.0.children.lock());
        drop(children);
    }
    pub fn from_values<K: Into<ArrayKey>, V: Into<TreeValue>>(
        values: impl IntoIterator<Item = (K, V)>,
        node: Option<Self>,
    ) -> Result<Self, ConsoleError> {
        Self::try_from_values(values.into_iter().map(Ok), node)
    }
    /// 逐项传播输入生产者的失败，保留失败前已追加的节点。
    pub fn try_from_values<K: Into<ArrayKey>, V: Into<TreeValue>>(
        values: impl IntoIterator<Item = Result<(K, V), ConsoleError>>,
        node: Option<Self>,
    ) -> Result<Self, ConsoleError> {
        let root = node.unwrap_or_default();
        let values = values
            .into_iter()
            .map(|entry| entry.map(|(key, value)| (key.into(), value.into())));
        // 用显式栈构造，避免输入层级直接消耗调用栈。
        enum Work<I> {
            Input(TreeNode, I),
            Values(TreeNode, std::vec::IntoIter<(ArrayKey, TreeValue)>),
            Iterable(
                TreeNode,
                Box<dyn Iterator<Item = Result<(ArrayKey, TreeValue), ConsoleError>> + Send>,
            ),
            Node(TreeNode, TreeChildren),
            Attach(TreeNode, TreeNode),
        }
        let mut stack = vec![Work::Input(root.clone(), values)];
        let mut active = std::collections::HashSet::new();
        while !stack.is_empty() {
            if matches!(stack.last(), Some(Work::Attach(..))) {
                if let Some(Work::Attach(parent, child)) = stack.pop() {
                    parent.add_child(child);
                }
                continue;
            }
            let work = stack.last_mut().expect("非空构造栈");
            let (parent, item) = match work {
                Work::Input(parent, values) => (parent.clone(), values.next().transpose()?),
                Work::Iterable(parent, values) => (parent.clone(), values.next().transpose()?),
                Work::Values(parent, values) => (parent.clone(), values.next()),
                Work::Attach(..) => unreachable!("附加节点已单独处理"),
                Work::Node(parent, children) => (
                    parent.clone(),
                    match children.next() {
                        Some(Ok(child)) => Some((
                            children.key().expect("已返回节点的键").clone(),
                            TreeValue::Node(child),
                        )),
                        Some(Err(error)) => return Err(error),
                        None => None,
                    },
                ),
            };
            let Some((key, value)) = item else {
                if let Some(Work::Node(_, children)) = stack.pop() {
                    active.remove(&children.node.identity());
                }
                continue;
            };
            match value {
                TreeValue::Text(value) => {
                    parent.add_child(value);
                }
                TreeValue::Iterable(values) => {
                    let child = Self::new(key.to_string());
                    stack.push(Work::Attach(parent, child.clone()));
                    stack.push(Work::Iterable(child, values));
                }
                TreeValue::Values(values) => {
                    let child = Self::new(key.to_string());
                    stack.push(Work::Attach(parent, child.clone()));
                    stack.push(Work::Values(child, values.into_iter()));
                }
                TreeValue::Node(node) => {
                    if !active.insert(node.identity()) {
                        return Err(ConsoleError::new("嵌套树值包含循环，无法展开"));
                    }
                    let child = Self::new(key.to_string());
                    stack.push(Work::Attach(parent, child.clone()));
                    stack.push(Work::Node(child, node.get_children()));
                }
            }
        }
        Ok(root)
    }
    pub(super) fn identity(&self) -> usize {
        Arc::as_ptr(&self.0) as usize
    }
}
impl fmt::Display for TreeNode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.get_value())
    }
}
impl fmt::Debug for TreeNode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TreeNode")
            .field("value", &self.get_value())
            .finish_non_exhaustive()
    }
}
impl IntoIterator for &TreeNode {
    type Item = Result<TreeNode, ConsoleError>;
    type IntoIter = TreeChildren;
    fn into_iter(self) -> TreeChildren {
        self.get_iterator()
    }
}
/// 每次创建得到新的惰性遍历；首次推进才取得子列表快照，工厂在锁外执行。
pub struct TreeChildren {
    node: TreeNode,
    children: Option<std::vec::IntoIter<TreeChild>>,
    active: Option<ChildrenIterator>,
    // 保留生成器最后的节点，直到调用方释放本次遍历。
    retained: Option<TreeNode>,
    key: Option<ArrayKey>,
    next_key: i64,
    failed: bool,
}
impl TreeChildren {
    pub fn key(&self) -> Option<&ArrayKey> {
        self.key.as_ref()
    }
}
impl Iterator for TreeChildren {
    type Item = Result<TreeNode, ConsoleError>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.failed {
            return None;
        }
        self.key = None;
        if self.children.is_none() {
            self.children = Some(self.node.0.children.lock().clone().into_iter());
        }
        loop {
            if let Some(active) = &mut self.active {
                match active.next() {
                    Some(Ok((key, node))) => {
                        self.key = Some(key);
                        self.retained = Some(node.clone());
                        return Some(Ok(node));
                    }
                    Some(Err(error)) => {
                        self.failed = true;
                        self.active = None;
                        return Some(Err(error));
                    }
                    None => self.active = None,
                }
            }
            match self.children.as_mut().expect("已初始化的子列表").next()? {
                TreeChild::Node(node) => {
                    self.retained = Some(node.clone());
                    self.key = Some(ArrayKey::Integer(self.next_key));
                    self.next_key = match self.next_key.checked_add(1) {
                        Some(key) => key,
                        None => {
                            self.failed = true;
                            return Some(Err(ConsoleError::new("树节点索引超过范围")));
                        }
                    };
                    return Some(Ok(node));
                }
                TreeChild::Lazy(factory) => self.active = Some(factory()),
            }
        }
    }
}
