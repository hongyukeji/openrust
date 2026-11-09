//! 供应用和源码工具共用的 Artisan 参数分解。

/// 一次已分解的应用控制台调用。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConsoleInvocation {
    name: String,
    arguments: Vec<String>,
    environment: Option<String>,
}

impl ConsoleInvocation {
    /// 分解全局参数及命令名称，保留命令自身的参数。
    ///
    /// 环境值仅用于选择 dotenv 文件。默认命令由调用入口决定。
    #[must_use]
    pub fn parse(args: impl IntoIterator<Item = String>, default_command: &str) -> Self {
        let mut command = Vec::new();
        let mut environment = None;
        let mut no_interaction = false;
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            if arg == "--env" {
                if let Some(value) = args.next()
                    && environment.is_none()
                {
                    environment = Some(value);
                }
            } else if let Some(value) = arg.strip_prefix("--env=") {
                if environment.is_none() {
                    environment = Some(value.to_owned());
                }
            } else if command.is_empty() && matches!(arg.as_str(), "--no-interaction" | "-n") {
                no_interaction = true;
            } else {
                command.push(arg);
            }
        }
        let mut command = command.into_iter();
        let name = match command.next() {
            Some(name) if matches!(name.as_str(), "--help" | "-h") => "help".to_owned(),
            Some(name) => name,
            None => default_command.to_owned(),
        };
        let mut arguments: Vec<_> = command.collect();
        if no_interaction && matches!(name.as_str(), "migrate" | "key:generate") {
            arguments.push("--no-interaction".to_owned());
        }
        Self {
            name,
            arguments,
            environment,
        }
    }

    /// 控制台命令的第一段名称。
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// 剩余命令参数，可能包含命令名后续段。
    #[must_use]
    pub fn arguments(&self) -> &[String] {
        &self.arguments
    }

    /// 显式选择环境文件时传入的环境名称。
    #[must_use]
    pub fn environment(&self) -> Option<&str> {
        self.environment.as_deref()
    }

    pub(crate) fn into_parts(self) -> (String, Vec<String>, Option<String>) {
        (self.name, self.arguments, self.environment)
    }
}
