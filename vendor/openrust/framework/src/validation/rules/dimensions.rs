use crate::{
    http::UploadedFile,
    validation::{Rule, ValidationError, ValidationResult, rule::Kind},
};

use super::file::detect_type;

/// Checks the dimensions and aspect ratio of an uploaded image.
#[derive(Clone, Default)]
pub struct Dimensions {
    width: Option<u32>,
    height: Option<u32>,
    min_width: Option<u32>,
    min_height: Option<u32>,
    max_width: Option<u32>,
    max_height: Option<u32>,
    ratio: Option<String>,
    min_ratio: Option<String>,
    max_ratio: Option<String>,
}

macro_rules! dimensions {
    ($($method:ident => $field:ident),+ $(,)?) => {
        $(pub fn $method(mut self, value: u32) -> Self {
            self.$field = Some(value);
            self
        })+
    };
}

#[allow(non_snake_case, reason = "Matches Laravel's image dimensions rule")]
impl Dimensions {
    dimensions!(
        width => width, height => height, minWidth => min_width, minHeight => min_height,
        maxWidth => max_width, maxHeight => max_height,
    );

    pub fn ratio(mut self, value: impl ToString) -> Self {
        self.ratio = Some(value.to_string());
        self
    }

    pub fn minRatio(mut self, value: impl ToString) -> Self {
        self.min_ratio = Some(value.to_string());
        self
    }

    pub fn maxRatio(mut self, value: impl ToString) -> Self {
        self.max_ratio = Some(value.to_string());
        self
    }

    pub fn ratioBetween(self, min: impl ToString, max: impl ToString) -> Self {
        self.minRatio(min).maxRatio(max)
    }

    pub(in crate::validation) fn parse(arguments: &str) -> ValidationResult<Self> {
        let invalid = || ValidationError::InvalidRule;
        let mut rule = Self::default();
        for parameter in arguments.split(',') {
            let (key, value) = parameter.split_once('=').ok_or_else(invalid)?;
            rule = match key {
                "width" => rule.width(value.parse().map_err(|_| invalid())?),
                "height" => rule.height(value.parse().map_err(|_| invalid())?),
                "min_width" => rule.minWidth(value.parse().map_err(|_| invalid())?),
                "min_height" => rule.minHeight(value.parse().map_err(|_| invalid())?),
                "max_width" => rule.maxWidth(value.parse().map_err(|_| invalid())?),
                "max_height" => rule.maxHeight(value.parse().map_err(|_| invalid())?),
                "ratio" => rule.ratio(value),
                "min_ratio" => rule.minRatio(value),
                "max_ratio" => rule.maxRatio(value),
                _ => return Err(invalid()),
            };
        }
        rule.ratios()?;
        Ok(rule)
    }

    pub(in crate::validation) fn passes(
        &self,
        file: Option<&UploadedFile>,
    ) -> ValidationResult<bool> {
        let [ratio, min, max] = self.ratios()?;
        let Some(file) = file else {
            return Ok(false);
        };
        if detect_type(file).0 == "svg" {
            return Ok(true);
        }
        let Some((width, height)) = crate::image::encoded_dimensions(&file.get()) else {
            return Ok(false);
        };
        let w = f64::from(width);
        let h = f64::from(height);
        let actual = w / h;
        let precision = 1.0 / (((w + h) / 2.0).max(h) + 1.0);

        Ok(self.width.is_none_or(|bound| width == bound)
            && self.height.is_none_or(|bound| height == bound)
            && self.min_width.is_none_or(|bound| width >= bound)
            && self.min_height.is_none_or(|bound| height >= bound)
            && self.max_width.is_none_or(|bound| width <= bound)
            && self.max_height.is_none_or(|bound| height <= bound)
            && ratio.is_none_or(|ratio| (ratio - actual).abs() <= precision)
            && min.is_none_or(|min| min - actual <= precision)
            && max.is_none_or(|max| actual - max <= precision))
    }

    fn ratios(&self) -> ValidationResult<[Option<f64>; 3]> {
        Ok([
            self.ratio.as_deref().map(parse_ratio).transpose()?,
            self.min_ratio.as_deref().map(parse_ratio).transpose()?,
            self.max_ratio.as_deref().map(parse_ratio).transpose()?,
        ])
    }
}

fn parse_ratio(raw: &str) -> ValidationResult<f64> {
    let ratio = if let Some((numerator, denominator)) = raw.split_once('/') {
        let numerator = numerator
            .trim()
            .parse::<f64>()
            .map_err(|_| ValidationError::InvalidRule)?;
        let denominator = denominator
            .trim()
            .parse::<u32>()
            .map_err(|_| ValidationError::InvalidRule)?;
        numerator / f64::from(denominator)
    } else {
        raw.trim()
            .parse()
            .map_err(|_| ValidationError::InvalidRule)?
    };
    if !ratio.is_finite() || ratio <= 0.0 {
        return Err(ValidationError::InvalidRule);
    }
    Ok(ratio)
}

impl From<Dimensions> for Rule {
    fn from(rule: Dimensions) -> Self {
        Rule::new(Kind::Dimensions(rule))
    }
}
