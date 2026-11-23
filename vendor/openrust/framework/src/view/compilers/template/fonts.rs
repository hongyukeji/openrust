use std::{borrow::Cow, collections::BTreeMap, io};

use cap_std::{ambient_authority, fs::Dir};
use serde::{Deserialize, Serialize};

use crate::{contracts::routing::UrlGenerator as _, support::facades::Url};

use super::{
    Expression, Node, RenderContext, ViewData, ViewError, ViewResult, ViewValue, array_entries,
    escape_html, evaluated_name, expression_value, name_value, render_error, value,
};

#[derive(Deserialize)]
struct Manifest {
    version: u32,
    families: BTreeMap<String, ViewValue>,
    #[serde(default)]
    preloads: Vec<Preload>,
    #[serde(default)]
    style: Option<Style>,
}

#[derive(Deserialize)]
struct Preload {
    alias: String,
    url: Option<String>,
    file: Option<String>,
    #[serde(rename = "as")]
    kind: Option<String>,
    #[serde(rename = "type")]
    media_type: Option<String>,
    crossorigin: Option<ViewValue>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Style {
    inline: Option<String>,
    file: Option<String>,
    #[serde(default)]
    family_styles: BTreeMap<String, String>,
    #[serde(default)]
    variables: BTreeMap<String, String>,
}

#[derive(Serialize, Deserialize)]
pub(super) enum Aliases {
    All,
    Value(Expression),
    List(Vec<Expression>),
}

impl Aliases {
    pub(super) fn resolve<'a>(
        &'a self,
        data: &'a ViewData,
        bindings: &[(&str, &'a ViewValue)],
    ) -> ViewResult<Option<Vec<Cow<'a, str>>>> {
        match self {
            Self::All => Ok(None),
            Self::List(aliases) => aliases
                .iter()
                .map(|alias| evaluated_name(alias, data, bindings))
                .collect::<ViewResult<Vec<_>>>()
                .map(Some),
            Self::Value(alias) => match value(alias, data, bindings)?.as_ref() {
                ViewValue::Null => Ok(None),
                ViewValue::Array(aliases) => aliases
                    .iter()
                    .map(|value| name_value(value).map(|name| Cow::Owned(name.to_owned())))
                    .collect::<ViewResult<_>>()
                    .map(Some),
                alias => Ok(Some(vec![Cow::Owned(name_value(alias)?.to_owned())])),
            },
        }
    }
}

pub(super) fn parse(arguments: &str) -> ViewResult<Node> {
    let arguments = arguments.trim();
    let aliases = if arguments.is_empty() {
        Aliases::All
    } else if arguments.starts_with('[') {
        Aliases::List(
            array_entries(arguments)?
                .into_iter()
                .map(expression_value)
                .collect::<ViewResult<_>>()?,
        )
    } else {
        Aliases::Value(expression_value(arguments)?)
    };
    Ok(Node::Fonts(aliases))
}

pub(super) fn render(
    context: &RenderContext<'_>,
    aliases: Option<Vec<Cow<str>>>,
    output: &mut String,
) -> ViewResult<()> {
    let aliases = aliases
        .as_ref()
        .map(|aliases| aliases.iter().map(Cow::as_ref).collect::<Vec<_>>());
    let Some(public_path) = context.public_path() else {
        return Ok(());
    };
    let public = match Dir::open_ambient_dir(public_path, ambient_authority()) {
        Ok(dir) => dir,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(ViewError::ReadFailed(error)),
    };
    let hot = public.is_file("hot");
    let manifest_path = if hot {
        "fonts-manifest.dev.json"
    } else {
        "build/fonts-manifest.json"
    };
    let source = match public.read_to_string(manifest_path) {
        Ok(source) => source,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(ViewError::ReadFailed(error)),
    };
    let manifest: Manifest =
        serde_json::from_str(&source).map_err(|error| ViewError::RenderFailed(Box::new(error)))?;
    if manifest.version != 1 {
        return Err(render_error("Unsupported Vite font manifest version."));
    }
    if aliases.as_ref().is_some_and(|aliases| {
        aliases
            .iter()
            .any(|alias| !manifest.families.contains_key(*alias))
    }) {
        return Err(render_error(
            "A Blade font alias is missing from the Vite font manifest.",
        ));
    }

    let mut tags = Vec::new();
    let mut preloaded = context.font_preloads();
    for preload in manifest.preloads {
        if aliases
            .as_ref()
            .is_some_and(|aliases| !aliases.contains(&preload.alias.as_str()))
        {
            continue;
        }
        let path = if hot {
            let url = preload
                .url
                .as_deref()
                .ok_or_else(|| render_error("A development font preload requires a URL."))?;
            if !url.starts_with("http://") && !url.starts_with("https://") {
                return Err(render_error(
                    "A development font preload requires an HTTP(S) URL.",
                ));
            }
            url.to_owned()
        } else {
            build_path(preload.file.as_deref())?
        };
        let url = if let Some(uri) = &context.request.url_uri {
            Url::service().and_then(|url| url.with_uri(Some(uri.clone())).asset(&path, None))
        } else {
            Url::asset(&path, None)
        }
        .map_err(|error| ViewError::RenderFailed(Box::new(error)))?;
        if !preloaded.insert(url.clone()) {
            continue;
        }
        let mut tag = String::from("<link rel=\"preload\"");
        attribute(&mut tag, "as", preload.kind.as_deref().unwrap_or("font"));
        attribute(&mut tag, "href", &url);
        if let Some(media_type) = &preload.media_type {
            attribute(&mut tag, "type", media_type);
        }
        match preload.crossorigin {
            Some(ViewValue::String(value)) => attribute(&mut tag, "crossorigin", &value),
            Some(ViewValue::Bool(true)) => tag.push_str(" crossorigin"),
            _ => {}
        }
        tag.push_str(" />");
        tags.push(tag);
    }
    if let Some(style) = manifest.style {
        let css = style.content(&public, aliases.as_deref())?;
        if !css.is_empty() {
            if css
                .as_bytes()
                .windows(7)
                .any(|bytes| bytes.eq_ignore_ascii_case(b"</style"))
            {
                return Err(render_error(
                    "The Vite font CSS contains a style closing tag.",
                ));
            }
            tags.push(format!("<style>\n{}\n</style>", css.trim_matches('\n')));
        }
    }
    output.push_str(&tags.join("\n"));
    Ok(())
}

impl Style {
    fn content(self, public: &Dir, aliases: Option<&[&str]>) -> ViewResult<String> {
        let Some(aliases) = aliases else {
            return match (self.inline, self.file) {
                (Some(inline), _) => Ok(inline),
                (None, Some(file)) => public
                    .read_to_string(build_path(Some(&file))?)
                    .map_err(ViewError::ReadFailed),
                (None, None) => Ok(String::new()),
            };
        };
        let mut styles: Vec<_> = aliases
            .iter()
            .filter_map(|alias| self.family_styles.get(*alias))
            .cloned()
            .collect();
        let variables: Vec<_> = aliases
            .iter()
            .filter_map(|alias| self.variables.get(*alias))
            .map(|variable| format!("  {variable}"))
            .collect();
        if !variables.is_empty() {
            styles.push(format!(":root {{\n{}\n}}", variables.join("\n")));
        }
        Ok(styles.join("\n\n"))
    }
}

fn build_path(file: Option<&str>) -> ViewResult<String> {
    let file = file
        .filter(|file| {
            file.split('/').all(|segment| {
                !matches!(segment, "" | "." | "..")
                    && segment.bytes().all(|byte| {
                        byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'@')
                    })
            })
        })
        .ok_or_else(|| render_error("Invalid path in the Vite font manifest."))?;
    Ok(format!("build/{file}"))
}

fn attribute(output: &mut String, name: &str, value: &str) {
    output.push(' ');
    output.push_str(name);
    output.push_str("=\"");
    escape_html(output, value);
    output.push('"');
}
