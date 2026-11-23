use std::sync::Arc;

use super::{Command, Error, FakeProcessDescription, Result};

/// The exit status and captured output of a completed process.
#[derive(Clone, Debug)]
pub struct ProcessResult {
    pub(super) command: Command,
    pub(super) exit_code: i32,
    pub(super) stdout: Vec<u8>,
    pub(super) stderr: Vec<u8>,
    pub(super) description: Option<Arc<FakeProcessDescription>>,
}

#[allow(non_snake_case, reason = "保留既定公开接口及命名")]
impl ProcessResult {
    pub fn command(&self) -> &Command {
        &self.command
    }

    pub fn exit_code(&self) -> i32 {
        self.exit_code
    }

    pub fn successful(&self) -> bool {
        self.exit_code == 0
    }

    pub fn failed(&self) -> bool {
        !self.successful()
    }

    pub fn output(&self) -> &[u8] {
        &self.stdout
    }

    pub fn errorOutput(&self) -> &[u8] {
        &self.stderr
    }

    pub fn seeInOutput(&self, needle: &str) -> bool {
        String::from_utf8_lossy(self.output()).contains(needle)
    }

    pub fn seeInErrorOutput(&self, needle: &str) -> bool {
        String::from_utf8_lossy(self.errorOutput()).contains(needle)
    }

    pub fn throw(self) -> Result<Self> {
        if self.failed() {
            return Err(Error::Failed(Box::new(self)));
        }
        Ok(self)
    }

    pub fn throw_if(self, condition: bool) -> Result<Self> {
        if condition { self.throw() } else { Ok(self) }
    }
}
