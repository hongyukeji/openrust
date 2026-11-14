// Company formats and Polish surnames from FakerPHP v1.24.1.
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

use super::{DatabaseResult, Faker, UniqueFaker, faker, locales};
use ::fake::Fake as _;

const EN_US_SUFFIXES: &[&str] = &["Inc", "and Sons", "LLC", "Group", "PLC", "Ltd"];

const FR_FR_SUFFIXES: &[&str] = &["SA", "S.A.", "SARL", "S.A.R.L.", "SAS", "S.A.S.", "et Fils"];

const IT_IT_SUFFIXES: &[&str] = &["SPA", "e figli", "Group", "s.r.l."];

const PL_PL_SUFFIXES: &[&str] = &[
    "S.A.",
    "i syn",
    "sp. z o.o.",
    "sp. j.",
    "sp. p.",
    "sp. k.",
    "S.K.A",
    "s. c.",
    "P.P.O.F",
];

const POLISH_PREFIXES: &[&str] = &["Grupa", "Fundacja", "Stowarzyszenie", "Spółdzielnia"];

const POLISH_MALE_SURNAMES: &[&str] = &[
    "Adamczyk",
    "Adamski",
    "Andrzejewski",
    "Baran",
    "Baranowski",
    "Bąk",
    "Błaszczyk",
    "Borkowski",
    "Borowski",
    "Brzeziński",
    "Chmielewski",
    "Cieślak",
    "Czarnecki",
    "Czerwiński",
    "Dąbrowski",
    "Duda",
    "Dudek",
    "Gajewski",
    "Głowacki",
    "Górski",
    "Grabowski",
    "Jabłoński",
    "Jakubowski",
    "Jankowski",
    "Jasiński",
    "Jaworski",
    "Kaczmarczyk",
    "Kaczmarek",
    "Kalinowski",
    "Kamiński",
    "Kaźmierczak",
    "Kołodziej",
    "Konieczny",
    "Kowalczyk",
    "Kowalski",
    "Kozłowski",
    "Krajewski",
    "Krawczyk",
    "Król",
    "Krupa",
    "Kubiak",
    "Kucharski",
    "Kwiatkowski",
    "Laskowski",
    "Lewandowski",
    "Lis",
    "Maciejewski",
    "Majewski",
    "Makowski",
    "Malinowski",
    "Marciniak",
    "Mazur",
    "Mazurek",
    "Michalak",
    "Michalski",
    "Mróz",
    "Nowak",
    "Nowakowski",
    "Nowicki",
    "Olszewski",
    "Ostrowski",
    "Pawlak",
    "Pawłowski",
    "Pietrzak",
    "Piotrowski",
    "Przybylski",
    "Rutkowski",
    "Sadowski",
    "Sawicki",
    "Sikora",
    "Sikorski",
    "Sobczak",
    "Sokołowski",
    "Stępień",
    "Szczepański",
    "Szewczyk",
    "Szulc",
    "Szymański",
    "Szymczak",
    "Tomaszewski",
    "Urbański",
    "Walczak",
    "Wasilewski",
    "Wieczorek",
    "Wilk",
    "Wiśniewski",
    "Witkowski",
    "Włodarczyk",
    "Wojciechowski",
    "Woźniak",
    "Wójcik",
    "Wróbel",
    "Wróblewski",
    "Wysocki",
    "Zając",
    "Zakrzewski",
    "Zalewski",
    "Zawadzki",
    "Zieliński",
    "Ziółkowski",
];

const POLISH_FEMALE_SURNAMES: &[&str] = &[
    "Adamczyk",
    "Adamska",
    "Andrzejewska",
    "Baran",
    "Baranowska",
    "Bąk",
    "Błaszczyk",
    "Borkowska",
    "Borowska",
    "Brzezińska",
    "Chmielewska",
    "Cieślak",
    "Czarnecka",
    "Czerwińska",
    "Dąbrowska",
    "Duda",
    "Dudek",
    "Gajewska",
    "Głowacka",
    "Górecka",
    "Górska",
    "Grabowska",
    "Jabłońska",
    "Jakubowska",
    "Jankowska",
    "Jasińska",
    "Jaworska",
    "Kaczmarczyk",
    "Kaczmarek",
    "Kalinowska",
    "Kamińska",
    "Kaźmierczak",
    "Kołodziej",
    "Kowalczyk",
    "Kowalska",
    "Kozłowska",
    "Krajewska",
    "Krawczyk",
    "Król",
    "Krupa",
    "Kubiak",
    "Kucharska",
    "Kwiatkowska",
    "Laskowska",
    "Lewandowska",
    "Lis",
    "Maciejewska",
    "Majewska",
    "Makowska",
    "Malinowska",
    "Marciniak",
    "Mazur",
    "Mazurek",
    "Michalak",
    "Michalska",
    "Mróz",
    "Nowak",
    "Nowakowska",
    "Nowicka",
    "Olszewska",
    "Ostrowska",
    "Pawlak",
    "Pawłowska",
    "Pietrzak",
    "Piotrowska",
    "Przybylska",
    "Rutkowska",
    "Sadowska",
    "Sawicka",
    "Sikora",
    "Sikorska",
    "Sobczak",
    "Sokołowska",
    "Stępień",
    "Szczepańska",
    "Szewczyk",
    "Szulc",
    "Szymańska",
    "Szymczak",
    "Tomaszewska",
    "Urbańska",
    "Walczak",
    "Wasilewska",
    "Wieczorek",
    "Wilk",
    "Wiśniewska",
    "Witkowska",
    "Włodarczyk",
    "Wojciechowska",
    "Woźniak",
    "Wójcik",
    "Wróbel",
    "Wróblewska",
    "Wysocka",
    "Zając",
    "Zakrzewska",
    "Zalewska",
    "Zawadzka",
    "Zielińska",
    "Ziółkowska",
];

fn sample(values: &'static [&'static str]) -> &'static str {
    values[fastrand::usize(..values.len())]
}

fn polish_surname() -> &'static str {
    sample(if fastrand::bool() {
        POLISH_MALE_SURNAMES
    } else {
        POLISH_FEMALE_SURNAMES
    })
}

#[allow(non_snake_case, reason = "Matches Faker formatter names")]
impl Faker {
    /// Samples a company name using the selected locale.
    pub fn company(&self) -> String {
        let surname = || self.lastName();
        let suffix = || self.companySuffix();
        match self.locale() {
            "en_US" => match fastrand::u8(0..3) {
                0 => format!("{} {}", surname(), suffix()),
                1 => format!("{}-{}", surname(), surname()),
                _ => format!("{}, {} and {}", surname(), surname(), surname()),
            },
            "fr_FR" => match fastrand::u8(0..4) {
                0 => format!("{} {}", surname(), suffix()),
                1 => format!("{} {} {}", surname(), surname(), suffix()),
                _ => surname(),
            },
            "it_IT" => match fastrand::u8(0..3) {
                0 => format!("{} {}", surname(), suffix()),
                1 => format!("{}-{} {}", surname(), surname(), suffix()),
                _ => format!("{}, {} e {} {}", surname(), surname(), surname(), suffix()),
            },
            "pl_PL" => match fastrand::u8(0..8) {
                0..=1 => polish_surname().to_owned(),
                2..=5 => format!("{} {}", polish_surname(), suffix()),
                6 => format!("{} {}", sample(POLISH_PREFIXES), polish_surname()),
                _ => format!("{}-{}", polish_surname(), polish_surname()),
            },
            locale => generate!(locale, faker::company::raw::CompanyName,),
        }
    }

    /// Samples the selected company provider's suffix.
    pub fn companySuffix(&self) -> String {
        let suffixes = match self.locale() {
            "en_US" => EN_US_SUFFIXES,
            "fr_FR" => FR_FR_SUFFIXES,
            "it_IT" => IT_IT_SUFFIXES,
            "pl_PL" => PL_PL_SUFFIXES,
            locale => return generate!(locale, faker::company::raw::CompanySuffix,),
        };
        sample(suffixes).to_owned()
    }
}

#[allow(non_snake_case, reason = "Matches Faker formatter names")]
impl UniqueFaker<'_> {
    pub fn company(&self) -> DatabaseResult<String> {
        self.generate("company", || self.0.company())
    }

    pub fn companySuffix(&self) -> DatabaseResult<String> {
        self.generate("companySuffix", || self.0.companySuffix())
    }
}
