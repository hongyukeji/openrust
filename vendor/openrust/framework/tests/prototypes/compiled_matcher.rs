//! 只在隔离副本中接入；保留注册顺序的保守前缀索引，不属于公开 API。
use super::{HashMap, Method, Router};
use std::sync::Arc;

pub(super) struct CompiledMatcher {
    methods: HashMap<Method, Arc<MethodMatcher>>,
}
struct MethodMatcher {
    prefixes: HashMap<Box<str>, Vec<Candidate>>,
    lengths: Vec<usize>,
    first_normal: Option<usize>,
    literals: usize,
    dynamic: usize,
}
#[derive(Clone, Copy)]
struct Candidate {
    ordinal: usize,
    index: usize,
}
fn prefix_len(path: &str) -> usize {
    // 可选参数可能连分隔符一起消失，只索引必然存在的部分。
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
        Self { methods }
    }
    // 沿用隔离注入器的接口；本版不再编译前缀正则，不存在集合编译失败。
    pub(super) fn requires_fallback(&self) -> bool {
        false
    }
    pub(super) fn matched(
        &self,
        router: &Router,
        method: &Method,
        host: &str,
        path: &str,
    ) -> Option<usize> {
        let matcher = self.methods.get(method)?;
        // 第一个普通路由命中时一定胜出，不必建立候选结果或检查索引。
        if let Some(index) = matcher.first_normal
            && router.routes[index].matches(host, path)
        {
            return Some(index);
        }
        let mut normal: Option<Candidate> = None;
        let mut fallback: Option<Candidate> = None;
        for &length in &matcher.lengths {
            if length > path.len() {
                break;
            }
            // 前缀是字节长度；请求的同一位置可能落在另一 Unicode 字符内部。
            let Some(prefix) = path.get(..length) else {
                continue;
            };
            let Some(candidates) = matcher.prefixes.get(prefix) else {
                continue;
            };
            for &candidate in candidates {
                if normal.is_some_and(|old| candidate.ordinal >= old.ordinal) {
                    break;
                }
                if Some(candidate.index) == matcher.first_normal {
                    continue;
                }
                let route = &router.routes[candidate.index];
                if route.is_fallback && fallback.is_some_and(|old| candidate.ordinal >= old.ordinal)
                {
                    continue;
                }
                if !route.matches(host, path) {
                    continue;
                }
                if route.is_fallback {
                    fallback = Some(candidate);
                } else {
                    normal = Some(candidate);
                    break;
                }
            }
        }
        // 桶按前缀长度访问，赢家只按原 ordinal 选择，绝不改为最长／静态优先。
        normal.or(fallback).map(|candidate| candidate.index)
    }
    pub(super) fn stats(&self) -> (usize, usize, usize) {
        let mut unique = Vec::new();
        let mut literals = 0;
        let mut dynamic = 0;
        for matcher in self.methods.values() {
            let pointer = Arc::as_ptr(matcher);
            if unique.contains(&pointer) {
                continue;
            }
            unique.push(pointer);
            literals += matcher.literals;
            dynamic += matcher.dynamic;
        }
        (literals, dynamic, 0)
    }
}
impl MethodMatcher {
    fn compile(router: &Router, positions: &[usize]) -> Self {
        let mut prefixes: HashMap<Box<str>, Vec<Candidate>> = HashMap::new();
        let mut lengths = Vec::new();
        let mut first_normal = None;
        let mut literals = 0;
        let mut dynamic = 0;
        for (ordinal, &index) in positions.iter().enumerate() {
            let route = &router.routes[index];
            let path = route.registration_uri();
            let length = if path.contains('{') {
                dynamic += 1;
                prefix_len(path)
            } else {
                literals += 1;
                path.len()
            };
            if !route.is_fallback {
                first_normal.get_or_insert(index);
            }
            let prefix = &path[..length];
            // get_mut 借用键；只为新的前缀分配，不为重复前缀临时构造 Box。
            if let Some(candidates) = prefixes.get_mut(prefix) {
                candidates.push(Candidate { ordinal, index });
            } else {
                prefixes.insert(prefix.into(), vec![Candidate { ordinal, index }]);
                lengths.push(length);
            }
        }
        lengths.sort_unstable();
        lengths.dedup();
        Self {
            prefixes,
            lengths,
            first_normal,
            literals,
            dynamic,
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
    fn unicode_separators_and_optional_prefixes() {
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
        let matcher = CompiledMatcher::compile(&router);
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
            let new = matcher
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
    #[test]
    fn prefix_buckets_keep_order_with_different_lengths_constraints_and_hosts() {
        // 前缀长度访问顺序不能决定赢家；约束和域名失败后仍应检查后续候选。
        for reverse in [false, true] {
            let mut router = Router::new();
            router.get("/sentinel", || "sentinel").unwrap();
            let mut paths = ["/a/{rest}", "/a/b/{id}", "/a/b/fixed", "/a/b/{id?}"];
            if reverse {
                paths.reverse();
            }
            for uri in paths {
                let route = router.get(uri, || "ok").unwrap();
                if uri == "/a/{rest}" {
                    route.r#where("rest", ".*").unwrap();
                }
            }
            router.get("/{all}", || "fallback").unwrap().is_fallback = true;
            router
                .get("/number/{id}", || "number")
                .unwrap()
                .where_number("id")
                .unwrap();
            router.get("/number/{slug}", || "slug").unwrap();
            router
                .group(RouteGroup::new().domain("one.test"), |r| {
                    r.get("/host/{id}", || "first")?;
                    Ok(())
                })
                .unwrap();
            router
                .group(RouteGroup::new().domain("two.test"), |r| {
                    r.get("/host/fixed", || "second")?;
                    Ok(())
                })
                .unwrap();
            for host in ["", "one.test", "TWO.TEST"] {
                same(
                    &router,
                    host,
                    &[
                        "/a/b",
                        "/a/b/fixed",
                        "/a/b/42",
                        "/a/b/c/d",
                        "/number/42",
                        "/number/slug",
                        "/host/fixed",
                        "/elsewhere",
                    ],
                );
            }
        }
    }
    #[test]
    fn unicode_boundaries_empty_prefix_and_fallback_first_do_not_skip_normal_routes() {
        let mut router = Router::new();
        router.get("/{rest}", || "fallback").unwrap().is_fallback = true;
        router.get("/xy/{id}", || "longer").unwrap();
        router.get("/é/{id}", || "unicode").unwrap();
        router.get("/{root?}", || "optional").unwrap();
        router.get("/first", || "literal").unwrap();
        same(
            &router,
            "",
            &[
                "/",
                "/xy/a",
                "/é/a",
                "/文/a",
                "/文",
                "/first",
                "/none",
                "/none/two/three",
            ],
        );
        // 所有路由都是 fallback 时，也只按原 ordinal 选择。
        let mut fallback = Router::new();
        fallback.get("/a/{id}", || "early").unwrap().is_fallback = true;
        fallback.get("/{any}", || "late").unwrap().is_fallback = true;
        same(&fallback, "", &["/a/42", "/other", "/missing/extra/42"]);
    }
    #[test]
    fn generated_permutations_compare_every_query_to_the_legacy_matcher() {
        let uris = [
            "/p/{id}",
            "/p/fixed",
            "/p/{id?}",
            "/p/a/{id}",
            "/p/a/fixed",
            "/p/a/{id?}",
            "/file.{ext?}",
            "/pre-{id?}",
            "/café/{id}",
            "/xy/{id}",
            "/{id}",
            "/fixed",
        ];
        let mut state = 0x1234_5678_u64;
        for _ in 0..32 {
            let mut order: Vec<_> = (0..uris.len()).collect();
            for n in (1..order.len()).rev() {
                state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
                order.swap(n, state as usize % (n + 1));
            }
            let mut router = Router::new();
            for (ordinal, index) in order.into_iter().enumerate() {
                let route = router.get(uris[index], || "ok").unwrap();
                if ordinal.is_multiple_of(4) {
                    route.is_fallback = true;
                }
            }
            same(
                &router,
                "",
                &[
                    "/",
                    "/p",
                    "/p/fixed",
                    "/p/42",
                    "/p/a",
                    "/p/a/fixed",
                    "/p/a/42",
                    "/p/a/b/c",
                    "/file",
                    "/file.txt",
                    "/pre",
                    "/pre-a",
                    "/café/42",
                    "/文/42",
                    "/xy/42",
                    "/fixed",
                    "/missing",
                ],
            );
        }
    }
}
