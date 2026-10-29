# AutoNovel 实际协议核对

核对日期：2026-10-22。固定基准：`ce6fcb3c49675097d78b1d87b06eb8390c7db963`。

## 方案与实际工程的差异

1. 实际不是三个同级的 `api/server/web` package。`apps/api` 是 Go/chi 服务，目前注册 `/v1/banner` 与 `/v1/admin/banner`；小说业务仍在 Kotlin `server/src/main/kotlin/api`。
2. `deploy/web/Caddyfile` 将 `/api/v1/banner`、`/api/v1/admin/*` 转给 Go，其余 `/api*` 去掉前缀后转给 Kotlin `server:8081`。客户端使用站点 `/api/novel`，不能误接 Go `/v1/novel`。
3. `page=0` 起始，`pageSize=1..100`。列表响应是 `{items, pageNumber}`，`pageNumber` **是总页数**，无 `total/itemsPerPage` 等臆造字段。
4. `provider=""` 直接返回空列表，所以客户端默认显式传六个已注册来源。
5. 服务端 `filterLevel.isNsfw` 对“全部”和“R18”都要求访问权限，匿名默认必须选择 `level=1`（一般向）。本轮没有绕过登录权限。
6. Kotlin ChapterDto 仍包含 `baiduParagraphs`，但当前 web 的 DTO/段落选择器省略百度。Kindlia 保留可选百度兼容服务端；优选顺序 Sakura > GPT > 有道 > 百度。
7. web 在没有任何指定译文时输出缺译提示；Kindlia 按方案的客户端降级意图提示并显示日语原文，属于明确的客户端策略差异。

## 请求/响应映射

| 请求 | 参数/响应 | Kindlia |
|---|---|---|
| `GET /api/novel` | page, pageSize, query, provider(逗号列表), type, level, translate, sort | `api::ListQuery` / `model::Page<NovelOutline>` |
| `GET /api/novel/{providerId}/{novelId}` | `NovelDto`，不自带 providerId/novelId，调用方保留 key | `model::Novel` + `NovelKey` |
| `GET /api/novel/{providerId}/{novelId}/chapter/{chapterId}` | `ChapterDto`，无整数章节排序号 | `model::Chapter` + 请求 chapter_id |

所有字符串路径段逐字节 percent encode；本地 key 使用结构序列化的 SHA-256，避免分隔符碰撞与路径穿越。`api_base_url` 接受站点根或末尾 `/api`，也支持反代路径前缀；按标准 URL 规范归一后隔离缓存。

| 字段 | 实际含义与处理 |
|---|---|
| titleJp/titleZh | 中文为 null/空白时显示日文 |
| authors | `{name, link?}` 数组；不是单作者字符串 |
| type | 中文枚举序列化值（连载中/已完结/短篇），可空 |
| toc[].chapterId | null 为卷标题，保留并不登记点击区域 |
| toc[].createAt / syncAt / updateAt | Unix 秒；均按服务端的 Long/nullable 处理 |
| paragraphs | 原文顺序是对齐基准，空白行保留 |
| sakura/gpt/youdao/baiduParagraphs | 整个数组可缺失或 null；按索引对齐，短数组缺失项回退原文并提示；有意空字符串不当成缺译 |
| `<图片>` | 仅检查原文行前缀，截掉 4 个字符作为 URL；双语只生成一个图片块 |
| prevId/nextId | 首选章节响应，缺失时用缓存目录中有效 chapterId 的前后相邻项 |
| favored / lastReadChapterId | 服务端可空；0.2.0 接入云收藏及历史，本地精确分页锚点单独保存 |

无需登录的读取：列表/详情通过 `authenticateDb(optional=true)`，章节路由不在强制登录块中。匿名读取仍可能受源站分级权限、源小说删除、爬虫错误或站点网关限制。客户端不会将网关 HTML 当成成功 JSON 缓存。

章节缓存保存 DTO 的全部已知章节字段，不缓存排版后的 HTML/位图替代正文，切译文时不联网。插图采用单独原图缓存与按显示尺寸的内存灰度图。部分设置和 paragraph anchor 是本地协议，不假装兼容源站云进度。

## 固定源码链接

- [Go api 实际路由](https://github.com/auto-novel/auto-novel/blob/ce6fcb3c49675097d78b1d87b06eb8390c7db963/apps/api/main.go)
- [站点 Caddy 分流](https://github.com/auto-novel/auto-novel/blob/ce6fcb3c49675097d78b1d87b06eb8390c7db963/deploy/web/Caddyfile)
- [Kotlin 小说路由、NovelDto、ChapterDto、列表过滤](https://github.com/auto-novel/auto-novel/blob/ce6fcb3c49675097d78b1d87b06eb8390c7db963/server/src/main/kotlin/api/RouteWebNovel.kt)
- [列表 DTO](https://github.com/auto-novel/auto-novel/blob/ce6fcb3c49675097d78b1d87b06eb8390c7db963/server/src/main/kotlin/api/model/WebNovel.kt)
- [页码验证](https://github.com/auto-novel/auto-novel/blob/ce6fcb3c49675097d78b1d87b06eb8390c7db963/server/src/main/kotlin/api/UtilPage.kt)
- [总页数模型](https://github.com/auto-novel/auto-novel/blob/ce6fcb3c49675097d78b1d87b06eb8390c7db963/server/src/main/kotlin/infra/common/Common.kt)
- [网页 API 调用](https://github.com/auto-novel/auto-novel/blob/ce6fcb3c49675097d78b1d87b06eb8390c7db963/web/src/api/WebNovelApi.ts)
- [网页模型](https://github.com/auto-novel/auto-novel/blob/ce6fcb3c49675097d78b1d87b06eb8390c7db963/web/src/model/WebNovel.ts)
- [网页段落组装](https://github.com/auto-novel/auto-novel/blob/ce6fcb3c49675097d78b1d87b06eb8390c7db963/web/src/pages/reader/components/BuildParagraphs.ts)

0.2.0 新增协议见下文。


## 0.2.0 普通读者协议

认证与评论为源站另行依赖的服务，已核对本地固定源码快照：auth `5d1e8a2640b68050b97cc0a7b3b9b3859efdd4d4`、forum `1cc0de616c9d89c5a5043c912f625c04c1515d76`。地址可在配置分别设置，不能将它们误拼到小说服务下。

| 功能 | 实际请求 |
|---|---|
| 登录 | `POST https://auth.novelia.cc/api/v1/auth/login`，JSON `{username,password,app:"n"}`；响应正文为原始 access token，并设置 refresh-token Cookie |
| 刷新/退出 | `POST /api/v1/auth/refresh?app=n` / `POST /api/v1/auth/logout`，带 refresh Cookie；小说和评论请求带 Bearer access token |
| 收藏夹 | `GET /api/user/favored`；`POST user/favored-web` 或 `user/favored-wenku` 新建；`PUT/DELETE .../{folder}` 改名/删除 |
| 收藏内容 | `GET user/favored-web/{folder}` / `user/favored-wenku/{folder}`；`PUT/DELETE .../{folder}/{provider}/{novel}`，文库不带 provider |
| 历史 | `GET /api/user/read-history`；`PUT .../{provider}/{novel}`，纯文本 chapterId；`DELETE` 删除单项/全部；`GET/PUT/DELETE .../paused` 查询/暂停/恢复 |
| 文库 | `GET /api/wenku` 列表；`GET /api/wenku/{id}` 详情，volumeZh 为文件名数组，volumeJp 包含 volumeId 和各译文进度 |
| 人工卷下载 | `/files-wenku/{id}/{encoded volumeId}` |
| 机翻卷下载 | `/api/wenku/{id}/file/{encoded volumeId}?mode=zh或zh-jp&translationsMode=priority&translations=sakura&translations=gpt&translations=youdao&filename=...`，返回重定向到生成文件 |
| 排行 | `/api/novel/rank/syosetu` / `kakuyomu`；中文 type/range/status/genre，syosetu 的 page 从 1 起 |
| 书籍评论 | `https://forum.novelia.cc/api/v1/external/comment/novel/{subject}`，subject 为 web-provider-novel 或 wenku-id；GET page 从 1 起、page_size；POST content/rootId |
| 回复与编辑 | `GET .../{subject}/{rootId}/reply`；`PATCH/DELETE /api/v1/external/comment/novel/{commentId}` |

主站列表页码从 0 起，评论从 1 起；不要混用。文库生成端不支持纯 jp mode 且要求至少一个 translations 参数。Kindlia 在原文阅读时获取双语文件，按源站 `opacity:0.4` 的原文段落标记剔除对应译文，保留未翻译段落。人工卷不做语言猜测。UTF-8 TXT 文件也可作为单章节导入。

具体会话、缓存、评论操作需用户实测，不以接口实现代替源站在线验收。
