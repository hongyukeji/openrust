use std::{
    error::Error,
    fmt,
    path::{Path, PathBuf},
};

use super::ViewError;
use crate::{
    contracts::debug::{Exception, SharedException},
    http::HttpError,
};

/// 保留实际执行的视图路径与渲染失败的来源。
#[derive(Debug)]
pub struct ViewException {
    path: PathBuf,
    previous: Box<dyn Error + Send + Sync>,
}

impl ViewException {
    pub(super) fn wrap(path: &Path, error: ViewError) -> ViewError {
        match error {
            ViewError::RenderFailed(previous) => ViewError::RenderFailed(Box::new(Self {
                path: path.to_owned(),
                previous,
            })),
            error => error,
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn previous(&self) -> &(dyn Error + Send + Sync + 'static) {
        self.previous.as_ref()
    }

    /// 仅穿过共享异常、HTTP 和视图的透明包装，保留应用错误自身的分组身份。
    pub fn from_exception<'a>(exception: &Exception<'a>) -> Option<&'a Self> {
        let mut error: &(dyn Error + 'static) = match *exception {
            Exception::Http(error) => error,
            Exception::Mapped(error) => error,
            Exception::Error(error) | Exception::Typed(error, _) => error,
        };
        for _ in 0..32 {
            if let Some(HttpError::Internal { source }) = error.downcast_ref::<HttpError>() {
                error = source.as_ref();
            } else if error.is::<SharedException>() {
                error = error.source()?;
            } else if let Some(ViewError::RenderFailed(source)) = error.downcast_ref::<ViewError>()
            {
                return source.downcast_ref();
            } else {
                return error.downcast_ref();
            }
        }
        None
    }
}

impl fmt::Display for ViewException {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self.previous(), formatter)
    }
}

impl Error for ViewException {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(self.previous())
    }
}
