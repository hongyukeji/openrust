"""Exercise Telescope source installation without modifying the working application."""
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile


def main():
    binary = str(Path(sys.argv[1]).resolve())
    project = Path(__file__).resolve().parents[3]
    with tempfile.TemporaryDirectory(prefix='openrust-telescope-install-') as directory:
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
        run('telescope:install')
        files = ['config/telescope.rs', 'config/mod.rs', 'app/providers/telescope_service_provider.rs', 'app/providers/mod.rs', 'bootstrap/providers.rs']
        first = {relative: (root / relative).read_text() for relative in files}
        assert 'mod telescope;' in first['config/mod.rs']
        assert '("telescope".to_owned(), telescope::load(env)?),' in first['config/mod.rs']
        assert first['bootstrap/providers.rs'].count('Box::new(crate::app::providers::telescope_service_provider::TelescopeServiceProvider)') == 1
        run('telescope:install')
        assert first == {relative: (root / relative).read_text() for relative in files}
        for relative in ['config/telescope.rs', 'app/providers/telescope_service_provider.rs']:
            with (root / relative).open('a') as file:
                file.write('\n// application customization must survive\n')
        customized = {relative: (root / relative).read_text() for relative in files}
        run('telescope:install')
        assert customized == {relative: (root / relative).read_text() for relative in files}
        run('telescope:publish')
        assert customized == {relative: (root / relative).read_text() for relative in files}
        print('PASS: Telescope install registers configuration/provider once and preserves application customizations')


if __name__ == '__main__':
    main()
