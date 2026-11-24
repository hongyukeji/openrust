"""只向项目内隔离源码注入原型；正式 Router 与公开 API 不变。"""
from pathlib import Path
import json, subprocess, tarfile, io, shutil, argparse, os
ROOT = Path(__file__).resolve().parents[5]
BASE = Path(os.environ.get('ROUTE_PROTO_DIR', ROOT / 'target/tmp/route-compiled-prototype')).resolve()
assert BASE.is_relative_to(ROOT / 'target/tmp')
parser = argparse.ArgumentParser()
parser.add_argument('variant', choices=['baseline', 'prototype'])
parser.add_argument('--revision', help='首次创建隔离副本时固定基线；已有副本继续使用 base.txt')
parser.add_argument('--prototype-revision', help='从指定提交读取旧原型作同基线对照')
args = parser.parse_args()
BASE.mkdir(parents=True, exist_ok=True)
source = BASE / 'source'
if not source.exists():
    source.mkdir()
    revision = subprocess.check_output(['git', 'rev-parse', args.revision or 'HEAD'], cwd=ROOT, text=True).strip()
    (BASE / 'base.txt').write_text(revision)
    with tarfile.open(fileobj=io.BytesIO(subprocess.check_output(['git', 'archive', revision], cwd=ROOT))) as archive:
        archive.extractall(source, filter='data')
    for name in ['router.rs', 'route.rs']:
        shutil.copy2(source / 'vendor/openrust/framework/src/routing' / name, BASE / ('original-' + name))
if args.revision:
    assert subprocess.check_output(['git', 'rev-parse', args.revision], cwd=ROOT, text=True).strip() == (BASE / 'base.txt').read_text().strip(), '隔离副本基线不同，请另选 ROUTE_PROTO_DIR'
prototype = ROOT / 'vendor/openrust/framework/tests/prototypes/compiled_matcher.rs'
(source / prototype.relative_to(ROOT)).parent.mkdir(parents=True, exist_ok=True)
if args.prototype_revision:
    (source / prototype.relative_to(ROOT)).write_bytes(subprocess.check_output(['git', 'show', args.prototype_revision + ':' + str(prototype.relative_to(ROOT))], cwd=ROOT))
else:
    shutil.copy2(prototype, source / prototype.relative_to(ROOT))
router = (BASE / 'original-router.rs').read_text()
router = router.replace('    fn matched(&self, method:', '    fn legacy_matched(&self, method:', 1)
if args.variant == 'prototype':
    router = router.replace('    route_cache: Option<super::route_cache::RouteCache>,', '    route_cache: Option<super::route_cache::RouteCache>,\n    prototype: std::sync::OnceLock<compiled_prototype::CompiledMatcher>,')
    router = router.replace('        self.route_cache = None;', '        self.route_cache = None;\n        self.prototype.take();')
    body = '''let compiled = self.prototype.get_or_init(|| compiled_prototype::CompiledMatcher::compile(self));
        if compiled.requires_fallback() { return self.legacy_matched(method, host, path); }
        let index = compiled.matched(self, method, host, path);
        #[cfg(debug_assertions)]
        { let old = self.legacy_matched(method, host, path).map(std::ptr::from_ref); let new = index.map(|i| std::ptr::from_ref(&self.routes[i])); assert_eq!(old, new, "原型与既有匹配结果不同"); }
        index.map(|i| &self.routes[i])'''
    module = '#[path=' + json.dumps(str(source / prototype.relative_to(ROOT))) + ']\nmod compiled_prototype;\n'
    prepare = 'self.prototype.get_or_init(|| compiled_prototype::CompiledMatcher::compile(self));'
    reset = 'self.prototype.take();'
    stats = 'self.prototype.get_or_init(|| compiled_prototype::CompiledMatcher::compile(self)).stats()'
else:
    body = 'self.legacy_matched(method, host, path)'
    module = ''
    prepare = 'std::hint::black_box(self.routes.len());'
    reset = ''
    stats = '(0,0,0)'
router += module + '''impl Router {
    fn matched(&self, method: &Method, host: &str, path: &str) -> Option<&Route> { BODY }
    pub fn __bench_prepare(&self) { PREPARE }
    pub fn __bench_stats(&self) -> (usize,usize,usize) { STATS }
    pub fn __bench_cache_dump(&self) -> String { serde_json::to_string(&self.compile_cache()).unwrap() }
    pub fn __bench_reset(&mut self) { RESET }
    pub fn __bench_match(&self, path: &str) -> Option<usize> { self.matched(&Method::GET, "", path).map(|r| (r as *const Route as usize - self.routes.as_ptr() as usize) / std::mem::size_of::<Route>()) }
}
'''.replace('BODY',body).replace('PREPARE',prepare).replace('STATS',stats).replace('RESET',reset)
(source / 'vendor/openrust/framework/src/routing/router.rs').write_text(router)
route = (BASE / 'original-route.rs').read_text() + '\nimpl Route { pub(super) fn __prototype_path_matches(&self, path: &str) -> bool { self.pattern.matches(path) } }\n' if args.variant == 'prototype' and args.prototype_revision else (BASE / 'original-route.rs').read_text()
(source / 'vendor/openrust/framework/src/routing/route.rs').write_text(route)
print(source)

for name in ['vendor/openrust/framework/tests/prototypes/workload.rs', 'vendor/openrust/framework/tests/routing_compiled_matrix.rs']:
    destination = source / name
    destination.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(ROOT / name, destination)
folder = Path(__file__).parent
examples = source / 'vendor/openrust/framework/examples'
(examples / 'compiled_support').mkdir(parents=True, exist_ok=True)
(examples / 'compiled_support/support.rs').write_text((folder / 'support.rs').read_text().replace('../../../../../vendor/openrust/framework/tests/prototypes/workload.rs', 'workload.rs'))
shutil.copy2(ROOT / 'vendor/openrust/framework/tests/prototypes/workload.rs', examples / 'compiled_support/workload.rs')
for kind in ['alloc', 'time']:
    (examples / f'compiled_prototype_{kind}.rs').write_text((folder / f'{kind}.rs').read_text().replace('path = "support.rs"', 'path = "compiled_support/support.rs"'))

# 两版都经过相同的准备边界；原型在应用启动完成前构建，不把编译隐藏在首个HTTP请求中。
application_path = Path('vendor/openrust/framework/src/foundation/application.rs')
original = subprocess.check_output(['git', 'show', (BASE / 'base.txt').read_text().strip() + ':' + str(application_path)], cwd=ROOT, text=True)
anchor = '        let loaded = routes.load_cache(&route_cache_path);'
assert original.count(anchor) == 1
(source / application_path).write_text(original.replace(anchor, anchor + '\n        routes.__bench_prepare();'))
