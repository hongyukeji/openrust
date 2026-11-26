use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

use crate::console::{CommandIo, ConsoleError, Input, generator_command::snake_case};

use super::name_parts;

pub(super) struct ControllerTypes<'a> {
    pub(super) model: Option<ModelType<'a>>,
    pub(super) parent: Option<ModelType<'a>>,
    pub(super) requests: Option<FormRequests>,
}

pub(super) struct ModelType<'a> {
    argument: &'a str,
    class: &'a str,
    path: PathBuf,
    ty: String,
    parameter: String,
}

pub(super) struct FormRequests {
    store: String,
    update: String,
}

impl<'a> ControllerTypes<'a> {
    pub(super) fn parse(input: &'a Input) -> Result<Self, ConsoleError> {
        let model = selected_model(input, "model")?;
        let parent = selected_model(input, "parent")?;
        let requests = model
            .as_ref()
            .filter(|_| input.matches().get_flag("requests"))
            .map(|model| FormRequests {
                store: format!("Store{}Request", model.class),
                update: format!("Update{}Request", model.class),
            });
        Ok(Self {
            model,
            parent,
            requests,
        })
    }

    pub(super) async fn generate_related(
        &self,
        root: &Path,
        io: &mut CommandIo<'_>,
    ) -> Result<(), ConsoleError> {
        if let Some(requests) = &self.requests {
            for name in [&requests.store, &requests.update] {
                let target = root
                    .join("app/http/requests")
                    .join(snake_case(name))
                    .with_extension("rs");
                super::check_target(root, &target).map_err(ConsoleError::from_source)?;
            }
        }
        for model in [&self.parent, &self.model].into_iter().flatten() {
            if !root.join(&model.path).is_file()
                && io
                    .confirm_with(&format!("模型 {} 不存在，是否生成？", model.class), true)
                    .await?
            {
                let path = root.join(&model.path);
                super::check_target(root, &path).map_err(ConsoleError::from_source)?;
                generate(io, "make:model", model.argument).await?;
            }
        }
        if let Some(requests) = &self.requests {
            for name in [&requests.store, &requests.update] {
                generate(io, "make:request", name).await?;
            }
        }
        Ok(())
    }

    pub(super) fn validated_request(&self, action: &str) -> String {
        let Some(class) = self
            .requests
            .as_ref()
            .and_then(|requests| requests.for_action(action))
        else {
            return String::new();
        };
        format!(
            "        _form: openrust::foundation::http::ValidatedFormRequest<{}>,\n",
            FormRequests::ty(class)
        )
    }

    pub(super) fn bindings(&self, action: &str, singleton: bool) -> String {
        let mut lines = String::new();
        if let Some(parent) = &self.parent {
            lines.push_str(&parent.binding("parent"));
        }
        if !singleton
            && matches!(action, "show" | "edit" | "update" | "destroy")
            && let Some(model) = &self.model
        {
            lines.push_str(&model.binding("model"));
        }
        lines
    }

    pub(super) fn replace_placeholders(&self, mut source: String) -> String {
        for (placeholder, model) in [("model", &self.model), ("parent_model", &self.parent)] {
            if let Some(model) = model {
                source = source.replace(&format!("{{{{{placeholder}}}}}"), &model.ty);
            }
        }
        if let Some(requests) = &self.requests {
            source = source
                .replace("{{store_request}}", &FormRequests::ty(&requests.store))
                .replace("{{update_request}}", &FormRequests::ty(&requests.update));
        }
        source
    }
}

impl ModelType<'_> {
    fn binding(&self, variable: &str) -> String {
        format!(
            "            let _{variable} = request.route_model::<{}>({:?}).ok_or(HttpError::NotFound)?;\n",
            self.ty, self.parameter
        )
    }
}

impl FormRequests {
    fn for_action(&self, action: &str) -> Option<&str> {
        match action {
            "store" => Some(&self.store),
            "update" => Some(&self.update),
            _ => None,
        }
    }

    fn ty(class: &str) -> String {
        format!("crate::app::http::requests::{}::{class}", snake_case(class))
    }
}

fn selected_model<'a>(
    input: &'a Input,
    option: &str,
) -> Result<Option<ModelType<'a>>, ConsoleError> {
    let Some(raw) = input
        .matches()
        .get_one::<String>(option)
        .filter(|x| !x.is_empty())
    else {
        return Ok(None);
    };
    let raw = raw.trim_start_matches(['/', '\\']);
    let argument = raw
        .strip_prefix("App/Models/")
        .or_else(|| raw.strip_prefix("App\\Models\\"))
        .unwrap_or(raw);
    let parts = name_parts(argument)?.collect::<Vec<_>>();
    let class = parts.last().copied().expect("名称至少包含类型");
    let modules = parts.into_iter().map(snake_case).collect::<Vec<_>>();
    let mut path = PathBuf::from("app/models");
    path.extend(&modules);
    let ty = format!("crate::app::models::{}::{class}", modules.join("::"));
    Ok(Some(ModelType {
        argument,
        class,
        path: path.with_extension("rs"),
        ty,
        parameter: snake_case(class),
    }))
}

async fn generate(io: &mut CommandIo<'_>, command: &str, name: &str) -> Result<(), ConsoleError> {
    let status = io.call(command, &[OsString::from(name)]).await?;
    if status == 0 {
        Ok(())
    } else {
        Err(ConsoleError::new(format!(
            "命令 {command} 为 {name} 生成源码失败，退出码 {status}。"
        )))
    }
}
