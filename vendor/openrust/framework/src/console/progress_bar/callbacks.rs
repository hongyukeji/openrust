use super::{ConsoleError, ProgressBar, ProgressIterable, ProgressIterableMut};
use crate::{
    console::{CommandIo, ConsoleFuture},
    support::ArrayKey,
};

impl CommandIo<'_> {
    /// 等待回调真实完成；错误或取消不补写完成帧。
    pub async fn with_progress_bar_async<T>(
        &mut self,
        steps: i64,
        callback: impl for<'a, 'output> FnOnce(
            &'a mut ProgressBar<'output>,
        ) -> ConsoleFuture<'a, Result<T, ConsoleError>>,
    ) -> Result<(), ConsoleError> {
        let mut bar = self.get_output().create_progress_bar(steps)?;
        bar.start()?;
        callback(&mut bar).await?;
        bar.finish()
    }
    pub async fn with_progress_bar_iter_async<I: ProgressIterable, T>(
        &mut self,
        items: I,
        mut callback: impl for<'a, 'output> FnMut(
            &'a I::Item,
            &'a mut ProgressBar<'output>,
            ArrayKey,
        ) -> ConsoleFuture<'a, Result<T, ConsoleError>>,
    ) -> Result<I, ConsoleError> {
        let count =
            i64::try_from(items.progress_iter().len()).map_err(ConsoleError::from_source)?;
        let mut bar = self.get_output().create_progress_bar(count)?;
        bar.start()?;
        for (key, value) in items.progress_iter() {
            callback(value, &mut bar, key).await?;
            bar.advance()?;
        }
        bar.finish()?;
        Ok(items)
    }
    pub fn with_progress_bar_iter_mut<I: ProgressIterableMut, T>(
        &mut self,
        mut items: I,
        mut callback: impl FnMut(
            &mut I::Item,
            &mut ProgressBar<'_>,
            ArrayKey,
        ) -> Result<T, ConsoleError>,
    ) -> Result<I, ConsoleError> {
        let count =
            i64::try_from(items.progress_iter().len()).map_err(ConsoleError::from_source)?;
        let mut bar = self.get_output().create_progress_bar(count)?;
        bar.start()?;
        for (key, value) in items.progress_iter_mut() {
            callback(value, &mut bar, key)?;
            bar.advance()?;
        }
        bar.finish()?;
        Ok(items)
    }
    pub async fn with_progress_bar_iter_mut_async<I: ProgressIterableMut, T>(
        &mut self,
        mut items: I,
        mut callback: impl for<'a, 'output> FnMut(
            &'a mut I::Item,
            &'a mut ProgressBar<'output>,
            ArrayKey,
        ) -> ConsoleFuture<'a, Result<T, ConsoleError>>,
    ) -> Result<I, ConsoleError> {
        let count =
            i64::try_from(items.progress_iter().len()).map_err(ConsoleError::from_source)?;
        let mut bar = self.get_output().create_progress_bar(count)?;
        bar.start()?;
        for (key, value) in items.progress_iter_mut() {
            callback(value, &mut bar, key).await?;
            bar.advance()?;
        }
        bar.finish()?;
        Ok(items)
    }
}
