use super::{ConsoleError, OutputStyle, ProgressBar, ProgressState};

/// 在取值时启动，处理完当前项后才推进；提前丢弃不补写完成状态。
pub struct ProgressIterator<'bar, 'output, I: IntoIterator> {
    bar: &'bar mut ProgressBar<'output>,
    steps: Steps<I>,
}
/// 样式惰性迭代器，仅在自然耗尽后写入样式的两个换行。
pub struct ProgressStyleIterator<'style, 'output, I: IntoIterator> {
    style: &'style mut OutputStyle<'output>,
    bar: Option<ProgressState>,
    steps: Steps<I>,
}
struct Steps<I: IntoIterator> {
    source: Option<I>,
    iterator: Option<I::IntoIter>,
    max: Option<i64>,
    started: bool,
    yielded: bool,
    done: bool,
    completed: bool,
}
impl<I: IntoIterator> Steps<I> {
    fn new(source: I, max: Option<i64>) -> Self {
        Self {
            source: Some(source),
            iterator: None,
            max: max.filter(|n| *n != 0),
            started: false,
            yielded: false,
            done: false,
            completed: false,
        }
    }
    fn next(&mut self, bar: &mut ProgressBar<'_>) -> Option<Result<I::Item, ConsoleError>> {
        if self.done {
            return None;
        }
        match self.step(bar) {
            Ok(Some(value)) => Some(Ok(value)),
            Ok(None) => {
                self.done = true;
                self.completed = true;
                None
            }
            Err(error) => {
                self.done = true;
                Some(Err(error))
            }
        }
    }
    fn step(&mut self, bar: &mut ProgressBar<'_>) -> Result<Option<I::Item>, ConsoleError> {
        if !self.started {
            self.iterator = self.source.take().map(IntoIterator::into_iter);
            let iterator = self.iterator.as_ref().expect("首次迭代已取得原始数据");
            let (lower, upper) = iterator.size_hint();
            let count = if upper == Some(lower) {
                Some(i64::try_from(lower).map_err(ConsoleError::from_source)?)
            } else {
                None
            };
            let max = self.max.or(count);
            self.started = true;
            if max == Some(0) {
                bar.state.max = Some(0);
                bar.state.step_width = 2;
                bar.finish()?;
                return Ok(None);
            }
            bar.start_at(max, 0)?;
        } else if self.yielded {
            // 与生成器在 yield 恢复后推进的顺序一致。
            bar.advance()?;
        }
        let next = self.iterator.as_mut().expect("迭代器已初始化").next();
        if next.is_some() {
            self.yielded = true;
        } else {
            bar.finish()?;
        }
        Ok(next)
    }
}
impl<'output> ProgressBar<'output> {
    pub fn iterate<I: IntoIterator>(&mut self, items: I) -> ProgressIterator<'_, 'output, I> {
        self.iterate_with_max(items, None)
    }
    pub fn iterate_with_max<I: IntoIterator>(
        &mut self,
        items: I,
        max: Option<i64>,
    ) -> ProgressIterator<'_, 'output, I> {
        ProgressIterator {
            bar: self,
            steps: Steps::new(items, max),
        }
    }
}
impl<'output, I: IntoIterator> ProgressIterator<'_, 'output, I> {
    pub fn progress_bar(&mut self) -> &mut ProgressBar<'output> {
        self.bar
    }
}
impl<I: IntoIterator> Iterator for ProgressIterator<'_, '_, I> {
    type Item = Result<I::Item, ConsoleError>;
    fn next(&mut self) -> Option<Self::Item> {
        self.steps.next(self.bar)
    }
}
impl<'output> OutputStyle<'output> {
    pub fn progress_iterate<I: IntoIterator>(
        &mut self,
        items: I,
    ) -> ProgressStyleIterator<'_, 'output, I> {
        self.progress_iterate_with_max(items, None)
    }
    pub fn progress_iterate_with_max<I: IntoIterator>(
        &mut self,
        items: I,
        max: Option<i64>,
    ) -> ProgressStyleIterator<'_, 'output, I> {
        ProgressStyleIterator {
            style: self,
            bar: None,
            steps: Steps::new(items, max),
        }
    }
}
impl<I: IntoIterator> Iterator for ProgressStyleIterator<'_, '_, I> {
    type Item = Result<I::Item, ConsoleError>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.steps.done {
            return None;
        }
        let mut bar = match self.bar.take() {
            Some(state) => ProgressBar::from_state(self.style.get_output(), state),
            None => match self.style.create_progress_bar(0) {
                Ok(bar) => bar,
                Err(error) => {
                    self.steps.done = true;
                    return Some(Err(error));
                }
            },
        };
        let result = self.steps.next(&mut bar);
        self.bar = Some(bar.into_state());
        if self.steps.completed
            && let Err(error) = self.style.new_line(2)
        {
            return Some(Err(error));
        }
        result
    }
}
