# Kindlia

基于 KinNovel Rust 技术栈的 AutoNovel Kindle 原生客户端，当前开发版 0.2.7。提供原生阅读器、EPUB 下载阅读、收藏、历史、搜索及书籍评论，持续对齐 KinNovel 的界面与交互。默认直连源站，可自行部署并配置 Cloudflare Worker 反代；项目不提供公共反代入口。接口以实际源站源码为准。

底部四栏与 KinNovel 对齐：**书架、历史、发现、我的**。我的收藏对应书架；阅读历史对应历史；网络小说、文库小说、排行和搜索统一归入发现。欢迎登录页、阅读器顶栏/底栏/菜单、前光面板直接复用并适配 KinNovel 实现。

本版代码包含：

- 账号登录、会话保存/刷新、退出登录，欢迎页可跳过。
- 云收藏夹浏览、新建/改名/删除、收藏/移除书籍，本地缓存书架和继续阅读。
- 云阅读历史、阅读章节同步、暂停/恢复记录、删除历史，以及本地阅读进度。
- 网络小说来源/排序等筛选、搜索、详情、完整简介、目录、排行。
- 排行按源站四大分类进入，筛选按钮显示条件与当前值，点按列表选择。
- 文库列表/搜索/详情；点击卷数下载 EPUB，解析目录、正文、插图后进入原生阅读器。兼容源站 UTF-8 TXT 卷和本地导入。
- 保留书籍评论区：分页、回复、发布、修改/删除自己的评论。论坛板块不接入。
- 翻译偏好与原文/译文显示分别设置；字号、行距、段距、缩进、边距、方向、夜间、前光、书签、字体切换。
- 按站点隔离缓存、离线读取、缓存管理和自定义端点。

上传/编辑小说、术语表编辑、翻译任务管理不在本轮普通读者范围。注册及密码找回使用源站账号网页；没有接入轻书架的账号、SignalR 或自动更新服务。完整 KinNovel UI 的一致性、源站在线账号操作及实机触控仍需用户验收。

## 构建和打包

工程独立，不修改或依赖原 KinNovel 工作目录。`references/` 仅用于核对协议，已被 Git 忽略。

克隆后进入 `Kindlia/rust`，使用 PowerShell：

```powershell
.\build.ps1 check       # 编译检查
.\build.ps1 host        # Windows 预览
.\build.ps1 kindle      # ARMv7 musl release
```

需要 Rust、对应 target、cargo-zigbuild 和 Python ziglang。Windows GNU 构建统一使用 Zig。Git Bash 也可使用 `./build.sh check|host|kindle`。

```powershell
cd ..
python tools/package_release.py
```

输出 `dist/kindlia-0.2.7.zip`，仅含程序及声明文档，不携带用户数据。复用来源见 [上游清单](docs/UPSTREAM-FILES.json)，接口见 [协议](docs/PROTOCOL.md)。

## 安装和使用

ZIP 内 `kindlia/` 对应 `/mnt/us/extensions/kindlia/`，从 KUAL 的 Kindlia 菜单启动。首次启动生成 `/mnt/us/extensions/kindlia/config.toml` 并显示欢迎登录页。

**部署默认保留配置与数据**：退出 Kindlia，备份程序文件和配置后原位更新，保留账号、字体、书架、进度和缓存。0.2.5 起首次升级自动将旧 `/mnt/us/kindlia/` 合并迁入扩展目录，旧外部备份迁入 `backups/legacy/`；冲突文件和迁移日志保存在内部 `backups/`。仅用户明确要求“清洁安装”时才添加 `--clean`，备份用户数据后创建新环境。不要改动 KinNovel 目录。

自动部署工具会验证 SSH 主机指纹、全部安装文件摘要和设备二进制 `--help`，备份后安装，失败时尝试回滚；不会自动启动阅读器 UI：

```powershell
python tools/deploy_kindle.py --host <Kindle-IP> --port 2222 --fingerprint <已核对的SSH主机指纹>
```

- 反代：在设置 → 网络中填写反代地址和密钥，再打开“反代开关”。小说、登录、评论、图片及文库统一转发；源站 API 地址保持默认。配置和部署步骤见 [docs/GATEWAY.md](docs/GATEWAY.md)。
- 数据、配置、日志：`/mnt/us/extensions/kindlia/`；日志为 `logs/kindlia.log`。
- 自定义字体：将合法的 `.ttf`、`.otf` 或 `.ttc` 文件放进 **`/mnt/us/extensions/kindlia/fonts/`**，进入阅读设置点击字体项，在列表中选择。字体目录扫描后生效；无效字体回退系统字体。安装包不附带额外字体。
- 本地书：放入 `/mnt/us/extensions/kindlia/books/`，由后续本地书入口使用；存储空间页仅负责缓存统计和清理。
- 网络小说切译文使用已缓存 DTO；机翻文库切翻译/原文选项可能需要重新下载对应卷。人工翻译卷显示下载文件已有内容。
- 书籍详情可进入评论；文库详情点击卷直接下载并阅读。首次下载需要联网。
- 阅读中左右区域翻页，中间点击或下滑呼出调节面板；目录仅保留目录/书签标签。返回目录保留现有分页，阅读会话内缓存最近三个已完成分页的章节。
- 设置“阅读”可选择字体、开启章节预加载；“显示”可切换原生翻页动画、动画阶数和清理间隔；“内容”可调参考日语字号和灰度。
- 登录会话保存在本机数据目录，不保存密码。清洁安装后需重新登录。
- Kindlia 启动器检查 KinNovel 运行锁；使用时先退出另一阅读器。

GPL-3.0-or-later。见 `LICENSE`、`THIRD-PARTY-NOTICES.md`。
