use crate::{
    http::UploadedFile,
    validation::{Rule, RuleFailure, ValidationError, ValidationResult, rule::Kind},
};

use super::{Dimensions, EncodingRule};

#[derive(Clone)]
enum Check {
    Uploaded,
    Image(bool),
    Mimes(Vec<String>),
    MimeTypes(Vec<String>),
    Extensions(Vec<String>),
}

#[derive(Clone)]
pub enum FileSize {
    Kilobytes(usize),
    Human(String),
}

impl From<usize> for FileSize {
    fn from(value: usize) -> Self {
        Self::Kilobytes(value)
    }
}

impl From<&str> for FileSize {
    fn from(value: &str) -> Self {
        Self::Human(value.to_owned())
    }
}

impl FileSize {
    fn kilobytes(&self) -> ValidationResult<usize> {
        let value = match self {
            Self::Kilobytes(value) => return Ok(*value),
            Self::Human(value) => value,
        };
        let value = value.trim().to_ascii_lowercase();
        let split = value
            .len()
            .checked_sub(2)
            .filter(|index| value.is_char_boundary(*index))
            .ok_or(ValidationError::InvalidRule)?;
        let (number, suffix) = value.split_at(split);
        let scale = match suffix {
            "kb" => 1.0,
            "mb" => 1_000.0,
            "gb" => 1_000_000.0,
            "tb" => 1_000_000_000.0,
            _ => return Err(ValidationError::InvalidRule),
        };
        let number = (number
            .trim_end()
            .parse::<f64>()
            .map_err(|_| ValidationError::InvalidRule)?
            * scale)
            .round();
        if !number.is_finite() || number < 0.0 || number >= usize::MAX as f64 {
            return Err(ValidationError::InvalidRule);
        }
        Ok(number as usize)
    }
}

/// Validates an uploaded file's content type, client extension and size.
#[derive(Clone)]
pub struct File {
    checks: Vec<Check>,
    minimum: Option<FileSize>,
    maximum: Option<FileSize>,
    dimensions: Option<Dimensions>,
    encoding: Option<String>,
}

impl Default for File {
    fn default() -> Self {
        Self::from_check(Check::Uploaded)
    }
}

impl File {
    pub fn image(allow_svg: bool) -> Self {
        Self::from_check(Check::Image(allow_svg))
    }

    pub fn types(types: impl IntoIterator<Item = impl Into<String>>) -> Self {
        let (mime_types, extensions): (Vec<_>, Vec<_>) = types
            .into_iter()
            .map(Into::into)
            .partition(|value: &String| value.contains('/'));
        let mut rule = Self::default();
        if !mime_types.is_empty() {
            rule.checks.push(Check::MimeTypes(mime_types));
        }
        if !extensions.is_empty() {
            rule.checks.push(Check::Mimes(extensions));
        }
        rule
    }

    pub fn extensions(mut self, extensions: impl IntoIterator<Item = impl AsRef<str>>) -> Self {
        let extensions: Vec<_> = extensions
            .into_iter()
            .map(|extension| extension.as_ref().to_ascii_lowercase())
            .collect();
        self.checks
            .retain(|check| !matches!(check, Check::Extensions(_)));
        if !extensions.is_empty() {
            self.checks.push(Check::Extensions(extensions));
        }
        self
    }

    pub fn dimensions(mut self, dimensions: Dimensions) -> Self {
        self.dimensions = Some(dimensions);
        self
    }

    pub fn encoding(mut self, encoding: impl Into<String>) -> Self {
        self.encoding = Some(encoding.into());
        self
    }

    pub fn min(mut self, size: impl Into<FileSize>) -> Self {
        self.minimum = Some(size.into());
        self
    }

    pub fn max(mut self, size: impl Into<FileSize>) -> Self {
        self.maximum = Some(size.into());
        self
    }

    pub fn between(self, min: impl Into<FileSize>, max: impl Into<FileSize>) -> Self {
        self.min(min).max(max)
    }

    pub fn size(self, size: impl Into<FileSize>) -> Self {
        let size = size.into();
        self.between(size.clone(), size)
    }

    pub(in crate::validation) fn parse(name: &str, arguments: &str) -> ValidationResult<Self> {
        let list = || {
            let values: Vec<_> = arguments.split(',').map(str::to_owned).collect();
            if values.iter().any(String::is_empty) {
                return Err(ValidationError::InvalidRule);
            }
            Ok(values)
        };
        let check = match name {
            "image" => Check::Image(arguments.split(',').any(|option| option == "allow_svg")),
            "mimes" => Check::Mimes(list()?),
            "mimetypes" => Check::MimeTypes(list()?),
            "extensions" => Check::Extensions(list()?),
            _ => return Err(ValidationError::InvalidRule),
        };
        Ok(Self::from_check(check))
    }

    pub(in crate::validation) fn failures(
        &self,
        file: Option<&UploadedFile>,
    ) -> ValidationResult<Vec<RuleFailure>> {
        let mut failures = Vec::new();
        let detected = file
            .filter(|_| {
                self.checks.iter().any(|check| {
                    matches!(
                        check,
                        Check::Image(_) | Check::Mimes(_) | Check::MimeTypes(_)
                    )
                })
            })
            .map(detect_type)
            .unwrap_or_default();
        for check in &self.checks {
            let pass = file.is_some_and(|file| {
                let (extension, mime) = detected;
                let client = client_extension(file);
                match check {
                    Check::Uploaded => true,
                    Check::Image(svg) => {
                        !blocked(&client, &[])
                            && (matches!(
                                extension,
                                "jpg"
                                    | "jpeg"
                                    | "png"
                                    | "gif"
                                    | "bmp"
                                    | "webp"
                                    | "avif"
                                    | "heic"
                                    | "heif"
                            ) || *svg && extension == "svg")
                    }
                    Check::Mimes(values) => {
                        !blocked(&client, values)
                            && values.iter().any(|value| {
                                value == extension
                                    || matches!(value.as_str(), "jpg" | "jpeg")
                                        && matches!(extension, "jpg" | "jpeg")
                            })
                    }
                    Check::MimeTypes(values) => {
                        !blocked(&client, values)
                            && values.iter().any(|value| {
                                mime == value
                                    || value.strip_suffix("/*").is_some_and(|family| {
                                        mime.split_once('/').is_some_and(|(kind, _)| kind == family)
                                    })
                            })
                    }
                    Check::Extensions(values) => {
                        !blocked(&client, values) && values.contains(&client)
                    }
                }
            });
            if !pass {
                let (key, values) = match check {
                    Check::Uploaded => ("file", None),
                    Check::Image(_) => ("image", None),
                    Check::Mimes(values) => ("mimes", Some(values)),
                    Check::MimeTypes(values) => ("mimetypes", Some(values)),
                    Check::Extensions(values) => ("extensions", Some(values)),
                };
                let mut failure = RuleFailure::new(format!("validation.{key}"));
                if let Some(values) = values {
                    failure
                        .replacements
                        .insert("values".into(), values.join(", "));
                }
                failures.push(failure);
            }
        }
        let min = self.minimum.as_ref().map(FileSize::kilobytes).transpose()?;
        let max = self.maximum.as_ref().map(FileSize::kilobytes).transpose()?;
        if let Some(file) = file
            && let Some(failure) = size_failure(file.getSize(), min, max)
        {
            failures.push(failure);
        }
        if let Some(dimensions) = &self.dimensions
            && !dimensions.passes(file)?
        {
            failures.push(RuleFailure::new("validation.dimensions"));
        }
        if let Some(encoding) = &self.encoding {
            let encoding = EncodingRule::parse(encoding)?;
            if let Some(file) = file
                && !encoding.bytes(&file.get())
            {
                failures.push(encoding.failure());
            }
        }
        Ok(failures)
    }

    fn from_check(check: Check) -> Self {
        Self {
            checks: vec![check],
            minimum: None,
            maximum: None,
            dimensions: None,
            encoding: None,
        }
    }
}

impl From<File> for Rule {
    fn from(rule: File) -> Self {
        Rule::new(Kind::File(rule))
    }
}

pub(in crate::validation) fn size_failure(
    bytes: usize,
    min: Option<usize>,
    max: Option<usize>,
) -> Option<RuleFailure> {
    let bytes = bytes as u128;
    if min.is_none_or(|bound| bytes >= bound as u128 * 1024)
        && max.is_none_or(|bound| bytes <= bound as u128 * 1024)
    {
        return None;
    }
    let (rule, bounds) = match (min, max) {
        (Some(min), Some(max)) if min == max => ("size", vec![("size", min)]),
        (Some(min), Some(max)) => ("between", vec![("min", min), ("max", max)]),
        (Some(min), None) => ("min", vec![("min", min)]),
        (None, Some(max)) => ("max", vec![("max", max)]),
        (None, None) => unreachable!(),
    };
    let mut failure = RuleFailure::new(format!("validation.{rule}.file"));
    failure.replacements.extend(
        bounds
            .into_iter()
            .map(|(key, bound)| (key.into(), bound.to_string())),
    );
    Some(failure)
}

fn client_extension(file: &UploadedFile) -> String {
    file.getClientOriginalName()
        .rsplit_once('.')
        .map_or(String::new(), |(_, extension)| {
            extension.to_ascii_lowercase()
        })
}

fn blocked(extension: &str, accepted: &[String]) -> bool {
    matches!(
        extension.trim(),
        "php" | "php3" | "php4" | "php5" | "php7" | "php8" | "phtml" | "phar"
    ) && !accepted.iter().any(|value| value == "php")
}

pub(super) fn detect_type(file: &UploadedFile) -> (&'static str, &'static str) {
    let kind = file
        .detected_type()
        .unwrap_or(("", "application/octet-stream"));
    if matches!(kind.0, "txt" | "xml") && svg_root(file.get().as_ref()) {
        ("svg", "image/svg+xml")
    } else {
        kind
    }
}

fn svg_root(bytes: &[u8]) -> bool {
    use quick_xml::{NsReader, events::Event, name::ResolveResult};
    let bytes = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes);
    let mut reader = NsReader::from_reader(bytes);
    reader.config_mut().trim_text(true);
    loop {
        match reader.read_resolved_event() {
            Ok((prefix, Event::Start(tag) | Event::Empty(tag))) => {
                return tag.local_name().as_ref() == "svg"
                    && match prefix {
                        ResolveResult::Unbound => true,
                        ResolveResult::Bound(namespace) => {
                            namespace.as_ref() == "http://www.w3.org/2000/svg"
                        }
                        ResolveResult::Unknown(_) => false,
                    };
            }
            Ok((_, Event::Decl(_) | Event::Comment(_) | Event::PI(_))) => {}
            Ok((_, Event::Text(text))) if text.as_ref().is_empty() => {}
            _ => return false,
        }
    }
}
