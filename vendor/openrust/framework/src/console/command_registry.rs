//! Provider 生命周期共享的命令所有权仓储。

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use crate::console::Command;

#[derive(Default)]
struct State {
    revision: Arc<()>,
    entries: Vec<Arc<dyn Command>>,
    reserved: HashSet<&'static str>,
}

/// 同名覆盖且保留首次注册顺序的共享命令仓储。
#[derive(Clone, Default)]
pub(crate) struct CommandRegistry {
    state: Arc<Mutex<State>>,
}

impl CommandRegistry {
    /// 注册命令；被替换实例在释放仓储锁后析构。
    pub(crate) fn register(&self, command: impl Command) {
        self.install(command, false);
    }

    /// 在同一次注册操作中保留命令名，首个保留者生效。
    pub(crate) fn reserve(&self, command: impl Command) {
        self.install(command, true);
    }

    fn install(&self, command: impl Command, reserve: bool) {
        // name 和 Drop 都可能进入用户代码，不能在持锁期间调用。
        let name = command.name();
        let command: Arc<dyn Command> = Arc::new(command);
        loop {
            let (revision, entries) = {
                let state = self
                    .state
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if state.reserved.contains(name) {
                    return;
                }
                (Arc::clone(&state.revision), state.entries.clone())
            };
            // 名称允许依赖实例状态；回调也可能注册命令，因此提交前核对版本。
            let position = entries.iter().position(|entry| entry.name() == name);
            let replaced = {
                let mut state = self
                    .state
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if !Arc::ptr_eq(&revision, &state.revision) {
                    // continue 会先释放本作用域的锁，再析构外层快照。
                    continue;
                }
                let replaced = if let Some(position) = position {
                    Some(std::mem::replace(
                        &mut state.entries[position],
                        Arc::clone(&command),
                    ))
                } else {
                    state.entries.push(Arc::clone(&command));
                    None
                };
                if reserve {
                    state.reserved.insert(name);
                }
                state.revision = Arc::new(());
                replaced
            };
            drop(replaced);
            break;
        }
    }

    /// 按首次注册顺序取得拥有独立生命周期的命令快照。
    pub(crate) fn snapshot(&self) -> Vec<Arc<dyn Command>> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .entries
            .clone()
    }

    /// 撤销所有命令登记，把旧对象交给调用方在锁外释放。
    pub(crate) fn take_entries(&self) -> Vec<Arc<dyn Command>> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.revision = Arc::new(());
        state.reserved.clear();
        std::mem::take(&mut state.entries)
    }
}

#[cfg(test)]
#[path = "../../tests/unit/console/command_registry_reservations.rs"]
mod reservation_tests;

#[cfg(test)]
mod tests {
    use std::{
        sync::{
            Arc, Mutex,
            atomic::{AtomicBool, AtomicUsize, Ordering},
            mpsc,
        },
        thread,
        time::Duration,
    };

    use super::CommandRegistry;
    use crate::console::Command;
    use crate::core::Result;
    use crate::foundation::Application;
    use crate::support::BoxFuture;

    struct TestCommand {
        name: &'static str,
        instance: &'static str,
        on_drop: Option<(CommandRegistry, mpsc::Sender<usize>)>,
        renamed: Option<Arc<AtomicBool>>,
        on_name: Option<Arc<Mutex<Option<CommandRegistry>>>>,
        name_calls: Option<Arc<AtomicUsize>>,
    }

    impl TestCommand {
        fn new(name: &'static str, instance: &'static str) -> Self {
            Self {
                name,
                instance,
                on_drop: None,
                renamed: None,
                on_name: None,
                name_calls: None,
            }
        }
    }

    impl Command for TestCommand {
        fn name(&self) -> &'static str {
            if let Some(calls) = &self.name_calls {
                calls.fetch_add(1, Ordering::SeqCst);
            }
            if let Some(on_name) = &self.on_name {
                let registry = on_name
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .take();
                if let Some(registry) = registry {
                    registry.register(TestCommand::new("nested", "nested"));
                }
            }
            if self
                .renamed
                .as_ref()
                .is_some_and(|renamed| renamed.load(Ordering::SeqCst))
            {
                return "renamed";
            }
            self.name
        }

        fn about(&self) -> &'static str {
            self.instance
        }

        fn run<'a>(
            &'a self,
            _app: &'a Application,
            _args: Vec<String>,
        ) -> BoxFuture<'a, Result<()>> {
            Box::pin(async { Ok(()) })
        }
    }

    impl Drop for TestCommand {
        fn drop(&mut self) {
            if let Some((registry, sender)) = self.on_drop.take() {
                let _ = sender.send(registry.snapshot().len());
            }
        }
    }

    #[test]
    fn clones_share_registration_and_replacement_preserves_first_slot() {
        let registry = CommandRegistry::default();
        let shared = registry.clone();
        registry.register(TestCommand::new("first", "original"));
        shared.register(TestCommand::new("second", "second"));
        shared.register(TestCommand::new("first", "replacement"));

        let snapshot = registry.snapshot();
        assert_eq!(snapshot.len(), 2);
        assert_eq!(snapshot[0].name(), "first");
        assert_eq!(snapshot[0].about(), "replacement");
        assert_eq!(snapshot[1].name(), "second");
    }

    #[test]
    fn replacement_keeps_existing_snapshot_instance_alive() {
        let registry = CommandRegistry::default();
        registry.register(TestCommand::new("task", "original"));
        let original = registry.snapshot();
        registry.register(TestCommand::new("task", "replacement"));

        assert_eq!(original[0].about(), "original");
        assert_eq!(registry.snapshot()[0].about(), "replacement");
    }

    #[test]
    fn replaced_command_can_reenter_registry_during_drop() {
        let registry = CommandRegistry::default();
        let (sender, receiver) = mpsc::channel();
        let mut original = TestCommand::new("task", "original");
        original.on_drop = Some((registry.clone(), sender));
        registry.register(original);

        let worker = thread::spawn(move || {
            registry.register(TestCommand::new("task", "replacement"));
        });
        // 回归时只让工作线程阻塞，测试线程有界失败，避免挂住整组验收。
        assert_eq!(receiver.recv_timeout(Duration::from_secs(5)), Ok(1));
        assert!(worker.join().is_ok());
    }

    #[test]
    fn renamed_command_is_replaced_using_its_current_name() {
        let registry = CommandRegistry::default();
        let renamed = Arc::new(AtomicBool::new(false));
        let mut original = TestCommand::new("original", "original");
        original.renamed = Some(Arc::clone(&renamed));
        registry.register(original);
        registry.register(TestCommand::new("second", "second"));
        renamed.store(true, Ordering::SeqCst);
        registry.register(TestCommand::new("renamed", "replacement"));

        let snapshot = registry.snapshot();
        assert_eq!(snapshot.len(), 2);
        assert_eq!(snapshot[0].about(), "replacement");
        assert_eq!(snapshot[1].name(), "second");
    }

    #[test]
    fn former_name_does_not_replace_a_renamed_command() {
        let registry = CommandRegistry::default();
        let renamed = Arc::new(AtomicBool::new(false));
        let mut original = TestCommand::new("original", "original");
        original.renamed = Some(Arc::clone(&renamed));
        registry.register(original);
        renamed.store(true, Ordering::SeqCst);
        registry.register(TestCommand::new("original", "new"));

        let snapshot = registry.snapshot();
        assert_eq!(snapshot.len(), 2);
        assert_eq!(snapshot[0].name(), "renamed");
        assert_eq!(snapshot[1].about(), "new");
    }

    #[test]
    fn name_callback_registration_retries_after_revision_changes() {
        let registry = CommandRegistry::default();
        let on_name = Arc::new(Mutex::new(None));
        let calls = Arc::new(AtomicUsize::new(0));
        let mut original = TestCommand::new("task", "original");
        original.on_name = Some(Arc::clone(&on_name));
        original.name_calls = Some(Arc::clone(&calls));
        registry.register(original);
        calls.store(0, Ordering::SeqCst);
        *on_name
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(registry.clone());
        let (sender, receiver) = mpsc::channel();
        let worker = thread::spawn(move || {
            registry.register(TestCommand::new("task", "replacement"));
            let snapshot = registry.snapshot();
            let descriptions: Vec<_> = snapshot.iter().map(|command| command.about()).collect();
            let _ = sender.send(descriptions);
        });

        assert_eq!(
            receiver.recv_timeout(Duration::from_secs(5)),
            Ok(vec!["replacement", "nested"])
        );
        assert!(worker.join().is_ok());
        // 外层扫描、重入扫描、版本漂移后的重新扫描各调用一次。
        assert_eq!(calls.load(Ordering::SeqCst), 3);
    }
}
