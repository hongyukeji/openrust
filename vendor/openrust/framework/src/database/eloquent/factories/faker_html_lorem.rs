use std::{collections::HashSet, fmt::Write as _};

use quick_xml::escape::{escape, partial_escape};

use super::{DatabaseError, DatabaseResult, Faker, UniqueFaker};

impl Faker {
    /// Generates a full sample HTML document. Both limits default to four.
    pub fn random_html(
        &self,
        max_depth: Option<i32>,
        max_width: Option<i32>,
    ) -> DatabaseResult<String> {
        let mut document = HtmlLorem {
            faker: self,
            html: String::from("<html><head>"),
            ids: HashSet::new(),
        };
        document.sentence_element("title", 10);
        document.html.push_str("</head><body>");
        document.login_form();
        document.subtree(max_depth.unwrap_or(4), max_width.unwrap_or(4))?;
        document.html.push_str("</body></html>\n");
        Ok(document.html)
    }
}

impl UniqueFaker<'_> {
    pub fn random_html(
        &self,
        max_depth: Option<i32>,
        max_width: Option<i32>,
    ) -> DatabaseResult<String> {
        self.try_generate("randomHtml", || self.0.random_html(max_depth, max_width))
    }
}

struct HtmlLorem<'a> {
    faker: &'a Faker,
    html: String,
    ids: HashSet<u32>,
}

impl HtmlLorem<'_> {
    fn login_form(&mut self) {
        let action = self.faker.safe_email_domain();
        write!(self.html, r#"<form action="{action}" method="POST">"#).unwrap();
        for (id, kind) in [("username", "text"), ("password", "password")] {
            write!(
                self.html,
                r#"<label for="{id}">{}</label><input type="{kind}" id="{id}">"#,
                partial_escape(self.faker.word())
            )
            .unwrap();
        }
        self.html.push_str("</form>");
    }

    fn subtree(&mut self, depth: i32, width: i32) -> DatabaseResult<()> {
        if depth <= 1 {
            return Ok(());
        }
        let depth = depth - 1;
        let siblings = self.faker.random.i32(width.min(1)..=width.max(1));
        for _ in 0..siblings {
            if depth == 1 {
                self.leaf();
            } else {
                self.open_div()?;
                self.subtree(self.faker.random.i32(0..=depth), width)?;
                self.html.push_str("</div>");
            }
        }
        Ok(())
    }

    fn open_div(&mut self) -> DatabaseResult<()> {
        if self.faker.random.bool() {
            write!(self.html, r#"<div class="{}">"#, escape(self.faker.word())).unwrap();
            return Ok(());
        }

        for _ in 0..10_000 {
            let id = self.faker.random.u32(..100_000);
            if self.ids.insert(id) {
                write!(self.html, r#"<div id="{id}">"#).unwrap();
                return Ok(());
            }
        }
        Err(DatabaseError::InvalidQuery(
            "unique Faker HTML IDs exhausted",
        ))
    }

    fn leaf(&mut self) {
        match self.faker.random.u8(1..=10) {
            1 => self.sentence_element("p", 10),
            2 => {
                let href = self.faker.safe_email_domain();
                write!(self.html, r#"<a href="{href}">"#).unwrap();
                self.sentence(10);
                self.html.push_str("</a>");
            }
            3 => self.sentence_element("span", 10),
            4 => {
                self.html.push_str("<ul>");
                for _ in 0..self.faker.random.u8(1..=11) {
                    self.sentence_element("li", 4);
                }
                self.html.push_str("</ul>");
            }
            5 => {
                let tag = ["h1", "h2", "h3"][self.faker.random.usize(..3)];
                self.sentence_element(tag, 10);
            }
            6 => self.sentence_element("b", 10),
            7 => self.sentence_element("i", 10),
            8 => self.table(),
            _ => self.sentence(10),
        }
    }

    fn table(&mut self) {
        let rows = self.faker.random.u8(1..=10);
        let columns = self.faker.random.u8(1..=6);
        self.html.push_str("<table><thead><tr>");
        for _ in 0..columns {
            self.sentence_element("th", 4);
        }
        self.html.push_str("</tr></thead><tbody>");
        for _ in 0..rows {
            self.html.push_str("<tr>");
            for _ in 0..columns {
                self.sentence_element("td", 10);
            }
            self.html.push_str("</tr>");
        }
        self.html.push_str("</tbody></table>");
    }

    fn sentence_element(&mut self, tag: &str, max_words: usize) {
        write!(self.html, "<{tag}>").unwrap();
        self.sentence(max_words);
        write!(self.html, "</{tag}>").unwrap();
    }

    fn sentence(&mut self, max_words: usize) {
        let text = self
            .faker
            .sentence_with_word_count(self.faker.random.usize(1..=max_words), true);
        self.html.push_str(&partial_escape(text));
    }
}
