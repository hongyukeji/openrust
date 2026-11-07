//! 本次受管 dispatch 的准入与回程许可。
use crate::container::Scope;
use axum::extract::Request;
use axum::response::Response;
use std::sync::{Arc, Mutex, MutexGuard, Weak};

#[derive(Default)]
struct Gate {
    active: usize,
    sealed: bool,
    failed: bool,
}

struct Owner {
    scope: Option<Weak<Scope>>,
    #[cfg(feature = "session")]
    cookie: Option<Arc<crate::http::cookie::CookieLayerPlan>>,
    // 同次派发的所有祖先闸共用事务锁。锁内只访问计数和状态。
    commit: Mutex<()>,
}

#[derive(Clone)]
pub(crate) struct Invocation {
    owner: Arc<Owner>,
    gate: Arc<Mutex<Gate>>,
    ancestors: Vec<Arc<Mutex<Gate>>>,
}

fn lock<T>(lock: &Mutex<T>) -> MutexGuard<'_, T> {
    lock.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl Invocation {
    pub(super) fn new(
        #[cfg(feature = "session")] cookie: Option<Arc<crate::http::cookie::CookieLayerPlan>>,
    ) -> (Self, Retire) {
        let invocation = Self {
            owner: Arc::new(Owner {
                scope: crate::foundation::http::http_entry_scope(),
                #[cfg(feature = "session")]
                cookie,
                commit: Mutex::new(()),
            }),
            gate: Arc::default(),
            ancestors: Vec::new(),
        };
        let retire = Retire(invocation.clone());
        (invocation, retire)
    }

    pub(super) fn child(&self) -> (Self, Retire) {
        let mut ancestors = self.ancestors.clone();
        ancestors.push(Arc::clone(&self.gate));
        let invocation = Self {
            owner: Arc::clone(&self.owner),
            gate: Arc::default(),
            ancestors,
        };
        let retire = Retire(invocation.clone());
        (invocation, retire)
    }

    fn gates(&self) -> impl Iterator<Item = &Mutex<Gate>> {
        self.ancestors
            .iter()
            .chain(std::iter::once(&self.gate))
            .map(Arc::as_ref)
    }

    pub(super) fn admit(&self) -> Option<Admission> {
        let _commit = lock(&self.owner.commit);
        if self.gates().any(|gate| {
            let gate = lock(gate);
            gate.sealed || gate.active == usize::MAX
        }) {
            return None;
        }
        for gate in self.gates() {
            lock(gate).active += 1;
        }
        Some(Admission {
            invocation: self.clone(),
            completed: false,
        })
    }

    pub(crate) fn fail(&self) {
        let _commit = lock(&self.owner.commit);
        for gate in self.gates() {
            let mut gate = lock(gate);
            if !gate.sealed {
                gate.failed = true;
            }
        }
    }

    pub(crate) fn seal(&self) -> bool {
        let _commit = lock(&self.owner.commit);
        let ancestors_open = self.ancestors.iter().all(|gate| !lock(gate).sealed);
        let mut gate = lock(&self.gate);
        let permitted = ancestors_open && !gate.sealed && gate.active == 0 && !gate.failed;
        gate.sealed = true;
        permitted
    }

    #[cfg(feature = "session")]
    pub(crate) fn protected_cookie(&self) -> Option<&Arc<crate::http::cookie::CookieLayerPlan>> {
        self.owner.cookie.as_ref()
    }

    #[cfg(feature = "session")]
    pub(crate) fn owns_scope(&self, request: &Request) -> bool {
        self.request_scope(request).is_some()
    }

    pub(crate) fn request_scope(&self, request: &Request) -> Option<Arc<Scope>> {
        let expected = self.owner.scope.as_ref()?.upgrade()?;
        let actual = request.extensions().get::<Arc<Scope>>()?;
        Arc::ptr_eq(&expected, actual).then_some(expected)
    }
}

pub(super) struct Retire(Invocation);
impl Drop for Retire {
    fn drop(&mut self) {
        self.0.seal();
    }
}

pub(super) struct Admission {
    invocation: Invocation,
    completed: bool,
}

impl Admission {
    pub(super) fn complete(mut self) {
        self.completed = true;
    }
}

impl Drop for Admission {
    fn drop(&mut self) {
        let _commit = lock(&self.invocation.owner.commit);
        for gate in self.invocation.gates() {
            let mut gate = lock(gate);
            gate.active -= 1;
            if !self.completed && !gate.sealed {
                gate.failed = true;
            }
        }
    }
}

pub(crate) fn suppress_cookies(mut response: Response) -> Response {
    response.headers_mut().remove(http::header::SET_COOKIE);
    response
}
