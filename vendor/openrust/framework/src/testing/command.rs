/// The exit code and console streams captured during an Artisan invocation.
#[derive(Debug)]
pub struct TestCommand {
    pub status: i32,
    pub stdout: String,
    pub stderr: String,
}

#[allow(
    non_snake_case,
    reason = "Matches Laravel's console testing assertions"
)]
impl TestCommand {
    #[track_caller]
    pub fn assertExitCode(&self, expected: i32) -> &Self {
        assert_eq!(self.status, expected, "Artisan stderr: {}", self.stderr);
        self
    }

    #[track_caller]
    pub fn assertSuccessful(&self) -> &Self {
        self.assertExitCode(0)
    }

    #[track_caller]
    pub fn expectsOutput(&self, expected: &str) -> &Self {
        assert!(
            self.stdout.lines().any(|line| line == expected),
            "expected Artisan output line {expected:?}, got {:?}",
            self.stdout
        );
        self
    }
}
