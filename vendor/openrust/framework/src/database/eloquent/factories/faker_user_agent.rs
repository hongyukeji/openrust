use crate::database::eloquent::factories::faker_rng::FakerRandom;

use super::{DatabaseResult, Faker, UniqueFaker};

const WINDOWS: [&str; 12] = [
    "Windows NT 6.2",
    "Windows NT 6.1",
    "Windows NT 6.0",
    "Windows NT 5.2",
    "Windows NT 5.1",
    "Windows NT 5.01",
    "Windows NT 5.0",
    "Windows NT 4.0",
    "Windows 98; Win 9x 4.90",
    "Windows 98",
    "Windows 95",
    "Windows CE",
];
const MAC: [&str; 4] = ["Intel", "PPC", "U; Intel", "U; PPC"];
const LINUX: [&str; 2] = ["i686", "x86_64"];
const LANGUAGES: [&str; 3] = ["en-US", "sl-SI", "nl-NL"];

macro_rules! formatters {
    ($($name:ident [$history:literal] -> $result:ty = $sample:path;)*) => {
        impl Faker {
            $(pub fn $name(&self) -> $result {
                $sample(&self.random)
            })*
        }

        impl UniqueFaker<'_> {
            $(pub fn $name(&self) -> DatabaseResult<$result> {
                self.generate($history, || self.0.$name())
            })*
        }
    };
}

formatters! {
    windows_platform_token ["windowsPlatformToken"] -> &'static str = windows;
    mac_processor ["macProcessor"] -> &'static str = mac_processor;
    linux_processor ["linuxProcessor"] -> &'static str = linux_processor;
    mac_platform_token ["macPlatformToken"] -> String = mac;
    linux_platform_token ["linuxPlatformToken"] -> String = linux;
    ios_mobile_token ["iosMobileToken"] -> String = ios;
    chrome ["chrome"] -> String = chrome;
    msedge ["msedge"] -> String = edge;
    safari ["safari"] -> String = safari;
    opera ["opera"] -> String = opera;
    internet_explorer ["internetExplorer"] -> String = internet_explorer;
}

impl Faker {
    pub fn user_agent(&self) -> DatabaseResult<String> {
        Ok(match self.random.u16(0..6) {
            0 => self.firefox()?,
            1 => self.chrome(),
            2 => self.internet_explorer(),
            3 => self.opera(),
            4 => self.safari(),
            _ => self.msedge(),
        })
    }

    pub fn firefox(&self) -> DatabaseResult<String> {
        let date = self
            .date_time_between_expressions("2010-01-01", "now")?
            .to_zoned(self.application_timezone.clone());
        let platform = match self.random.u16(0..3) {
            0 => format!(
                "{}; {}; rv:1.9.{}.20",
                windows(&self.random),
                language(&self.random),
                self.random.u16(0..=2)
            ),
            1 => format!("{}; rv:{}.0", linux(&self.random), self.random.u16(5..=7)),
            _ => format!("{} rv:{}.0", mac(&self.random), self.random.u16(2..=6)),
        };
        Ok(format!(
            "Mozilla/5.0 ({platform}) Gecko/{} Firefox/{}.0",
            date.strftime("%Y%m%d"),
            self.random.u16(35..=37)
        ))
    }
}

impl UniqueFaker<'_> {
    pub fn user_agent(&self) -> DatabaseResult<String> {
        self.try_generate("userAgent", || self.0.user_agent())
    }

    pub fn firefox(&self) -> DatabaseResult<String> {
        self.try_generate("firefox", || self.0.firefox())
    }
}

fn windows(random: &FakerRandom) -> &'static str {
    WINDOWS[random.usize(..WINDOWS.len())]
}

fn mac_processor(random: &FakerRandom) -> &'static str {
    MAC[random.usize(..MAC.len())]
}

fn linux_processor(random: &FakerRandom) -> &'static str {
    LINUX[random.usize(..LINUX.len())]
}

fn language(random: &FakerRandom) -> &'static str {
    LANGUAGES[random.usize(..LANGUAGES.len())]
}

fn mac(random: &FakerRandom) -> String {
    format!(
        "Macintosh; {} Mac OS X 10_{}_{}",
        mac_processor(random),
        random.u16(5..=8),
        random.u16(0..=9)
    )
}

fn linux(random: &FakerRandom) -> String {
    format!("X11; Linux {}", linux_processor(random))
}

fn ios(random: &FakerRandom) -> String {
    format!(
        "iPhone; CPU iPhone OS {}_{} like Mac OS X",
        random.u16(13..=15),
        random.u16(0..=2)
    )
}

fn chrome(random: &FakerRandom) -> String {
    let platform = match random.u16(0..3) {
        0 => linux(random),
        1 => windows(random).to_owned(),
        _ => mac(random),
    };
    let webkit = format!("{}{}", random.u16(531..=536), random.u16(0..=2));
    format!(
        "Mozilla/5.0 ({platform}) AppleWebKit/{webkit} (KHTML, like Gecko) Chrome/{}.0.{}.0 Mobile Safari/{webkit}",
        random.u16(36..=40),
        random.u16(800..=899)
    )
}

fn edge(random: &FakerRandom) -> String {
    let webkit = format!("{}.{}", random.u16(531..=537), random.u16(0..=2));
    let major = random.u16(79..=99);
    // FakerPHP concatenates the build directly after the minor zero.
    let version = format!(
        "{major}.0{}.{}",
        random.u16(1000..=1146),
        random.u16(0..=99)
    );
    let (platform, token) = match random.u16(0..4) {
        0 => (windows(random).to_owned(), "Edg"),
        1 => (mac(random), "Edg"),
        2 => (linux(random), "EdgA"),
        _ => {
            return format!(
                "Mozilla/5.0 ({}) AppleWebKit/{webkit} (KHTML, like Gecko) Version/15.0 EdgiOS/{version} Mobile/15E148 Safari/{webkit}",
                ios(random)
            );
        }
    };
    format!(
        "Mozilla/5.0 ({platform}) AppleWebKit/{webkit} (KHTML, like Gecko) Chrome/{major}.0.{}.{} Safari/{webkit} {token}/{version}",
        random.u16(4000..=4844),
        random.u16(10..=99)
    )
}

fn safari(random: &FakerRandom) -> String {
    let webkit = format!(
        "{}.{}.{}",
        random.u16(531..=535),
        random.u16(1..=50),
        random.u16(1..=7)
    );
    let platform = match random.u16(0..3) {
        0 => format!("Windows; U; {}", windows(random)),
        1 => format!(
            "{} rv:{}.0; {}",
            mac(random),
            random.u16(2..=6),
            language(random)
        ),
        _ => {
            let device = if random.bool() {
                "iPhone; CPU iPhone OS"
            } else {
                "iPad; CPU OS"
            };
            return format!(
                "Mozilla/5.0 ({device} {}_{}_{} like Mac OS X; {}) AppleWebKit/{webkit} (KHTML, like Gecko) Version/{}.0.5 Mobile/8B{} Safari/6{webkit}",
                random.u16(7..=8),
                random.u16(0..=2),
                random.u16(1..=2),
                language(random),
                random.u16(3..=4),
                random.u16(111..=119)
            );
        }
    };
    let major = random.u16(4..=5);
    let version = if random.bool() {
        format!("{major}.{}", random.u16(0..=1))
    } else {
        format!("{major}.0.{}", random.u16(1..=5))
    };
    format!(
        "Mozilla/5.0 ({platform}) AppleWebKit/{webkit} (KHTML, like Gecko) Version/{version} Safari/{webkit}"
    )
}

fn opera(random: &FakerRandom) -> String {
    let platform = if random.bool() {
        linux(random)
    } else {
        windows(random).to_owned()
    };
    format!(
        "Opera/{}.{} ({platform}; {}) Presto/2.{}.{} Version/{}.00",
        random.u16(8..=9),
        random.u16(10..=99),
        language(random),
        random.u16(8..=12),
        random.u16(160..=355),
        random.u16(10..=12)
    )
}

fn internet_explorer(random: &FakerRandom) -> String {
    format!(
        "Mozilla/5.0 (compatible; MSIE {}.0; {}; Trident/{}.{})",
        random.u16(5..=11),
        windows(random),
        random.u16(3..=5),
        random.u16(0..=1)
    )
}
