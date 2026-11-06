//! 固定 PHP 8.5 普通比较与模块优先级稳定排序。

// 排序驱动改写自 php-src php-8.5.0 Zend/zend_sort.c：
// https://github.com/php/php-src/blob/php-8.5.0/Zend/zend_sort.c
// 原实现由 Xinchen Hui、Sterling Hughes 编写，基于 LLVM libc++ std::sort。
// Zend 文件适用 Zend Engine License 2.00：
// https://github.com/php/php-src/blob/php-8.5.0/Zend/LICENSE
// The Zend Engine License, Version 2.00
// Copyright (c) 1999-2006 Zend Technologies Ltd. All rights reserved.
//
// Redistribution and use in source and binary forms, with or without
// modification, is permitted provided that the following conditions
// are met:
//
// 1. Redistributions of source code must retain the above copyright
//    notice, this list of conditions and the following disclaimer.
// 2. Redistributions in binary form must reproduce the above
//    copyright notice, this list of conditions and the following
//    disclaimer in the documentation and/or other materials provided
//    with the distribution.
// 3. The names "Zend" and "Zend Engine" must not be used to endorse
//    or promote products derived from this software without prior
//    permission from Zend Technologies Ltd. For written permission,
//    please contact license@zend.com.
// 4. Zend Technologies Ltd. may publish revised and/or new versions
//    of the license from time to time. Each version will be given a
//    distinguishing version number.
//    Once covered code has been published under a particular version
//    of the license, you may always continue to use it under the
//    terms of that version. You may also choose to use such covered
//    code under the terms of any subsequent version of the license
//    published by Zend Technologies Ltd. No one other than Zend
//    Technologies Ltd. has the right to modify the terms applicable
//    to covered code created under this License.
// 5. Redistributions of any form whatsoever must retain the following
//    acknowledgment:
//    "This product includes the Zend Engine, freely available at
//    http://www.zend.com"
// 6. All advertising materials mentioning features or use of this
//    software must display the following acknowledgment:
//    "The Zend Engine is freely available at http://www.zend.com"
//
// THIS SOFTWARE IS PROVIDED BY ZEND TECHNOLOGIES LTD. ``AS IS'' AND
// ANY EXPRESSED OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO,
// THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A
// PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL ZEND
// TECHNOLOGIES LTD. BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
// SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT
// LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF
// USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND
// ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY,
// OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT
// OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
// SUCH DAMAGE.
//
// 下列派生排序驱动保留原文件内 libc++ 的 MIT 许可：
// Copyright (c) 2009-2012 by the contributors listed at
// http://llvm.org/svn/llvm-project/libcxx/trunk/CREDITS.TXT
//
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
// FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS
// IN THE SOFTWARE.

use std::cmp::Ordering;

use serde_json::Value;

use openrust::core::{Error, Result};

/// 按固定 `Module::getPriority` 的 `(string)($raw ?? 0)` 生成显示值。
///
/// JSON 对象和数组均对应 PHP array；其字符串强制转换产生 warning，
/// Laravel 将该边界提升为错误，因此这里明确拒绝，不返回静默的 "Array"。
pub(crate) fn priority_label(value: &Value) -> Result<String> {
    match value {
        Value::Null => Ok("0".to_owned()),
        Value::Bool(true) => Ok("1".to_owned()),
        Value::Bool(false) => Ok(String::new()),
        Value::String(value) => Ok(value.clone()),
        Value::Number(value) => {
            if let Some(value) = value.as_i64() {
                Ok(value.to_string())
            } else {
                php_double_string(value.as_f64().ok_or_else(priority_error)?)
            }
        }
        Value::Array(_) | Value::Object(_) => Err(Error::custom(
            "module_priority_display_invalid",
            "模块优先级的关联数组无法转换为显示字符串",
        )),
    }
}

/// 按 PHP `SORT_REGULAR` 排序；null priority 等同缺省 0，平局保留输入顺序。
///
/// PHP 普通比较并非全序，因此这里保留固定 Zend 的比较/交换调用顺序，
/// 不将它交给要求全序的 Rust 排序器。先验证并排序索引，成功后才移动调用方元素。
pub(crate) fn sort_by_priority<T, F>(values: &mut [T], value: F, descending: bool) -> Result<()>
where
    F: Fn(&T) -> &Value,
{
    sort_priorities(values, value, descending, PrioritySort::Manifest)
}

/// 按固定 `FileRepository::getOrdered` 的严格相等及方向分支排序。
///
/// 此回调与 `SORT_REGULAR` 不同：弱相等但不严格相等时返回负值，缺值保留 null。
pub(crate) fn sort_repository_priority<T, F>(
    values: &mut [T],
    value: F,
    descending: bool,
) -> Result<()>
where
    F: Fn(&T) -> &Value,
{
    sort_priorities(values, value, descending, PrioritySort::Repository)
}

#[derive(Clone, Copy)]
enum PrioritySort {
    Manifest,
    Repository,
}

fn sort_priorities<T, F>(
    values: &mut [T],
    value: F,
    descending: bool,
    mode: PrioritySort,
) -> Result<()>
where
    F: Fn(&T) -> &Value,
{
    let priorities = values
        .iter()
        .map(|entry| {
            let priority = value(entry);
            if priority.is_null() && matches!(mode, PrioritySort::Manifest) {
                Ok(PhpValue::Integer(0))
            } else {
                PhpValue::from_json(priority)
            }
        })
        .collect::<Result<Vec<_>>>()?;
    let mut order: Vec<_> = (0..values.len()).collect();
    let compare = |left: usize, right: usize| {
        let result = priority_compare(&priorities[left], &priorities[right], descending, mode);
        result.then_with(|| left.cmp(&right))
    };
    let count = order.len();
    zend_sort(&mut order, 0, count, &compare);
    drop(priorities);

    let mut destinations = vec![0; order.len()];
    for (destination, original) in order.into_iter().enumerate() {
        destinations[original] = destination;
    }
    for position in 0..values.len() {
        while destinations[position] != position {
            let destination = destinations[position];
            values.swap(position, destination);
            destinations.swap(position, destination);
        }
    }
    Ok(())
}

fn priority_compare(
    left: &PhpValue<'_>,
    right: &PhpValue<'_>,
    descending: bool,
    mode: PrioritySort,
) -> Ordering {
    match mode {
        PrioritySort::Manifest => {
            let raw = php_compare(left, right);
            // 降序只反转值比较，不反转平局原序号；也不能交换左右参数。
            if descending { raw.reverse() } else { raw }
        }
        PrioritySort::Repository => {
            if php_identical(left, right) {
                return Ordering::Equal;
            }
            // PHP 将 a > b 编译为交换操作数后的 b < a；关联数组比较
            // 并不反对称，不能替换成 zend_compare(a, b) > 0。
            let relation = if descending {
                php_compare(left, right)
            } else {
                php_compare(right, left)
            };
            if relation == Ordering::Less {
                Ordering::Greater
            } else {
                Ordering::Less
            }
        }
    }
}

enum PhpValue<'a> {
    Null,
    Bool(bool),
    Integer(i64),
    Double { number: f64, text: String },
    String(&'a str),
    Array(Vec<(PhpKey<'a>, PhpValue<'a>)>),
}

#[derive(PartialEq, Eq)]
enum PhpKey<'a> {
    Integer(i64),
    String(&'a str),
}

impl<'a> PhpValue<'a> {
    fn from_json(value: &'a Value) -> Result<Self> {
        Ok(match value {
            Value::Null => Self::Null,
            Value::Bool(value) => Self::Bool(*value),
            Value::String(value) => Self::String(value),
            Value::Number(value) => {
                if let Some(value) = value.as_i64() {
                    Self::Integer(value)
                } else {
                    let number = value.as_f64().ok_or_else(priority_error)?;
                    if !number.is_finite() {
                        return Err(priority_error());
                    }
                    Self::Double {
                        number,
                        text: php_double_string(number)?,
                    }
                }
            }
            Value::Array(values) => Self::Array(
                values
                    .iter()
                    .enumerate()
                    .map(|(key, value)| {
                        Ok((
                            PhpKey::Integer(i64::try_from(key).map_err(|_| priority_error())?),
                            Self::from_json(value)?,
                        ))
                    })
                    .collect::<Result<_>>()?,
            ),
            Value::Object(values) => Self::Array(
                values
                    .iter()
                    .map(|(key, value)| {
                        let key = key
                            .parse::<i64>()
                            .ok()
                            .filter(|number| number.to_string() == *key)
                            .map_or(PhpKey::String(key), PhpKey::Integer);
                        Ok((key, Self::from_json(value)?))
                    })
                    .collect::<Result<_>>()?,
            ),
        })
    }

    fn truthy(&self) -> bool {
        match self {
            Self::Null => false,
            Self::Bool(value) => *value,
            Self::Integer(value) => *value != 0,
            Self::Double { number, .. } => *number != 0.0,
            Self::String(value) => !value.is_empty() && *value != "0",
            Self::Array(values) => !values.is_empty(),
        }
    }
}

fn priority_error() -> Error {
    Error::custom(
        "module_priority_unrepresentable",
        "模块优先级无法按固定 PHP 数值规则表示",
    )
}

// PHP === 对 double 使用精确的 IEEE 相等（包括 -0.0 === 0.0）。
#[allow(clippy::float_cmp)]
fn php_identical(left: &PhpValue<'_>, right: &PhpValue<'_>) -> bool {
    match (left, right) {
        (PhpValue::Null, PhpValue::Null) => true,
        (PhpValue::Bool(left), PhpValue::Bool(right)) => left == right,
        (PhpValue::Integer(left), PhpValue::Integer(right)) => left == right,
        (PhpValue::Double { number: left, .. }, PhpValue::Double { number: right, .. }) => {
            left == right
        }
        (PhpValue::String(left), PhpValue::String(right)) => left == right,
        (PhpValue::Array(left), PhpValue::Array(right)) => {
            left.len() == right.len()
                && left
                    .iter()
                    .zip(right)
                    .all(|((left_key, left), (right_key, right))| {
                        left_key == right_key && php_identical(left, right)
                    })
        }
        _ => false,
    }
}

fn php_compare(left: &PhpValue<'_>, right: &PhpValue<'_>) -> Ordering {
    match (left, right) {
        (PhpValue::Array(left), PhpValue::Array(right)) => compare_arrays(left, right),
        (PhpValue::String(left), PhpValue::String(right)) => compare_strings(left, right),
        (PhpValue::Null, PhpValue::String(right)) => "".cmp(right),
        (PhpValue::String(left), PhpValue::Null) => left.cmp(&""),
        (PhpValue::Bool(_) | PhpValue::Null, _) | (_, PhpValue::Bool(_) | PhpValue::Null) => {
            left.truthy().cmp(&right.truthy())
        }
        (PhpValue::String(string), PhpValue::Integer(number)) => {
            compare_number_string(Numeric::Integer(*number), &number.to_string(), string).reverse()
        }
        (PhpValue::Integer(number), PhpValue::String(string)) => {
            compare_number_string(Numeric::Integer(*number), &number.to_string(), string)
        }
        (PhpValue::String(string), PhpValue::Double { number, text }) => compare_number_string(
            Numeric::Double {
                number: *number,
                overflow: 0,
            },
            text,
            string,
        )
        .reverse(),
        (PhpValue::Double { number, text }, PhpValue::String(string)) => compare_number_string(
            Numeric::Double {
                number: *number,
                overflow: 0,
            },
            text,
            string,
        ),
        (PhpValue::Array(_), _) => Ordering::Greater,
        (_, PhpValue::Array(_)) => Ordering::Less,
        (PhpValue::Integer(left), PhpValue::Integer(right)) => left.cmp(right),
        (PhpValue::Double { number: left, .. }, PhpValue::Double { number: right, .. }) => {
            compare_double(*left, *right)
        }
        (PhpValue::Integer(left), PhpValue::Double { number: right, .. }) => {
            compare_double(Numeric::Integer(*left).double(), *right)
        }
        (PhpValue::Double { number: left, .. }, PhpValue::Integer(right)) => {
            compare_double(*left, Numeric::Integer(*right).double())
        }
    }
}

fn compare_arrays(
    left: &[(PhpKey<'_>, PhpValue<'_>)],
    right: &[(PhpKey<'_>, PhpValue<'_>)],
) -> Ordering {
    let length = left.len().cmp(&right.len());
    if length != Ordering::Equal {
        return length;
    }
    for (key, value) in left {
        let Some((_, other)) = right.iter().find(|(other_key, _)| key == other_key) else {
            // PHP 缺 key 时两方向都可能返回 Greater，不能改成 key 字典序。
            return Ordering::Greater;
        };
        let result = php_compare(value, other);
        if result != Ordering::Equal {
            return result;
        }
    }
    Ordering::Equal
}

#[derive(Clone, Copy)]
enum Numeric {
    Integer(i64),
    Double { number: f64, overflow: i8 },
}

impl Numeric {
    // 固定 PHP 的 Long/Double 混合比较确实将整数转 IEEE double。
    #[allow(clippy::cast_precision_loss)]
    fn double(self) -> f64 {
        match self {
            Self::Integer(value) => value as f64,
            Self::Double { number, .. } => number,
        }
    }

    fn overflow(self) -> i8 {
        match self {
            Self::Integer(_) => 0,
            Self::Double { overflow, .. } => overflow,
        }
    }
}

fn compare_numbers(left: Numeric, right: Numeric) -> Ordering {
    match (left, right) {
        (Numeric::Integer(left), Numeric::Integer(right)) => left.cmp(&right),
        _ => compare_double(left.double(), right.double()),
    }
}

fn compare_double(left: f64, right: f64) -> Ordering {
    if left < right {
        Ordering::Less
    } else if left > right {
        Ordering::Greater
    } else {
        Ordering::Equal
    }
}

fn compare_number_string(number: Numeric, text: &str, string: &str) -> Ordering {
    numeric_string(string).map_or_else(
        || text.as_bytes().cmp(string.as_bytes()),
        |other| compare_numbers(number, other),
    )
}

// 这些精确相等判定是 Zend 的溢出 fallback 条件，不能改成误差阈值。
#[allow(clippy::float_cmp)]
fn compare_strings(left: &str, right: &str) -> Ordering {
    let (Some(left_number), Some(right_number)) = (numeric_string(left), numeric_string(right))
    else {
        return left.as_bytes().cmp(right.as_bytes());
    };
    let left_overflow = left_number.overflow();
    let right_overflow = right_number.overflow();
    if left_overflow != 0
        && left_overflow == right_overflow
        && left_number.double() - right_number.double() == 0.0
    {
        return left.as_bytes().cmp(right.as_bytes());
    }
    match (left_number, right_number) {
        (Numeric::Integer(left), Numeric::Integer(right)) => left.cmp(&right),
        (Numeric::Integer(_), Numeric::Double { overflow, .. }) if overflow != 0 => {
            0.cmp(&overflow)
        }
        (Numeric::Double { overflow, .. }, Numeric::Integer(_)) if overflow != 0 => {
            overflow.cmp(&0)
        }
        _ => {
            let left_double = left_number.double();
            let right_double = right_number.double();
            if left_double == right_double && !left_double.is_finite() {
                left.as_bytes().cmp(right.as_bytes())
            } else {
                compare_double(left_double, right_double)
            }
        }
    }
}

fn numeric_string(value: &str) -> Option<Numeric> {
    let value = value.trim_matches([' ', '\t', '\n', '\r', '\u{b}', '\u{c}']);
    let bytes = value.as_bytes();
    let mut position = usize::from(matches!(bytes.first(), Some(b'+' | b'-')));
    let integer_start = position;
    while bytes.get(position).is_some_and(u8::is_ascii_digit) {
        position += 1;
    }
    let integer_end = position;
    let mut has_fraction = false;
    if bytes.get(position) == Some(&b'.') {
        has_fraction = true;
        position += 1;
        while bytes.get(position).is_some_and(u8::is_ascii_digit) {
            position += 1;
        }
    }
    let has_digits = integer_end > integer_start || (has_fraction && position > integer_end + 1);
    if !has_digits {
        return None;
    }
    let mut has_exponent = false;
    if matches!(bytes.get(position), Some(b'e' | b'E')) {
        has_exponent = true;
        position += 1;
        if matches!(bytes.get(position), Some(b'+' | b'-')) {
            position += 1;
        }
        let exponent_start = position;
        while bytes.get(position).is_some_and(u8::is_ascii_digit) {
            position += 1;
        }
        if exponent_start == position {
            return None;
        }
    }
    if position != bytes.len() {
        return None;
    }
    if !has_fraction
        && !has_exponent
        && let Ok(number) = value.parse::<i64>()
    {
        return Some(Numeric::Integer(number));
    }
    let integer_digits = bytes[integer_start..integer_end]
        .iter()
        .skip_while(|byte| **byte == b'0')
        .count();
    // Zend 64 位解析器在20个有效整数位处先记录溢出，再读后面的指数；
    // 即使负指数令最终数值很小，此标记仍影响 string/string smart 比较。
    let integer_overflow = integer_digits >= 20 || (!has_fraction && !has_exponent);
    let overflow = if integer_overflow {
        if bytes.first() == Some(&b'-') { -1 } else { 1 }
    } else {
        0
    };
    Some(Numeric::Double {
        number: value.parse::<f64>().ok()?,
        overflow,
    })
}

fn php_double_string(number: f64) -> Result<String> {
    if !number.is_finite() {
        return Err(priority_error());
    }
    // Zend dtoa mode2 与 Rust 显式精度均按 half-even 舍入。
    // 先取14个有效数字，再按 gcvt 布局；不能截断 shortest 表示。
    let scientific = format!("{number:.13e}");
    let (mantissa, exponent) = scientific.split_once('e').ok_or_else(priority_error)?;
    let exponent: i32 = exponent.parse().map_err(|_| priority_error())?;
    let negative = mantissa.starts_with('-');
    let mantissa = mantissa.strip_prefix('-').unwrap_or(mantissa);
    let mut digits = mantissa.replace('.', "");
    while digits.len() > 1 && digits.ends_with('0') {
        digits.pop();
    }
    let mut result = String::new();
    if negative {
        result.push('-');
    }
    if !(-4..14).contains(&exponent) {
        result.push_str(&digits[..1]);
        result.push('.');
        result.push_str(if digits.len() == 1 { "0" } else { &digits[1..] });
        result.push('E');
        if exponent >= 0 {
            result.push('+');
        }
        result.push_str(&exponent.to_string());
    } else if exponent < 0 {
        result.push_str("0.");
        let zeros = usize::try_from(-exponent - 1).map_err(|_| priority_error())?;
        result.extend(std::iter::repeat_n('0', zeros));
        result.push_str(&digits);
    } else {
        let point = usize::try_from(exponent + 1).map_err(|_| priority_error())?;
        if point >= digits.len() {
            result.push_str(&digits);
            result.extend(std::iter::repeat_n('0', point - digits.len()));
        } else {
            result.push_str(&digits[..point]);
            result.push('.');
            result.push_str(&digits[point..]);
        }
    }
    Ok(result)
}

fn greater<C>(values: &[usize], left: usize, right: usize, compare: &C) -> bool
where
    C: Fn(usize, usize) -> Ordering,
{
    compare(values[left], values[right]) == Ordering::Greater
}

fn sort_three<C>(values: &mut [usize], first: usize, second: usize, third: usize, compare: &C)
where
    C: Fn(usize, usize) -> Ordering,
{
    if !greater(values, first, second, compare) {
        if !greater(values, second, third, compare) {
            return;
        }
        values.swap(second, third);
        if greater(values, first, second, compare) {
            values.swap(first, second);
        }
    } else if !greater(values, third, second, compare) {
        values.swap(first, third);
    } else {
        values.swap(first, second);
        if greater(values, second, third, compare) {
            values.swap(second, third);
        }
    }
}

fn sort_four<C>(values: &mut [usize], positions: [usize; 4], compare: &C)
where
    C: Fn(usize, usize) -> Ordering,
{
    let [first, second, third, fourth] = positions;
    sort_three(values, first, second, third, compare);
    if greater(values, third, fourth, compare) {
        values.swap(third, fourth);
        if greater(values, second, third, compare) {
            values.swap(second, third);
            if greater(values, first, second, compare) {
                values.swap(first, second);
            }
        }
    }
}

fn sort_five<C>(values: &mut [usize], positions: [usize; 5], compare: &C)
where
    C: Fn(usize, usize) -> Ordering,
{
    let [first, second, third, fourth, fifth] = positions;
    sort_four(values, [first, second, third, fourth], compare);
    if greater(values, fourth, fifth, compare) {
        values.swap(fourth, fifth);
        if greater(values, third, fourth, compare) {
            values.swap(third, fourth);
            if greater(values, second, third, compare) {
                values.swap(second, third);
                if greater(values, first, second, compare) {
                    values.swap(first, second);
                }
            }
        }
    }
}

fn insert_sort<C>(values: &mut [usize], base: usize, length: usize, compare: &C)
where
    C: Fn(usize, usize) -> Ordering,
{
    match length {
        0 | 1 => {}
        2 => {
            if greater(values, base, base + 1, compare) {
                values.swap(base, base + 1);
            }
        }
        3 => sort_three(values, base, base + 1, base + 2, compare),
        4 => sort_four(values, [base, base + 1, base + 2, base + 3], compare),
        5 => sort_five(
            values,
            [base, base + 1, base + 2, base + 3, base + 4],
            compare,
        ),
        _ => {
            for candidate in base + 1..base + 6 {
                let mut target = candidate - 1;
                if !greater(values, target, candidate, compare) {
                    continue;
                }
                while target != base {
                    target -= 1;
                    if !greater(values, target, candidate, compare) {
                        target += 1;
                        break;
                    }
                }
                for position in (target + 1..=candidate).rev() {
                    values.swap(position, position - 1);
                }
            }
            for candidate in base + 6..base + length {
                let mut target = candidate - 1;
                if !greater(values, target, candidate, compare) {
                    continue;
                }
                loop {
                    target -= 2;
                    if !greater(values, target, candidate, compare) {
                        target += 1;
                        if !greater(values, target, candidate, compare) {
                            target += 1;
                        }
                        break;
                    }
                    if target == base {
                        break;
                    }
                    if target == base + 1 {
                        target -= 1;
                        if greater(values, candidate, target, compare) {
                            target += 1;
                        }
                        break;
                    }
                }
                for position in (target + 1..=candidate).rev() {
                    values.swap(position, position - 1);
                }
            }
        }
    }
}

fn zend_sort<C>(values: &mut [usize], mut base: usize, mut length: usize, compare: &C)
where
    C: Fn(usize, usize) -> Ordering,
{
    loop {
        if length <= 16 {
            insert_sort(values, base, length, compare);
            return;
        }
        let end = base + length;
        let middle = base + length / 2;
        if length >= 1024 {
            let delta = (length / 2) / 2;
            sort_five(
                values,
                [base, base + delta, middle, middle + delta, end - 1],
                compare,
            );
        } else {
            sort_three(values, base, middle, end - 1, compare);
        }
        values.swap(base + 1, middle);
        let pivot = base + 1;
        let mut left = pivot + 1;
        let mut right = end - 1;
        'partition: loop {
            while greater(values, pivot, left, compare) {
                left += 1;
                if left == right {
                    break 'partition;
                }
            }
            right -= 1;
            if right == left {
                break;
            }
            while greater(values, right, pivot, compare) {
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
        let left_length = left - base - 1;
        let right_length = end - left;
        if left_length < right_length {
            zend_sort(values, base, left_length, compare);
            base = left;
            length = right_length;
        } else {
            zend_sort(values, left, right_length, compare);
            length = left_length;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn compare(left: &Value, right: &Value) -> Result<Ordering> {
        Ok(php_compare(
            &PhpValue::from_json(left)?,
            &PhpValue::from_json(right)?,
        ))
    }

    fn sorted_ids(priorities: Vec<Value>, descending: bool) -> Result<Vec<usize>> {
        let mut entries: Vec<_> = priorities.into_iter().enumerate().collect();
        sort_by_priority(&mut entries, |entry| &entry.1, descending)?;
        Ok(entries.into_iter().map(|entry| entry.0).collect())
    }

    #[test]
    fn regular_comparison_preserves_php_mixed_type_rules() -> Result<()> {
        for (left, right, expected) in [
            (json!(false), json!([]), Ordering::Equal),
            (json!(false), json!({}), Ordering::Equal),
            (json!(true), json!("0.0"), Ordering::Equal),
            (json!(null), json!(""), Ordering::Equal),
            (json!(null), json!("0"), Ordering::Less),
            (json!([]), json!(0), Ordering::Greater),
            (json!(0), json!("a"), Ordering::Less),
            (json!(2), json!("10"), Ordering::Less),
            (json!("2"), json!("10"), Ordering::Less),
            (json!("10"), json!("1a"), Ordering::Less),
            (json!("1a"), json!("2"), Ordering::Less),
            (json!(["x"]), json!({"0": "x"}), Ordering::Equal),
            (json!([]), json!({}), Ordering::Equal),
            (json!([100]), json!([0, 0]), Ordering::Less),
        ] {
            assert_eq!(compare(&left, &right)?, expected, "{left} / {right}");
        }
        Ok(())
    }

    #[test]
    fn associated_arrays_keep_keys_and_input_order_without_forcing_antisymmetry() -> Result<()> {
        let left = json!({"a": 1});
        let right = json!({"b": 1});
        assert_eq!(compare(&left, &right)?, Ordering::Greater);
        assert_eq!(compare(&right, &left)?, Ordering::Greater);
        let left: Value = serde_json::from_str(r#"{"a":2,"b":1}"#).map_err(Error::internal)?;
        let right: Value = serde_json::from_str(r#"{"b":2,"a":1}"#).map_err(Error::internal)?;
        assert_eq!(compare(&left, &right)?, Ordering::Greater);
        assert_eq!(compare(&right, &left)?, Ordering::Greater);
        for key in ["01", "-0", "+0", " 0", "9223372036854775808"] {
            let mut map = serde_json::Map::new();
            map.insert(key.into(), json!("x"));
            let other = Value::Object(map);
            assert_eq!(compare(&json!(["x"]), &other)?, Ordering::Greater);
            assert_eq!(compare(&other, &json!(["x"]))?, Ordering::Greater);
        }
        Ok(())
    }

    #[test]
    fn numeric_strings_keep_full_grammar_and_zend_overflow_information() -> Result<()> {
        for string in [" +.5e1 \t", "5.", "0005", "5e0", "\u{b}5\u{c}"] {
            assert_eq!(compare(&json!(string), &json!(5))?, Ordering::Equal);
        }
        for string in ["5tail", "5e", "0x5", "NaN", "INF", "\u{a0}5", "5\0"] {
            assert!(numeric_string(string).is_none());
        }
        assert_eq!(
            compare(&json!("9223372036854775808"), &json!("9223372036854775809"))?,
            Ordering::Less
        );
        assert_eq!(
            compare(&json!(i64::MAX), &json!("9223372036854775808"))?,
            Ordering::Equal
        );
        assert_eq!(
            compare(&json!(u64::MAX), &json!("18446744073709551616"))?,
            Ordering::Equal
        );
        assert_eq!(compare(&json!("1e999"), &json!("2e999"))?, Ordering::Less);
        assert_eq!(
            compare(&json!("123456789012345678901e-100"), &json!("1"))?,
            Ordering::Greater
        );
        assert_eq!(
            compare(&json!("123456789012345678901e-100"), &json!(1))?,
            Ordering::Less
        );
        Ok(())
    }

    #[test]
    fn double_strings_follow_precision_fourteen_half_even_and_gcvt_layout() -> Result<()> {
        for (number, text) in [
            (0.0, "0"),
            (-0.0, "-0"),
            (1.0, "1"),
            (0.0001, "0.0001"),
            (0.00001, "1.0E-5"),
            (1e13, "10000000000000"),
            (1e14, "1.0E+14"),
            (1.234_567_890_123_45, "1.2345678901235"),
            (123_456_789_012_345.0, "1.2345678901234E+14"),
            (123_456_789_012_355.0, "1.2345678901236E+14"),
            (f64::MAX, "1.7976931348623E+308"),
            (f64::from_bits(1), "4.9406564584125E-324"),
        ] {
            assert_eq!(php_double_string(number)?, text);
        }
        assert!(php_double_string(f64::INFINITY).is_err());
        assert!(php_double_string(f64::NAN).is_err());
        assert_eq!(compare(&json!(1e14), &json!("1.0E+14x"))?, Ordering::Less);
        Ok(())
    }

    #[test]
    fn labels_keep_php_casts_and_reject_array_display() -> Result<()> {
        for (value, expected) in [
            (json!(null), "0"),
            (json!(true), "1"),
            (json!(false), ""),
            (json!("005"), "005"),
            (json!(i64::MIN), "-9223372036854775808"),
            (json!(i64::MAX), "9223372036854775807"),
            (json!(9_223_372_036_854_775_808_u64), "9.2233720368548E+18"),
            (json!(u64::MAX), "1.844674407371E+19"),
            (json!(1e14), "1.0E+14"),
            (json!(-0.0), "-0"),
        ] {
            assert_eq!(priority_label(&value)?, expected);
        }
        for value in [json!([]), json!({}), json!(["private"])] {
            let result = priority_label(&value);
            assert!(
                matches!(result, Err(error) if error.code() == "module_priority_display_invalid")
            );
        }
        Ok(())
    }

    #[test]
    fn stable_sort_preserves_nontransitive_small_sort_calls_and_descending_ties() -> Result<()> {
        assert_eq!(
            sorted_ids(vec![json!(2), json!(true), json!(1)], false)?,
            [0, 1, 2]
        );
        assert_eq!(
            sorted_ids(vec![json!("2"), json!("10"), json!("1a")], false)?,
            [0, 1, 2]
        );
        let priorities = vec![json!(null), json!(0), json!("0"), json!(2), json!(-1)];
        assert_eq!(sorted_ids(priorities.clone(), false)?, [4, 0, 1, 2, 3]);
        assert_eq!(sorted_ids(priorities, true)?, [3, 0, 1, 2, 4]);
        let arrays = vec![json!({"a": 1}), json!({"b": 1})];
        assert_eq!(sorted_ids(arrays.clone(), false)?, [1, 0]);
        // 降序取反 raw 结果；交换左右参数在此会得出相反结果。
        assert_eq!(sorted_ids(arrays, true)?, [0, 1]);
        Ok(())
    }

    #[test]
    fn repository_callback_keeps_strict_types_array_order_and_raw_null() -> Result<()> {
        let left: Value = serde_json::from_str(r#"{"a":1,"b":2}"#).map_err(Error::internal)?;
        let right: Value = serde_json::from_str(r#"{"b":2,"a":1}"#).map_err(Error::internal)?;
        for (left, right) in [
            (json!(2), json!("2")),
            (json!(2), json!(2.0)),
            (json!(null), json!(false)),
            (json!(null), json!("")),
            (json!([1]), json!(["1"])),
            (left, right),
        ] {
            let left = PhpValue::from_json(&left)?;
            let right = PhpValue::from_json(&right)?;
            assert!(!php_identical(&left, &right));
            assert_eq!(php_compare(&left, &right), Ordering::Equal);
            for descending in [false, true] {
                assert_eq!(
                    priority_compare(&left, &right, descending, PrioritySort::Repository),
                    Ordering::Less
                );
                assert_eq!(
                    priority_compare(&right, &left, descending, PrioritySort::Repository),
                    Ordering::Less
                );
            }
        }
        assert!(php_identical(
            &PhpValue::from_json(&json!(["x"]))?,
            &PhpValue::from_json(&json!({"0": "x"}))?,
        ));
        assert!(php_identical(
            &PhpValue::from_json(&json!(-0.0))?,
            &PhpValue::from_json(&json!(0.0))?,
        ));
        let priorities = vec![json!(null), json!("")];
        assert_eq!(sorted_ids(priorities.clone(), false)?, [1, 0]);
        let mut entries: Vec<_> = priorities.into_iter().enumerate().collect();
        sort_repository_priority(&mut entries, |entry| &entry.1, false)?;
        assert_eq!(
            entries.iter().map(|entry| entry.0).collect::<Vec<_>>(),
            [0, 1]
        );
        Ok(())
    }

    #[test]
    fn all_sort_size_boundaries_preserve_equal_ranks_and_move_nonclone_values() -> Result<()> {
        struct Entry {
            id: usize,
            priority: Value,
        }
        for length in [0, 1, 2, 3, 4, 5, 6, 7, 15, 16, 17, 1023, 1024, 1025] {
            for descending in [false, true] {
                let mut entries: Vec<_> = (0..length)
                    .map(|id| Entry {
                        id,
                        priority: json!((length - id) % 11),
                    })
                    .collect();
                sort_by_priority(&mut entries, |entry| &entry.priority, descending)?;
                for pair in entries.windows(2) {
                    let left = pair[0].priority.as_u64();
                    let right = pair[1].priority.as_u64();
                    assert!(if descending {
                        left >= right
                    } else {
                        left <= right
                    });
                    if left == right {
                        assert!(pair[0].id < pair[1].id);
                    }
                }
                let mut ids: Vec<_> = entries.into_iter().map(|entry| entry.id).collect();
                ids.sort_unstable();
                assert_eq!(ids, (0..length).collect::<Vec<_>>());
            }
        }
        Ok(())
    }

    #[test]
    fn repository_greater_uses_reversed_smaller_for_incomparable_array_keys() -> Result<()> {
        // Docker PHP 8.5.0 oracle case1487：两方向 zend_compare 均为 Greater，
        // 但两个方向的关系运算 > 都为 false，仓储回调因此都返回负值。
        for priorities in [
            vec![json!([0]), json!({"a": 1})],
            vec![json!({"a": 1}), json!([0])],
        ] {
            assert_eq!(sorted_ids(priorities.clone(), false)?, [1, 0]);
            for descending in [false, true] {
                let mut entries: Vec<_> = priorities.iter().cloned().enumerate().collect();
                sort_repository_priority(&mut entries, |entry| &entry.1, descending)?;
                assert_eq!(
                    entries.iter().map(|entry| entry.0).collect::<Vec<_>>(),
                    [0, 1]
                );
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod php_oracle_tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Deserialize)]
    struct Oracle {
        php_version: String,
        precision: String,
        integer_bits: u32,
        cases: Vec<Case>,
    }
    #[derive(Deserialize)]
    struct Case {
        values: Vec<Value>,
        manifest: Vec<usize>,
        repository_asc: Vec<usize>,
        repository_desc: Vec<usize>,
        labels: Vec<Value>,
    }

    // 非全序比较的结果取决于 Zend 实际比较调用顺序；此 oracle 能发现普通排序替换及舍入偏差。
    #[test]
    fn fixed_php85_oracle_matches_manifest_repository_and_display()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        let oracle: Oracle = serde_json::from_str(include_str!("fixtures/priority-php85.json"))?;
        assert_eq!(oracle.php_version, "8.5.0");
        assert_eq!(oracle.precision, "14");
        assert_eq!(oracle.integer_bits, 64);
        for (case_index, case) in oracle.cases.iter().enumerate() {
            for (mode, descending, expected) in [
                ("manifest", false, &case.manifest),
                ("repository_asc", false, &case.repository_asc),
                ("repository_desc", true, &case.repository_desc),
            ] {
                let mut rows: Vec<_> = case.values.iter().cloned().enumerate().collect();
                if mode == "manifest" {
                    sort_by_priority(&mut rows, |row| &row.1, descending)?;
                } else {
                    sort_repository_priority(&mut rows, |row| &row.1, descending)?;
                }
                let actual: Vec<_> = rows.into_iter().map(|row| row.0).collect();
                assert_eq!(&actual, expected, "PHP oracle case {case_index} / {mode}");
            }
            for (value, expected) in case.values.iter().zip(&case.labels) {
                let actual = match priority_label(value) {
                    Ok(label) => serde_json::json!({"value":label}),
                    Err(error) => serde_json::json!({"error":error.code()}),
                };
                assert_eq!(
                    &actual, expected,
                    "PHP label oracle case {case_index} / {value}"
                );
            }
        }
        Ok(())
    }
}
