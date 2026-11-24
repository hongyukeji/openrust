//! 仅由隔离原型脚本接入 Router；不属于正式路由实现或公开 API。
use super::{HashMap, Method, Router};
use regex::{RegexSet, RegexSetBuilder};
use std::sync::Arc;

pub(super) struct CompiledMatcher {
    methods: HashMap<Method, Arc<MethodMatcher>>,
    requires_fallback: bool,
}
struct MethodMatcher {
    literals: HashMap<Box<str>, Vec<(usize, usize)>>,
    dynamic: Vec<Candidate>,
    chunks: Vec<CandidateChunk>,
}
// Symfony 同样使用有序的动态正则块；不将万级前缀塞入同一个集合。
const CHUNK_SIZE: usize = 256;
struct CandidateChunk {
    start: usize,
    end: usize,
    prefixes: Option<RegexSet>,
}
struct Candidate {
    ordinal: usize,
    index: usize,
    prefix_len: usize,
}
fn prefix_len(path: &str) -> usize {
    // 可选参数可能连分隔符一起消失；只使用必然存在的保守字面前缀。
    path.split('{')
        .next()
        .unwrap_or_default()
        .trim_end_matches(|character| "/,;.:-_~+*=@|".contains(character))
        .len()
}
impl CompiledMatcher {
    pub(super) fn compile(router: &Router) -> Self {
        let mut shared: Vec<(&[usize], Arc<MethodMatcher>)> = Vec::new();
        let methods = router
            .by_method
            .iter()
            .map(|(method, positions)| {
                let matcher = if let Some((_, matcher)) =
                    shared.iter().find(|(old, _)| *old == positions.as_slice())
                {
                    matcher.clone()
                } else {
                    let matcher = Arc::new(MethodMatcher::compile(router, positions));
                    shared.push((positions, matcher.clone()));
                    matcher
                };
                (method.clone(), matcher)
            })
            .collect();
        let requires_fallback = shared
            .iter()
            .any(|(_, matcher)| matcher.chunks.iter().any(|chunk| chunk.prefixes.is_none()));
        Self {
            methods,
            requires_fallback,
        }
    }
    pub(super) fn requires_fallback(&self) -> bool {
        self.requires_fallback
    }
    pub(super) fn matched(
        &self,
        router: &Router,
        method: &Method,
        host: &str,
        path: &str,
    ) -> Option<usize> {
        let matcher = self.methods.get(method)?;
        let mut fallback: Option<(usize, usize)> = None;
        let mut select = |ordinal: usize, index: usize| {
            let route = &router.routes[index];
            if !route.matches(host, path) {
                return None;
            }
            if route.is_fallback {
                if fallback.is_none_or(|(old, _)| ordinal < old) {
                    fallback = Some((ordinal, index));
                }
                None
            } else {
                Some(index)
            }
        };
        // 能进入该表的静态候选不受任何在先动态路径遮蔽；域名仍精确核对。
        if let Some(candidates) = matcher.literals.get(path) {
            for &(ordinal, index) in candidates {
                if let Some(index) = select(ordinal, index) {
                    return Some(index);
                }
            }
        }
        for chunk in &matcher.chunks {
            if let Some(prefixes) = &chunk.prefixes {
                // 失败的块不创建 SetMatches，避免每个未命中块分配结果集合。
                if !prefixes.is_match(path) {
                    continue;
                }
                for candidate in prefixes
                    .matches(path)
                    .iter()
                    .map(|id| &matcher.dynamic[chunk.start + id])
                {
                    if let Some(index) = select(candidate.ordinal, candidate.index) {
                        return Some(index);
                    }
                }
            } else {
                for candidate in &matcher.dynamic[chunk.start..chunk.end] {
                    let uri = router.routes[candidate.index].registration_uri();
                    if path.starts_with(&uri[..candidate.prefix_len])
                        && let Some(index) = select(candidate.ordinal, candidate.index)
                    {
                        return Some(index);
                    }
                }
            }
        }
        fallback.map(|(_, index)| index)
    }
    pub(super) fn stats(&self) -> (usize, usize, usize) {
        let mut unique = Vec::new();
        let mut literals = 0;
        let mut dynamic = 0;
        let mut fallback = 0;
        for matcher in self.methods.values() {
            let pointer = Arc::as_ptr(matcher);
            if unique.contains(&pointer) {
                continue;
            }
            unique.push(pointer);
            literals += matcher.literals.values().map(Vec::len).sum::<usize>();
            dynamic += matcher.dynamic.len();
            fallback += matcher
                .chunks
                .iter()
                .filter(|chunk| chunk.prefixes.is_none())
                .count();
        }
        (literals, dynamic, fallback)
    }
}
impl MethodMatcher {
    fn compile(router: &Router, positions: &[usize]) -> Self {
        let mut literals: HashMap<Box<str>, Vec<(usize, usize)>> = HashMap::new();
        let mut dynamic: Vec<Candidate> = Vec::new();
        for (ordinal, &index) in positions.iter().enumerate() {
            let path = router.routes[index].registration_uri();
            let shadowed = !path.contains('{')
                && dynamic.iter().any(|candidate| {
                    let previous = &router.routes[candidate.index];
                    path.starts_with(&previous.registration_uri()[..candidate.prefix_len])
                        && previous.__prototype_path_matches(path)
                });
            if !path.contains('{') && !shadowed {
                literals
                    .entry(path.into())
                    .or_default()
                    .push((ordinal, index));
            } else {
                dynamic.push(Candidate {
                    ordinal,
                    index,
                    prefix_len: prefix_len(path),
                });
            }
        }
        let chunks = dynamic
            .chunks(CHUNK_SIZE)
            .enumerate()
            .map(|(number, candidates)| {
                let expressions = candidates.iter().map(|candidate| {
                    let path = router.routes[candidate.index].registration_uri();
                    if path.contains('{') {
                        format!(r"\A{}", regex::escape(&path[..candidate.prefix_len]))
                    } else {
                        format!(r"\A{}\z", regex::escape(path))
                    }
                });
                CandidateChunk {
                    start: number * CHUNK_SIZE,
                    end: number * CHUNK_SIZE + candidates.len(),
                    prefixes: RegexSetBuilder::new(expressions)
                        .size_limit(10 * 1024 * 1024)
                        .build()
                        .ok(),
                }
            })
            .collect();
        Self {
            literals,
            dynamic,
            chunks,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routing::RouteGroup;
    fn same(router: &Router, host: &str, paths: &[&str]) {
        let matcher = CompiledMatcher::compile(router);
        for path in paths {
            let old = router
                .legacy_matched(&Method::GET, host, path)
                .map(std::ptr::from_ref);
            let new = matcher
                .matched(router, &Method::GET, host, path)
                .map(|id| std::ptr::from_ref(&router.routes[id]));
            assert_eq!(old, new, "{host} {path}");
        }
    }
    #[test]
    fn overlap_constraints_domains_and_fallback_keep_original_order() {
        let mut router = Router::new();
        router.get("/early/{id}", || "dynamic").unwrap();
        router.get("/early/fixed", || "literal").unwrap();
        router.get("/late/fixed", || "literal").unwrap();
        router.get("/late/{id}", || "dynamic").unwrap();
        router.get("/optional/{id?}", || "optional").unwrap();
        router
            .get("/number/{id}", || "number")
            .unwrap()
            .where_number("id")
            .unwrap();
        router.get("/number/fixed", || "literal").unwrap();
        router
            .get("/pcre/{id}", || "pcre")
            .unwrap()
            .r#where("id", "(?=a)a+")
            .unwrap();
        router.get("/pcre/fixed", || "literal").unwrap();
        router.get("/{rest}", || "fallback").unwrap().is_fallback = true;
        router.get("/normal", || "normal").unwrap();
        router
            .group(RouteGroup::new().domain("{tenant}.example.test"), |r| {
                r.get("/host/{id}", || "dynamic")?;
                Ok(())
            })
            .unwrap();
        router
            .group(RouteGroup::new().domain("STATIC.example.test"), |r| {
                r.get("/host/fixed", || "literal")?;
                Ok(())
            })
            .unwrap();
        for host in [
            "",
            "a.example.test",
            "STATIC.example.TEST",
            "elsewhere.test",
        ] {
            same(
                &router,
                host,
                &[
                    "/early/fixed",
                    "/late/fixed",
                    "/optional",
                    "/optional/42",
                    "/number/42",
                    "/number/fixed",
                    "/pcre/aaa",
                    "/pcre/fixed",
                    "/normal",
                    "/unmatched",
                    "/host/fixed",
                ],
            );
        }
    }
    #[test]
    fn unicode_separators_optional_prefixes_and_failed_set_fallback() {
        let mut router = Router::new();
        for uri in [
            "/café/文件",
            "/file.{ext?}",
            "/pre-{value?}",
            "/x/{a?}/{b?}",
            "/with space",
            "/encoded%7B",
        ] {
            router.get(uri, || "ok").unwrap();
        }
        let positions = router.by_method.get(&Method::GET).unwrap();
        let mut method = MethodMatcher::compile(&router, positions);
        for chunk in &mut method.chunks {
            chunk.prefixes = None;
        }
        let fallback = CompiledMatcher {
            methods: [(Method::GET, Arc::new(method))].into(),
            requires_fallback: true,
        };
        for path in [
            "/café/文件",
            "/file",
            "/file.txt",
            "/pre",
            "/pre-value",
            "/x",
            "/x/a",
            "/x/a/b",
            "/with space",
            "/encoded%7B",
            "/missing",
        ] {
            let old = router
                .legacy_matched(&Method::GET, "", path)
                .map(std::ptr::from_ref);
            let new = fallback
                .matched(&router, &Method::GET, "", path)
                .map(|id| std::ptr::from_ref(&router.routes[id]));
            assert_eq!(old, new, "{path}");
        }
    }
    #[test]
    fn compiled_index_is_invalidated_by_registration_and_shared_only_within_application() {
        let mut first = Router::new();
        first.get("/same", || "one").unwrap();
        assert_eq!(first.__bench_match("/same"), Some(0));
        first.get("/same", || "two").unwrap();
        assert_eq!(first.__bench_match("/same"), Some(1));
        let mut second = Router::new();
        second.get("/other", || "other").unwrap();
        assert_eq!(second.__bench_match("/same"), None);
        assert_eq!(first.__bench_match("/same"), Some(1));
        let compiled = CompiledMatcher::compile(&first);
        assert!(Arc::ptr_eq(
            compiled.methods.get(&Method::GET).unwrap(),
            compiled.methods.get(&Method::HEAD).unwrap()
        ));
    }
}
