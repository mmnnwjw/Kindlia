"""准备或发布 Kindlia Worker；密钥只写本地忽略文件，不回显、不写聊天。"""
import argparse
import json
import re
import secrets
import shutil
import subprocess
import time
import urllib.request
from pathlib import Path

root = Path(__file__).resolve().parents[1]
worker = root / "workers/kindlia-gateway"
parser = argparse.ArgumentParser()
parser.add_argument("--domain", help="默认读取 Worker 配置内的自定义域名")
parser.add_argument("--deploy", action="store_true", help="发布到已登录的 Cloudflare 账号")
parser.add_argument("--verify", action="store_true", help="仅核验已发布的入口，不重新发布")
args = parser.parse_args()

config = json.loads("\n".join(line for line in (worker / "wrangler.jsonc").read_text(encoding="utf-8").splitlines() if not line.lstrip().startswith("//")))
domain = args.domain or next((r.get("pattern") for r in config.get("routes", []) if r.get("custom_domain")), None)
if not domain:
    parser.error("请使用 --domain reader.example.com 指定自己的域名，或先填写 wrangler.jsonc 的 routes")
if not re.fullmatch(r"(?=.{1,253}$)[A-Za-z0-9](?:[A-Za-z0-9.-]*[A-Za-z0-9])?", domain) or "." not in domain or ".." in domain:
    parser.error("--domain 须为域名，不含协议、端口或路径")
config["routes"] = [{"pattern": domain.lower(), "custom_domain": True}]
config["workers_dev"] = False
(worker / "wrangler.local.jsonc").write_text(json.dumps(config, indent=2) + "\n", encoding="utf-8")
secret_path = worker / ".gateway-secret.json"
if secret_path.exists():
    secret = json.loads(secret_path.read_text(encoding="utf-8"))["GATEWAY_TOKEN"]
    if not re.fullmatch(r"[A-Za-z0-9_.~-]{16,256}", secret):
        raise SystemExit("本地反代密钥格式无效；未覆盖原文件")
else:
    secret = secrets.token_urlsafe(32)
    secret_path.write_text(json.dumps({"GATEWAY_TOKEN": secret}) + "\n", encoding="utf-8")
(worker / ".dev.vars").write_text(f"GATEWAY_TOKEN={json.dumps(secret)}\n", encoding="utf-8")
output = root / "build/gateway"
output.mkdir(parents=True, exist_ok=True)
client_config = output / "config.toml"
client_config.write_text(f'gateway_enabled = true\ngateway_url = "https://{domain}"\ngateway_key = {json.dumps(secret)}\n', encoding="utf-8")
report = {"domain": domain, "prepared": True, "deployed": False, "client_config": str(client_config), "secret_printed": False}

if args.deploy:
    node = shutil.which("node")
    cli = worker / "node_modules/wrangler/bin/wrangler.js"
    if not node or not cli.is_file():
        raise SystemExit("请先在 Worker 目录执行 npm ci")
    def wrangler(*arguments):
        subprocess.run([node, str(cli), *arguments], cwd=worker, check=True)
    wrangler("deploy", "--config", "wrangler.local.jsonc")
    wrangler("secret", "bulk", str(secret_path), "--config", "wrangler.local.jsonc")
    report["deployed"] = True
if args.deploy or args.verify:
    # 只做匿名读检查，不使用账号或发表评论。不给报告记录密钥和完整响应正文。
    failure = None
    for attempt in range(12):
        try:
            with urllib.request.urlopen(urllib.request.Request(f"https://{domain}/health",headers={"User-Agent":"Kindlia/0.2"}), timeout=15) as response:
                health = json.load(response)
            if health.get("service") != "kindlia-gateway" or health.get("authenticated") is not True:
                raise RuntimeError("域名未指向预期 Worker 或密钥未生效")
            request = urllib.request.Request(f"https://{domain}/main/api/novel?page=0&pageSize=1&provider=syosetu&level=1&type=0&translate=0&sort=0", headers={"X-Kindlia-Key": secret,"User-Agent":"Kindlia/0.2"})
            with urllib.request.urlopen(request, timeout=30) as response:
                data = json.load(response)
            if not isinstance(data.get("items"), list):
                raise RuntimeError("反代小说 API 返回格式无效")
            report["deployed"] = True
            report["health_passed"] = True
            report["anonymous_api_passed"] = True
            break
        except Exception as error:
            failure = f"{type(error).__name__}: {error}"
            if attempt < 11:
                time.sleep(3)
    else:
        report["verification_error"] = failure
    (output / "deployment.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(report, indent=2))
    if not report.get("anonymous_api_passed"):
        raise SystemExit("Worker 已发布；域名或上游读取尚未通过验证，详见本地部署记录")
else:
    (output / "deployment.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(report, indent=2))
