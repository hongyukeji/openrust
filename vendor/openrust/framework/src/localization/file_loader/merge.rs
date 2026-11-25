use crate::localization::{Line, Lines};

pub(super) fn merge(lines: &mut Lines, overlay: Lines) {
    for (key, incoming) in overlay {
        if let Some(current) = lines.get_mut(&key) {
            merge_line(current, incoming);
        } else {
            lines.insert(key, incoming);
        }
    }
}

fn merge_line(current: &mut Line, incoming: Line) {
    match (current, incoming) {
        (Line::Group(current), Line::Group(incoming)) => merge(current, incoming),
        (Line::List(current), Line::List(incoming)) => {
            for (index, value) in incoming.into_iter().enumerate() {
                if let Some(previous) = current.get_mut(index) {
                    merge_line(previous, value);
                } else {
                    current.push(value);
                }
            }
        }
        (Line::Group(current), Line::List(incoming)) => {
            merge(current, indexed(incoming));
        }
        (current @ Line::List(_), Line::Group(incoming)) => {
            let Line::List(items) = std::mem::replace(current, Line::Group(Lines::new())) else {
                unreachable!()
            };
            let mut entries = indexed(items);
            merge(&mut entries, incoming);
            *current = Line::Group(entries);
        }
        (current, incoming) => *current = incoming,
    }
}

pub(in crate::localization) fn indexed(items: Vec<Line>) -> Lines {
    items
        .into_iter()
        .enumerate()
        .map(|(i, value)| (i.to_string(), value))
        .collect()
}
