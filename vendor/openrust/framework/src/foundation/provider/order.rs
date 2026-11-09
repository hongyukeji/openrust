//! 在注册前校验 Provider 依赖，并在包段内稳定排序。

use std::any::TypeId;
use std::collections::{HashMap, HashSet};

use super::{Error, ProviderEntry, ProviderId, Result};

pub(super) fn prepare(
    entries: Vec<ProviderEntry>,
    removed: &HashSet<ProviderId>,
) -> Result<Vec<ProviderEntry>> {
    check_package_origins(&entries, removed)?;
    let mut seen = HashSet::new();
    let mut order = Order {
        all: entries
            .iter()
            .filter(|entry| {
                let id = entry.provider.id();
                let first = seen.insert(id.type_id);
                !removed.contains(&id) && (first || entry.package.is_none())
            })
            .flat_map(identities)
            .filter(|id| !removed.contains(id))
            .map(|id| id.type_id)
            .collect(),
        ready: HashSet::new(),
        conditional: HashSet::new(),
        removed,
    };
    let mut sorted = Vec::with_capacity(entries.len());
    let mut entries = entries.into_iter().peekable();
    while let Some(entry) = entries.next() {
        if entry.package.is_some() {
            let mut packages = vec![entry];
            packages.extend(std::iter::from_fn(|| {
                entries.next_if(|next| next.package.is_some())
            }));
            order.packages(packages, &mut sorted)?;
        } else {
            order.advance(&entry)?;
            sorted.push(entry);
        }
    }
    Ok(sorted)
}

fn check_package_origins(entries: &[ProviderEntry], removed: &HashSet<ProviderId>) -> Result<()> {
    let mut sources = HashMap::new();
    for entry in entries {
        let Some(source) = entry.package else {
            continue;
        };
        let id = entry.provider.id();
        if !removed.contains(&id)
            && let Some(first) = sources.insert(id.type_id, source)
            && first != source
        {
            return Err(Error::custom(
                "package_provider_conflict",
                format!(
                    "Provider {} 同时由包 `{first}` 和 `{source}` 声明",
                    id.name()
                ),
            ));
        }
    }
    Ok(())
}

fn definite_identities(entry: &ProviderEntry) -> impl Iterator<Item = ProviderId> {
    std::iter::once(entry.provider.id()).chain(entry.provider.nested_providers())
}

fn identities(entry: &ProviderEntry) -> impl Iterator<Item = ProviderId> {
    definite_identities(entry).chain(entry.provider.possible_nested_providers())
}

fn first_instances(entries: &[ProviderEntry]) -> impl Iterator<Item = (usize, &ProviderEntry)> {
    let mut seen = HashSet::new();
    entries
        .iter()
        .enumerate()
        .filter(move |(_, entry)| seen.insert(entry.provider.id().type_id))
}

struct Order<'a> {
    all: HashSet<TypeId>,
    ready: HashSet<TypeId>,
    conditional: HashSet<TypeId>,
    removed: &'a HashSet<ProviderId>,
}

impl Order<'_> {
    fn skips(&self, entry: &ProviderEntry) -> bool {
        let id = entry.provider.id();
        self.removed.contains(&id) || (!entry.force && self.ready.contains(&id.type_id))
    }

    fn available(&self, id: ProviderId) -> bool {
        self.ready.contains(&id.type_id) || self.conditional.contains(&id.type_id)
    }

    fn advance(&mut self, entry: &ProviderEntry) -> Result<()> {
        if self.skips(entry) {
            return Ok(());
        }
        for dependency in entry.provider.dependencies() {
            if !self.all.contains(&dependency.type_id) || !self.available(dependency) {
                return Err(self.dependency_error(entry, dependency));
            }
        }
        self.ready.extend(
            entry
                .provider
                .nested_providers()
                .into_iter()
                .filter(|id| !self.removed.contains(id))
                .map(|id| id.type_id),
        );
        self.ready.insert(entry.provider.id().type_id);
        self.conditional.extend(
            entry
                .provider
                .possible_nested_providers()
                .into_iter()
                .filter(|id| !self.removed.contains(id))
                .map(|id| id.type_id),
        );
        Ok(())
    }

    fn packages(
        &mut self,
        mut pending: Vec<ProviderEntry>,
        sorted: &mut Vec<ProviderEntry>,
    ) -> Result<()> {
        while !pending.is_empty() {
            let definite: HashSet<_> = first_instances(&pending)
                .map(|(_, entry)| entry)
                .filter(|entry| !self.skips(entry))
                .flat_map(definite_identities)
                .filter(|id| !self.removed.contains(id))
                .map(|id| id.type_id)
                .collect();
            // 优先等候确定会登记的依赖。可能嵌套的声明仍由实际注册器核验。
            let next = self
                .next_package(&pending, |id| {
                    self.ready.contains(&id.type_id)
                        || (self.available(id) && !definite.contains(&id.type_id))
                })
                .or_else(|| self.next_package(&pending, |id| self.available(id)));
            let Some(index) = next else {
                return Err(self.blocked_packages(&pending));
            };
            let entry = pending.remove(index);
            self.advance(&entry)?;
            sorted.push(entry);
        }
        Ok(())
    }

    fn next_package(
        &self,
        pending: &[ProviderEntry],
        available: impl Fn(ProviderId) -> bool,
    ) -> Option<usize> {
        // 同一个包重复提供同一类型时，第一项仍决定登记顺序。
        first_instances(pending)
            .find(|(_, entry)| {
                self.skips(entry) || entry.provider.dependencies().into_iter().all(&available)
            })
            .map(|(index, _)| index)
    }

    fn blocked_packages(&self, pending: &[ProviderEntry]) -> Error {
        let first: Vec<_> = first_instances(pending).map(|(_, entry)| entry).collect();
        let offered: HashSet<_> = first
            .iter()
            .flat_map(|entry| identities(entry))
            .filter(|id| !self.removed.contains(id))
            .map(|id| id.type_id)
            .collect();
        for entry in &first {
            for dependency in entry.provider.dependencies() {
                if !self.available(dependency) && !offered.contains(&dependency.type_id) {
                    return self.dependency_error(entry, dependency);
                }
            }
        }
        let sources = first.into_iter().map(label).collect::<Vec<_>>().join("、");
        Error::custom(
            "package_provider_cycle",
            format!("包 Provider 无法按依赖完成排序，存在循环：{sources}"),
        )
    }

    fn dependency_error(&self, entry: &ProviderEntry, dependency: ProviderId) -> Error {
        let provider = label(entry);
        let dependency_name = dependency.name();
        if self.all.contains(&dependency.type_id) {
            Error::custom(
                "provider_order_invalid",
                format!("{provider} 必须在 {dependency_name} 之后启动"),
            )
        } else {
            Error::custom(
                "provider_dependency_missing",
                format!("{provider} 缺少依赖 {dependency_name}"),
            )
        }
    }
}

fn label(entry: &ProviderEntry) -> String {
    let name = entry.provider.id().name();
    entry.package.map_or_else(
        || name.to_owned(),
        |source| format!("包 `{source}` 的 {name}"),
    )
}

#[cfg(test)]
#[allow(clippy::expect_used, reason = "断言校验器确实拒绝错误的包清单")]
mod tests {
    use super::prepare;
    use crate::Result;
    use crate::foundation::provider::{ProviderEntry, ProviderId};
    use crate::support::ServiceProvider;
    use std::collections::HashSet;

    struct Fixture {
        id: ProviderId,
        dependency: Option<ProviderId>,
        nested: Option<ProviderId>,
        possible: Option<ProviderId>,
    }

    impl ServiceProvider for Fixture {
        fn id(&self) -> ProviderId {
            self.id
        }

        fn dependencies(&self) -> Vec<ProviderId> {
            self.dependency.into_iter().collect()
        }

        fn nested_providers(&self) -> Vec<ProviderId> {
            self.nested.into_iter().collect()
        }

        fn possible_nested_providers(&self) -> Vec<ProviderId> {
            self.possible.into_iter().collect()
        }
    }

    fn entry<T: 'static>(
        package: Option<&'static str>,
        dependency: Option<ProviderId>,
    ) -> ProviderEntry {
        ProviderEntry {
            provider: Box::new(Fixture {
                id: ProviderId::of::<T>(),
                dependency,
                nested: None,
                possible: None,
            }),
            package,
            force: false,
        }
    }

    #[test]
    fn package_dependencies_keep_the_first_duplicate_and_explicit_segments() -> Result<()> {
        let base = ProviderId::of::<u16>();
        let sorted = prepare(
            vec![
                entry::<u8>(None, None),
                entry::<u32>(Some("consumer"), Some(base)),
                entry::<u32>(Some("consumer"), None),
                entry::<u64>(Some("independent"), None),
                entry::<u16>(Some("base"), Some(ProviderId::of::<u8>())),
                entry::<u128>(None, Some(ProviderId::of::<u32>())),
            ],
            &HashSet::new(),
        )?;
        assert_eq!(
            sorted
                .iter()
                .map(|entry| entry.provider.id())
                .collect::<Vec<_>>(),
            [
                ProviderId::of::<u8>(),
                ProviderId::of::<u64>(),
                base,
                ProviderId::of::<u32>(),
                ProviderId::of::<u32>(),
                ProviderId::of::<u128>(),
            ]
        );
        assert_eq!(sorted[3].provider.dependencies(), [base]);
        Ok(())
    }

    #[test]
    fn package_conflict_reports_both_sources_and_removal_applies_first() -> Result<()> {
        let duplicate = || {
            vec![
                entry::<u8>(Some("first"), None),
                entry::<u8>(Some("second"), None),
            ]
        };
        let error = prepare(duplicate(), &HashSet::new())
            .map(drop)
            .expect_err("跨包重复身份");
        assert_eq!(error.code(), "package_provider_conflict");
        assert!(error.to_string().contains("`first`") && error.to_string().contains("`second`"));
        prepare(duplicate(), &[ProviderId::of::<u8>()].into())?;
        Ok(())
    }

    #[test]
    fn missing_later_and_cyclic_dependencies_have_distinct_diagnostics() {
        let other = ProviderId::of::<u16>();
        let depends = || entry::<u8>(Some("dependent"), Some(other));
        let missing = prepare(vec![depends()], &HashSet::new())
            .map(drop)
            .expect_err("缺失依赖");
        assert_eq!(missing.code(), "provider_dependency_missing");
        assert!(missing.to_string().contains("dependent"));
        let later = prepare(vec![depends(), entry::<u16>(None, None)], &HashSet::new())
            .map(drop)
            .expect_err("依赖后置应用项");
        assert_eq!(later.code(), "provider_order_invalid");
        let cycle = prepare(
            vec![
                depends(),
                entry::<u16>(Some("other"), Some(ProviderId::of::<u8>())),
            ],
            &HashSet::new(),
        )
        .map(drop)
        .expect_err("循环依赖");
        assert_eq!(cycle.code(), "package_provider_cycle");
        assert!(cycle.to_string().contains("dependent") && cycle.to_string().contains("other"));
    }

    #[test]
    fn conditional_nested_identity_waits_for_a_definite_package_provider() -> Result<()> {
        let child = ProviderId::of::<u16>();
        let conditional = ProviderEntry {
            provider: Box::new(Fixture {
                id: ProviderId::of::<u8>(),
                dependency: None,
                nested: None,
                possible: Some(child),
            }),
            ..entry::<u8>(Some("conditional"), None)
        };
        let sorted = prepare(
            vec![
                conditional,
                entry::<u32>(Some("dependent"), Some(child)),
                entry::<u16>(Some("definite"), None),
            ],
            &HashSet::new(),
        )?;
        assert_eq!(sorted[1].provider.id(), child);
        Ok(())
    }

    #[test]
    fn discarded_duplicate_cannot_advertise_a_missing_nested_provider() {
        let child = ProviderId::of::<u16>();
        let second = ProviderEntry {
            provider: Box::new(Fixture {
                id: ProviderId::of::<u8>(),
                dependency: None,
                nested: Some(child),
                possible: None,
            }),
            ..entry::<u8>(Some("duplicate"), None)
        };
        let error = prepare(
            vec![
                entry::<u8>(Some("duplicate"), None),
                second,
                entry::<u32>(Some("dependent"), Some(child)),
            ],
            &HashSet::new(),
        )
        .map(drop)
        .expect_err("只在重复项声明的依赖没有登记者");
        assert_eq!(error.code(), "provider_dependency_missing");
    }
}
