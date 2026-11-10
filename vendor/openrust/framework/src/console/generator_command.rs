use std::{borrow::Cow, path::PathBuf};

use crate::foundation::Application;

use super::{Command, ConsoleError, Input};

/// Supplies the primary destination and stub for a file generator.
pub trait GeneratorCommand: Command {
    fn get_path(
        &self,
        app: &Application,
        name: &str,
        input: &Input,
    ) -> Result<PathBuf, ConsoleError>;

    fn get_stub(&self, app: &Application, input: &Input)
    -> Result<Cow<'static, str>, ConsoleError>;
}
