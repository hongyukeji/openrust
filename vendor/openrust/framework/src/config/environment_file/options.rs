/// 新增变量的位置。锚点按变量名匹配，不按行号或变量值匹配。
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum EnvironmentPosition {
    Start,
    #[default]
    End,
    Before(String),
    After(String),
}

/// 显式控制新增位置与两侧空行；None 保留原有间距，Some(n) 表示恰好 n 行。
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct EnvironmentWriteOptions {
    pub position: EnvironmentPosition,
    pub blank_lines_before: Option<usize>,
    pub blank_lines_after: Option<usize>,
    /// 默认只在原位置更新已有变量；启用后才按指定位置移动整条声明。
    pub move_existing: bool,
}
impl EnvironmentWriteOptions {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn position(mut self, position: EnvironmentPosition) -> Self {
        self.position = position;
        self
    }
    pub fn before(self, key: impl Into<String>) -> Self {
        self.position(EnvironmentPosition::Before(key.into()))
    }
    pub fn after(self, key: impl Into<String>) -> Self {
        self.position(EnvironmentPosition::After(key.into()))
    }
    pub fn at_start(self) -> Self {
        self.position(EnvironmentPosition::Start)
    }
    pub fn at_end(self) -> Self {
        self.position(EnvironmentPosition::End)
    }
    pub fn blank_lines_before(mut self, lines: usize) -> Self {
        self.blank_lines_before = Some(lines);
        self
    }
    pub fn blank_lines_after(mut self, lines: usize) -> Self {
        self.blank_lines_after = Some(lines);
        self
    }
    pub fn blank_lines(self, lines: usize) -> Self {
        self.blank_lines_before(lines).blank_lines_after(lines)
    }
    pub fn move_existing(mut self, enabled: bool) -> Self {
        self.move_existing = enabled;
        self
    }
}
