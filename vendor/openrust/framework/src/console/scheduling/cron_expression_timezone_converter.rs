use chrono::{DateTime, Offset};
use chrono_tz::Tz;

pub(super) fn for_event(expression: &str, next: DateTime<Tz>, display: Tz) -> Vec<String> {
    let target = next.with_timezone(&display);
    let offset =
        (target.offset().fix().local_minus_utc() - next.offset().fix().local_minus_utc()) / 60;
    if offset != 0
        && let Ok(fields) = expression.split_whitespace().collect::<Vec<_>>().try_into()
        && let Some(converted) = convert(fields, offset)
    {
        return converted;
    }
    vec![expression.to_owned()]
}

fn convert([minute, hour, day, month, weekday]: [&str; 5], offset: i32) -> Option<Vec<String>> {
    let unrestricted = (day, month, weekday) == ("*", "*", "*");
    let every_hour = unrestricted && expand(hour, 0, 23).is_some_and(|values| values.len() == 24);
    let mut expressions = Vec::new();
    for (hour_carry, minute) in shifted(minute, offset % 60, 59, every_hour)? {
        for (day_carry, hour) in shifted(hour, offset / 60 + hour_carry, 23, unrestricted)? {
            if day_carry == 0 || unrestricted {
                expressions.push(format!("{minute} {hour} {day} {month} {weekday}"));
            } else if weekday == "*" {
                for (day, month) in shift_month_days(day, month, day_carry)? {
                    expressions.push(format!("{minute} {hour} {day} {month} *"));
                }
            } else {
                // A shift cannot preserve cron's day-of-month OR day-of-week rule.
                if day != "*" || month != "*" {
                    return None;
                }
                let mut weekdays = expand(weekday, 0, 7)?;
                for value in &mut weekdays {
                    *value = (*value + day_carry).rem_euclid(7);
                }
                weekdays.sort_unstable();
                weekdays.dedup();
                let weekdays = collapse(&weekdays, 0, 6);
                expressions.push(format!("{minute} {hour} * * {weekdays}"));
            }
        }
    }
    Some(expressions)
}

fn shifted(field: &str, offset: i32, max: i32, merge: bool) -> Option<Vec<(i32, String)>> {
    if offset == 0 {
        return Some(vec![(0, field.to_owned())]);
    }
    let period = max + 1;
    let mut groups: Vec<(i32, Vec<i32>)> = Vec::new();
    for value in expand(field, 0, max)? {
        let shifted = value + offset;
        let carry = if merge { 0 } else { shifted.div_euclid(period) };
        let value = shifted.rem_euclid(period);
        if let Some((last, values)) = groups.last_mut()
            && *last == carry
        {
            values.push(value);
        } else {
            groups.push((carry, vec![value]));
        }
    }
    Some(
        groups
            .into_iter()
            .map(|(carry, mut values)| {
                values.sort_unstable();
                (carry, collapse(&values, 0, max))
            })
            .collect(),
    )
}

fn expand(field: &str, min: i32, max: i32) -> Option<Vec<i32>> {
    fn number(value: &str) -> Option<i32> {
        if value.bytes().all(|byte| byte.is_ascii_digit()) {
            return value.parse().ok();
        }
        None
    }

    let mut values = Vec::new();
    for part in field.split(',') {
        let (range, step) = match part.split_once('/') {
            Some((range, step)) => (range, Some(number(step)? as usize)),
            None => (part, None),
        };
        let step_size = step.unwrap_or(1);
        let (start, end) = if range == "*" {
            (min, max)
        } else if let Some((start, end)) = range.split_once('-') {
            (number(start)?, number(end)?)
        } else {
            let start = number(range)?;
            (start, step.map_or(start, |_| max))
        };
        if step_size == 0 || start < min || end > max || start > end {
            return None;
        }
        values.extend((start..=end).step_by(step_size));
    }
    values.sort_unstable();
    values.dedup();
    Some(values)
}

fn collapse(values: &[i32], min: i32, max: i32) -> String {
    if values.len() == (max - min + 1) as usize {
        return "*".to_owned();
    }
    if let [first, second, _, ..] = values {
        let step = second - first;
        if values.windows(2).all(|pair| pair[1] - pair[0] == step) {
            let last = values[values.len() - 1];
            return if step == 1 {
                format!("{first}-{last}")
            } else if *first == min && last + step > max {
                format!("*/{step}")
            } else {
                format!("{first}-{last}/{step}")
            };
        }
    }
    values
        .chunk_by(|left, right| *right == *left + 1)
        .flat_map(|run| match run {
            [first, _, _, ..] => vec![format!("{first}-{}", run[run.len() - 1])],
            _ => run.iter().map(i32::to_string).collect(),
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn shift_month_days(day: &str, month: &str, carry: i32) -> Option<Vec<(String, String)>> {
    const MONTH_LENGTHS: [i32; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let length = |month: i32| MONTH_LENGTHS[(month - 1) as usize];
    let days = expand(day, 1, 31)?;
    let mut by_month: Vec<(i32, Vec<i32>)> = Vec::new();

    for month in expand(month, 1, 12)? {
        for &day in &days {
            let (mut month, mut day) = (month, day);
            if month == 2 && day == 29 {
                return None;
            }
            if day > length(month) {
                continue;
            }
            day += carry;
            while day < 1 {
                month = (month - 2).rem_euclid(12) + 1;
                if month == 2 {
                    return None;
                }
                day += length(month);
            }
            while day > length(month) {
                if month == 2 {
                    return None;
                }
                day -= length(month);
                month = month % 12 + 1;
            }
            if let Some((_, days)) = by_month.iter_mut().find(|(key, _)| *key == month) {
                days.push(day);
            } else {
                by_month.push((month, vec![day]));
            }
        }
    }

    let mut by_days: Vec<(Vec<i32>, Vec<i32>)> = Vec::new();
    for (month, mut days) in by_month {
        days.sort_unstable();
        days.dedup();
        if let Some((_, months)) = by_days.iter_mut().find(|(key, _)| *key == days) {
            months.push(month);
        } else {
            by_days.push((days, vec![month]));
        }
    }
    if by_days.is_empty() {
        return None;
    }
    Some(
        by_days
            .into_iter()
            .map(|(days, mut months)| {
                months.sort_unstable();
                (collapse(&days, 1, 31), collapse(&months, 1, 12))
            })
            .collect(),
    )
}
