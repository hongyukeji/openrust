use std::{collections::HashMap, sync::OnceLock};

use regex::{Regex, RegexBuilder};
use serde::Deserialize;

/// The language of a Doctrine-compatible word inflection.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum InflectionLanguage {
    #[default]
    English,
    Esperanto,
    French,
    Italian,
    NorwegianBokmal,
    Portuguese,
    Spanish,
    Turkish,
}

impl InflectionLanguage {
    pub(crate) fn plural(self, value: &str, count: i64) -> String {
        if count.unsigned_abs() == 1
            || value.eq_ignore_ascii_case("recommended")
            || value.eq_ignore_ascii_case("related")
            || !value.chars().next_back().is_some_and(|end| {
                end.is_ascii_alphanumeric() || ('\u{80}'..='\u{ffff}').contains(&end)
            })
        {
            return value.to_owned();
        }
        self.rules().plural.inflect(value)
    }

    pub(crate) fn singular(self, value: &str) -> String {
        self.rules().singular.inflect(value)
    }

    fn rules(self) -> &'static RulePair {
        static DATA: OnceLock<HashMap<String, RulePair>> = OnceLock::new();
        let data = DATA.get_or_init(|| {
            // The checked-in data is exported in Doctrine rule order and keeps
            // the compiled direction of the irregular substitutions.
            let source: HashMap<String, SourcePair> =
                serde_json::from_str(include_str!("doctrine-inflector-2.1.0.json"))
                    .expect("valid pinned Doctrine inflection data");
            source
                .into_iter()
                .map(|(name, rule)| {
                    (
                        name,
                        RulePair {
                            plural: rule.plural.compile(),
                            singular: rule.singular.compile(),
                        },
                    )
                })
                .collect()
        });
        &data[match self {
            Self::English => "English",
            Self::Esperanto => "Esperanto",
            Self::French => "French",
            Self::Italian => "Italian",
            Self::NorwegianBokmal => "NorwegianBokmal",
            Self::Portuguese => "Portuguese",
            Self::Spanish => "Spanish",
            Self::Turkish => "Turkish",
        }]
    }
}

#[derive(Deserialize)]
struct SourcePair {
    singular: SourceRules,
    plural: SourceRules,
}

#[derive(Deserialize)]
struct SourceRules {
    uninflected: String,
    irregular: Vec<(String, String)>,
    regular: Vec<(String, String)>,
}

struct RulePair {
    singular: Rules,
    plural: Rules,
}

struct Rules {
    uninflected: Regex,
    irregular: HashMap<String, String>,
    regular: Vec<Transformation>,
}

struct Transformation {
    pattern: Regex,
    replacement: String,
    skip_regatta: bool,
}

impl SourceRules {
    fn compile(self) -> Rules {
        let compile_pattern = |input: &str| {
            let (pattern, flags) = input[1..]
                .rsplit_once('/')
                .expect("delimited Doctrine regex");
            RegexBuilder::new(pattern)
                .case_insensitive(flags.contains('i'))
                .build()
                .unwrap_or_else(|error| panic!("invalid Doctrine rule {input}: {error}"))
        };
        let capture = Regex::new(r"\\([0-9]+)").expect("capture pattern");
        Rules {
            uninflected: compile_pattern(&self.uninflected),
            irregular: self.irregular.into_iter().collect(),
            regular: self
                .regular
                .into_iter()
                .map(|(pattern, replacement)| {
                    // Doctrine's English singular rule excludes words ending in regatta.
                    let skip_regatta = pattern.contains("(?<!regatta)");
                    let pattern = compile_pattern(&pattern.replace("(?<!regatta)", ""));
                    let replacement = capture
                        .replace_all(&replacement, |caps: &regex::Captures<'_>| {
                            format!("${{{}}}", &caps[1])
                        })
                        .into_owned();
                    Transformation {
                        pattern,
                        replacement,
                        skip_regatta,
                    }
                })
                .collect(),
        }
    }
}

impl Rules {
    fn inflect(&self, word: &str) -> String {
        if word.is_empty() || self.uninflected.is_match(word) {
            return word.to_owned();
        }
        let lowercase = word.to_ascii_lowercase();
        if let Some(replacement) = self.irregular.get(&lowercase) {
            return match_case(replacement, word);
        }
        for transformation in &self.regular {
            if !(transformation.skip_regatta && lowercase.ends_with("regatta"))
                && transformation.pattern.is_match(word)
            {
                let result = transformation
                    .pattern
                    .replace_all(word, transformation.replacement.as_str());
                return match_case(&result, word);
            }
        }
        word.to_owned()
    }
}

fn match_case(value: &str, comparison: &str) -> String {
    if comparison.to_lowercase() == comparison {
        value.to_lowercase()
    } else if comparison.to_uppercase() == comparison {
        value.to_uppercase()
    } else if ascii_capitalize(comparison, false) == comparison {
        ascii_capitalize(value, false)
    } else if ascii_capitalize(comparison, true) == comparison {
        ascii_capitalize(value, true)
    } else {
        value.into()
    }
}

fn ascii_capitalize(value: &str, all_words: bool) -> String {
    let mut upper = true;
    value
        .chars()
        .map(|ch| {
            let next = if upper { ch.to_ascii_uppercase() } else { ch };
            upper = all_words && matches!(ch, ' ' | '\t' | '\n' | '\r' | '\x0b' | '\x0c');
            next
        })
        .collect()
}
