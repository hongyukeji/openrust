use openrust::view::{Factory, FileViewFinder, ViewData};
use std::{fs, path::PathBuf, sync::Arc};

struct Root(PathBuf);
impl Root {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("blade-query-{:032x}", fastrand::u128(..)));
        fs::create_dir_all(path.join("views/components")).unwrap();
        Self(path)
    }
    fn write(&self, name: &str, source: &str) {
        fs::write(
            self.0.join("views").join(format!("{name}.blade.html")),
            source,
        )
        .unwrap();
    }
    fn factory(&self) -> Factory {
        Factory::new(
            FileViewFinder::new([self.0.join("views")]),
            self.0.join("cache"),
        )
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

fn data(locale: &str) -> ViewData {
    ViewData::from_iter([
        ("query".into(), serde_json::json!({"lang":"old", "search":"a&b <script>", "filter":{"tags":["甲", "乙"]}}).into()),
        ("parameter".into(), "lang".into()),
        ("locale".into(), locale.into()),
    ])
}

#[test]
fn query_helpers_preserve_parameters_encode_and_escape_without_mutating_input() {
    let root = Root::new();
    root.write("links", r#"<a href="/?{{ Arr::query([...Arr::except($query, [$parameter]), $parameter => $locale]) }}">{{ Arr::query(Arr::except($query, [$parameter])) }}</a>"#);
    let input = data("zh-Hant");
    let before = input.clone();
    let factory = root.factory();
    let output = factory.render("links", &input).unwrap();
    assert!(output.contains("lang=zh-Hant"));
    assert!(output.contains("search=a%26b%20%3Cscript%3E"));
    assert!(output.contains("filter%5Btags%5D%5B0%5D=%E7%94%B2"));
    assert!(output.contains("&amp;"));
    assert!(!output.contains("old"));
    assert!(!output.contains("<script>"));
    assert_eq!(input, before);
    assert_eq!(factory.render("links", &input).unwrap(), output);
}

#[test]
fn membership_supports_associative_values_strictness_and_nested_expressions() {
    let root = Root::new();
    root.write("membership", r#"@foreach(['en', 'zh-Hans', 'fr'] as $choice)@if(in_array($choice, ['first' => 'en', 'second' => 'zh-Hans'], true)){{ $choice }};@endif
@endforeach|@if(in_array('1', [1])){{ 'loose' }}@endif|@if(!in_array('1', [1], true)){{ 'strict' }}@endif|{{ Arr::query([]) }}|{{ Arr::query(Arr::except(['a' => ['b' => 1, 'c' => 2]], ['a.b'])) }}"#);
    assert_eq!(
        root.factory()
            .render("membership", &ViewData::new())
            .unwrap(),
        "en;zh-Hans;|loose|strict||a%5Bc%5D=2"
    );
}

#[test]
fn cached_includes_and_components_use_each_consumers_data() {
    let root = Root::new();
    root.write("part", "{{ Arr::query([$parameter => $locale]) }}");
    root.write(
        "components/link",
        "@props(['locale', 'parameter']){{ Arr::query([$parameter => $locale]) }}",
    );
    root.write(
        "page",
        "@include('part')|<x-link :locale=\"$locale\" :parameter=\"$parameter\" />",
    );
    let factory = Arc::new(root.factory());
    for _ in 0..2 {
        let jobs: Vec<_> = ["en", "zh-Hans", "zh-Hant"]
            .into_iter()
            .map(|locale| {
                let factory = factory.clone();
                std::thread::spawn(move || {
                    assert_eq!(
                        factory.render("page", &data(locale)).unwrap(),
                        format!("lang={locale}|lang={locale}")
                    );
                })
            })
            .collect();
        for job in jobs {
            job.join().unwrap();
        }
    }
}

#[test]
fn helper_errors_propagate_and_unregistered_calls_are_rejected() {
    let root = Root::new();
    let factory = root.factory();
    for (index, source) in [
        "{{ Arr::query('bad') }}",
        "{{ Arr::except([], 42) }}",
        "{{ Arr::except([], [false]) }}",
        "{{ in_array('en', 'bad') }}",
        "{{ in_array('en', [], []) }}",
        "{{ Arr::query() }}",
        "{{ Arr::query([], []) }}",
        "{{ Arr::except([]) }}",
        "{{ in_array('en') }}",
        "{{ in_array('en', [], true, false) }}",
        "{{ Arr::query(, []) }}",
        "{{ shell_exec('id') }}",
        "{{ Arr::unknown([]) }}",
    ]
    .into_iter()
    .enumerate()
    {
        let name = format!("bad_{index}");
        root.write(&name, source);
        assert!(factory.render(&name, &ViewData::new()).is_err(), "{source}");
    }
    root.write("trailing", "{{ Arr::query(['a' => 'b'],) }}");
    assert_eq!(factory.render("trailing", &ViewData::new()).unwrap(), "a=b");
}

#[test]
fn language_override_uses_array_union_to_preserve_numeric_query_keys() {
    let root = Root::new();
    root.write("numeric", "{{ Arr::query(['5' => 'en'] + ['5' => 'zh-Hant', '9' => 'keep']) }}|{{ Arr::query(Arr::except(['5' => 'zh-Hant', '9' => 'keep'], ['5'])) }}");
    assert_eq!(
        root.factory().render("numeric", &ViewData::new()).unwrap(),
        "5=en&amp;9=keep|9=keep"
    );
}

#[test]
fn query_helpers_reject_non_finite_data_instead_of_silently_encoding_null() {
    let root = Root::new();
    root.write("invalid", "{{ Arr::query(['value' => $value]) }}");
    let data = ViewData::from_iter([("value".into(), f64::INFINITY.into())]);
    assert!(root.factory().render("invalid", &data).is_err());
}
