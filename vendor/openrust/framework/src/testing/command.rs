/// The exit code and console streams captured during an Artisan invocation.
#[derive(Debug)]
pub struct TestCommand {
    pub status: i32,
    pub stdout: String,
    pub stderr: String,
}

impl TestCommand {
    #[track_caller]
    pub fn assert_exit_code(&self, expected: i32) -> &Self {
        assert_eq!(self.status, expected, "Artisan stderr: {}", self.stderr);
        self
    }

    #[track_caller]
    pub fn assert_successful(&self) -> &Self {
        self.assert_exit_code(0)
    }

    #[track_caller]
    pub fn expects_output(&self, expected: &str) -> &Self {
        assert!(
            self.stdout.lines().any(|line| line == expected),
            "expected Artisan output line {expected:?}, got {:?}",
            self.stdout
        );
        self
    }
}
