"""提取三语 Webhook 公开契约示例，在隔离应用中编译。"""
import argparse
from pathlib import Path
import re
ROOT=Path(__file__).resolve().parents[1]
p=argparse.ArgumentParser();p.add_argument("--source",type=Path,required=True);source=p.parse_args().source.resolve()
if not source.is_relative_to(ROOT/"target/tmp"):p.error("源码必须位于项目 target/tmp 下")
parts=[]
for locale in ["en","zh-Hans","zh-Hant"]:
    for kind in ["client","server"]:
        text=(source/"docs"/locale/f"packages/webhook-{kind}.md").read_text()
        blocks=re.findall(r"```rust\n(.*?)\n```",text,re.S)
        assert len(blocks)==(2 if kind=="client" else 1),(locale,kind)
        name=f"{locale}_{kind}".lower().replace("-","_")
        uses="let _ = verify_signature; let _ = receive; let _ = register_routes; let _ = register_dispatcher; let _ = process_named; let _ = register_replay_policy;" if kind=="client" else "let _ = spatie_signature; let _ = standard_headers; let _ = prepare_invoice; let _ = submit_invoice; let _ = queue_invoice; let _ = schedule_webhook_recovery; let _ = register_delivery_job; let _ = register_application_replay_policy; let _ = prepare_endpoint_invoice; let _ = configure_webhook_limits; let _ = publish_invoice; let _ = grant_invoice_tls; let _ = grant_invoice_proxy; let _ = inspect_invoice_attempts; let _ = preview_invoice_pruning; let _ = preview_shared_body_pruning; let _ = hold_invoice_delivery;"
        examples="\n".join(blocks)
        parts.append(f"mod {name} {{\n{examples}\n#[test] fn examples_compile() {{ {uses} }}\n}}")
(source/"tests/webhooks_docs_generated.rs").write_text("\n\n".join(parts)+"\n")
print("已提取三语六组 Webhook 契约示例")
