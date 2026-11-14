use super::{DatabaseResult, Faker, UniqueFaker};

#[path = "faker_color_names.rs"]
mod catalogs;

#[allow(non_snake_case, reason = "Matches Faker formatter names")]
impl Faker {
    pub fn hexColor(&self) -> String {
        format!("#{:06x}", sample_rgb())
    }

    pub fn safeHexColor(&self) -> String {
        // FakerPHP expands three hex digits drawn from 0x000..=0x0ff.
        let value = fastrand::u8(..);
        let (high, low) = (value >> 4, value & 0x0f);
        format!("#00{high:x}{high:x}{low:x}{low:x}")
    }

    pub fn rgbColorAsArray(&self) -> [u8; 3] {
        let [_, red, green, blue] = sample_rgb().to_be_bytes();
        [red, green, blue]
    }

    pub fn rgbColor(&self) -> String {
        triplet(self.rgbColorAsArray())
    }

    pub fn rgbCssColor(&self) -> String {
        format!("rgb({})", self.rgbColor())
    }

    pub fn rgbaCssColor(&self) -> String {
        let rgb = self.rgbColor();
        let alpha = f64::from(fastrand::i32(0..=i32::MAX)) / f64::from(i32::MAX);
        let alpha = (alpha * 10.0).round() / 10.0;
        format!("rgba({rgb},{alpha})")
    }

    pub fn safeColorName(&self) -> &'static str {
        let (safe, _) = catalogs::names(self.locale());
        safe[fastrand::usize(..safe.len())]
    }

    pub fn colorName(&self) -> &'static str {
        let (_, all) = catalogs::names(self.locale());
        all[fastrand::usize(..all.len())]
    }

    pub fn hslColorAsArray(&self) -> [u16; 3] {
        [
            fastrand::u16(0..=360),
            fastrand::u16(0..=100),
            fastrand::u16(0..=100),
        ]
    }

    pub fn hslColor(&self) -> String {
        triplet(self.hslColorAsArray())
    }
}

macro_rules! unique_colors {
    ($($name:ident -> $result:ty),+ $(,)?) => {
        #[allow(non_snake_case, reason = "Matches Faker formatter names")]
        impl UniqueFaker<'_> {
            $(pub fn $name(&self) -> DatabaseResult<$result> {
                self.generate(stringify!($name), || self.0.$name())
            })+
        }
    };
}

unique_colors! {
    hexColor -> String,
    safeHexColor -> String,
    rgbColor -> String,
    rgbCssColor -> String,
    rgbaCssColor -> String,
    safeColorName -> &'static str,
    colorName -> &'static str,
    hslColor -> String,
}

#[allow(non_snake_case, reason = "Matches Faker formatter names")]
impl UniqueFaker<'_> {
    pub fn rgbColorAsArray(&self) -> DatabaseResult<[u8; 3]> {
        self.generate_keyed(
            "rgbColorAsArray",
            || Ok(self.0.rgbColorAsArray()),
            |&channels| Ok(triplet(channels)),
        )
    }

    pub fn hslColorAsArray(&self) -> DatabaseResult<[u16; 3]> {
        self.generate_keyed(
            "hslColorAsArray",
            || Ok(self.0.hslColorAsArray()),
            |&channels| Ok(triplet(channels)),
        )
    }
}

fn sample_rgb() -> u32 {
    fastrand::u32(1..=0xff_ffff)
}

fn triplet<T: std::fmt::Display>([first, second, third]: [T; 3]) -> String {
    format!("{first},{second},{third}")
}
