use super::*;
use std::{collections::HashSet, sync::LazyLock};

pub(super) fn parse_attributes(attributes: &Attributes) -> ViteResult<Vec<String>> {
    attributes
        .iter()
        .filter_map(|(name, value)| match value {
            Value::Null | Value::Bool(false) => None,
            Value::Bool(true) => Some(Ok(name.clone())),
            Value::String(value) => Some(Ok(format!("{name}=\"{value}\""))),
            Value::Number(value) => Some(Ok(format!("{name}=\"{value}\""))),
            _ => Some(Err(ViteError::InvalidAttribute(name.clone()))),
        })
        .collect()
}
fn css(path: &str) -> bool {
    static CSS: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r"\.(css|less|sass|scss|styl|stylus|pcss|postcss)(\?[^.]*)?$")
            .expect("资源样式后缀")
    });
    CSS.is_match(path)
}
pub(super) fn strings(chunk: &Value, key: &str) -> ViteResult<Vec<String>> {
    let Some(value) = chunk.get(key) else {
        return Ok(vec![]);
    };
    value
        .as_array()
        .ok_or_else(|| ViteError::InvalidChunk(key.into()))?
        .iter()
        .map(|v| {
            v.as_str()
                .map(str::to_owned)
                .ok_or_else(|| ViteError::InvalidChunk(key.into()))
        })
        .collect()
}
fn imports(manifest: &Manifest, chunk: &Value) -> ViteResult<Vec<String>> {
    let mut frames = vec![(strings(chunk, "imports")?, 0, HashSet::new())];
    let mut out = vec![];
    while let Some((entries, index, seen)) = frames.last_mut() {
        if *index >= entries.len() {
            frames.pop();
            continue;
        }
        let name = entries[*index].clone();
        *index += 1;
        if !seen.insert(name.clone()) {
            continue;
        }
        let seen = seen.clone();
        out.push(name.clone());
        if let Some(chunk) = manifest.get(&name) {
            frames.push((strings(chunk, "imports")?, 0, seen));
        }
    }
    Ok(out)
}
fn resolved_attributes(
    settings: &Settings,
    context: &TagContext<'_>,
    style: bool,
) -> ViteResult<Attributes> {
    let mut attributes = Attributes::new();
    if let Some(key) = &settings.integrity_key {
        attributes.insert(
            "integrity".into(),
            context
                .chunk
                .and_then(|c| c.get(key))
                .cloned()
                .unwrap_or(Value::Bool(false)),
        );
    }
    for resolver in if style {
        &settings.style_attributes
    } else {
        &settings.script_attributes
    } {
        attributes.extend(resolver(context)?);
    }
    Ok(attributes)
}
fn tag(vite: &Vite, context: &TagContext<'_>) -> ViteResult<String> {
    let style = css(context.url);
    let resolved = resolved_attributes(&vite.settings()?, context, style)?;
    let mut attributes: Attributes = if style {
        [
            ("rel".into(), "stylesheet".into()),
            ("href".into(), context.url.into()),
        ]
        .into()
    } else {
        [
            ("type".into(), "module".into()),
            ("src".into(), context.url.into()),
        ]
        .into()
    };
    // 属性解析可修改nonce；与实际基线一致，在回调之后读取标签的nonce。
    attributes.insert(
        "nonce".into(),
        vite.csp_nonce()?
            .map(Value::String)
            .unwrap_or(Value::Bool(false)),
    );
    attributes.extend(resolved);
    let attributes = parse_attributes(&attributes)?.join(" ");
    Ok(if style {
        format!("<link {attributes} />")
    } else {
        format!("<script {attributes}></script>")
    })
}
pub(super) fn preload_attributes(
    vite: &Vite,
    context: &TagContext<'_>,
) -> ViteResult<Option<Attributes>> {
    let settings = vite.settings()?;
    let style = css(context.url);
    let nonce = settings
        .nonce
        .clone()
        .map(Value::String)
        .unwrap_or(Value::Bool(false));
    let cross = resolved_attributes(&settings, context, style)?
        .get("crossorigin")
        .cloned()
        .unwrap_or(Value::Bool(false));
    let mut attributes: Attributes = [
        (
            "rel".into(),
            if style { "preload" } else { "modulepreload" }.into(),
        ),
        ("as".into(), if style { "style" } else { "script" }.into()),
        ("href".into(), context.url.into()),
        ("nonce".into(), nonce),
        ("crossorigin".into(), cross),
    ]
    .into();
    let settings = vite.settings()?;
    if let Some(key) = &settings.integrity_key {
        attributes.insert(
            "integrity".into(),
            context
                .chunk
                .and_then(|c| c.get(key))
                .cloned()
                .unwrap_or(Value::Bool(false)),
        );
    }
    for resolver in &settings.preload_attributes {
        let Some(resolved) = resolver(context)? else {
            return Ok(None);
        };
        attributes.extend(resolved);
    }
    Ok(Some(attributes))
}
fn preload(vite: &Vite, context: &TagContext<'_>) -> ViteResult<Option<(String, Vec<String>)>> {
    let Some(mut attributes) = preload_attributes(vite, context)? else {
        return Ok(None);
    };
    let tag = format!("<link {} />", parse_attributes(&attributes)?.join(" "));
    attributes.shift_remove("href");
    Ok(Some((tag, parse_attributes(&attributes)?)))
}
#[derive(PartialEq)]
struct Preload {
    source: Option<String>,
    url: String,
    chunk: Option<Value>,
}
impl Preload {
    fn context<'a>(&'a self, manifest: &'a Manifest) -> TagContext<'a> {
        TagContext {
            source: self.source.as_deref(),
            url: &self.url,
            chunk: self.chunk.as_ref(),
            manifest: Some(manifest),
        }
    }
}

impl Vite {
    pub fn render<I, S>(&self, entries: I, build: Option<&str>) -> ViteResult<HtmlString>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let entries: Vec<String> = entries.into_iter().map(|v| v.as_ref().to_owned()).collect();
        let state = self.state()?;
        let settings = state
            .settings
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        if self.hot_path(&settings)?.is_file() {
            let mut html = String::new();
            for entry in std::iter::once("@vite/client").chain(entries.iter().map(String::as_str)) {
                let server = self.dev_server_url()?.unwrap_or_default();
                html.push_str(&tag(
                    self,
                    &TagContext {
                        source: Some(entry),
                        url: &format!("{server}/{entry}"),
                        chunk: None,
                        manifest: None,
                    },
                )?);
            }
            return Ok(HtmlString::new(html));
        }
        let build = build.unwrap_or(&settings.build_directory);
        let manifest = self.manifest(&settings, build)?;
        let mut tags = Vec::new();
        let mut preloads = Vec::new();
        for entry in &entries {
            let chunk = self.chunk(&manifest, entry)?;
            let source = chunk.get("src").and_then(Value::as_str);
            let url = self.asset_path(&format!("{build}/{}", self.file(chunk, entry)?))?;
            preloads.push(Preload {
                source: source.map(str::to_owned),
                url: url.clone(),
                chunk: Some(chunk.clone()),
            });
            for name in imports(&manifest, chunk)? {
                let imported = self.chunk(&manifest, &name)?;
                let url = self.asset_path(&format!("{build}/{}", self.file(imported, &name)?))?;
                preloads.push(Preload {
                    source: Some(name),
                    url,
                    chunk: Some(imported.clone()),
                });
                self.add_css(build, &manifest, imported, &mut preloads, &mut tags)?;
            }
            let url = self.asset_path(&format!("{build}/{}", self.file(chunk, entry)?))?;
            tags.push(tag(
                self,
                &TagContext {
                    source: Some(entry),
                    url: &url,
                    chunk: Some(chunk),
                    manifest: Some(&manifest),
                },
            )?);
            self.add_css(build, &manifest, chunk, &mut preloads, &mut tags)?;
        }
        let mut unique = Vec::new();
        for value in preloads {
            if !unique.contains(&value) {
                unique.push(value);
            }
        }
        unique.sort_by_key(|value| !css(&value.url));
        let mut output = String::new();
        for value in unique {
            if let Some((html, attributes)) = preload(self, &value.context(&manifest))? {
                state
                    .preloaded
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .insert(value.url, attributes);
                output.push_str(&html);
            }
        }
        let mut unique = Vec::new();
        for value in tags {
            if !unique.contains(&value) {
                unique.push(value);
            }
        }
        for style in [true, false] {
            for html in &unique {
                if html.starts_with("<link") == style {
                    output.push_str(html)
                }
            }
        }
        self.render_prefetch(&entries, build, &manifest, &mut output)?;
        Ok(HtmlString::new(output))
    }
    fn add_css(
        &self,
        build: &str,
        manifest: &Manifest,
        chunk: &Value,
        preloads: &mut Vec<Preload>,
        tags: &mut Vec<String>,
    ) -> ViteResult<()> {
        for file in strings(chunk, "css")? {
            let found = manifest.iter().find(|(_, value)| {
                value.get("file").and_then(Value::as_str) == Some(file.as_str())
            });
            let source = found.map(|(key, _)| key.clone());
            let chunk = found.map(|(_, value)| value.clone());
            let url = self.asset_path(&format!("{build}/{file}"))?;
            preloads.push(Preload {
                source: source.clone(),
                url,
                chunk: chunk.clone(),
            });
            let url = self.asset_path(&format!("{build}/{file}"))?;
            tags.push(tag(
                self,
                &TagContext {
                    source: source.as_deref(),
                    url: &url,
                    chunk: chunk.as_ref(),
                    manifest: Some(manifest),
                },
            )?);
        }
        Ok(())
    }
}
