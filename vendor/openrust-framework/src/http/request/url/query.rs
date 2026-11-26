use std::cmp::Ordering;

use serde_json::{Map, Value};

use crate::{
    http::HttpError,
    support::{Arr, input_scalars::numeric_prefix},
};

pub(super) fn encode(value: &Value) -> Result<String, HttpError> {
    Arr::query(value).map_err(|_| HttpError::InvalidRequest)
}

pub(super) fn canonical(raw: &str) -> Result<String, HttpError> {
    let decoded = super::super::query::parse_canonical(raw)?;
    let entries: Map<String, Value> = match decoded {
        Value::Object(entries) => entries,
        Value::Array(entries) => entries
            .into_iter()
            .enumerate()
            .map(|(index, value)| (index.to_string(), value))
            .collect(),
        _ => unreachable!("query parsing always returns an array or object"),
    };
    let mut sorted: Vec<_> = entries
        .into_iter()
        .enumerate()
        .map(|(index, (key, value))| (index, key, value))
        .collect();
    sort_keys(&mut sorted);
    let query = sorted
        .into_iter()
        .map(|(_, key, value)| (key, value))
        .collect();
    encode(&Value::Object(query))
}

fn integer_key(key: &str) -> Option<i64> {
    key.parse::<i64>()
        .ok()
        .filter(|integer| integer.to_string() == key)
}

type SortEntry = (usize, String, Value);

fn greater(values: &[SortEntry], left: usize, right: usize) -> bool {
    compare_keys(&values[left].1, &values[right].1)
        .then_with(|| values[left].0.cmp(&values[right].0))
        .is_gt()
}

// PHP regular comparison is not a total order for mixed keys. Preserve the
// comparison sequence of Zend's bounded sort.
fn sort_keys(values: &mut [SortEntry]) {
    let mut pending = vec![(0, values.len())];
    while let Some((start, end)) = pending.pop() {
        let count = end - start;
        if count <= 16 {
            sort_small(&mut values[start..end]);
            continue;
        }
        let middle = start + count / 2;
        if count >= 1024 {
            let delta = (count / 2) / 2;
            sort_sample(
                values,
                &[start, start + delta, middle, middle + delta, end - 1],
            );
        } else {
            sort_sample(values, &[start, middle, end - 1]);
        }
        let pivot = start + 1;
        values.swap(pivot, middle);
        let mut left = pivot + 1;
        let mut right = end - 1;
        'partition: loop {
            while greater(values, pivot, left) {
                left += 1;
                if left == right {
                    break 'partition;
                }
            }
            right -= 1;
            if right == left {
                break;
            }
            while greater(values, right, pivot) {
                right -= 1;
                if right == left {
                    break 'partition;
                }
            }
            values.swap(left, right);
            left += 1;
            if left == right {
                break;
            }
        }
        values.swap(pivot, left - 1);
        pending.push((start, left - 1));
        pending.push((left, end));
    }
}

fn sort_sample(values: &mut [SortEntry], sample: &[usize]) {
    let [a, b, c, rest @ ..] = sample else {
        return;
    };
    if greater(values, *a, *b) {
        if greater(values, *c, *b) {
            values.swap(*a, *b);
            if greater(values, *b, *c) {
                values.swap(*b, *c);
            }
        } else {
            values.swap(*a, *c);
        }
    } else if greater(values, *b, *c) {
        values.swap(*b, *c);
        if greater(values, *a, *b) {
            values.swap(*a, *b);
        }
    }
    for offset in 0..rest.len() {
        let mut position = offset + 3;
        while position > 0 && greater(values, sample[position - 1], sample[position]) {
            values.swap(sample[position - 1], sample[position]);
            position -= 1;
        }
    }
}

fn sort_small(values: &mut [SortEntry]) {
    if values.len() <= 1 {
        return;
    }
    if values.len() == 2 {
        if greater(values, 0, 1) {
            values.swap(0, 1);
        }
        return;
    }
    if values.len() <= 5 {
        sort_sample(values, &(0..values.len()).collect::<Vec<_>>());
        return;
    }
    for index in 1..values.len() {
        let mut destination = index - 1;
        if !greater(values, destination, index) {
            continue;
        }
        if index < 6 {
            while destination > 0 {
                destination -= 1;
                if !greater(values, destination, index) {
                    destination += 1;
                    break;
                }
            }
        } else {
            loop {
                destination -= 2;
                if !greater(values, destination, index) {
                    destination += 1;
                    if !greater(values, destination, index) {
                        destination += 1;
                    }
                    break;
                }
                if destination == 0 {
                    break;
                }
                if destination == 1 {
                    destination = usize::from(greater(values, index, 0));
                    break;
                }
            }
        }
        for position in (destination + 1..=index).rev() {
            values.swap(position - 1, position);
        }
    }
}

#[allow(
    clippy::float_cmp,
    reason = "Matches PHP comparisons for equal doubles and overflowing integers"
)]
fn compare_keys(left: &str, right: &str) -> Ordering {
    let numeric = |key: &str| {
        let key = key.trim_matches([' ', '\t', '\n', '\r', '\u{b}', '\u{c}']);
        (numeric_prefix(key) == Some(key))
            .then(|| (key.parse::<i64>().ok(), key.parse::<f64>().ok()))
    };
    match (numeric(left), numeric(right)) {
        (Some((Some(a), _)), Some((Some(b), _))) => a.cmp(&b),
        (Some((a, Some(a_float))), Some((b, Some(b_float)))) => {
            if integer_key(left).is_none() && integer_key(right).is_none() {
                let overflow = |text: &str| {
                    let text = text.trim_matches([' ', '\t', '\n', '\r', '\u{b}', '\u{c}']);
                    !text.contains(['.', 'e', 'E']) && text.parse::<i64>().is_err()
                };
                if a_float == b_float
                    && (!a_float.is_finite() || (overflow(left) && overflow(right)))
                {
                    return left.cmp(right);
                }
                if a.is_some() && overflow(right) {
                    return if b_float.is_sign_negative() {
                        Ordering::Greater
                    } else {
                        Ordering::Less
                    };
                }
                if b.is_some() && overflow(left) {
                    return if a_float.is_sign_negative() {
                        Ordering::Less
                    } else {
                        Ordering::Greater
                    };
                }
            }
            a_float.partial_cmp(&b_float).unwrap_or(Ordering::Equal)
        }
        _ => left.cmp(right),
    }
}
