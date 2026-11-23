use crate::database::eloquent::factories::faker_rng::FakerRandom;

use std::{
    collections::HashMap,
    sync::{LazyLock, OnceLock},
};

use regex::Regex;

use super::{DatabaseError, DatabaseResult, Faker, UniqueFaker};

const MIN_MAX_ERROR: DatabaseError =
    DatabaseError::InvalidQuery("Faker real text maximum length must be at least 10");

impl Faker {
    pub fn real_text(
        &self,
        max_nb_chars: Option<usize>,
        index_size: Option<usize>,
    ) -> DatabaseResult<String> {
        let max = max_nb_chars.unwrap_or(200);
        if self.provider_locale() == "fa_IR" {
            validate(1, max, index_size.unwrap_or(2))?;
            let (model, syntax) = text_model(self.provider_locale(), index_size.unwrap_or(2));
            return Ok(model.generate(&self.random, max, syntax, true));
        }
        let min = max - (max / 5 + usize::from(max % 5 >= 3));
        let mut text = self.real_text_between(Some(min), Some(max), index_size)?;
        if self.provider_locale() == "cs_CZ" {
            text.retain(|ch| !matches!(ch, '„' | '“'));
        }
        Ok(text)
    }

    pub fn real_text_between(
        &self,
        min_nb_chars: Option<usize>,
        max_nb_chars: Option<usize>,
        index_size: Option<usize>,
    ) -> DatabaseResult<String> {
        let (min, max, index) = (
            min_nb_chars.unwrap_or(160),
            max_nb_chars.unwrap_or(200),
            index_size.unwrap_or(2),
        );
        validate(min, max, index)?;
        let (model, syntax) = text_model(self.provider_locale(), index);
        for _ in 0..99 {
            let text = model.generate(&self.random, max, syntax, false);
            if text.chars().count() > min {
                return Ok(text);
            }
        }
        Err(DatabaseError::InvalidQuery(
            "Faker real text retries exhausted",
        ))
    }
}

impl UniqueFaker<'_> {
    pub fn real_text(
        &self,
        max_nb_chars: Option<usize>,
        index_size: Option<usize>,
    ) -> DatabaseResult<String> {
        self.try_generate("realText", || self.0.real_text(max_nb_chars, index_size))
    }

    pub fn real_text_between(
        &self,
        min_nb_chars: Option<usize>,
        max_nb_chars: Option<usize>,
        index_size: Option<usize>,
    ) -> DatabaseResult<String> {
        self.try_generate("realTextBetween", || {
            self.0
                .real_text_between(min_nb_chars, max_nb_chars, index_size)
        })
    }
}

fn validate(min: usize, max: usize, index: usize) -> DatabaseResult<()> {
    if min == 0 {
        return Err(DatabaseError::InvalidQuery(
            "Faker real text minimum length must be at least 1",
        ));
    }
    if max < 10 {
        return Err(MIN_MAX_ERROR);
    }
    if !(1..=5).contains(&index) {
        return Err(DatabaseError::InvalidQuery(
            "Faker real text index size must be between 1 and 5",
        ));
    }
    if min >= max {
        return Err(DatabaseError::InvalidQuery(
            "Faker real text minimum length must be smaller than maximum length",
        ));
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum Syntax {
    Uppercase,
    Arabic,
    Unrestricted,
    Japanese,
    TraditionalChinese,
}

impl Syntax {
    fn separator(self) -> &'static str {
        match self {
            Self::Japanese | Self::TraditionalChinese => "",
            _ => " ",
        }
    }

    fn can_start(self, token: &str) -> bool {
        static UPPERCASE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\p{Lu}").unwrap());
        static ARABIC: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\p{Arabic}").unwrap());
        match self {
            Self::Uppercase => UPPERCASE.is_match(token),
            Self::Arabic => ARABIC.is_match(token),
            Self::Unrestricted => true,
            Self::Japanese | Self::TraditionalChinese => {
                !["、", "。", "」", "』", "！", "？", "ー", "，", "：", "；"].contains(&token)
            }
        }
    }

    fn finish(self, mut text: String) -> String {
        match self {
            // FakerPHP with mbstring compares the entire prefix against the punctuation list.
            Self::Japanese => text.push('。'),
            Self::TraditionalChinese => {
                if text.ends_with(['。', '」', '』', '！', '？']) {
                    return text;
                }
                if text.ends_with(['、', '「', '『', 'ー', '，', '：', '；']) {
                    text.pop();
                }
                text.push('。');
            }
            _ => {
                text.truncate(
                    text.trim_end_matches([' ', ',', '-', ':', ';', '–', '—'])
                        .len(),
                );
                text.push('.');
            }
        }
        text
    }
}

type Context = [u32; 5];

struct TextModel {
    vocabulary: Vec<&'static str>,
    transitions: HashMap<Context, Vec<u32>>,
    starts: Vec<Context>,
    index: usize,
}

impl TextModel {
    fn new(source: &'static str, index: usize, syntax: Syntax, trailing_empty: bool) -> Self {
        let source = source.trim_end_matches('\n');
        let mut vocabulary = Vec::new();
        let mut ids = HashMap::new();
        let mut intern = |token| {
            *ids.entry(token).or_insert_with(|| {
                let id = u32::try_from(vocabulary.len()).unwrap();
                vocabulary.push(token);
                id
            })
        };
        let mut tokens = Vec::new();
        if syntax.separator().is_empty() {
            let mut characters = source.char_indices().peekable();
            while let Some((start, ch)) = characters.next() {
                let end = characters
                    .peek()
                    .map_or(source.len(), |&(offset, _)| offset);
                if match syntax {
                    Syntax::Japanese => !ch.is_whitespace(),
                    _ => ch != '0',
                } {
                    tokens.push(intern(&source[start..end]));
                }
            }
        } else {
            tokens.extend(source.split(' ').map(&mut intern));
            if trailing_empty {
                tokens.push(intern(""));
            }
        }

        let mut transitions: HashMap<Context, Vec<u32>> = HashMap::new();
        let mut starts = Vec::new();
        for window in tokens.windows(index + 1) {
            let mut context = [0; 5];
            context[..index].copy_from_slice(&window[..index]);
            let followers = transitions.entry(context).or_insert_with(|| {
                starts.push(context);
                Vec::new()
            });
            followers.push(window[index]);
        }
        Self {
            vocabulary,
            transitions,
            starts,
            index,
        }
    }

    fn generate(&self, random: &FakerRandom, max: usize, syntax: Syntax, persian: bool) -> String {
        let mut context = self.starts[random.usize(..self.starts.len())];
        let separator = syntax.separator();
        let mut length = 0_usize;
        let mut words = Vec::new();
        let mut skipped = 0;
        while length < max {
            let Some(followers) = self.transitions.get(&context) else {
                break;
            };
            let next = followers[random.usize(..followers.len())];
            context.copy_within(1..self.index, 0);
            context[self.index - 1] = next;
            let word = self.vocabulary[next as usize];
            if length == 0
                && if persian {
                    !word.starts_with(|ch: char| ('\u{0600}'..='\u{06ff}').contains(&ch))
                } else {
                    !syntax.can_start(word)
                }
            {
                skipped += 1;
                if skipped >= self.starts.len() {
                    break;
                }
                continue;
            }

            let word_length = if persian {
                word.len()
            } else {
                word.chars().count()
            };
            length = length.saturating_add(word_length + separator.len());
            words.push(word);
        }
        // Upstream also drops the last token when a chain ends before reaching the limit.
        words.pop();
        let text = words.join(separator);
        if persian {
            text + "."
        } else {
            syntax.finish(text)
        }
    }
}

macro_rules! model {
    ($locale:literal, $index:ident, $syntax:ident $(, $trailing:literal)?) => {{
        static MODELS: [OnceLock<TextModel>; 5] = [const { OnceLock::new() }; 5];
        let syntax = Syntax::$syntax;
        let model = MODELS[$index - 1].get_or_init(|| {
            TextModel::new(
                include_str!(concat!("faker_text_corpora/", $locale, ".txt")),
                $index,
                syntax,
                false $(|| $trailing)?,
            )
        });
        (model, syntax)
    }};
}

fn text_model(locale: &str, index: usize) -> (&'static TextModel, Syntax) {
    match locale {
        "ar_EG" => model!("ar_EG", index, Arabic),
        "ar_JO" | "ar_SA" => model!("ar_JO", index, Arabic),
        "cs_CZ" => model!("cs_CZ", index, Uppercase),
        "de_CH" => model!("de_CH", index, Uppercase),
        "de_AT" | "de_DE" => model!("de_DE", index, Uppercase),
        "el_GR" => model!("el_GR", index, Uppercase),
        "es_ES" => model!("es_ES", index, Uppercase),
        "fa_IR" => model!("fa_IR", index, Uppercase),
        "fr_CA" => model!("fr_CA", index, Uppercase),
        "fr_CH" | "fr_FR" => model!("fr_FR", index, Uppercase),
        "hu_HU" => model!("hu_HU", index, Uppercase),
        "it_CH" | "it_IT" => model!("it_IT", index, Uppercase),
        "ja_JP" => model!("ja_JP", index, Japanese),
        "ka_GE" => model!("ka_GE", index, Unrestricted),
        "kk_KZ" => model!("kk_KZ", index, Uppercase, true),
        "ko_KR" => model!("ko_KR", index, Uppercase),
        "nl_BE" => model!("nl_BE", index, Uppercase),
        "nl_NL" => model!("nl_NL", index, Uppercase),
        "pl_PL" => model!("pl_PL", index, Uppercase),
        "pt_BR" => model!("pt_BR", index, Uppercase, true),
        "ro_MD" => model!("ro_MD", index, Uppercase),
        "ro_RO" => model!("ro_RO", index, Uppercase),
        "ru_RU" => model!("ru_RU", index, Uppercase, true),
        "uk_UA" => model!("uk_UA", index, Uppercase, true),
        "zh_TW" => model!("zh_TW", index, TraditionalChinese),
        _ => model!("en_US", index, Uppercase),
    }
}
