# Kindlia

Novelia（轻小说机翻机器人）Kindle 原生客户端，基于 [KinNovel](https://github.com/mmnnwjw/KinNovel) 的 Rust 墨水屏技术栈构建。

[![Version](https://img.shields.io/badge/version-1.0.0-blue.svg)](https://github.com/mmnnwjw/Kindlia)
[![License: GPL-3.0](https://img.shields.io/badge/license-GPL--3.0-green.svg)](LICENSE)
[![Platform](https://img.shields.io/badge/platform-Kindle%20(ARMv7%20Linux)-orange.svg)]()
[![Rust](https://img.shields.io/badge/rust-2021%20edition-lightgrey.svg)]()

<p align="center">
  <img src="docs/images/shelf.png" width="23%" alt="书架：继续阅读与云端收藏夹">
  <img src="docs/images/reader.png" width="23%" alt="阅读器：译文排版与首行缩进">
  <img src="docs/images/reader-menu.png" width="23%" alt="阅读菜单：目录、进度与阅读调节">
  <img src="docs/images/stats.png" width="23%" alt="阅读统计：阅读时长、页数与书籍排行">
</p>

以上为 KPW5 实机（固件 5.17.1）的实际运行截图，依次展示书架、阅读器、阅读菜单与阅读统计。演示书籍为[《极地恋爱》](https://n.novelia.cc/novel/syosetu/n1896br)，正文选用随机章节「第九话　曲折妖精」，阅读字体为[霞鹜新致宋·屏幕阅读版（补全）](https://github.com/lxgw/LxgwNeoXiZhi-Screen/releases/tag/26.08.21)。统计通过实际打开书籍、停留与翻页积累。

[功能特性](#功能特性) • [安装与部署](#安装与使用) • [配置说明](#配置与个性化) • [反代网关](#可选-cloudflare-worker-反代) • [编译打包](#编译与打包) • [许可与说明](#开源许可与说明)

---

## 项目概述

Kindlia 是专为 Amazon Kindle 电子墨水屏设备打造的 Novelia（轻小说机翻机器人）原生阅读客户端。

本项目直接复用并扩展了我另一个项目 **[KinNovel](https://github.com/mmnnwjw/KinNovel)** 的底层基础设施（包括 Framebuffer 硬件驱动、字体与文本排版引擎、分页计算、手势交互和输入法体系），专门对接 Novelia 源站接口与数据协议。纯 Rust 编写，无浏览器运行时，内存占用低且响应迅速，针对电子墨水屏特性进行了深度优化。

Kindlia 与 KinNovel 互相独立运行，两者配置与数据目录彼此隔离；启动脚本内置互斥检测锁，可在同一台设备上共存安装。

---

## 功能特性

- **墨水屏原生性能**：纯 Rust 开发，直接操作 Kindle 帧缓冲设备（`/dev/fb0`）与硬件刷新控制器，支持原生翻页动画、可调动画阶数、定时全刷清残影及前光滑动调节。
- **Novelia 生态接入**：
  - 支持账号登录、会话保存与刷新（不保存密码明文），欢迎登录页支持跳过。
  - 云端收藏夹管理（网络/文库分类）、新建/改名/删除、本地缓存书架与一键继续阅读。
  - 云端阅读历史同步、章节阅读进度记录、支持暂停/恢复历史或删除记录。
  - 覆盖源站网络小说各数据源，支持多条件筛选、搜索、全简介、目录及四大分类排行榜。
  - 文库小说浏览、搜索与分卷 EPUB 一键下载阅读（支持人工精翻与多种机翻卷）。
  - 保留书籍评论区：查看分页评论、发表新评论、回复他人以及修改/删除自己的评论。
- **多翻译源与排版引擎**：
  - 支持 Sakura、GPT、有道、百度等多翻译源自动或手动选择。
  - 支持纯译文、纯日文原文以及中日双语对照模式，可单独微调参考日文字号与灰度。
  - 支持字号、行高、段距、首行缩进、页边距、横竖屏旋转与 OpenCC 简繁转换。
  - 支持从本地字体目录无缝扫描加载 `.ttf`、`.otf` 或 `.ttc` 字体文件。
- **EPUB 与离线支持**：
  - 支持 EPUB2 NCX 与 EPUB3 nav 目录解析、XHTML 正文、内联 CSS 选择器、栅格插图与嵌入式 SVG 封面。
  - 渐进式分页排版，章节后台预加载，相邻页位图缓存。
  - 站点级别隔离本地缓存，支持离线阅读与存储空间用量管理。
- **休眠书封（Sleep Cover）**：原生支持在 Kindle 锁屏休眠时将当前在读书籍的封面自动设为锁屏壁纸。
- **本地阅读统计**：记录阅读时长与翻页数，查看今日、近 7 天、累计数据、连续阅读天数、近 14 天图表及阅读最多的书籍；统计数据仅保存在本机。
- **网络与反代支持**：默认直连源站；同时附带开源的 Cloudflare Worker 边缘网关源码，可零成本部署在 Cloudflare 免费套餐上，用于在受限网络环境下代理 API、鉴权、评论、图片白名单加速与文库 EPUB 流转发。

---

## 界面与导航

底部四栏交互设计全面对齐 KinNovel：

| 栏目 | 功能描述 |
| :--- | :--- |
| **书架** | 浏览与整理云端收藏夹（支持网络小说与文库分类）、本地缓存书架与一键继续阅读 |
| **历史** | 查看云端与本地阅读历史、阅读章节进度同步、暂停/恢复记录与批量清理 |
| **发现** | 网络小说检索与条件筛选、文库分卷下载、各源榜单（日/周/月/总榜）及全站搜索 |
| **我的** | 阅读统计、账号管理、阅读与排版预设、显示与动画设置、前光面板、存储清理及反代网关配置 |

---

## 安装与使用

### 前置要求
1. **已越狱的 Kindle 设备**（支持常见触控机型，如 Paperwhite、Oasis、Basic 等）。
2. 已安装 **[KUAL](https://www.mobileread.com/forums/showthread.php?t=203326)**（Kindle 统一应用启动器）。

---

### 安装

1. 前往项目的 [Releases 页面](https://github.com/mmnnwjw/Kindlia/releases) 下载最新的发布包（例如 `kindlia-1.0.0.zip`）。
2. 将 Kindle 连接至电脑，解压压缩包，将内部的 `kindlia` 文件夹完整拷贝至 Kindle 盘符的 **`extensions/`** 目录：
   ```text
   /mnt/us/extensions/kindlia/
   ├── bin/
   │   ├── kindlia          # 主程序
   │   └── kindlia.sh       # 启动管理脚本
   ├── config.xml           # KUAL 扩展清单
   ├── menu.json            # KUAL 菜单项
   └── config.example.toml  # 配置文件模板
   ```
3. 安全弹出并拔掉 USB 数据线。
4. 打开 Kindle 上的 **KUAL** 菜单，点击 **Kindlia** 启动。首次启动将自动生成配置文件并进入欢迎页。

> **关于版本更新**：日常升级时退出 Kindlia，将新版压缩包原位覆盖解压即可。程序升级默认完整保留用户配置、账号登录会话、字体、本地书架与阅读进度。

---

## 配置与个性化

程序的所有运行数据、配置和字体统一保存在 `/mnt/us/extensions/kindlia/` 目录下。

### 1. 自定义字体
将任意合法的 `.ttf`、`.otf` 或 `.ttc` 字体文件放入：
```text
/mnt/us/extensions/kindlia/fonts/
```
进入 Kindlia 的 **我的 → 设置 → 阅读 → 字体**，刷新后即可从列表中选取使用。

### 2. 配置文件 (`config.toml`)
首次运行后会自动生成 `config.toml`，常见项说明如下：

```toml
# 接口服务地址（默认直连）
api_base_url = "https://n.novelia.cc"       # 小说业务
auth_base_url = "https://auth.novelia.cc"   # 账号认证
forum_base_url = "https://forum.novelia.cc" # 仅用于书籍评论

# 版面排版
font_size = 48                              # 正文字号
line_spacing = 1.4                          # 行高倍数，1.0–2.0，步进 0.05
paragraph_spacing = 0.1                     # 额外段间距（行高倍数），0–2.0，步进 0.05
first_line_indent = true                    # 首行缩进（默认开启）
margin = 34                                 # 页边距
reader_font = ""                            # 字体文件名（空为系统默认）
convert = ""                                # 简繁转换："" (关) / "t2s" (繁转简) / "s2t" (简转繁)

# 翻译偏好
translation = "auto"                        # auto / sakura / gpt / youdao / baidu
show_original = false                       # 显示日文原文
show_translation = true                     # 显示译文
original_size = 85                          # 参考日语字号百分比 (60..120)
original_gray = 80                          # 参考日语灰度 (0 最深..200 最浅)

# 墨水屏刷新与显示
page_turn_animation = true                  # 是否启用原生翻页动画
swipe_steps = 12                            # 动画阶数 (6..60)
full_refresh_every = 6                      # 全刷清理残影间隔 (屏)
prefetch_chapters = false                   # 相邻网文章节预加载
sleep_cover = false                         # 休眠时显示当前在读书籍封面

# 可选反代网关
gateway_enabled = false
gateway_url = ""                            # 如: https://reader.example.com
gateway_key = ""                            # 与 Worker 部署一致的私密令牌
```

---

## 可选 Cloudflare Worker 反代

若直接访问源站或加载外部图片遇到网络阻碍，可自行搭建配套的 Cloudflare Worker 反向代理网关：

- **免费个人额度**：基于 Cloudflare Workers Free 套餐（每天 100,000 次请求），无需任何付费组件。
- **全链路转发**：统一转发小说接口、账号认证、书籍评论、图片白名单访问及文库 EPUB 断点续传流。
- **详细部署指引**：请参考文档 **[docs/GATEWAY.md](docs/GATEWAY.md)**。

部署完成后，在 Kindlia 的 **我的 → 设置 → 网络** 中填入域名与密钥并开启反代开关即可生效。

---

## 编译与打包

工程结构独立，不需要依赖原 KinNovel 源码目录。

### 依赖环境
- Rust (edition 2021)
- Python 3
- `cargo-zigbuild` 与 Python `ziglang`（用于交叉编译目标 `armv7-unknown-linux-musleabihf`）

### 编译步骤

进入 `rust/` 目录：

```powershell
# 1. 代码质量与语法检查
.\build.ps1 check

# 2. 本地宿主机预览（Windows/Linux 桌面调试模式）
.\build.ps1 host

# 3. 交叉编译 Kindle ARMv7 musl 目标
.\build.ps1 kindle
```

*(Linux / macOS / Git Bash 环境下可使用对应脚本 `./build.sh check|host|kindle`)*

### 打包发布

```powershell
cd ..
python tools/package_release.py
```
执行后将在 `dist/` 目录下生成 `kindlia-1.0.0.zip`，可直接部署至设备。分发包仅包含运行文件、配置模板与许可证；使用说明和图文教程请查看本仓库。

---

## 注意事项

- **EPUB 支持规范**：支持 EPUB2 NCX 目录、EPUB3 nav 目录、XHTML 正文、栅格插图及嵌入式 SVG 封面；支持主流 CSS 选择器排版与跨文件注释。暂不支持完整浏览器级别 CSS 浮动、复杂嵌套表格或纯矢量动画 SVG。
- **业务边界**：提供读者端完整的浏览、阅读、搜索、历史、收藏与评论功能；小说投稿编辑、术语表维护及论坛独立板块请通过 Novelia 网页端操作。
- **进程互斥**：Kindlia 启动脚本会在启动时检测 KinNovel 的运行状态锁；若正在运行 KinNovel，请先退出它再打开 Kindlia，避免显示设备发生争抢。

---

## 开源许可与说明

- 本项目基于 **[GNU General Public License v3.0](LICENSE)** (GPL-3.0-or-later) 开源。
- **技术底座**：底层复用自我的另一开源项目 **[KinNovel](https://github.com/mmnnwjw/KinNovel)** 的 Rust 平台驱动、UI 框架及文本渲染管线。复用清单见 [docs/UPSTREAM-FILES.json](docs/UPSTREAM-FILES.json)。
- **源站接口**：源站协议对接自 Novelia（轻小说机翻机器人）源站，协议映射及核对基准见 [docs/PROTOCOL.md](docs/PROTOCOL.md)。
- 第三方组件与依赖声明详见 [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md)。
