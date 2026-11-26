use super::{Dimensions, File, FileSize};
use crate::validation::Rule;

/// 图像规则保留既有文件校验管线和默认SVG策略。
#[derive(Clone)]
pub struct ImageFile(File);
impl ImageFile {
    pub fn new(allow_svg: bool) -> Self {
        Self(File::image(allow_svg))
    }
    pub fn min(mut self, size: impl Into<FileSize>) -> Self {
        self.0 = self.0.min(size);
        self
    }
    pub fn max(mut self, size: impl Into<FileSize>) -> Self {
        self.0 = self.0.max(size);
        self
    }
    pub fn size(mut self, size: impl Into<FileSize>) -> Self {
        self.0 = self.0.size(size);
        self
    }
    pub fn between(mut self, min: impl Into<FileSize>, max: impl Into<FileSize>) -> Self {
        self.0 = self.0.between(min, max);
        self
    }
    pub fn dimensions(mut self, dimensions: Dimensions) -> Self {
        self.0 = self.0.dimensions(dimensions);
        self
    }
    pub fn extensions(mut self, extensions: impl IntoIterator<Item = impl AsRef<str>>) -> Self {
        self.0 = self.0.extensions(extensions);
        self
    }
    pub fn encoding(mut self, encoding: impl Into<String>) -> Self {
        self.0 = self.0.encoding(encoding);
        self
    }
}
impl From<ImageFile> for Rule {
    fn from(rule: ImageFile) -> Self {
        rule.0.into()
    }
}

impl Default for ImageFile {
    fn default() -> Self {
        Self::new(false)
    }
}
