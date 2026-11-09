//! 阅读摘要格式化预览；不保存或宣称真实阅读统计，时区仅支持显式固定偏移。

use chrono::{DateTime, Datelike, FixedOffset, SecondsFormat};
use openrust::http::{Inject, Json, Query};
use openrust::translation::DateFormatStyle;
use openrust::translation::DateTimeFormatStyle;
use openrust::translation::MessageArgs;
use openrust::translation::RequestLocale;
use openrust::translation::Translator;
use openrust::{Error, Result, ValidationErrors};

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ReadingSummaryInput {
    count: u32,
    at: String,
    zone: String,
}
#[derive(serde::Serialize)]
pub(crate) struct ReadingSummary {
    preview: bool,
    locale: String,
    count: u32,
    reading: String,
    date: String,
    datetime: String,
    as_of: String,
    time_zone: String,
    local_iso: String,
}
fn invalid(locale: &RequestLocale, i18n: &Translator, field: &str, key: &str) -> Error {
    let mut errors = ValidationErrors::new();
    errors.add(field, locale.translate(i18n, key, &MessageArgs::new()));
    Error::validation_with_status(openrust::http::axum::http::StatusCode::BAD_REQUEST, errors)
}
pub(crate) async fn show(
    locale: RequestLocale,
    Inject(i18n): Inject<Translator>,
    Query(input): Query<ReadingSummaryInput>,
) -> Result<Json<ReadingSummary>> {
    let mut errors = ValidationErrors::new();
    if input.count > 1_000_000 {
        errors.add(
            "count",
            locale.translate(&i18n, "example::reading_invalid_count", &MessageArgs::new()),
        );
    }
    if input.at.len() > 40 {
        errors.add(
            "at",
            locale.translate(&i18n, "example::reading_invalid_at", &MessageArgs::new()),
        );
    }
    let offset = match input.zone.as_str() {
        "utc" => FixedOffset::east_opt(0),
        "utc+08:00" => FixedOffset::east_opt(8 * 60 * 60),
        _ => None,
    };
    if offset.is_none() {
        errors.add(
            "zone",
            locale.translate(&i18n, "example::reading_invalid_zone", &MessageArgs::new()),
        );
    }
    if !errors.is_empty() {
        return Err(Error::validation_with_status(
            openrust::http::axum::http::StatusCode::BAD_REQUEST,
            errors,
        ));
    }
    let offset =
        offset.ok_or_else(|| invalid(&locale, &i18n, "zone", "example::reading_invalid_zone"))?;
    let instant = DateTime::parse_from_rfc3339(&input.at)
        .map_err(|_| invalid(&locale, &i18n, "at", "example::reading_invalid_at"))?;
    let local = instant.with_timezone(&offset);
    if !(2000..=2100).contains(&local.year()) {
        return Err(invalid(
            &locale,
            &i18n,
            "at",
            "example::reading_invalid_year",
        ));
    }
    let date = i18n
        .format_date(locale.locale(), local.date_naive(), DateFormatStyle::Long)
        .map_err(Error::internal)?;
    let datetime = i18n
        .format_datetime(locale.locale(), &local, DateTimeFormatStyle::Long)
        .map_err(Error::internal)?;
    let reading = i18n.choice(
        "example::reading_choice",
        i64::from(input.count).into(),
        &MessageArgs::new(),
        Some(locale.locale()),
    )?;
    let as_of = i18n.string(
        "example::reading_as_of",
        [("date", date.as_str())],
        Some(locale.locale()),
        true,
    )?;
    Ok(Json(ReadingSummary {
        preview: true,
        locale: locale.as_str().into(),
        count: input.count,
        reading,
        date,
        datetime,
        as_of,
        time_zone: input.zone,
        local_iso: local.to_rfc3339_opts(SecondsFormat::Secs, false),
    }))
}
