use icu_experimental::transliterate::Transliterator;
use icu_locale_core::Locale;
use icu_normalizer::ComposingNormalizer;
use icu_properties::{
    CodePointMapData, PropertyNamesShort,
    props::{GeneralCategory, Script},
};
use std::collections::HashMap;

const LATIN_ASCII: &str = "und-t-und-latn-d0-ascii";

// ICU4X exposes CLDR's script-specific transforms. Select them by source
// script and keep Common and Inherited characters with the current run.
pub(super) fn transliterate(value: &str) -> String {
    let nfkc = ComposingNormalizer::new_nfkc();
    let categories = CodePointMapData::<GeneralCategory>::new();
    let unmarked: String = nfkc
        .normalize(value)
        .chars()
        .filter(|ch| categories.get(*ch) != GeneralCategory::NonspacingMark)
        .collect();
    let normalized = nfkc.normalize(&unmarked);

    let scripts = CodePointMapData::<Script>::new();
    let mut start = 0;
    let mut current = Script::Common;
    let mut transforms = HashMap::new();
    let mut output = String::with_capacity(normalized.len());
    for (index, ch) in normalized.char_indices() {
        let script = scripts.get(ch);
        if matches!(script, Script::Common | Script::Inherited) {
            continue;
        }
        if script != current {
            append_run(
                &mut output,
                &normalized[start..index],
                current,
                &mut transforms,
            );
            start = index;
            current = script;
        }
    }
    append_run(&mut output, &normalized[start..], current, &mut transforms);
    let id = LATIN_ASCII
        .parse::<Locale>()
        .expect("valid Latin-ASCII transform ID");
    Transliterator::try_new(&id).map_or_else(
        |_| output.clone(),
        |transform| transform.transliterate(output.clone()),
    )
}

fn append_run(
    output: &mut String,
    value: &str,
    script: Script,
    transforms: &mut HashMap<Script, Option<Transliterator>>,
) {
    if value.is_empty() {
        return;
    }
    let transform = transforms.entry(script).or_insert_with(|| {
        let name = match script {
            Script::Han => "Hans",
            Script::Latin | Script::Common | Script::Unknown => return None,
            _ => PropertyNamesShort::new().get(script)?,
        };
        // The compiled CLDR dataset has Latin transforms for these source scripts.
        if !matches!(
            name,
            "Arab"
                | "Armn"
                | "Beng"
                | "Bopo"
                | "Cans"
                | "Cyrl"
                | "Deva"
                | "Ethi"
                | "Geor"
                | "Grek"
                | "Gujr"
                | "Guru"
                | "Hang"
                | "Hans"
                | "Hebr"
                | "Hira"
                | "Jamo"
                | "Kana"
                | "Knda"
                | "Mlym"
                | "Orya"
                | "Syrc"
                | "Taml"
                | "Telu"
                | "Thaa"
        ) {
            return None;
        }
        let id: Locale = format!("und-latn-t-und-{name}").parse().ok()?;
        Transliterator::try_new(&id).ok()
    });
    match transform {
        Some(transform) => output.push_str(&transform.transliterate(value.to_owned())),
        None => output.push_str(value),
    }
}
