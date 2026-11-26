use std::{
    collections::BTreeMap,
    io::{ErrorKind, Read as _},
    path::{Component, Path, PathBuf},
    sync::Arc,
};

use cap_std::{ambient_authority, fs::Dir};
use saphyr::{LoadableYamlNode as _, Yaml};

use crate::{AiError, AiResult};

const MANIFEST: &str = "SKILL.md";
pub(crate) const MAX_FILE_BYTES: usize = 256 * 1024;

/// 应用提供给 Agent 的技能说明和可按需读取的配套文件。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub instructions: String,
    pub files: BTreeMap<String, String>,
    pub path: Option<PathBuf>,
}

impl Skill {
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        instructions: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            instructions: instructions.into(),
            files: BTreeMap::new(),
            path: None,
        }
    }

    pub fn from_directory(directory: impl AsRef<Path>) -> AiResult<Option<Self>> {
        let path = directory.as_ref();
        let directory = match Dir::open_ambient_dir(path, ambient_authority()) {
            Ok(directory) => directory,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(AiError::InvalidSkill),
        };
        Self::from_open_dir(path, &directory)
    }

    fn from_open_dir(path: &Path, directory: &Dir) -> AiResult<Option<Self>> {
        let document = match read_file(directory, Path::new(MANIFEST)) {
            Resource::Text(document) => document,
            Resource::Missing => return Ok(None),
            Resource::TooLarge | Resource::Binary => return Err(AiError::InvalidSkill),
        };
        let mut lines = document.split_inclusive('\n');
        let Some(first) = lines.next().filter(|line| delimiter(line)) else {
            return Ok(None);
        };
        let start = first.len();
        let mut offset = start;
        let (metadata, instructions) = loop {
            let Some(line) = lines.next() else {
                return Ok(None);
            };
            if delimiter(line) {
                break (&document[start..offset], &document[offset + line.len()..]);
            }
            offset += line.len();
        };
        let documents = Yaml::load_from_str(metadata).map_err(|_| AiError::InvalidSkill)?;
        let Some(metadata) = documents.first() else {
            return Ok(None);
        };
        let Some(description) = metadata
            .as_mapping_get("description")
            .and_then(Yaml::as_str)
            .filter(|text| !text.trim().is_empty())
        else {
            return Ok(None);
        };
        let name = metadata
            .as_mapping_get("name")
            .and_then(Yaml::as_str)
            .filter(|text| !text.trim().is_empty())
            .or_else(|| path.file_name().and_then(|name| name.to_str()))
            .ok_or(AiError::InvalidSkill)?;
        let mut skill = Self::new(name, description, instructions.trim());
        skill.path = Some(path.to_owned());
        Ok(Some(skill))
    }

    pub(crate) fn bundled_files(&self) -> Vec<String> {
        if !self.files.is_empty() {
            return self.files.keys().cloned().collect();
        }
        let mut files = Vec::new();
        if let Some(directory) = self.open_directory() {
            list_files(&directory, "", &mut files);
            files.retain(|file| file != MANIFEST);
            files.sort_unstable();
        }
        files
    }

    pub(crate) fn resource(&self, name: &str) -> Resource {
        if let Some(text) = self.files.get(name) {
            return if text.len() > MAX_FILE_BYTES {
                Resource::TooLarge
            } else {
                Resource::Text(text.clone())
            };
        }
        let path = Path::new(name);
        if name.is_empty()
            || !path
                .components()
                .all(|part| matches!(part, Component::Normal(_)))
        {
            return Resource::Missing;
        }
        self.open_directory()
            .map_or(Resource::Missing, |directory| read_file(&directory, path))
    }

    fn open_directory(&self) -> Option<Dir> {
        Dir::open_ambient_dir(self.path.as_ref()?, ambient_authority()).ok()
    }
}

/// 为技能加载工具提供内联技能、集合目录或运行时解析器。
#[derive(Clone)]
pub enum SkillSource {
    Skill(Skill),
    Directory(PathBuf),
    Resolver(Arc<dyn Fn() -> AiResult<Vec<Skill>> + Send + Sync>),
}

impl SkillSource {
    pub fn resolver(resolve: impl Fn() -> AiResult<Vec<Skill>> + Send + Sync + 'static) -> Self {
        Self::Resolver(Arc::new(resolve))
    }

    pub(crate) fn resolve(self) -> AiResult<Vec<Skill>> {
        match self {
            Self::Skill(skill) => Ok(vec![skill]),
            Self::Directory(path) => discover(&path),
            Self::Resolver(resolve) => resolve(),
        }
    }
}

impl From<Skill> for SkillSource {
    fn from(skill: Skill) -> Self {
        Self::Skill(skill)
    }
}

impl From<PathBuf> for SkillSource {
    fn from(path: PathBuf) -> Self {
        Self::Directory(path)
    }
}

fn discover(path: &Path) -> AiResult<Vec<Skill>> {
    let directory = match Dir::open_ambient_dir(path, ambient_authority()) {
        Ok(directory) => directory,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => return Err(AiError::InvalidSkill),
    };
    let mut directories = Vec::new();
    for entry in directory.entries().map_err(|_| AiError::InvalidSkill)? {
        let entry = entry.map_err(|_| AiError::InvalidSkill)?;
        if entry
            .file_type()
            .map_err(|_| AiError::InvalidSkill)?
            .is_dir()
        {
            directories.push(entry.file_name());
        }
    }
    directories.sort_unstable();
    let mut skills = Vec::new();
    for name in directories {
        let child = directory
            .open_dir(&name)
            .map_err(|_| AiError::InvalidSkill)?;
        if let Some(skill) = Skill::from_open_dir(&path.join(name), &child)? {
            skills.push(skill);
        }
    }
    Ok(skills)
}

fn delimiter(line: &str) -> bool {
    line.trim_end_matches(['\r', '\n']) == "---"
}

pub(crate) enum Resource {
    Text(String),
    Missing,
    TooLarge,
    Binary,
}

fn read_file(directory: &Dir, path: &Path) -> Resource {
    if !directory
        .metadata(path)
        .is_ok_and(|metadata| metadata.is_file())
    {
        return Resource::Missing;
    }
    let Ok(file) = directory.open(path) else {
        return Resource::Missing;
    };
    let mut bytes = Vec::new();
    if file
        .take((MAX_FILE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .is_err()
    {
        return Resource::Missing;
    }
    if bytes.len() > MAX_FILE_BYTES {
        return Resource::TooLarge;
    }
    match String::from_utf8(bytes) {
        Ok(text) => Resource::Text(text),
        Err(_) => Resource::Binary,
    }
}

fn list_files(directory: &Dir, prefix: &str, files: &mut Vec<String>) {
    let Ok(entries) = directory.entries() else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        let name = format!("{prefix}{name}");
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_file() {
            files.push(name);
        } else if kind.is_dir()
            && let Ok(child) = entry.open_dir()
        {
            list_files(&child, &format!("{name}/"), files);
        }
    }
}
