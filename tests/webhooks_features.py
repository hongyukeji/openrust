"""从隔离源码分别构建关闭、单包和双包，实测正式应用的包发现与发布。"""
import argparse
import os
from pathlib import Path
import subprocess
import tempfile
ROOT=Path(__file__).resolve().parents[1]
p=argparse.ArgumentParser();p.add_argument("--source",type=Path,required=True);args=p.parse_args()
source=args.source.resolve()
if not source.is_relative_to(ROOT/"target/tmp"):p.error("源码必须位于项目 target/tmp 下")
binary=Path(os.environ["CARGO_TARGET_DIR"])/"debug/main"
for features in ((),("webhook-client",),("webhook-server",),("webhook-client","webhook-server")):
    build=["cargo","build","--manifest-path",str(source/"Cargo.toml"),"--bin","main","--no-default-features","--locked"]
    if features:build.extend(["--features",",".join(features)])
    subprocess.run(build,check=True,timeout=300)
    with tempfile.TemporaryDirectory(dir=ROOT/"target/tmp",prefix="webhook-feature-") as temp:
        app=Path(temp)
        env=os.environ|{"APP_BASE_PATH":temp,"APP_ENV":"testing","CACHE_STORE":"file","SESSION_DRIVER":"array",
            "APP_DISABLED_RUNNERS":"queue,schedule","APP_CONFIG_CACHE":str(app/"configuration.json"),"APP_SUPPORTED_LOCALES":""}
        discover=subprocess.run([str(binary),"artisan","package:discover"],env=env,capture_output=True,text=True,timeout=30)
        assert discover.returncode==0,discover.stdout+discover.stderr
        for kind in ("client","server"):
            result=subprocess.run([str(binary),"artisan","config:show",f"webhook-{kind}"],env=env,capture_output=True,text=True,timeout=30)
            enabled=f"webhook-{kind}" in features
            assert (result.returncode==0)==enabled,(features,kind,result.stdout,result.stderr)
            path=app/"config"/f"webhook_{kind}.rs"
            assert not path.exists()
            if enabled:
                publish=subprocess.run([str(binary),"artisan","vendor:publish",f"--tag=webhook-{kind}-config"],env=env,capture_output=True,text=True,timeout=30)
                assert publish.returncode==0 and path.exists(),publish.stdout+publish.stderr
    print(f"features={','.join(features) or 'disabled'}: 正式应用包发现／具名配置／显式发布通过",flush=True)
