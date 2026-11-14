// Russian, Ukrainian and Kazakh company data from FakerPHP v1.24.1.
// https://github.com/FakerPHP/Faker/tree/v1.24.1/src/Faker/Provider
//
// Copyright (c) 2011 François Zaninotto
// Portions Copyright (c) 2008 Caius Durling
// Portions Copyright (c) 2008 Adam Royle
// Portions Copyright (c) 2008 Fiona Burrows
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
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
// THE SOFTWARE.

use super::sample;

const RU_RU_PREFIXES: &[&str] = &[
    "ООО",
    "ЗАО",
    "ООО Компания",
    "ОАО",
    "ОАО",
    "ПАО",
    "МКК",
    "МФО",
];

const RU_RU_SUFFIXES: &[&str] = &["Ltd"];

const RU_RU_ELEMENTS: &[&str] = &[
    "ЖелДор",
    "Гараж",
    "Цемент",
    "Асбоцемент",
    "Строй",
    "Лифт",
    "Креп",
    "Авто",
    "Теле",
    "Транс",
    "Алмаз",
    "Метиз",
    "Мотор",
    "Рос",
    "Тяж",
    "Тех",
    "Сантех",
    "Урал",
    "Башкир",
    "Тверь",
    "Казань",
    "Обл",
    "Бух",
    "Хоз",
    "Электро",
    "Текстиль",
    "Восток",
    "Орион",
    "Юпитер",
    "Финанс",
    "Микро",
    "Радио",
    "Мобайл",
    "Дизайн",
    "Метал",
    "Нефть",
    "Телеком",
    "Инфо",
    "Сервис",
    "Софт",
    "IT",
    "Рыб",
    "Глав",
    "Вектор",
    "Рем",
    "Гор",
    "Газ",
    "Монтаж",
    "Мор",
    "Реч",
    "Флот",
    "Cиб",
    "Каз",
    "Инж",
    "Вод",
    "Пив",
    "Хмель",
    "Мяс",
    "Томск",
    "Омск",
    "Север",
    "Лен",
];

const RU_RU_NAME_SUFFIXES: &[&str] = &[
    "Маш",
    "Наладка",
    "Экспедиция",
    "Пром",
    "Комплекс",
    "Машина",
    "Снос",
    "-М",
    "Лизинг",
    "Траст",
    "Снаб",
    "-H",
    "Трест",
    "Банк",
    "Опт",
    "Проф",
    "Сбыт",
    "Центр",
];

const UK_UA_PREFIXES: &[&str] = &["ТОВ", "ПП", "ПАТ", "ПрАТ"];

const UK_UA_SUFFIXES: &[&str] = &["Сервіс", "Плюс", "Груп", "Стиль", "Дизайн"];

const UK_UA_NAMES: &[&str] = &[
    "Вектор",
    "Едельвейс",
    "Смарт",
    "Альфа",
    "Система",
    "Універсал",
    "Інфоком",
    "Макс",
    "Планета",
    "Вектор",
    "Приват",
    "Еко",
    "Мега",
    "Мегамакс",
    "Мульти",
    "Майнер",
];

const KK_KZ_PREFIXES: &[&str] = &["АҚ", "ЖШС", "ЖАҚ"];

const KK_KZ_SUFFIXES: &[&str] = &["Ltd"];

const KK_KZ_ELEMENTS: &[&str] = &[
    "Қазақ",
    "Кітап",
    "Цемент",
    "Лифт",
    "Креп",
    "Авто",
    "Теле",
    "Транс",
    "Алмаз",
    "Метиз",
    "Мотор",
    "Қаз",
    "Тех",
    "Сантех",
    "Алматы",
    "Астана",
    "Электро",
];

const KK_KZ_NAME_SUFFIXES: &[&str] = &[
    "Құрылыс",
    "Машина",
    "Бұзу",
    "-М",
    "Лизинг",
    "Страх",
    "Ком",
    "Телеком",
];

pub(super) fn company(locale: &str) -> Option<String> {
    let prefix = || sample(prefixes(locale).expect("company provider has prefixes"));
    Some(match locale {
        "uk_UA" => {
            let name = sample(UK_UA_NAMES);
            match fastrand::u8(0..4) {
                0 => name.to_owned(),
                1 => format!("{} \"{name}\"", prefix()),
                2 => format!("{name}-{}", sample(UK_UA_SUFFIXES)),
                _ => format!("{} \"{name}-{}\"", prefix(), sample(UK_UA_SUFFIXES)),
            }
        }
        "ru_RU" | "kk_KZ" => {
            let elements = elements(locale)?;
            let count = fastrand::u8(1..=4);
            let mut name = format!("{} ", prefix());
            for _ in 0..count.min(3) {
                name.push_str(sample(elements));
            }
            if count == 4 {
                name.push_str(sample(name_suffixes(locale)?));
            }
            name
        }
        _ => return None,
    })
}

pub(super) fn prefixes(locale: &str) -> Option<&'static [&'static str]> {
    match locale {
        "ru_RU" => Some(RU_RU_PREFIXES),
        "uk_UA" => Some(UK_UA_PREFIXES),
        "kk_KZ" => Some(KK_KZ_PREFIXES),
        _ => None,
    }
}

pub(super) fn suffixes(locale: &str) -> Option<&'static [&'static str]> {
    match locale {
        "ru_RU" => Some(RU_RU_SUFFIXES),
        "uk_UA" => Some(UK_UA_SUFFIXES),
        "kk_KZ" => Some(KK_KZ_SUFFIXES),
        _ => None,
    }
}

pub(super) fn elements(locale: &str) -> Option<&'static [&'static str]> {
    match locale {
        "ru_RU" => Some(RU_RU_ELEMENTS),
        "kk_KZ" => Some(KK_KZ_ELEMENTS),
        _ => None,
    }
}

pub(super) fn name_suffixes(locale: &str) -> Option<&'static [&'static str]> {
    match locale {
        "ru_RU" => Some(RU_RU_NAME_SUFFIXES),
        "kk_KZ" => Some(KK_KZ_NAME_SUFFIXES),
        _ => None,
    }
}

pub(super) fn ukrainian_name() -> &'static str {
    sample(UK_UA_NAMES)
}
