use std::{borrow::Cow, collections::BTreeMap, fmt::Write as _, sync::Arc};

use openrust::{
    contracts::foundation::Application as _,
    http::HttpFuture,
    json_schema::{JsonSchemaTypeFactory, types::ObjectType},
    support::facades::{App, Log},
};
use serde::Deserialize;

use crate::{
    Agent, AiError, AiResult, Skill, SkillSource, Tool, ToolRequest, localization, skill::Resource,
};

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

    pub fn from_sources(sources: impl IntoIterator<Item = SkillSource>) -> AiResult<Self> {
        let mut sources = sources.into_iter().peekable();
        if sources.peek().is_none() {
            let application = App::get_instance().map_err(|_| AiError::Unavailable)?;
            let source = SkillSource::Directory(application.base_path().join("resources/skills"));
            return Ok(Self::new(source.resolve()?));
        }
        let mut skills = Vec::new();
        for source in sources {
            skills.extend(source.resolve()?);
        }
        Ok(Self::new(skills))
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

    fn as_load_skill(&self) -> Option<&Self> {
        Some(self)
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

pub(crate) fn tools_for(agent: &impl Agent) -> AiResult<Vec<Arc<dyn Tool>>> {
    let mut tools = agent.tools();
    let sources = agent.skills();
    if sources.is_empty() {
        return Ok(tools);
    }
    let mut loader = LoadSkill::from_sources(sources)?;
    if let Some((slot, existing)) = tools
        .iter()
        .enumerate()
        .find_map(|(slot, tool)| tool.as_load_skill().map(|loader| (slot, loader)))
    {
        let mut skills = existing.skills.clone();
        for (name, skill) in loader.skills {
            skills.entry(name).or_insert(skill);
        }
        loader.skills = skills;
        tools[slot] = Arc::new(loader);
        let _ = Log::warning(
            "已将 Agent 声明的技能合并到现有技能加载工具",
            &Default::default(),
        );
    } else {
        tools.push(Arc::new(loader));
    }
    Ok(tools)
}

fn text(key: &str, fallback: &str) -> String {
    localization::text(
        &format!("ai::tools.load_skill.{key}"),
        &Default::default(),
        fallback,
    )
}
