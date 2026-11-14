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

use super::{DatabaseError, DatabaseResult, Faker, UniqueFaker, faker, locales};
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

const CHINESE_PREFIXES: &[&str] = &[
    "超艺",
    "和泰",
    "九方",
    "鑫博腾飞",
    "戴硕电子",
    "济南亿次元",
    "海创",
    "创联世纪",
    "凌云",
    "泰麒麟",
    "彩虹",
    "兰金电子",
    "晖来计算机",
    "天益",
    "恒聪百汇",
    "菊风公司",
    "惠派国际公司",
    "创汇",
    "思优",
    "时空盒数字",
    "易动力",
    "飞海科技",
    "华泰通安",
    "盟新",
    "商软冠联",
    "图龙信息",
    "易动力",
    "华远软件",
    "创亿",
    "时刻",
    "开发区世创",
    "明腾",
    "良诺",
    "天开",
    "毕博诚",
    "快讯",
    "凌颖信息",
    "黄石金承",
    "恩悌",
    "雨林木风计算机",
    "双敏电子",
    "维旺明",
    "网新恒天",
    "数字100",
    "飞利信",
    "立信电子",
    "联通时科",
    "中建创业",
    "新格林耐特",
    "新宇龙信息",
    "浙大万朋",
    "MBP软件",
    "昂歌信息",
    "万迅电脑",
    "方正科技",
    "联软",
    "七喜",
    "南康",
    "银嘉",
    "巨奥",
    "佳禾",
    "国讯",
    "信诚致远",
    "浦华众城",
    "迪摩",
    "太极",
    "群英",
    "合联电子",
    "同兴万点",
    "襄樊地球村",
    "精芯",
    "艾提科信",
    "昊嘉",
    "鸿睿思博",
    "四通",
    "富罳",
    "商软冠联",
    "诺依曼软件",
    "东方峻景",
    "华成育卓",
    "趋势",
    "维涛",
    "通际名联",
];

const TAIWANESE_PREFIXES: &[&str] = &[
    "乾坤",
    "亞神",
    "光譜",
    "典選",
    "凱基",
    "前衛花園",
    "創見",
    "友訊",
    "台達",
    "合勤",
    "喜瑪拉雅",
    "喬山",
    "多利安",
    "大國",
    "大宇",
    "太陽",
    "太陽動力",
    "宏碁",
    "宏達",
    "小白兔",
    "巨大",
    "巨室",
    "康師傅",
    "延伸",
    "弘煜",
    "彎的",
    "擎天",
    "新亞洲",
    "旺旺",
    "昱泉",
    "智冠",
    "未來",
    "松崗",
    "正新",
    "洧誠",
    "海蝶",
    "添翼創越",
    "滾石",
    "王品",
    "當然",
    "相信",
    "睛水",
    "研華",
    "福茂",
    "種子",
    "米樂士",
    "紅元素",
    "紅心辣椒",
    "統一",
    "美利達",
    "美妙",
    "美樂帝",
    "群石",
    "老鷹",
    "聯強",
    "芮河",
    "英特衛",
    "茂為",
    "華特",
    "華研",
    "華碩",
    "華義",
    "角頭",
    "訊連",
    "豐華",
    "豪客",
    "豪記",
    "貴族",
    "趨勢",
    "遊戲橘子",
    "野火樂集",
    "金革",
    "銀魚",
    "阿爾發",
    "風和日麗",
    "風潮",
    "風雲",
];

const TAIWANESE_MODIFIERS: &[&str] = &[
    "科技", "電腦", "國際", "電子", "娛樂", "音樂", "唱片", "證券", "集團", "企業", "機械", "工業",
    "文化", "汽車", "百貨", "酒店", "數位",
];

const TAIWANESE_SUFFIXES: &[&str] = &["公司", "有限公司", "股份有限公司"];

const JAPANESE_PREFIXES: &[&str] = &["株式会社", "有限会社"];

const KOREAN_PREFIXES: &[&str] = &["(주)", "(주)", "(주)", "(유)"];

const KOREAN_SUFFIXES: &[&str] = &[
    "전자",
    "건설",
    "식품",
    "인터넷",
    "그룹",
    "은행",
    "보험",
    "제약",
    "금융",
    "네트웍스",
    "기획",
    "미디어",
    "연구소",
    "모바일",
    "스튜디오",
    "캐피탈",
];

const KOREAN_MALE_GIVEN_NAMES: &[&str] = &[
    "건우", "건호", "경석", "경수", "경춘", "경환", "광수", "광현", "구범", "규산", "기수", "남수",
    "남호", "대선", "대수", "도윤", "도현", "동윤", "동하", "동현", "명식", "명호", "문용", "문창",
    "민석", "민성", "민수", "민재", "민준", "민철", "민환", "병철", "병호", "상선", "상수", "상우",
    "상욱", "상준", "상철", "상현", "상호", "상훈", "서준", "서호", "선엽", "성곤", "성령", "성민",
    "성수", "성진", "성현", "성호", "성훈", "수원", "승민", "승현", "승호", "시우", "영길", "영수",
    "영식", "영일", "영진", "영철", "영하", "영호", "영환", "예준", "용태", "용환", "용훈", "우진",
    "원준", "원진", "원희", "은성", "은택", "인규", "재윤", "재철", "재혁", "재현", "재호", "재훈",
    "정남", "정수", "정식", "정웅", "정호", "정훈", "종수", "종주", "종훈", "주원", "주형", "준",
    "준범", "준서", "준영", "준혁", "준형", "준호", "중수", "지후", "지훈", "진수", "진우", "진호",
    "창용", "채현", "태현", "태호", "혁상", "현규", "현우", "현종", "현준", "형민", "형철", "호민",
    "호진", "홍선", "효일",
];

const KOREAN_FEMALE_GIVEN_NAMES: &[&str] = &[
    "가람", "강은", "강희", "경은", "경주", "근영", "기연", "나루", "나리", "나연", "나은", "나형",
    "누리", "다영", "도연", "동현", "미경", "미라", "미란", "미영", "미정", "민서", "민아", "민지",
    "민형", "민희", "반희", "보람", "보미", "보민", "봄", "상명", "새미", "서연", "서영", "서윤",
    "서현", "선영", "선우", "선정", "선호", "성미", "성민", "성은", "세원", "소민", "소연", "소영",
    "소정", "수란", "수민", "수빈", "수연", "수원", "수정", "수진", "순항", "슬기", "시은", "신애",
    "아름", "아린", "여진", "연선", "연희", "영진", "영화", "예원", "예은", "예지", "예진", "유리",
    "유정", "유진", "윤경", "윤미", "윤서", "윤영", "은경", "은미", "은상", "은서", "은애", "은영",
    "은정", "은주", "은지", "은진", "은형", "은혜", "은희", "인화", "재연", "정란", "정민", "정은",
    "정화", "주명", "주미", "주연", "주희", "지민", "지선", "지숙", "지아", "지연", "지영", "지예",
    "지우", "지원", "지은", "지현", "지혜", "지희", "진아", "진희", "채원", "태희", "하나", "하윤",
    "하은", "한나", "헤선", "현영", "현정", "현주", "현지", "혜나", "혜림", "혜민", "혜숙", "혜연",
    "혜진", "효진", "희경", "희원",
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
            "zh_CN" => format!("{}{}", sample(CHINESE_PREFIXES), suffix()),
            "zh_TW" => {
                let prefix = sample(TAIWANESE_PREFIXES);
                match fastrand::u8(0..3) {
                    0 => format!("{prefix}{}", sample(TAIWANESE_MODIFIERS)),
                    1 => format!("{prefix}{}", suffix()),
                    _ => format!("{prefix}{}{}", sample(TAIWANESE_MODIFIERS), suffix()),
                }
            }
            "ja_JP" => format!("{} {}", sample(JAPANESE_PREFIXES), surname()),
            "ko_KR" => {
                let name = sample(if fastrand::bool() {
                    KOREAN_MALE_GIVEN_NAMES
                } else {
                    KOREAN_FEMALE_GIVEN_NAMES
                });
                match fastrand::u8(0..6) {
                    0 => format!("{}{name}", sample(KOREAN_PREFIXES)),
                    1 => format!("{}{name}{}", sample(KOREAN_PREFIXES), suffix()),
                    _ => format!("{name}{}", suffix()),
                }
            }
            locale => generate!(locale, faker::company::raw::CompanyName,),
        }
    }

    /// Samples a company prefix for a locale with a prefix catalog.
    pub fn companyPrefix(&self) -> DatabaseResult<&'static str> {
        let prefixes = match self.locale() {
            "zh_CN" => CHINESE_PREFIXES,
            "zh_TW" => TAIWANESE_PREFIXES,
            "ja_JP" => JAPANESE_PREFIXES,
            "ko_KR" => KOREAN_PREFIXES,
            "pl_PL" => POLISH_PREFIXES,
            _ => {
                return Err(DatabaseError::InvalidQuery(
                    "unsupported Faker company prefix locale",
                ));
            }
        };
        Ok(sample(prefixes))
    }

    /// Samples the modifier in a Taiwanese company name.
    pub fn companyModifier(&self) -> DatabaseResult<&'static str> {
        if self.locale() != "zh_TW" {
            return Err(DatabaseError::InvalidQuery(
                "unsupported Faker company modifier locale",
            ));
        }
        Ok(sample(TAIWANESE_MODIFIERS))
    }

    /// Samples a Taiwanese English company-name suffix.
    pub fn companyEnSuffix(&self) -> DatabaseResult<&'static str> {
        if self.locale() != "zh_TW" {
            return Err(DatabaseError::InvalidQuery(
                "unsupported Faker English company suffix locale",
            ));
        }
        Ok(sample(EN_US_SUFFIXES))
    }

    /// Samples the selected company provider's suffix.
    pub fn companySuffix(&self) -> String {
        let suffixes = match self.locale() {
            "en_US" => EN_US_SUFFIXES,
            "fr_FR" => FR_FR_SUFFIXES,
            "it_IT" => IT_IT_SUFFIXES,
            "pl_PL" => PL_PL_SUFFIXES,
            "zh_CN" => &[
                "科技有限公司",
                "网络有限公司",
                "信息有限公司",
                "传媒有限公司",
            ],
            "zh_TW" => TAIWANESE_SUFFIXES,
            "ja_JP" => &["Ltd"],
            "ko_KR" => KOREAN_SUFFIXES,
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

    pub fn companyPrefix(&self) -> DatabaseResult<&'static str> {
        self.try_generate("companyPrefix", || self.0.companyPrefix())
    }

    pub fn companyModifier(&self) -> DatabaseResult<&'static str> {
        self.try_generate("companyModifier", || self.0.companyModifier())
    }

    pub fn companyEnSuffix(&self) -> DatabaseResult<&'static str> {
        self.try_generate("companyEnSuffix", || self.0.companyEnSuffix())
    }

    pub fn companySuffix(&self) -> DatabaseResult<String> {
        self.generate("companySuffix", || self.0.companySuffix())
    }
}
