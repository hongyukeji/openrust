use super::{Builtin, Faker, Json, arguments, flag, integer};
use crate::database::{DatabaseError, DatabaseResult, Value};

pub(super) fn formatter(name: &str) -> Option<Builtin> {
    Some(match name {
        "md5" => |faker, _| Ok(faker.md5().into()),
        "sha1" => |faker, _| Ok(faker.sha1().into()),
        "sha256" => |faker, _| Ok(faker.sha256().into()),
        "randomhtml" => |faker, args| {
            let [depth, width] = arguments(args)?;
            faker
                .random_html(html_limit(depth)?, html_limit(width)?)
                .map(Value::from)
        },
        "word" => |faker, _| Ok(faker.word().into()),
        "words" => |faker, args| series(faker, args, " ", Faker::words),
        "sentences" => |faker, args| series(faker, args, " ", Faker::sentences),
        "paragraphs" => |faker, args| series(faker, args, "\n\n", Faker::paragraphs),
        "sentence" => |faker, args| {
            let (count, variable) = length_and_variability(args, 6)?;
            Ok(faker.sentence_with_word_count(count, variable).into())
        },
        "paragraph" => |faker, args| {
            let (count, variable) = length_and_variability(args, 3)?;
            Ok(faker.paragraph(count, variable).into())
        },
        "text" => |faker, args| {
            let [max] = arguments(args)?;
            faker.text(length(max, 200)?).map(Value::from)
        },
        "realtext" => |faker, args| {
            let [max, order] = arguments(args)?;
            faker
                .real_text(Some(length(max, 200)?), Some(length(order, 2)?))
                .map(Value::from)
        },
        "realtextbetween" => |faker, args| {
            let [min, max, order] = arguments(args)?;
            faker
                .real_text_between(
                    Some(length(min, 160)?),
                    Some(length(max, 200)?),
                    Some(length(order, 2)?),
                )
                .map(Value::from)
        },
        "password" => |faker, args| {
            let [min, max] = arguments(args)?;
            faker
                .password(Some(length(min, 6)?), Some(length(max, 20)?))
                .map(Value::from)
        },
        "slug" => |faker, args| {
            let (count, variable) = length_and_variability(args, 6)?;
            Ok(faker.slug(Some(count), variable).into())
        },
        "companyemail" => |faker, _| faker.company_email().map(Value::from),
        "safeemaildomain" => |faker, _| Ok(faker.safe_email_domain().into()),
        "freeemaildomain" => |faker, _| Ok(faker.free_email_domain().into()),
        "domainword" => |faker, _| faker.domain_word().map(Value::from),
        "domainname" => |faker, _| faker.domain_name().map(Value::from),
        "tld" => |faker, _| Ok(faker.tld().into()),
        "url" => |faker, _| faker.url().map(Value::from),
        "ipv4" => |faker, _| Ok(faker.ipv4().into()),
        "ipv6" => |faker, _| Ok(faker.ipv6().into()),
        "localipv4" => |faker, _| Ok(faker.local_ipv4().into()),
        "macaddress" => |faker, _| Ok(faker.mac_address().into()),
        "useragent" => |faker, _| faker.user_agent().map(Value::from),
        "firefox" => |faker, _| faker.firefox().map(Value::from),
        "chrome" => |faker, _| Ok(faker.chrome().into()),
        "msedge" => |faker, _| Ok(faker.msedge().into()),
        "safari" => |faker, _| Ok(faker.safari().into()),
        "opera" => |faker, _| Ok(faker.opera().into()),
        "internetexplorer" => |faker, _| Ok(faker.internet_explorer().into()),
        "macprocessor" => |faker, _| Ok(faker.mac_processor().into()),
        "linuxprocessor" => |faker, _| Ok(faker.linux_processor().into()),
        "windowsplatformtoken" => |faker, _| Ok(faker.windows_platform_token().into()),
        "macplatformtoken" => |faker, _| Ok(faker.mac_platform_token().into()),
        "linuxplatformtoken" => |faker, _| Ok(faker.linux_platform_token().into()),
        "iosmobiletoken" => |faker, _| Ok(faker.ios_mobile_token().into()),
        _ => return None,
    })
}

fn length(value: Option<&Value>, default: usize) -> DatabaseResult<usize> {
    integer(value)?
        .map(|length| {
            usize::try_from(length.max(0))
                .map_err(|_| DatabaseError::InvalidQuery("Faker length is out of range"))
        })
        .transpose()
        .map(|length| length.unwrap_or(if value.is_none() { default } else { 0 }))
}

fn check_capacity(count: usize) -> DatabaseResult<()> {
    if count > isize::MAX as usize / std::mem::size_of::<String>() {
        return Err(DatabaseError::InvalidQuery(
            "Faker text count exceeds allocation capacity",
        ));
    }
    Ok(())
}

fn length_and_variability(args: &[Value], default: usize) -> DatabaseResult<(usize, bool)> {
    let [count, variable] = arguments(args)?;
    let count = length(count, default)?;
    let variable = variable.map_or(Ok(true), |value| flag(Some(value)))?;
    let maximum = if variable {
        count.saturating_add(count / 2).saturating_add(1)
    } else {
        count
    };
    check_capacity(maximum)?;
    Ok((count, variable))
}

fn series(
    faker: &Faker,
    args: &[Value],
    separator: &str,
    generate: impl FnOnce(&Faker, usize) -> Vec<String>,
) -> DatabaseResult<Value> {
    let [count, as_text] = arguments(args)?;
    let count = length(count, 3)?;
    let as_text = flag(as_text)?;
    check_capacity(count)?;
    let sample = generate(faker, count);
    Ok(if as_text {
        sample.join(separator).into()
    } else {
        Value::Json(Json::Array(sample.into_iter().map(Json::from).collect()))
    })
}

fn html_limit(value: Option<&Value>) -> DatabaseResult<Option<i32>> {
    integer(value)?
        .map(|limit| {
            i32::try_from(limit)
                .map_err(|_| DatabaseError::InvalidQuery("Faker HTML limit is out of range"))
        })
        .transpose()
}
