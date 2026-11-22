use crate::database::eloquent::factories::faker_rng::FakerRandom;

// Data from FakerPHP 1.24.1, src/Faker/Provider/pl_PL/LicensePlate.php.
// The upstream MIT notice follows.
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

use std::sync::OnceLock;

use crate::database::{DatabaseError, DatabaseResult};

use super::{Faker, RegexPattern, UniqueFaker};

struct PlateRegion {
    name: &'static str,
    prefixes: &'static [&'static str],
    counties: &'static [(&'static str, &'static [&'static str])],
}

const STANDARD_REGION_COUNT: usize = 16;
const REGIONS: &[PlateRegion] = &[
    PlateRegion {
        name: "dolnośląskie",
        prefixes: &["D", "V"],
        counties: &[
            ("Jelenia Góra", &["J"]),
            ("Legnica", &["L"]),
            ("Wałbrzych", &["B"]),
            ("Wrocław", &["W", "X"]),
            ("bolesławiecki", &["BL"]),
            ("dzierżoniowski", &["DZ"]),
            ("głogowski", &["GL"]),
            ("górowski", &["GR"]),
            ("jaworski", &["JA"]),
            ("jeleniogórski", &["JE"]),
            ("kamiennogórski", &["KA"]),
            ("kłodzki", &["KL"]),
            ("legnicki", &["LE"]),
            ("lubański", &["LB"]),
            ("lubiński", &["LU"]),
            ("lwówecki", &["LW"]),
            ("milicki", &["MI"]),
            ("oleśnicki", &["OL"]),
            ("oławski", &["OA"]),
            ("polkowicki", &["PL"]),
            ("strzeliński", &["ST"]),
            ("średzki", &["SR"]),
            ("świdnicki", &["SW"]),
            ("trzebnicki", &["TR"]),
            ("wałbrzyski", &["BA"]),
            ("wołowski", &["WL"]),
            ("wrocławski", &["WR"]),
            ("ząbkowicki", &["ZA"]),
            ("zgorzelecki", &["ZG"]),
            ("złotoryjski", &["ZL"]),
        ],
    },
    PlateRegion {
        name: "kujawsko-pomorskie",
        prefixes: &["C"],
        counties: &[
            ("Bydgoszcz", &["B"]),
            ("Grudziądz", &["G"]),
            ("Toruń", &["T"]),
            ("Włocławek", &["W"]),
            ("aleksandrowski", &["AL"]),
            ("brodnicki", &["BR"]),
            ("bydgoski", &["BY"]),
            ("chełmiński", &["CH"]),
            ("golubsko-dobrzyński", &["GD"]),
            ("grudziądzki", &["GR"]),
            ("inowrocławski", &["IN"]),
            ("lipnowski", &["LI"]),
            ("mogileński", &["MG"]),
            ("nakielski", &["NA"]),
            ("radziejowski", &["RA"]),
            ("rypiński", &["RY"]),
            ("sępoleński", &["SE"]),
            ("świecki", &["SW"]),
            ("toruński", &["TR"]),
            ("tucholski", &["TU"]),
            ("wąbrzeski", &["WA"]),
            ("włocławski", &["WL"]),
            ("żniński", &["ZN"]),
        ],
    },
    PlateRegion {
        name: "lubelskie",
        prefixes: &["L"],
        counties: &[
            ("Biała Podlaska", &["B"]),
            ("Chełm", &["C"]),
            ("Lublin", &["U"]),
            ("Zamość", &["Z"]),
            ("bialski", &["BI"]),
            ("biłgorajski", &["BL"]),
            ("chełmski", &["CH"]),
            ("hrubieszowski", &["HR"]),
            ("janowski", &["JA"]),
            ("krasnostawski", &["KS"]),
            ("kraśnicki", &["KR"]),
            ("lubartowski", &["LB"]),
            ("lubelski", &["UB"]),
            ("łęczyński", &["LE"]),
            ("łukowski", &["LU"]),
            ("opolski", &["OP"]),
            ("parczewski", &["PA"]),
            ("puławski", &["PU"]),
            ("radzyński", &["RA"]),
            ("rycki", &["RY"]),
            ("świdnicki", &["SW"]),
            ("tomaszowski", &["TM"]),
            ("włodawski", &["WL"]),
            ("zamojski", &["ZA"]),
        ],
    },
    PlateRegion {
        name: "lubuskie",
        prefixes: &["F"],
        counties: &[
            ("Gorzów Wielkopolski", &["G"]),
            ("Zielona Góra", &["Z"]),
            ("gorzowski", &["GW"]),
            ("krośnieński", &["KR"]),
            ("międzyrzecki", &["MI"]),
            ("nowosolski", &["NW"]),
            ("słubicki", &["SL"]),
            ("strzelecko-drezdenecki", &["SD"]),
            ("sulęciński", &["SU"]),
            ("świebodziński", &["SW"]),
            ("wschowski", &["WS"]),
            ("zielonogórski", &["ZI"]),
            ("żagański", &["ZG"]),
            ("żarski", &["ZA"]),
        ],
    },
    PlateRegion {
        name: "łódzkie",
        prefixes: &["E"],
        counties: &[
            ("Łódź", &["L", "D"]),
            ("Piotrków Trybunalski", &["P"]),
            ("Skierniewice", &["S"]),
            ("brzeziński", &["BR"]),
            ("bełchatowski", &["BE"]),
            ("kutnowski", &["KU"]),
            ("łaski", &["LA"]),
            ("łęczycki", &["LE"]),
            ("łowicki", &["LC"]),
            ("łódzki wschodni", &["LW"]),
            ("opoczyński", &["OP"]),
            ("pabianicki", &["PA"]),
            ("pajęczański", &["PJ"]),
            ("piotrkowski", &["PI"]),
            ("poddębicki", &["PD"]),
            ("radomszczański", &["RA"]),
            ("rawski", &["RW"]),
            ("sieradzki", &["SI"]),
            ("skierniewicki", &["SK"]),
            ("tomaszowski", &["TM"]),
            ("wieluński", &["WI"]),
            ("wieruszowski", &["WE"]),
            ("zduńskowolski", &["ZD"]),
            ("zgierski", &["ZG"]),
        ],
    },
    PlateRegion {
        name: "małopolskie",
        prefixes: &["K", "J"],
        counties: &[
            ("Kraków", &["R", "K"]),
            ("Nowy Sącz", &["N"]),
            ("Tarnów", &["T"]),
            ("bocheński", &["BA", "BC"]),
            ("brzeski", &["BR"]),
            ("chrzanowski", &["CH"]),
            ("dąbrowski", &["DA"]),
            ("gorlicki", &["GR"]),
            ("krakowski", &["RA", "RK"]),
            ("limanowski", &["LI"]),
            ("miechowski", &["MI"]),
            ("myślenicki", &["MY"]),
            ("nowosądecki", &["NS"]),
            ("nowotarski", &["NT"]),
            ("olkuski", &["OL"]),
            ("oświęcimski", &["OS"]),
            ("proszowicki", &["PR"]),
            ("suski", &["SU"]),
            ("tarnowski", &["TA"]),
            ("tatrzański", &["TT"]),
            ("wadowicki", &["WA"]),
            ("wielicki", &["WI"]),
        ],
    },
    PlateRegion {
        name: "mazowieckie",
        prefixes: &["W", "A"],
        counties: &[
            ("Ostrołęka", &["O"]),
            ("Płock", &["P"]),
            ("Radom", &["R"]),
            ("Siedlce", &["S"]),
            ("białobrzeski", &["BR"]),
            ("ciechanowski", &["CI"]),
            ("garwoliński", &["G"]),
            ("gostyniński", &["GS"]),
            ("grodziski", &["GM"]),
            ("grójecki", &["GR"]),
            ("kozienicki", &["KZ"]),
            ("legionowski", &["L"]),
            ("lipski", &["LI"]),
            ("łosicki", &["LS"]),
            ("makowski", &["MA"]),
            ("miński", &["M"]),
            ("mławski", &["ML"]),
            ("nowodworski", &["ND"]),
            ("ostrołęcki", &["OS"]),
            ("ostrowski", &["OR"]),
            ("otwocki", &["OT"]),
            ("piaseczyński", &["PA", "PI", "PW", "PX"]),
            ("płocki", &["PL"]),
            ("płoński", &["PN"]),
            ("pruszkowski", &["PP", "PR", "PS"]),
            ("przasnyski", &["PZ"]),
            ("przysuski", &["PY"]),
            ("pułtuski", &["PU"]),
            ("radomski", &["RA"]),
            ("siedlecki", &["SI"]),
            ("sierpecki", &["SE"]),
            ("sochaczewski", &["SC"]),
            ("sokołowski", &["SK"]),
            ("szydłowiecki", &["SZ"]),
            (
                "warszawski",
                &[
                    "A", "B", "D", "E", "F", "H", "I", "J", "K", "N", "T", "U", "W", "X", "Y",
                ],
            ),
            ("warszawski zachodni", &["Z"]),
            ("węgrowski", &["WE"]),
            ("wołomiński", &["WL", "V"]),
            ("wyszkowski", &["WY"]),
            ("zwoleński", &["ZW"]),
            ("żuromiński", &["ZU"]),
            ("żyrardowski", &["ZY"]),
        ],
    },
    PlateRegion {
        name: "opolskie",
        prefixes: &["O"],
        counties: &[
            ("Opole", &["P"]),
            ("brzeski", &["B"]),
            ("głubczycki", &["GL"]),
            ("kędzierzyńsko-kozielski", &["K"]),
            ("kluczborski", &["KL"]),
            ("krapkowicki", &["KR"]),
            ("namysłowski", &["NA"]),
            ("nyski", &["NY"]),
            ("oleski", &["OL"]),
            ("opolski", &["PO"]),
            ("prudnicki", &["PR"]),
            ("strzelecki", &["ST"]),
        ],
    },
    PlateRegion {
        name: "podkarpackie",
        prefixes: &["R", "Y"],
        counties: &[
            ("Krosno", &["K"]),
            ("Przemyśl", &["P"]),
            ("Rzeszów", &["Z"]),
            ("Tarnobrzeg", &["T"]),
            ("bieszczadzki", &["BI"]),
            ("brzozowski", &["BR"]),
            ("dębicki", &["DE"]),
            ("jarosławski", &["JA"]),
            ("jasielski", &["JS"]),
            ("kolbuszowski", &["KL"]),
            ("krośnieński", &["KR"]),
            ("leski", &["LS"]),
            ("leżajski", &["LE"]),
            ("lubaczowski", &["LU"]),
            ("łańcucki", &["LA"]),
            ("mielecki", &["MI"]),
            ("niżański", &["NI"]),
            ("przemyski", &["PR"]),
            ("przeworski", &["PZ"]),
            ("ropczycko-sędziszowski", &["RS"]),
            ("rzeszowski", &["ZE", "ZR", "ZZ"]),
            ("sanocki", &["SA"]),
            ("stalowowolski", &["ST"]),
            ("strzyżowski", &["SR"]),
            ("tarnobrzeski", &["TA"]),
        ],
    },
    PlateRegion {
        name: "podlaskie",
        prefixes: &["B"],
        counties: &[
            ("Białystok", &["I"]),
            ("Łomża", &["L"]),
            ("Suwałki", &["S"]),
            ("augustowski", &["AU"]),
            ("białostocki", &["IA", "IB"]),
            ("bielski", &["BI"]),
            ("grajewski", &["GR"]),
            ("hajnowski", &["HA"]),
            ("kolneński", &["KL"]),
            ("łomżyński", &["LM"]),
            ("moniecki", &["MN"]),
            ("sejneński", &["SE"]),
            ("siemiatycki", &["SI"]),
            ("sokólski", &["SK"]),
            ("suwalski", &["SU"]),
            ("wysokomazowiecki", &["WM"]),
            ("zambrowski", &["ZA"]),
        ],
    },
    PlateRegion {
        name: "pomorskie",
        prefixes: &["G", "X"],
        counties: &[
            ("Gdańsk", &["D"]),
            ("Gdynia", &["A"]),
            ("Słupsk", &["S"]),
            ("Sopot", &["SP"]),
            ("bytowski", &["BY"]),
            ("chojnicki", &["CH"]),
            ("człuchowski", &["CZ"]),
            ("gdański", &["DA"]),
            ("kartuski", &["KA", "KY", "KZ"]),
            ("kościerski", &["KS"]),
            ("kwidzyński", &["KW"]),
            ("lęborski", &["LE"]),
            ("malborski", &["MB"]),
            ("nowodworski", &["ND"]),
            ("pucki", &["PU"]),
            ("słupski", &["SL"]),
            ("starogardzki", &["ST"]),
            ("sztumski", &["SZ"]),
            ("tczewski", &["TC"]),
            ("wejherowski", &["WE", "WO"]),
        ],
    },
    PlateRegion {
        name: "śląskie",
        prefixes: &["S", "I"],
        counties: &[
            ("Bielsko-Biała", &["B"]),
            ("Bytom", &["Y"]),
            ("Chorzów", &["H"]),
            ("Częstochowa", &["C"]),
            ("Dąbrowa Górnicza", &["D"]),
            ("Gliwice", &["G"]),
            ("Jastrzębie-Zdrój", &["JZ"]),
            ("Jaworzno", &["J"]),
            ("Katowice", &["K"]),
            ("Mysłowice", &["M"]),
            ("Piekary Śląskie", &["PI"]),
            ("Ruda Śląska,", &["L", "RS"]),
            ("Rybnik", &["R"]),
            ("Siemianowice Śląskie", &["I"]),
            ("Sosnowiec", &["O"]),
            ("Świętochłowice", &["W"]),
            ("Tychy", &["T"]),
            ("Zabrze", &["Z"]),
            ("Żory", &["ZO"]),
            ("będziński", &["BE", "BN", "E"]),
            ("bielski", &["BI"]),
            ("cieszyński", &["CI", "CN"]),
            ("częstochowski", &["CZ"]),
            ("gliwicki", &["GL"]),
            ("kłobucki", &["KL"]),
            ("lubliniecki", &["LU"]),
            ("mikołowski", &["MI"]),
            ("myszkowski", &["MY"]),
            ("pszczyński", &["PS"]),
            ("raciborski", &["RC"]),
            ("rybnicki", &["RB"]),
            ("tarnogórski", &["TA"]),
            ("bieruńsko - lędziński", &["BL"]),
            ("wodzisławski", &["WD", "WZ"]),
            ("zawierciański", &["ZA"]),
            ("żywiecki", &["ZY"]),
        ],
    },
    PlateRegion {
        name: "świętokrzyskie",
        prefixes: &["T"],
        counties: &[
            ("Kielce", &["K"]),
            ("buski", &["BU"]),
            ("jędrzejowski", &["JE"]),
            ("kazimierski", &["KA"]),
            ("kielecki", &["KI"]),
            ("konecki", &["KN"]),
            ("opatowski", &["OP"]),
            ("ostrowiecki", &["OS"]),
            ("pińczowski", &["PI"]),
            ("sandomierski", &["SA"]),
            ("skarżyski", &["SK"]),
            ("starachowicki", &["ST"]),
            ("staszowski", &["SZ"]),
            ("włoszczowski", &["LW"]),
        ],
    },
    PlateRegion {
        name: "warmińsko-mazurskie",
        prefixes: &["N"],
        counties: &[
            ("Elbląg", &["E"]),
            ("Olsztyn", &["O"]),
            ("bartoszycki", &["BA"]),
            ("braniewski", &["BR"]),
            ("działdowski", &["DZ"]),
            ("elbląski", &["EB"]),
            ("ełcki", &["EL"]),
            ("giżycki", &["GI"]),
            ("iławski", &["IL"]),
            ("kętrzyński", &["KE"]),
            ("lidzbarski", &["LI"]),
            ("mrągowski", &["MR"]),
            ("nidzicki", &["NI"]),
            ("nowomiejski", &["NM"]),
            ("olecki", &["OE"]),
            ("gołdapski", &["GO"]),
            ("olsztyński", &["OL"]),
            ("ostródzki", &["OS"]),
            ("piski", &["PI"]),
            ("szczycieński", &["SZ"]),
            ("węgorzewski", &["WE"]),
        ],
    },
    PlateRegion {
        name: "wielkopolskie",
        prefixes: &["P", "M"],
        counties: &[
            ("Kalisz", &["A", "K"]),
            ("Konin", &["KO", "N"]),
            ("Leszno", &["L"]),
            ("Poznań", &["O", "Y"]),
            ("chodzieski", &["CH"]),
            ("czarnkowsko-trzcianecki", &["CT"]),
            ("gnieźnieński", &["GN"]),
            ("gostyński", &["GS"]),
            ("grodziski", &["GO"]),
            ("jarociński", &["JA"]),
            ("kaliski", &["KA"]),
            ("kępiński", &["KE"]),
            ("kolski", &["KL"]),
            ("koniński", &["KN"]),
            ("kościański", &["KS"]),
            ("krotoszyński", &["KR"]),
            ("leszczyński", &["LE"]),
            ("międzychodzki", &["MI"]),
            ("nowotomyski", &["NT"]),
            ("obornicki", &["OB"]),
            ("ostrowski", &["OS"]),
            ("ostrzeszowski", &["OT"]),
            ("pilski", &["P"]),
            ("pleszewski", &["PL"]),
            ("poznański", &["OZ", "Z"]),
            ("rawicki", &["RA"]),
            ("słupecki", &["SL"]),
            ("szamotulski", &["SZ"]),
            ("średzki", &["SR"]),
            ("śremski", &["SE"]),
            ("turecki", &["TU"]),
            ("wągrowiecki", &["WA"]),
            ("wolsztyński", &["WL"]),
            ("wrzesiński", &["WR"]),
            ("złotowski", &["ZL"]),
        ],
    },
    PlateRegion {
        name: "zachodniopomorskie",
        prefixes: &["Z"],
        counties: &[
            ("Koszalin", &["K"]),
            ("Szczecin", &["S", "Z"]),
            ("Świnoujście", &["SW"]),
            ("białogardzki", &["BI"]),
            ("choszczeński", &["CH"]),
            ("drawski", &["DR"]),
            ("goleniowski", &["GL"]),
            ("gryficki", &["GY"]),
            ("gryfiński", &["GR"]),
            ("kamieński", &["KA"]),
            ("kołobrzeski", &["KL"]),
            ("koszaliński", &["KO"]),
            ("łobeski", &["LO"]),
            ("myśliborski", &["MY"]),
            ("policki", &["PL"]),
            ("pyrzycki", &["PY"]),
            ("sławieński", &["SL"]),
            ("stargardzki", &["ST"]),
            ("szczecinecki", &["SZ"]),
            ("świdwiński", &["SD"]),
            ("wałecki", &["WA"]),
        ],
    },
    PlateRegion {
        name: "army",
        prefixes: &["U"],
        counties: &[(
            "Siły Zbrojne Rzeczypospolitej Polskiej",
            &["A", "B", "C", "D", "E", "G", "I", "J", "K", "L"],
        )],
    },
    PlateRegion {
        name: "services",
        prefixes: &["H"],
        counties: &[
            ("Centralne Biuro Antykorupcyjne", &["A"]),
            ("Służba Ochrony Państwa", &["BA", "BB", "BE", "BF", "BG"]),
            (
                "Służba Celno-Skarbowa",
                &[
                    "CA", "CB", "CC", "CD", "CE", "CF", "CG", "CH", "CJ", "CK", "CL", "CM", "CN",
                    "CO", "CP", "CR",
                ],
            ),
            ("Agencja Bezpieczeństwa Wewnętrznego", &["K"]),
            ("Agencja Wywiadu", &["K"]),
            ("Służba Kontrwywiadu Wojskowego", &["M"]),
            ("Służba Wywiadu Wojskowego", &["M"]),
            (
                "Policja",
                &[
                    "PA", "PB", "PC", "PD", "PE", "PF", "PG", "PH", "PJ", "PK", "PL", "PL", "PL",
                    "PL", "PL", "PM", "PN", "PP", "PS", "PT", "PU", "PW", "PZ",
                ],
            ),
            ("Straż Graniczna", &["WA", "WK"]),
        ],
    },
];

const SUFFIX_GROUP_1: &[&str] = &[
    "\\d{5}",
    "\\d{4}[A-PR-Z]",
    "\\d{3}[A-PR-Z]{2}",
    "[1-9][A-PR-Z]\\d{3}",
    "[1-9][A-PR-Z]{2}\\d{2}",
];
const SUFFIX_GROUP_2: &[&str] = &[
    "[A-PR-Z]\\d{3}",
    "\\d{2}[A-PR-Z]{2}",
    "[1-9][A-PR-Z]\\d{2}",
    "\\d{2}[A-PR-Z][1-9]",
    "[1-9][A-PR-Z]{2}[1-9]",
    "[A-PR-Z]{2}\\d{2}",
    "\\d{5}",
    "\\d{4}[A-PR-Z]",
    "\\d{3}[A-PR-Z]{2}",
    "[A-PR-Z]\\d{2}[A-PR-Z]",
    "[A-PR-Z][1-9][A-PR-Z]{2}",
];

static SUFFIX_PATTERNS_1: OnceLock<Vec<RegexPattern>> = OnceLock::new();
static SUFFIX_PATTERNS_2: OnceLock<Vec<RegexPattern>> = OnceLock::new();

#[allow(non_snake_case, reason = "Matches Faker formatter names")]
impl Faker {
    pub fn licensePlate(&self) -> DatabaseResult<String> {
        self.licensePlateWithOptions(false, None, None)
    }

    pub fn licensePlateWithOptions(
        &self,
        special: bool,
        voivodeships: Option<&[&str]>,
        counties: Option<&[&str]>,
    ) -> DatabaseResult<String> {
        if self.provider_locale() != "pl_PL" {
            return Err(DatabaseError::InvalidQuery(
                "unsupported Faker license plate locale",
            ));
        }

        let regions = if special {
            REGIONS
        } else {
            &REGIONS[..STANDARD_REGION_COUNT]
        };
        let region = sample_area(&self.random, regions, voivodeships, |region| region.name);
        let region_code = self.random.sample(region.prefixes);
        let (_, county_codes) =
            sample_area(&self.random, region.counties, counties, |county| county.0);
        let county_code = self.random.sample(county_codes);
        let (cache, suffixes) = if county_code.len() == 1 {
            (&SUFFIX_PATTERNS_1, SUFFIX_GROUP_1)
        } else {
            (&SUFFIX_PATTERNS_2, SUFFIX_GROUP_2)
        };
        let patterns = cache.get_or_init(|| {
            suffixes
                .iter()
                .map(|suffix| RegexPattern::parse(suffix).expect("fixed Polish plate suffix"))
                .collect()
        });
        let suffix = patterns[self.random.usize(..patterns.len())].sample(&self.random);
        Ok(format!("{region_code}{county_code} {suffix}"))
    }
}

#[allow(non_snake_case, reason = "Matches Faker formatter names")]
impl UniqueFaker<'_> {
    pub fn licensePlate(&self) -> DatabaseResult<String> {
        self.licensePlateWithOptions(false, None, None)
    }

    pub fn licensePlateWithOptions(
        &self,
        special: bool,
        voivodeships: Option<&[&str]>,
        counties: Option<&[&str]>,
    ) -> DatabaseResult<String> {
        self.try_generate("licensePlate", || {
            self.0
                .licensePlateWithOptions(special, voivodeships, counties)
        })
    }
}

fn sample_area<'a, T>(
    random: &FakerRandom,
    areas: &'a [T],
    requested: Option<&[&str]>,
    name: impl Fn(&T) -> &'static str,
) -> &'a T {
    if let Some(requested) = requested.filter(|requested| !requested.is_empty()) {
        let mut matches = areas.iter().filter(|area| requested.contains(&name(area)));
        let count = matches.clone().count();
        if count > 0 {
            return matches
                .nth(random.usize(..count))
                .expect("selected a matching Polish plate area");
        }
    }
    &areas[random.usize(..areas.len())]
}
