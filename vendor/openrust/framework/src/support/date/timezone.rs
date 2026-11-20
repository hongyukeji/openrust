#[path = "../../../resources/calendar/tzif.rs"]
mod tzif;

const DATA: &[u8] = include_bytes!("../../../resources/calendar/tzif.data");

pub(super) fn get(name: &str) -> Option<(&'static str, &'static [u8])> {
    let index = tzif::ZONES
        .binary_search_by(|(candidate, _)| {
            candidate
                .bytes()
                .map(|byte| byte.to_ascii_lowercase())
                .cmp(name.bytes().map(|byte| byte.to_ascii_lowercase()))
        })
        .ok()?;
    let (name, range) = &tzif::ZONES[index];
    Some((name, &DATA[range.clone()]))
}

#[cfg(test)]
#[path = "../../../tests/unit/timezone_database.rs"]
mod tests;
