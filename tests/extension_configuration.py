"""Check native package configuration ownership, publication and cache boundaries."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import tempfile


def verify(binary, enabled):
    source = Path(__file__).resolve().parents[1]
    with tempfile.TemporaryDirectory(prefix="openrust-package-config-") as directory:
        root = Path(directory)
        (root / "config").mkdir()
        (root / "config/mod.rs").write_bytes((source / "config/mod.rs").read_bytes())
        (root / "Cargo.toml").write_text("[package]\nname='probe'\nversion='0.0.0'\n")
        env = dict(os.environ, APP_BASE_PATH=str(root), APP_ENV="testing",
                   APP_CONFIG_CACHE=str(root / "config.json"),
                   APP_PACKAGES_CACHE=str(root / "packages.json"),
                   CACHE_STORE="file", SESSION_DRIVER="array", QUEUE_CONNECTION="sync",
                   REVERB_ENABLED="false", REVERB_SERVER_PORT="9123", PAY_DRIVER="alipay")

        def artisan(*args, overrides=None, success=True):
            result = subprocess.run([str(binary), "artisan", *args, "--no-interaction"],
                                    env=env | (overrides or {}), capture_output=True, text=True, timeout=40)
            assert (result.returncode == 0) == success, f"{args}: {result.stdout}\n{result.stderr}"
            return result.stdout + result.stderr

        artisan("package:discover")
        artisan("config:cache")
        config = json.loads((root / "config.json").read_text())
        for name in ["oidc", "reverb", "settings", "pay"]:
            assert (name in config) == enabled, name
            assert not (root / f"config/{name}.rs").exists(), name
        if not enabled:
            print("feature off: no package configuration or published files")
            return
        assert config["oidc"]["providers"] == {}
        assert config["reverb"]["servers"]["reverb"]["port"] == 9123
        assert config["pay"]["default"] == "alipay"
        cached = artisan("config:show", "reverb", overrides={"REVERB_SERVER_PORT": "invalid"})
        assert "9123" in cached
        # Published Rust configuration is generated only after an explicit command.
        artisan("init:resources", "--extensions=oidc,reverb")
        loader = root / "config/mod.rs"
        installed = loader.read_text()
        for name in ["oidc", "reverb"]:
            assert (root / f"config/{name}.rs").is_file()
            assert f"mod {name};" in installed
            assert f"{name}::load(env)?" in installed
        custom = root / "config/oidc.rs"
        custom.write_text(custom.read_text() + "\n// application customization\n")
        preserved = custom.read_bytes()
        artisan("init:resources", "--extensions=oidc,reverb")
        assert custom.read_bytes() == preserved
        assert loader.read_text() == installed
        # A generic vendor publication does not silently alter the compiled loader.
        for name in ["settings", "pay"]:
            artisan("vendor:publish", f"--tag={name}-config")
            assert (root / f"config/{name}.rs").is_file()
            assert f"mod {name};" not in loader.read_text()
        # Cache contents remain fixed even after a template is published.
        assert json.loads((root / "config.json").read_text()) == config
        artisan("config:clear")
        artisan("config:show", "reverb", overrides={"REVERB_SERVER_PORT": "invalid"}, success=False)
        print("feature on: defaults, empty OIDC providers, cached snapshot, explicit publication and preserved customizations")


def verify_authentication(binary, root):
    # Fortify is explicitly installed in this fixture; discovery alone only
    # makes its installation resources available.
    cache = root / "storage/framework/config.json"
    env = dict(os.environ, APP_BASE_PATH=str(root), APP_CONFIG_CACHE=str(cache))
    def artisan(*args, overrides=None):
        result = subprocess.run([str(binary), "artisan", *args],
                                env=env | (overrides or {}), capture_output=True, timeout=40)
        assert result.returncode == 0, result.stderr.decode(errors="replace")
        return result.stdout
    artisan("config:cache")
    snapshot = cache.read_bytes()
    configuration = json.loads(snapshot)
    assert all(name in configuration for name in ["fortify", "passkeys", "passport"])
    for name in ["fortify", "passkeys", "passport"]:
        artisan("config:show", name, overrides={
            "PASSKEYS_USER_HANDLE_SECRET": os.fsdecode(b"\xff"),
            "PASSPORT_PRIVATE_KEY": os.fsdecode(b"\xff"),
        })
    assert cache.read_bytes() == snapshot
    print("installed authentication: cached Fortify/Passkeys/Passport ignore changed environment secrets")


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("binary", type=Path)
    parser.add_argument("--enabled", action="store_true")
    parser.add_argument("--authentication-root", type=Path)
    options = parser.parse_args()
    if options.authentication_root:
        verify_authentication(options.binary.resolve(), options.authentication_root.resolve())
    else:
        verify(options.binary.resolve(), options.enabled)
