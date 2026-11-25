use super::{ConsoleFormatError, FormatterStyle, OutputFormatterStyleStack};
use parking_lot::ReentrantMutex;
use std::sync::{Arc, Mutex};
/// 格式器内实际样式栈的共享引用；复制句柄不复制栈。
#[derive(Clone)]
pub struct OutputFormatterStyleStackHandle {
    order: Arc<ReentrantMutex<()>>,
    stack: Arc<Mutex<OutputFormatterStyleStack>>,
}
impl OutputFormatterStyleStackHandle {
    pub(crate) fn new(stack: OutputFormatterStyleStack, order: Arc<ReentrantMutex<()>>) -> Self {
        Self {
            order,
            stack: Arc::new(Mutex::new(stack)),
        }
    }
    pub(crate) fn snapshot(&self) -> OutputFormatterStyleStack {
        let _order = self.order.lock();
        self.stack
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
    pub fn reset(&self) {
        let _order = self.order.lock();
        let old = std::mem::take(
            &mut self
                .stack
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .styles,
        );
        drop(old);
    }
    pub fn push(&self, style: FormatterStyle) {
        let _order = self.order.lock();
        self.stack
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(style);
    }
    pub fn pop(&self) -> Result<FormatterStyle, ConsoleFormatError> {
        let _order = self.order.lock();
        self.stack
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .pop()
    }
    pub fn pop_with_style(
        &self,
        style: &FormatterStyle,
    ) -> Result<FormatterStyle, ConsoleFormatError> {
        let _order = self.order.lock();
        let snapshot = self.snapshot();
        if snapshot.styles.is_empty() {
            return Ok(snapshot.empty.clone());
        }
        for (index, candidate) in snapshot.styles.iter().enumerate().rev() {
            // 用户样式可在比较时复位同一栈；回调期间不持有栈数据锁。
            if style.apply("")? == candidate.apply("")? {
                let removed = {
                    let mut stack = self
                        .stack
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    if index < stack.styles.len() {
                        stack.styles.split_off(index)
                    } else {
                        Vec::new()
                    }
                };
                drop(removed);
                return Ok(candidate.clone());
            }
        }
        Err(ConsoleFormatError::IncorrectlyNestedTag)
    }
    pub fn get_current(&self) -> FormatterStyle {
        let _order = self.order.lock();
        self.stack
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get_current()
    }
    pub fn get_empty_style(&self) -> FormatterStyle {
        let _order = self.order.lock();
        self.stack
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get_empty_style()
    }
    pub fn set_empty_style(&self, style: FormatterStyle) -> &Self {
        let _order = self.order.lock();
        let old = {
            let mut stack = self
                .stack
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            std::mem::replace(&mut stack.empty, style)
        };
        drop(old);
        self
    }
}
impl crate::contracts::service::ResetInterface for OutputFormatterStyleStackHandle {
    fn reset(&mut self) {
        Self::reset(self);
    }
}
