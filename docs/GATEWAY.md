# Cloudflare Worker 反代

Kindlia 默认直连源站，反代地址和密钥均为空。需要反代时，自行部署 Worker 并在客户端填写地址、密钥后开启。项目不提供公共反代入口。域名根路径不是网页，浏览器诊断用 `/health`。

## 转发链路

```text
Kindle --HTTPS + X-Kindlia-Key--> Worker 自定义域名
    /main/api/...       → n.novelia.cc/api/...（小说、收藏、历史、文库）
    /main/files-...      → n.novelia.cc/files-...（EPUB 等文件）
    /auth/api/v1/...     → auth.novelia.cc/api/v1/...（登录、刷新、退出）
    /forum/api/v1/...    → forum.novelia.cc/api/v1/...（书籍评论）
    /image?url=...       → 配置白名单内的图片站
```

客户端在传输层改地址，不修改源站 JSON，章节和本地缓存仍以原源站标识保存。登录、评论请求也走同一入口。EPUB 的相对/绝对重定向会改回反代地址，Range / If-Range / Content-Range / ETag 保留。文件以流转发，不在 Worker 内收齐后解析。

源站账号凭证只传到相应小说/评论服务，refresh-token Cookie 只传认证服务；不传给图片站。Worker 访问密钥不转发上游；客户端跟随重定向时也只把密钥送给初始 origin。动态图片 URL 只允许白名单 HTTPS 主机，禁止 IP、自定义端口或用户信息。论坛板块及小说编辑上传接口未开放。

## 个人免费使用

实现不需要 KV、R2、D1、Durable Objects 或其他付费绑定，不执行正文改写/EPUB 解析，不开启账号响应共享缓存。账号本身应使用 Workers Free 计划；本脚本不会购买或升级套餐。

2026-10-22 核对官方限制：Free 每日 100,000 次请求（UTC 午夜重置），每次 10 ms CPU、128 MB 内存；网络等待不计 CPU。个人阅读通常远小于请求额度，插图与断点续传会增加请求数，本地缓存可减少重复请求。实际 CPU/用量可在 Cloudflare Worker 的 Metrics 查看。

免费全球网络不等于中国大陆边缘节点；自定义域名也不能保证所有大陆运营商/时段都稳定，需用户实际网络验收。Cloudflare China Network 是 Enterprise 的单独订阅，不在本方案范围。

官方资料：
- [限制与 CPU 定义](https://developers.cloudflare.com/workers/platform/limits/)
- [自定义域名要求](https://developers.cloudflare.com/workers/configuration/routing/custom-domains/)
- [中国网络说明](https://developers.cloudflare.com/china-network/)

## 首次部署及修改域名

Worker 源码位于 `workers/kindlia-gateway/`。仅依赖 Wrangler 开发工具，已锁定 package-lock.json：

```powershell
cd workers/kindlia-gateway
npm ci
npm run check
npm run build     # 仅打包，不发布
npx wrangler login --scopes user:read account:read workers:write workers_scripts:write workers_routes:write zone:read
cd ../..
python tools/deploy_gateway.py --domain reader.example.com --deploy
```

将示例域名替换成自己的子域名。`wrangler.jsonc` 不包含默认路由；也可自行填写其中的 `routes` 后省略 `--domain`。域名需要属于同一账号的已激活 Cloudflare zone。工具只创建/更新名为 kindlia-gateway 的 Worker，并绑定指定子域名。

部署工具初次生成随机访问密钥，保存在忽略文件 `workers/kindlia-gateway/.gateway-secret.json`，同时生成 `.dev.vars` 供本地调试。通过官方 secret bulk 命令写入 Worker secret `GATEWAY_TOKEN`，不会打印密钥；后续部署复用同一密钥。

可直接修改 `wrangler.jsonc` 的 `routes` 和 `vars.IMAGE_HOSTS`，随后重新运行部署工具。新增封面/插图站如果收到 404，核对图片主机后加入精确域名或显式 `*.suffix`，不要用全域通配。

重新核验已部署入口：

```powershell
python tools/deploy_gateway.py --domain reader.example.com --verify
```

部署摘要在 `build/gateway/deployment.json`，不包含密钥。客户端配置文件在 `build/gateway/config.toml`，包含密钥，请自行保存，不上传仓库/聊天。`.dev.vars`、本地部署配置、密钥文件和 build 目录均已忽略。

## Kindle 配置

在“我的 → 设置 → 网络”中先填反代地址、密钥，再打开反代开关。密钥输入有遮罩，设置页只显示是否已设置。API/登录/评论服务地址保持默认源站。

也可修改 `/mnt/us/extensions/kindlia/config.toml`：

```toml
gateway_enabled = true
gateway_url = "https://reader.example.com"
gateway_key = "与 Worker GATEWAY_TOKEN 相同的密钥"
```

关闭开关会恢复原直连请求，不删除源站缓存和进度。修改域名只需改 gateway_url；自行轮换密钥时，Worker secret 与设备 gateway_key 须同时更新。切换其他反代服务时需保持本文的路由协议。

本地调试可使用 `npm run dev`，客户端允许 `http://127.0.0.1:8787`，其他反代地址必须 HTTPS。不要将本机开发地址填写到 Kindle。

设备普通更新保留已有反代配置及其他数据。源码默认值只影响新配置，不会自动清空已有用户设置。
