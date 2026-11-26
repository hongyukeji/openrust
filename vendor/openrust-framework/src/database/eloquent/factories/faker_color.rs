use crate::database::eloquent::factories::faker_rng::FakerRandom;

use super::{DatabaseResult, Faker, UniqueFaker};

#[path = "faker_color_names.rs"]
mod catalogs;

impl Faker {
    pub fn hex_color(&self) -> String {
        format!("#{:06x}", sample_rgb(&self.random))
    }

    pub fn safe_hex_color(&self) -> String {
        // FakerPHP expands three hex digits drawn from 0x000..=0x0ff.
        let value = self.random.u8(..);
        let (high, low) = (value >> 4, value & 0x0f);
        format!("#00{high:x}{high:x}{low:x}{low:x}")
    }

    pub fn rgb_color_as_array(&self) -> [u8; 3] {
        let [_, red, green, blue] = sample_rgb(&self.random).to_be_bytes();
        [red, green, blue]
    }

    pub fn rgb_color(&self) -> String {
        triplet(self.rgb_color_as_array())
    }

    pub fn rgb_css_color(&self) -> String {
        format!("rgb({})", self.rgb_color())
    }

    pub fn rgba_css_color(&self) -> String {
        let rgb = self.rgb_color();
        let alpha = f64::from(self.random.i32(0..=i32::MAX)) / f64::from(i32::MAX);
        let alpha = (alpha * 10.0).round() / 10.0;
        format!("rgba({rgb},{alpha})")
    }

    pub fn safe_color_name(&self) -> &'static str {
        self.sample_color_name(self.provider_locale(), true)
    }

    pub fn color_name(&self) -> &'static str {
        self.sample_color_name(self.provider_locale(), false)
    }

    pub(in crate::database::eloquent::factories) fn sample_color_name(
        &self,
        locale: &str,
        safe: bool,
    ) -> &'static str {
        let (safe_names, all_names) = catalogs::names(locale);
        self.random
            .sample(if safe { safe_names } else { all_names })
    }

    pub fn hsl_color_as_array(&self) -> [u16; 3] {
        [
            self.random.u16(0..=360),
            self.random.u16(0..=100),
            self.random.u16(0..=100),
        ]
    }

    pub fn hsl_color(&self) -> String {
        triplet(self.hsl_color_as_array())
    }
}

macro_rules! unique_colors {
    ($($name:ident [$history:literal] -> $result:ty),+ $(,)?) => {
        impl UniqueFaker<'_> {
            $(pub fn $name(&self) -> DatabaseResult<$result> {
                self.generate($history, || self.0.$name())
            })+
        }
    };
}

unique_colors! {
    hex_color ["hexColor"] -> String,
    safe_hex_color ["safeHexColor"] -> String,
    rgb_color ["rgbColor"] -> String,
    rgb_css_color ["rgbCssColor"] -> String,
    rgba_css_color ["rgbaCssColor"] -> String,
    safe_color_name ["safeColorName"] -> &'static str,
    color_name ["colorName"] -> &'static str,
    hsl_color ["hslColor"] -> String,
}

impl UniqueFaker<'_> {
    pub fn rgb_color_as_array(&self) -> DatabaseResult<[u8; 3]> {
        self.generate_keyed(
            "rgbColorAsArray",
            || Ok(self.0.rgb_color_as_array()),
            |&channels| Ok(triplet(channels)),
        )
    }

    pub fn hsl_color_as_array(&self) -> DatabaseResult<[u16; 3]> {
        self.generate_keyed(
            "hslColorAsArray",
            || Ok(self.0.hsl_color_as_array()),
            |&channels| Ok(triplet(channels)),
        )
    }
}

fn sample_rgb(random: &FakerRandom) -> u32 {
    random.u32(1..=0xff_ffff)
}

fn triplet<T: std::fmt::Display>([first, second, third]: [T; 3]) -> String {
    format!("{first},{second},{third}")
}
