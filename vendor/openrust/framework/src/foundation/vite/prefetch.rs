use super::*;
use std::collections::HashSet;
impl Vite {
    pub fn prefetch(&self, concurrency: Option<i64>, event: Option<&str>) -> ViteResult<&Self> {
        self.update(|settings| {
            settings.prefetch_event = event.unwrap_or("load").to_owned();
            settings.prefetch_strategy = Some(
                if concurrency.is_some() {
                    "waterfall"
                } else {
                    "aggressive"
                }
                .into(),
            );
            if let Some(concurrency) = concurrency {
                settings.prefetch_concurrency = concurrency
            }
        })
    }
    pub fn use_waterfall_prefetching(&self, concurrency: Option<i64>) -> ViteResult<&Self> {
        self.use_prefetch_strategy(Some("waterfall"), concurrency)
    }
    pub fn use_aggressive_prefetching(&self) -> ViteResult<&Self> {
        self.use_prefetch_strategy(Some("aggressive"), None)
    }
    pub fn use_prefetch_strategy(
        &self,
        strategy: Option<&str>,
        concurrency: Option<i64>,
    ) -> ViteResult<&Self> {
        self.update(|settings| {
            settings.prefetch_strategy = strategy.map(str::to_owned);
            if strategy == Some("waterfall")
                && let Some(concurrency) = concurrency
            {
                settings.prefetch_concurrency = concurrency
            }
        })
    }
    pub(super) fn render_prefetch(
        &self,
        entries: &[String],
        build: &str,
        manifest: &Manifest,
        output: &mut String,
    ) -> ViteResult<()> {
        if self.settings()?.prefetch_strategy.is_none() || self.is_running_hot()? {
            return Ok(());
        }
        let mut discovered = HashSet::new();
        let mut assets = vec![];
        let mut urls = HashSet::new();
        for entry in entries {
            let mut chunks = vec![];
            for name in render::strings(self.chunk(manifest, entry)?, "dynamicImports")? {
                let chunk = self.chunk(manifest, &name)?;
                let file = self.file(chunk, &name)?;
                if !(file.ends_with(".js") || file.ends_with(".css")) {
                    continue;
                }
                // 显式栈保留原类先标记整组子项、深度优先及CSS尾随的顺序，环图不会递归溢出。
                let mut frames = vec![(chunk.clone(), false)];
                while let Some((chunk, exit)) = frames.pop() {
                    if exit {
                        for css in render::strings(&chunk, "css")? {
                            chunks.push(
                                manifest
                                    .values()
                                    .find(|value| {
                                        value.get("file").and_then(Value::as_str) == Some(&css)
                                    })
                                    .cloned()
                                    .unwrap_or_else(|| serde_json::json!({"file":css})),
                            );
                        }
                        continue;
                    }
                    chunks.push(chunk.clone());
                    let children = render::strings(&chunk, "imports")?
                        .into_iter()
                        .chain(render::strings(&chunk, "dynamicImports")?)
                        .filter(|name| discovered.insert(name.clone()))
                        .collect::<Vec<_>>();
                    frames.push((chunk, true));
                    for name in children.into_iter().rev() {
                        frames.push((self.chunk(manifest, &name)?.clone(), false))
                    }
                }
            }
            for chunk in chunks {
                let url = self.asset_path(&format!("{build}/{}", self.file(&chunk, entry)?))?;
                let context = TagContext {
                    source: chunk.get("src").and_then(Value::as_str),
                    url: &url,
                    chunk: Some(&chunk),
                    manifest: Some(manifest),
                };
                let Some(mut attributes) = render::preload_attributes(self, &context)? else {
                    return Err(ViteError::DisabledPrefetchAttributes);
                };
                attributes.insert("rel".into(), "prefetch".into());
                attributes.insert("fetchpriority".into(), "low".into());
                attributes.insert("href".into(), url.clone().into());
                attributes.retain(|_, value| !matches!(value, Value::Null | Value::Bool(false)));
                for (key, value) in &mut attributes {
                    if *value == Value::Bool(true) {
                        *value = Value::String(key.clone())
                    }
                }
                if self
                    .state()?
                    .preloaded
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .contains_key(&url)
                {
                    continue;
                }
                if urls.insert(url) {
                    assets.push(attributes)
                }
            }
        }
        let assets = serde_json::to_value(assets).map_err(ViteError::callback)?;
        let assets = crate::view::compilers::javascript_expression(&assets.into())
            .map_err(ViteError::callback)?;
        let settings = self.settings()?;
        let template = match settings.prefetch_strategy.as_deref() {
            Some("waterfall") => include_str!("stubs/waterfall.html.stub"),
            Some("aggressive") => include_str!("stubs/aggressive.html.stub"),
            strategy => {
                return Err(ViteError::InvalidPrefetchStrategy(
                    strategy.unwrap_or("null").into(),
                ));
            }
        };
        let nonce = settings
            .nonce
            .map(|nonce| format!(" nonce=\"{nonce}\""))
            .unwrap_or_default();
        // 单遍替换，应用回调中的同名文本不会被当作模板占位符再次执行。
        let mut rest = template;
        while let Some(start) = rest.find("__") {
            output.push_str(&rest[..start]);
            rest = &rest[start..];
            let (key, value) = if rest.starts_with("__NONCE__") {
                ("__NONCE__", nonce.clone())
            } else if rest.starts_with("__EVENT__") {
                ("__EVENT__", settings.prefetch_event.clone())
            } else if rest.starts_with("__ASSETS__") {
                ("__ASSETS__", assets.clone())
            } else if rest.starts_with("__CONCURRENCY__") {
                ("__CONCURRENCY__", settings.prefetch_concurrency.to_string())
            } else {
                output.push_str("__");
                rest = &rest[2..];
                continue;
            };
            output.push_str(&value);
            rest = &rest[key.len()..];
        }
        output.push_str(rest);
        Ok(())
    }
}
