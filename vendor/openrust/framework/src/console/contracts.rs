/// 提供样式输出的换行计数与末行状态。
pub trait NewLineAware {
    fn new_lines_written(&self) -> usize;
    fn new_line_written(&self) -> bool;
}
