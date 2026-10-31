# 应用内更新与 R2 镜像

入口为「我的 → 检查更新」，不增加 KUAL 菜单。设置 → 网络中可切换「自动检查更新」和 R2 根地址。自动检查每天最多一次，后台执行；手动检查可随时重试。默认 R2 地址为 `https://update1.mmnnwjw.dpdns.org`，GitHub 来源为本项目的公开 Releases；私有仓库的发行版不会向匿名客户端提供。不会把开发者 GitHub Token 发到设备。

沿用 KinNovel 的根目录布局和 GitHub Releases 数组格式，Kindlia 使用独立的包名、签名域和 Ed25519 公钥。两个更新源并行检查，取得有效清单后最多再等另一源 5 秒，选最高正式版本；下载优先 R2，失败后尝试 GitHub。相同版本的两个源必须给出相同 SHA256 才合并下载地址。下载后必须通过 SHA256 与 Ed25519 签名校验；缺少签名的包不会安装。

## 本地准备

```powershell
./rust/build.ps1 kindle
python tools/package_release.py
python tools/sign_update.py dist/kindlia-1.1.0.zip
python tools/prepare_r2.py --package dist/kindlia-1.1.0.zip
```

签名私钥只存本机 `~/.kinovel-secrets/KINDLIA_UPDATE_SIGNING_KEY`，也可用 `--key` 指定。它与 KinNovel 的密钥独立，不要上传或公开；构建包不含私钥。签名工具依赖 PyNaCl。换电脑发布时需要安全迁移此密钥，不能任意换钥，否则既有客户端无法验证更新。

`prepare_r2.py` 只在本地生成上传目录，不上传桶、不调用自动化流程。可用 `--notes` 指定 UTF-8 更新说明文本，用 `--history` 合并已下载的旧 `releases.json`，保留旧版条目。GitHub 发布时应附带 ZIP 和同名 `.zip.sig`，GitHub 资产提供的 `digest` 用作 SHA256。

## 发布时同步到 R2

发布 GitHub Release 并上传 ZIP、签名后运行：

```powershell
python tools/r2_publish.py v1.1.0 --zip dist/kindlia-1.1.0.zip --local-secrets
```

固定目标桶为 `kindlia-updates`，不会上传到 KinNovel 的桶。凭据读取 `~/.kinovel-secrets/{S3_API,Access_Key_ID,Secret_Access_Key,GithubToken}`，不打印；先核对 GitHub 资产的大小、SHA256 和签名，再上传包与签名，最后上传清单，并经公开域名回读核验。无需提供列举所有桶的权限。

## 也可手动上传


把 `dist/r2/` **里面的三个文件**上传到 R2 桶的根目录，不要把 `r2` 或 `dist` 文件夹本身作为前缀上传：

```text
桶根目录/
├── releases.json
├── kindlia-1.1.0.zip
└── kindlia-1.1.0.zip.sig
```

对应公开地址：

- `https://update1.mmnnwjw.dpdns.org/releases.json`
- `https://update1.mmnnwjw.dpdns.org/kindlia-1.1.0.zip`
- `https://update1.mmnnwjw.dpdns.org/kindlia-1.1.0.zip.sig`

先上传 ZIP 与签名，确认可下载后再覆盖 `releases.json`，防止清单先指向尚未上传的文件。清单建议禁用缓存或设短缓存期；版本包与签名可长期缓存。以后发布时把文件名里的版本号替换为新版本即可。

## 安装与恢复

用户确认后才下载并安装；安装完成退出应用，再从 KUAL 打开 Kindlia。只替换已验证的运行文件和许可证，配置、会话、字体、书架、书签与进度不覆盖。旧程序备份保留在程序目录内的 `backups/update-*`。移植 KinNovel 的 stage/swap/done 日志，失败或中断时在下次启动恢复；所有持久文件都在 `/mnt/us/extensions/kindlia/` 内。
