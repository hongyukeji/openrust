"""Exercise Horizon source installation without modifying the working application."""
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile


def main():
    binary = str(Path(sys.argv[1]).resolve())
    project = Path(__file__).resolve().parents[4]
    with tempfile.TemporaryDirectory(prefix='openrust-horizon-install-') as directory:
        root = Path(directory)
        for relative in ['Cargo.toml', 'config/mod.rs', 'bootstrap/providers.rs', 'app/providers/mod.rs']:
            destination = root / relative
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(project / relative, destination)
        env = dict(os.environ, APP_BASE_PATH=str(root), APP_ENV='testing', LOG_CHANNEL='null')
        def run(*arguments):
            result = subprocess.run([binary, 'artisan', *arguments], env=env, capture_output=True, text=True, timeout=20)
            assert result.returncode == 0, (arguments, result.stdout, result.stderr)
            return result.stdout
        run('horizon:install')
        files = ['config/horizon.rs', 'config/mod.rs', 'app/providers/horizon_service_provider.rs', 'app/providers/mod.rs', 'bootstrap/providers.rs']
        first = {relative: (root / relative).read_text() for relative in files}
        assert 'mod horizon;' in first['config/mod.rs']
        assert '("horizon".to_owned(), horizon::load(env)?),' in first['config/mod.rs']
        assert first['bootstrap/providers.rs'].count('Box::new(crate::app::providers::horizon_service_provider::HorizonServiceProvider)') == 1
        run('horizon:install')
        assert first == {relative: (root / relative).read_text() for relative in files}
        for relative in ['config/horizon.rs', 'app/providers/horizon_service_provider.rs']:
            with (root / relative).open('a') as file:
                file.write('\n// application customization must survive\n')
        customized = {relative: (root / relative).read_text() for relative in files}
        run('horizon:install')
        assert customized == {relative: (root / relative).read_text() for relative in files}
        assert 'no longer publishes' in run('horizon:publish')
        print('PASS: Horizon install registers configuration/provider once and preserves application customizations')


if __name__ == '__main__':
    main()
