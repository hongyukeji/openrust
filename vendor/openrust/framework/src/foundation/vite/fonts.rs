//! 字体清单解析沿用原模板职责，正式服务与指令共用缓存及预加载记录。
use super::*;
use cap_std::{ambient_authority, fs::Dir};

fn invalid(message: impl Into<String>) -> ViteError {
    ViteError::InvalidFonts(message.into())
}
fn object<'a>(value: &'a Value, field: &str) -> ViteResult<&'a serde_json::Map<String, Value>> {
    if value.as_array().is_some_and(Vec::is_empty) {
        static EMPTY: std::sync::LazyLock<serde_json::Map<String, Value>> =
            std::sync::LazyLock::new(serde_json::Map::new);
        return Ok(&EMPTY);
    }
    value
        .as_object()
        .ok_or_else(|| invalid(format!("{field}必须是按别名索引的对象")))
}
fn string<'a>(value: &'a Value, field: &str) -> ViteResult<&'a str> {
    value
        .as_str()
        .ok_or_else(|| invalid(format!("{field}必须是字符串")))
}
fn member<'a>(value: &'a Value, field: &str) -> ViteResult<&'a str> {
    string(
        value
            .get(field)
            .ok_or_else(|| invalid(format!("缺少{field}")))?,
        field,
    )
}
fn present(value: Option<&Value>) -> Option<&Value> {
    value.filter(|value| !value.is_null())
}
impl Vite {
    fn font_manifest(&self, settings: &Settings, hot: bool) -> ViteResult<Option<Arc<Value>>> {
        let path = if hot {
            self.hot_path(settings)?
                .parent()
                .unwrap_or(Path::new("."))
                .join("fonts-manifest.dev.json")
        } else {
            self.public_path(&format!(
                "{}/{}",
                settings.build_directory, settings.fonts_manifest_filename
            ))?
        };
        if let Some(manifest) = self
            .font_manifests
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&path)
        {
            return Ok(Some(manifest.clone()));
        }
        if !path.is_file() {
            return Ok(None);
        }
        let bytes = fs::read(&path).map_err(|error| io_error(&path, error))?;
        let manifest = Arc::new(serde_json::from_slice(&bytes).map_err(|source| {
            ViteError::InvalidManifest {
                path: path.clone(),
                source,
            }
        })?);
        let mut cache = self
            .font_manifests
            .write()
            .unwrap_or_else(PoisonError::into_inner);
        Ok(Some(cache.entry(path).or_insert(manifest).clone()))
    }
    pub fn fonts(&self, aliases: Option<&[&str]>) -> ViteResult<HtmlString> {
        let settings = self.settings()?;
        let hot = self.hot_path(&settings)?.is_file();
        let Some(manifest) = self.font_manifest(&settings, hot)? else {
            return Ok(HtmlString::new(""));
        };
        if manifest.get("version") != Some(&Value::from(1)) {
            return Err(invalid("只支持version为1的清单"));
        }
        let families = manifest
            .get("families")
            .filter(|value| value.is_object() || value.is_array())
            .ok_or_else(|| invalid("缺少families对象"))?;
        if let Some(aliases) = aliases {
            for alias in aliases {
                if families.get(*alias).is_none() {
                    return Err(invalid(format!("清单中没有字体别名[{alias}]")));
                }
            }
        }
        let preloads = match manifest.get("preloads").filter(|value| !value.is_null()) {
            None => &[][..],
            Some(value) => value
                .as_array()
                .ok_or_else(|| invalid("preloads必须是数组"))?
                .as_slice(),
        };
        let preloads: Vec<_> = preloads
            .iter()
            .filter(|preload| {
                aliases.is_none_or(|aliases| {
                    preload
                        .get("alias")
                        .and_then(Value::as_str)
                        .is_some_and(|alias| aliases.contains(&alias))
                })
            })
            .collect();
        // 先验证本次选中的全部条目，再产生任何预加载副作用。
        for preload in &preloads {
            member(preload, "alias")?;
            member(preload, if hot { "url" } else { "file" })?;
        }
        let state = self.state()?;
        let mut tags = vec![];
        for preload in preloads {
            let url = if let Some(url) = present(preload.get("url")) {
                string(url, "url")?.to_owned()
            } else {
                self.asset_path(&format!(
                    "{}/{}",
                    self.settings()?.build_directory,
                    member(preload, "file")?
                ))?
            };
            if state
                .preloaded
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .contains_key(&url)
            {
                continue;
            }
            let mut attributes: Attributes = [
                ("rel".into(), "preload".into()),
                (
                    "as".into(),
                    present(preload.get("as"))
                        .cloned()
                        .unwrap_or_else(|| "font".into()),
                ),
                ("href".into(), url.clone().into()),
                (
                    "type".into(),
                    preload.get("type").cloned().unwrap_or(Value::Bool(false)),
                ),
                (
                    "crossorigin".into(),
                    preload
                        .get("crossorigin")
                        .cloned()
                        .unwrap_or(Value::Bool(false)),
                ),
                (
                    "nonce".into(),
                    self.csp_nonce()?
                        .map(Value::String)
                        .unwrap_or(Value::Bool(false)),
                ),
            ]
            .into();
            let chunk = Value::Array(vec![]);
            let empty = Manifest::new();
            let context = TagContext {
                source: Some("fonts"),
                url: &url,
                chunk: Some(&chunk),
                manifest: Some(&empty),
            };
            let mut enabled = true;
            for resolver in self.settings()?.preload_attributes {
                match resolver(&context)? {
                    Some(resolved) => attributes.extend(resolved),
                    None => {
                        enabled = false;
                        break;
                    }
                }
            }
            if !enabled {
                continue;
            }
            let tag = format!(
                "<link {} />",
                render::parse_attributes(&attributes)?.join(" ")
            );
            attributes.shift_remove("href");
            state
                .preloaded
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .insert(url, render::parse_attributes(&attributes)?);
            tags.push(tag);
        }
        let css = self.font_style(&manifest, aliases, &self.settings()?.build_directory)?;
        if css
            .as_bytes()
            .windows(7)
            .any(|bytes| bytes.eq_ignore_ascii_case(b"</style"))
        {
            return Err(invalid("字体CSS不能包含style结束标签"));
        }
        if !css.is_empty() {
            let attributes: Attributes = [(
                "nonce".into(),
                self.csp_nonce()?
                    .map(Value::String)
                    .unwrap_or(Value::Bool(false)),
            )]
            .into();
            let attributes = render::parse_attributes(&attributes)?.join(" ");
            let attributes = if attributes.is_empty() {
                String::new()
            } else {
                format!(" {attributes}")
            };
            tags.push(format!(
                "<style{attributes}>\n{}\n</style>",
                css.trim_matches('\n')
            ));
        }
        Ok(HtmlString::new(tags.join("\n")))
    }
    fn font_style(
        &self,
        manifest: &Value,
        aliases: Option<&[&str]>,
        build: &str,
    ) -> ViteResult<String> {
        let Some(style) = present(manifest.get("style")) else {
            return Ok(String::new());
        };
        if let Some(aliases) = aliases {
            let empty = serde_json::Map::new();
            let families = present(style.get("familyStyles"))
                .map(|value| object(value, "style.familyStyles"))
                .transpose()?
                .unwrap_or(&empty);
            let variables = present(style.get("variables"))
                .map(|value| object(value, "style.variables"))
                .transpose()?
                .unwrap_or(&empty);
            let mut parts = aliases
                .iter()
                .filter_map(|alias| present(families.get(*alias)))
                .map(|value| string(value, "familyStyles").map(str::to_owned))
                .collect::<ViteResult<Vec<_>>>()?;
            if !variables.is_empty() {
                let declarations = aliases
                    .iter()
                    .filter_map(|alias| present(variables.get(*alias)))
                    .map(|value| string(value, "variables").map(|text| format!("  {text}")))
                    .collect::<ViteResult<Vec<_>>>()?;
                parts.push(if declarations.is_empty() {
                    String::new()
                } else {
                    format!(":root {{\n{}\n}}", declarations.join("\n"))
                });
            }
            return Ok(parts.join("\n\n"));
        }
        if let Some(inline) = present(style.get("inline")) {
            return Ok(string(inline, "style.inline")?.to_owned());
        }
        if let Some(file) = present(style.get("file")) {
            let path = self.public_path(&format!("{build}/{}", string(file, "style.file")?))?;
            let public = self.public_path("")?;
            let directory = Dir::open_ambient_dir(&public, ambient_authority())
                .map_err(|error| io_error(&public, error))?;
            // 沿用原字体读取器的public能力边界，防止清单中的符号链接越出公开目录。
            let relative = path
                .strip_prefix(&public)
                .map_err(|_| invalid("字体CSS必须位于公开目录"))?;
            return directory
                .read_to_string(relative)
                .map_err(|error| io_error(&path, error));
        }
        Ok(String::new())
    }
}
