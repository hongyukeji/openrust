use std::{borrow::Cow, collections::BTreeMap, fmt::Write as _};

use openrust::{
    http::HttpFuture,
    json_schema::{JsonSchemaTypeFactory, types::ObjectType},
};
use serde::Deserialize;

use crate::{AiResult, Skill, Tool, ToolRequest, localization, skill::Resource};

const DESCRIPTION: &str = "匹配任务时加载技能说明，也可读取指定的配套文件。";

/// 公开技能目录，并在模型请求时加载说明或配套文件。
pub struct LoadSkill {
    skills: BTreeMap<String, Skill>,
}

impl LoadSkill {
    pub fn new(skills: impl IntoIterator<Item = Skill>) -> Self {
        let mut sources = BTreeMap::new();
        for skill in skills {
            sources.entry(skill.name.clone()).or_insert(skill);
        }
        Self { skills: sources }
    }

    fn instructions(&self, skill: &Skill) -> String {
        let name = skill
            .name
            .replace('&', "&amp;")
            .replace('"', "&quot;")
            .replace('<', "&lt;");
        let mut contents = format!("<skill_content name=\"{name}\">\n{}", skill.instructions);
        let files = skill.bundled_files();
        if !files.is_empty() {
            let hint = text(
                "read_hint",
                "如需读取配套文件，请使用技能名称与对应文件路径调用本工具。",
            );
            let _ = write!(
                contents,
                "\n\n<skill_resources>\n{}\n</skill_resources>\n{hint}",
                files.join("\n")
            );
        }
        contents.push_str("\n</skill_content>");
        contents
    }
}

#[derive(Deserialize)]
struct Input {
    name: String,
    path: Option<String>,
}

impl Tool for LoadSkill {
    fn name(&self) -> &str {
        "load_skill"
    }

    fn description(&self) -> &str {
        DESCRIPTION
    }

    fn localized_description(&self) -> Cow<'_, str> {
        let mut description = text("description", DESCRIPTION);
        if !self.skills.is_empty() {
            let _ = write!(description, "\n\n{}", text("available", "可用技能"));
        }
        for skill in self.skills.values() {
            let summary = skill
                .description
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            let _ = write!(description, "\n- {}: {summary}", skill.name);
        }
        Cow::Owned(description)
    }

    fn schema(&self, schema: JsonSchemaTypeFactory) -> ObjectType {
        let mut name = schema
            .string()
            .description(text("name", "要加载的技能名称。"))
            .required(true);
        if !self.skills.is_empty() {
            name = name.r#enum(self.skills.keys().cloned().map(serde_json::Value::String));
        }
        schema.object([
            ("name", name.into()),
            (
                "path",
                schema
                    .string()
                    .description(text("path", "可选的配套文件相对路径。"))
                    .into(),
            ),
        ])
    }

    fn handle<'a>(&'a self, request: ToolRequest<'a>) -> HttpFuture<'a, AiResult<String>> {
        Box::pin(async move {
            let Input { name, path } = request.decode()?;
            let Some(skill) = self.skills.get(&name) else {
                return Ok(localization::text(
                    "ai::tools.load_skill.missing_skill",
                    &[("name".into(), name.clone())].into(),
                    &format!("技能 [{name}] 不存在。"),
                ));
            };
            let Some(path) = path.filter(|path| !path.trim().is_empty()) else {
                return Ok(self.instructions(skill));
            };
            let (key, fallback) = match skill.resource(&path) {
                Resource::Text(text) => return Ok(text),
                Resource::TooLarge => (
                    "file_too_large",
                    format!("文件 [{path}] 太大，无法内联读取。"),
                ),
                Resource::Binary => ("file_binary", format!("文件 [{path}] 不是 UTF-8 文本。")),
                Resource::Missing => (
                    "missing_file",
                    format!("技能 [{name}] 未提供文件 [{path}]。"),
                ),
            };
            Ok(localization::text(
                &format!("ai::tools.load_skill.{key}"),
                &[("name".into(), name), ("path".into(), path)].into(),
                &fallback,
            ))
        })
    }
}

fn text(key: &str, fallback: &str) -> String {
    localization::text(
        &format!("ai::tools.load_skill.{key}"),
        &Default::default(),
        fallback,
    )
}
