"""Compile and run the Rust blocks from all three container/facade guides."""
from pathlib import Path
import json
import re
import subprocess
import sys
from tempfile import TemporaryDirectory

source = Path(sys.argv[1]).resolve()
deps = source / 'target/debug/deps'
library = max(deps.glob('libopenrust-*.rlib'), key=lambda path: path.stat().st_mtime_ns)
tokio = max(deps.glob('libtokio-*.rlib'), key=lambda path: path.stat().st_mtime_ns)
report = []
driver = '''
use openrust::contracts::{container::Container as _, foundation::Application as _};
struct Bindings;
impl openrust::support::ServiceProvider for Bindings {
    fn register(&mut self, app: &openrust::foundation::Application) -> openrust::support::ProviderResult {
        app.container().instance::<str>(std::sync::Arc::from("Hello"));
        app.container().alias(ServiceKey::typed::<str>(), "message")?;
        assert_eq!(Messages::text()?, "Hello");
        Ok(())
    }
}
fn main() {
    tokio::runtime::Builder::new_current_thread().build().unwrap().block_on(async {
        let mut app = openrust::foundation::Application::configure(std::env::current_dir().unwrap())
            .withProviders(vec![Box::new(Bindings)]).create().unwrap();
        app.boot().await.unwrap();
    });
}
'''
with TemporaryDirectory(prefix='openrust-01-01-examples-') as directory:
    root = Path(directory)
    for language in ['en', 'zh-Hans', 'zh-Hant']:
        for topic in ['container', 'facades']:
            page = source / f'docs/{language}/architecture-concepts/{topic}.md'
            blocks = re.findall(r'```rust\n(.*?)```', page.read_text(), re.S)
            assert len(blocks) == 1, page
            example = root / 'example.rs'
            example.write_text(blocks[0] + (driver if topic == 'facades' else ''))
            binary = root / 'example'
            subprocess.run(['rustc', '+1.94.0', '--edition=2024', '--crate-name=doc_example', '-Dwarnings', str(example), '--extern', f'openrust={library}', '--extern', f'tokio={tokio}', '-L', f'dependency={deps}', '-o', str(binary)], check=True)
            subprocess.run([binary], cwd=source, check=True)
            report.append(dict(language=language, topic=topic, result='passed'))
(Path(__file__).resolve().parent / 'example-results.json').write_text(json.dumps(report, indent=2) + '\n')
print('PASS: 6 documented Rust examples compiled and executed')
