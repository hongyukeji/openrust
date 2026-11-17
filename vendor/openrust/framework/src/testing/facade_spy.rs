use std::sync::{Arc, Mutex, PoisonError};

struct Call<A> {
    method: &'static str,
    arguments: Arc<A>,
}

/// A call recorder shared between a typed facade replacement and the test.
pub struct FacadeSpy<A> {
    calls: Arc<Mutex<Vec<Call<A>>>>,
}

impl<A> Clone for FacadeSpy<A> {
    fn clone(&self) -> Self {
        Self {
            calls: self.calls.clone(),
        }
    }
}

#[allow(non_snake_case, reason = "Matches Laravel's facade spy assertions")]
impl<A> FacadeSpy<A> {
    pub(crate) fn new() -> Self {
        Self {
            calls: Arc::default(),
        }
    }

    /// Records a call from the replacement's service implementation.
    pub fn record(&self, method: &'static str, arguments: A) {
        self.calls
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(Call {
                method,
                arguments: Arc::new(arguments),
            });
    }

    /// Returns the recorded arguments for a method in invocation order.
    pub fn received(&self, method: &str) -> Vec<Arc<A>> {
        self.calls
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .filter(|call| call.method == method)
            .map(|call| call.arguments.clone())
            .collect()
    }

    /// Asserts a call was received and returns it for argument and count assertions.
    #[track_caller]
    pub fn shouldHaveReceived(&self, method: &str) -> ReceivedCalls<A> {
        let calls = ReceivedCalls {
            method: method.to_owned(),
            arguments: self.received(method),
        };
        calls.assert_received();
        calls
    }

    /// Asserts that a method was never called.
    #[track_caller]
    pub fn shouldNotHaveReceived(&self, method: &str) {
        self.shouldNotHaveReceivedWhere(method, |_| true);
    }

    /// Asserts that a method was never called with matching arguments.
    #[track_caller]
    pub fn shouldNotHaveReceivedWhere(&self, method: &str, mut matches: impl FnMut(&A) -> bool) {
        assert!(
            !self
                .received(method)
                .iter()
                .any(|arguments| matches(arguments)),
            "Facade spy unexpectedly received [{method}] with matching arguments."
        );
    }
}

/// A snapshot of calls for further facade spy assertions.
pub struct ReceivedCalls<A> {
    method: String,
    arguments: Vec<Arc<A>>,
}

#[allow(non_snake_case, reason = "Matches Mockery's facade spy assertions")]
impl<A> ReceivedCalls<A> {
    /// Retains only calls with the given arguments and asserts a match exists.
    #[track_caller]
    pub fn with(self, arguments: A) -> Self
    where
        A: PartialEq,
    {
        self.withArgs(|received| received == &arguments)
    }

    /// Retains only calls whose arguments satisfy the predicate and asserts a match exists.
    #[track_caller]
    pub fn withArgs(mut self, mut matches: impl FnMut(&A) -> bool) -> Self {
        self.arguments.retain(|arguments| matches(arguments));
        self.assert_received();
        self
    }

    /// Asserts that the method received exactly the given number of matching calls.
    #[track_caller]
    pub fn times(self, count: usize) {
        assert_eq!(
            self.arguments.len(),
            count,
            "Facade spy invocation count for [{}].",
            self.method
        );
    }

    /// Asserts that the method received exactly one matching call.
    #[track_caller]
    pub fn once(self) {
        self.times(1);
    }

    #[track_caller]
    fn assert_received(&self) {
        assert!(
            !self.arguments.is_empty(),
            "Facade spy did not receive [{}] with matching arguments.",
            self.method
        );
    }
}
