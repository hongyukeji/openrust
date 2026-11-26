// PHP 的常规比较可能不满足传递性，固定 PHP 8.5.12 的比较顺序会影响输出。
// 对照 Zend/zend_sort.c，使用索引切片和旋转表达小区间插入及大区间分割。
// 上游和衍生算法的许可见 regular_string_sort.LICENSE。

pub(super) fn sort(mut indices: &mut [usize], greater: impl Fn(usize, usize) -> bool + Copy) {
    while indices.len() > 16 {
        let last = indices.len() - 1;
        let middle = indices.len() / 2;
        if indices.len() >= 1024 {
            let quarter = middle / 2;
            sort_sample(
                indices,
                &[0, quarter, middle, middle + quarter, last],
                greater,
            );
        } else {
            sort_sample(indices, &[0, middle, last], greater);
        }

        indices.swap(1, middle);
        let mut lower_len = 2;
        let mut upper_start = last;
        'partition: loop {
            while greater(indices[1], indices[lower_len]) {
                lower_len += 1;
                if lower_len == upper_start {
                    break 'partition;
                }
            }
            upper_start -= 1;
            if upper_start == lower_len {
                break;
            }
            while greater(indices[upper_start], indices[1]) {
                upper_start -= 1;
                if upper_start == lower_len {
                    break 'partition;
                }
            }
            indices.swap(lower_len, upper_start);
            lower_len += 1;
            if lower_len == upper_start {
                break;
            }
        }

        indices.swap(1, lower_len - 1);
        let (lower_with_pivot, upper) = indices.split_at_mut(lower_len);
        let lower = &mut lower_with_pivot[..lower_len - 1];
        if lower.len() < upper.len() {
            sort(lower, greater);
            indices = upper;
        } else {
            sort(upper, greater);
            indices = lower;
        }
    }

    const SAMPLE: [usize; 5] = [0, 1, 2, 3, 4];
    if indices.len() <= SAMPLE.len() {
        sort_sample(indices, &SAMPLE[..indices.len()], greater);
        return;
    }
    for end in 1..6 {
        let mut position = end;
        while position > 0 && greater(indices[position - 1], indices[end]) {
            position -= 1;
        }
        indices[position..=end].rotate_right(1);
    }
    for end in 6..indices.len() {
        if !greater(indices[end - 1], indices[end]) {
            continue;
        }
        let value = indices[end];
        let mut position = end - 3;
        loop {
            if !greater(indices[position], value) {
                position += 1;
                if !greater(indices[position], value) {
                    position += 1;
                }
                break;
            }
            if position <= 1 {
                position = usize::from(position == 1 && greater(value, indices[0]));
                break;
            }
            position -= 2;
        }
        indices[position..=end].rotate_right(1);
    }
}

fn sort_sample(indices: &mut [usize], sample: &[usize], greater: impl Fn(usize, usize) -> bool) {
    match sample {
        [] | [_] => return,
        &[left, right] => {
            if greater(indices[left], indices[right]) {
                indices.swap(left, right);
            }
            return;
        }
        &[left, middle, right, ..] => {
            if greater(indices[left], indices[middle]) {
                if greater(indices[right], indices[middle]) {
                    indices.swap(left, middle);
                    if greater(indices[middle], indices[right]) {
                        indices.swap(middle, right);
                    }
                } else {
                    indices.swap(left, right);
                }
            } else if greater(indices[middle], indices[right]) {
                indices.swap(middle, right);
                if greater(indices[left], indices[middle]) {
                    indices.swap(left, middle);
                }
            }
        }
    }
    for end in 3..sample.len() {
        for pair in sample[..=end].windows(2).rev() {
            if !greater(indices[pair[0]], indices[pair[1]]) {
                break;
            }
            indices.swap(pair[0], pair[1]);
        }
    }
}
