//! RFC 8536 POSIX 尾规则；算法来源 timelib 2022.14，许可见 LICENSE.timelib。
//! 固定上游与版权许可见同目录 SOURCES.md / LICENSE.timelib / LICENSE.carbon。

use super::civil::{Civil, days_in_month};
use super::{CalendarInstant, calendar_error, range_error};
use crate::Result;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct TailRecord {
    pub name: &'static str,
    pub from_epoch: i64,
    pub first_epoch: Option<i64>,
    pub first_offset: i32,
    pub first_is_dst: bool,
    pub first_abbreviation: &'static str,
    pub rule: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Day {
    MonthWeek { month: i64, week: i64, weekday: i64 },
    Julian(i64),
    Ordinal(i64),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Basis {
    Wall,
    Standard,
    Utc,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Change {
    day: Day,
    seconds: i64,
    basis: Basis,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Saving {
    name: &'static str,
    offset: i32,
    start: Change,
    end: Change,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Tail {
    standard: &'static str,
    offset: i32,
    saving: Option<Saving>,
}
#[derive(Clone, Copy)]
pub(super) struct State {
    pub offset: i32,
    pub abbreviation: &'static str,
    pub is_dst: bool,
}

pub(super) fn record(name: &str) -> Option<&'static TailRecord> {
    let rows = super::tail_rules::ZONES;
    rows.binary_search_by(|row| row.name.cmp(name))
        .ok()
        .and_then(|index| rows.get(index))
        .or_else(|| rows.iter().find(|row| row.name.eq_ignore_ascii_case(name)))
}

impl Tail {
    pub fn parse(text: &'static str) -> Result<Self> {
        let mut cursor = Parser { text, index: 0 };
        let standard = cursor.name()?;
        let offset = cursor.offset()?.checked_neg().ok_or_else(range_error)?;
        let saving = if cursor.rest().is_empty() {
            None
        } else {
            let name = cursor.name()?;
            let saving_offset = if cursor.peek() == Some(b',') {
                offset.checked_add(3600).ok_or_else(range_error)?
            } else {
                cursor.offset()?.checked_neg().ok_or_else(range_error)?
            };
            cursor.literal(b',')?;
            let start = cursor.change()?;
            cursor.literal(b',')?;
            let end = cursor.change()?;
            Some(Saving {
                name,
                offset: saving_offset,
                start,
                end,
            })
        };
        if !cursor.rest().is_empty() {
            return Err(invalid());
        }
        Ok(Self {
            standard,
            offset,
            saving,
        })
    }
    pub fn state(self, instant: CalendarInstant) -> Result<State> {
        let Some(saving) = self.saving else {
            return Ok(State {
                offset: self.offset,
                abbreviation: self.standard,
                is_dst: false,
            });
        };
        let year = Civil::from_instant(instant, 0)?.year;
        let mut latest = None;
        // 前后一年覆盖有符号的跨午夜/跨年规则；不将历史年份投影到未来。
        for delta in -1..=1 {
            let Some(year) = year.checked_add(delta) else {
                continue;
            };
            for (change, before, state) in [
                (
                    saving.start,
                    self.offset,
                    State {
                        offset: saving.offset,
                        abbreviation: saving.name,
                        is_dst: true,
                    },
                ),
                (
                    saving.end,
                    saving.offset,
                    State {
                        offset: self.offset,
                        abbreviation: self.standard,
                        is_dst: false,
                    },
                ),
            ] {
                if let Ok(at) = change.at(year, before, self.offset)
                    && at <= instant.epoch_seconds
                    && latest.is_none_or(|(time, _)| at > time)
                {
                    latest = Some((at, state));
                }
            }
        }
        Ok(latest.map_or(
            State {
                offset: self.offset,
                abbreviation: self.standard,
                is_dst: false,
            },
            |(_, state)| state,
        ))
    }
    pub fn resolve(self, civil: Civil, from_epoch: i64) -> Result<Option<CalendarInstant>> {
        let standard = civil.instant(self.offset)?;
        let saving = self
            .saving
            .map(|saving| civil.instant(saving.offset))
            .transpose()?;
        if standard.epoch_seconds < from_epoch
            && saving.is_none_or(|instant| instant.epoch_seconds < from_epoch)
        {
            return Ok(None);
        }
        let valid_standard =
            standard.epoch_seconds >= from_epoch && self.state(standard)?.offset == self.offset;
        let valid_saving = if let (Some(instant), Some(saving)) = (saving, self.saving) {
            instant.epoch_seconds >= from_epoch && self.state(instant)?.offset == saving.offset
        } else {
            false
        };
        let chosen = match (valid_standard, valid_saving) {
            (true, false) => standard,
            (false, true) => saving.ok_or_else(invalid)?,
            (true, true) => {
                let guess = self.state(civil.instant(0)?)?.offset;
                if guess == self.offset {
                    standard
                } else {
                    saving.ok_or_else(invalid)?
                }
            }
            (false, false) => {
                // 前进 gap 使用跳变前较小的偏移，而非拒绝不存在的本地时间。
                let before = self
                    .saving
                    .map_or(self.offset, |saving| self.offset.min(saving.offset));
                civil.instant(before)?
            }
        };
        Ok(Some(chosen))
    }
}

impl Change {
    fn at(self, year: i64, wall_offset: i32, standard_offset: i32) -> Result<i64> {
        let mut civil = Civil {
            year,
            ..Civil::epoch()
        };
        match self.day {
            Day::MonthWeek {
                month,
                week,
                weekday,
            } => {
                civil.month = month;
                civil.day = 1;
                civil.day += (weekday - civil.weekday()?).rem_euclid(7) + (week - 1) * 7;
                let days = days_in_month(year, month)?;
                if civil.day > days {
                    civil.day -= 7;
                }
            }
            Day::Julian(day) => {
                let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
                civil.day = day + i64::from(leap && day >= 60);
            }
            Day::Ordinal(day) => civil.day = day + 1,
        }
        civil.second = self.seconds;
        let offset = match self.basis {
            Basis::Wall => wall_offset,
            Basis::Standard => standard_offset,
            Basis::Utc => 0,
        };
        Ok(civil.instant(offset)?.epoch_seconds)
    }
}

struct Parser {
    text: &'static str,
    index: usize,
}
impl Parser {
    fn rest(&self) -> &'static str {
        self.text.get(self.index..).unwrap_or("")
    }
    fn peek(&self) -> Option<u8> {
        self.text.as_bytes().get(self.index).copied()
    }
    fn literal(&mut self, expected: u8) -> Result<()> {
        if self.peek() != Some(expected) {
            return Err(invalid());
        }
        self.index += 1;
        Ok(())
    }
    fn name(&mut self) -> Result<&'static str> {
        if self.peek() == Some(b'<') {
            self.index += 1;
            let start = self.index;
            while self.peek().is_some_and(|value| value != b'>') {
                self.index += 1;
            }
            let name = self.text.get(start..self.index).ok_or_else(invalid)?;
            self.literal(b'>')?;
            if name.is_empty() {
                return Err(invalid());
            }
            return Ok(name);
        }
        let start = self.index;
        while self.peek().is_some_and(|value| value.is_ascii_alphabetic()) {
            self.index += 1;
        }
        if self.index - start < 3 {
            return Err(invalid());
        }
        self.text.get(start..self.index).ok_or_else(invalid)
    }
    fn integer(&mut self) -> Result<i64> {
        let start = self.index;
        while self.peek().is_some_and(|value| value.is_ascii_digit()) {
            self.index += 1;
        }
        self.text
            .get(start..self.index)
            .and_then(|text| text.parse().ok())
            .ok_or_else(invalid)
    }
    fn seconds(&mut self) -> Result<i64> {
        let negative = self.peek() == Some(b'-');
        if matches!(self.peek(), Some(b'+' | b'-')) {
            self.index += 1;
        }
        let hours = self.integer()?;
        let minutes = if self.peek() == Some(b':') {
            self.index += 1;
            self.integer()?
        } else {
            0
        };
        let seconds = if self.peek() == Some(b':') {
            self.index += 1;
            self.integer()?
        } else {
            0
        };
        if minutes > 59 || seconds > 59 {
            return Err(invalid());
        }
        let value = hours
            .checked_mul(3600)
            .and_then(|value| value.checked_add(minutes * 60))
            .and_then(|value| value.checked_add(seconds))
            .ok_or_else(range_error)?;
        if negative {
            value.checked_neg().ok_or_else(range_error)
        } else {
            Ok(value)
        }
    }
    fn offset(&mut self) -> Result<i32> {
        i32::try_from(self.seconds()?).map_err(|_| range_error())
    }
    fn change(&mut self) -> Result<Change> {
        let day = match self.peek() {
            Some(b'M') => {
                self.index += 1;
                let month = self.integer()?;
                self.literal(b'.')?;
                let week = self.integer()?;
                self.literal(b'.')?;
                let weekday = self.integer()?;
                if !(1..=12).contains(&month)
                    || !(1..=5).contains(&week)
                    || !(0..=6).contains(&weekday)
                {
                    return Err(invalid());
                }
                Day::MonthWeek {
                    month,
                    week,
                    weekday,
                }
            }
            Some(b'J') => {
                self.index += 1;
                let day = self.integer()?;
                if !(1..=365).contains(&day) {
                    return Err(invalid());
                }
                Day::Julian(day)
            }
            _ => {
                let day = self.integer()?;
                if !(0..=365).contains(&day) {
                    return Err(invalid());
                }
                Day::Ordinal(day)
            }
        };
        let seconds = if self.peek() == Some(b'/') {
            self.index += 1;
            self.seconds()?
        } else {
            7200
        };
        let basis = match self.peek() {
            Some(b's') => {
                self.index += 1;
                Basis::Standard
            }
            Some(b'u' | b'g' | b'z') => {
                self.index += 1;
                Basis::Utc
            }
            Some(b'w') => {
                self.index += 1;
                Basis::Wall
            }
            _ => Basis::Wall,
        };
        Ok(Change {
            day,
            seconds,
            basis,
        })
    }
}
fn invalid() -> crate::Error {
    calendar_error("input_timezone_invalid")
}

#[cfg(test)]
mod tests {
    use super::*;

    // 防止某个非热门时区或有符号/半小时规则在配置启动时才暴露不支持。
    #[test]
    fn all_fixed_official_zone_footers_parse() -> Result<()> {
        assert_eq!(super::super::tail_rules::ZONES.len(), 598);
        assert_eq!(super::super::tail_rules::RULES.len(), 94);
        for row in super::super::tail_rules::ZONES {
            Tail::parse(row.rule)?;
        }
        Ok(())
    }

    // 保证无闰日 Jn、含闰日 n 与有符号跨午夜规则不会混用。
    #[test]
    fn julian_ordinal_and_signed_transition_rules_are_distinct() -> Result<()> {
        let julian = Tail::parse("STD0DST,J60/-1,J300/26")?;
        let ordinal = Tail::parse("STD0DST,59/-1,299/26")?;
        let february = Civil {
            year: 2020,
            month: 2,
            day: 29,
            hour: 12,
            ..Civil::epoch()
        }
        .instant(0)?;
        assert_eq!(julian.state(february)?.offset, 0);
        assert_eq!(ordinal.state(february)?.offset, 3600);
        Ok(())
    }
}
