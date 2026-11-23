use std::collections::HashSet;

use openrust::database::eloquent::factories::fake;
use quick_xml::{Reader, events::Event};
use regex::Regex;

#[test]
fn faker_html_preserves_the_document_form_and_subtree_limits() {
    let faker = fake(Some("zh-Hant")).unwrap();
    let form = Regex::new(
        r#"^<html><head><title>[^<]+</title></head><body><form action="example\.(com|net|org)" method="POST"><label for="username">[^<]+</label><input type="text" id="username"><label for="password">[^<]+</label><input type="password" id="password"></form>"#,
    )
    .unwrap();
    for depth in [i32::MIN, 0, 1] {
        let html = faker.random_html(Some(depth), None).unwrap();
        let form = form.find(&html).unwrap();
        assert_eq!(&html[form.end()..], "</body></html>\n");
    }
    for width in [-1, 0, 1] {
        let html = faker.random_html(Some(2), Some(width)).unwrap();
        assert!(!html.contains("<div"));
        if width == 1 {
            let form = form.find(&html).unwrap();
            assert!(!html[form.end()..].starts_with("</body>"));
        }
    }

    let ids = Regex::new(r#"<div id="([0-9]+)">"#).unwrap();
    for _ in 0..32 {
        let html = faker.random_html(None, None).unwrap();
        assert!(form.is_match(&html));
        let mut seen = HashSet::new();
        for captures in ids.captures_iter(&html) {
            let id = captures[1].parse::<u32>().unwrap();
            assert!(id < 100_000);
            assert!(seen.insert(id));
        }
        let xml = html
            .replace(r#"<input type="text" id="username">"#, "<input/>")
            .replace(r#"<input type="password" id="password">"#, "<input/>");
        let mut reader = Reader::from_str(&xml);
        let mut depth = 0;
        let mut siblings = vec![0];
        loop {
            match reader.read_event().unwrap() {
                Event::Start(tag) if tag.name().as_ref() == "div" => {
                    let count = siblings.last_mut().unwrap();
                    *count += 1;
                    assert!(*count <= 4);
                    depth += 1;
                    assert!(depth <= 2);
                    siblings.push(0);
                }
                Event::End(tag) if tag.name().as_ref() == "div" => {
                    depth -= 1;
                    siblings.pop();
                }
                Event::Eof => break,
                _ => {}
            }
        }
        assert_eq!(depth, 0);
    }
    let unique = faker.unique(false);
    let first = unique.random_html(Some(1), None).unwrap();
    assert_ne!(unique.random_html(Some(1), None).unwrap(), first);
}
