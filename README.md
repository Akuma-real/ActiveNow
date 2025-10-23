**ActiveNow（Rust，最小 WS 在线人数）**

- 房间粒度在线人数统计；仅 WebSocket；单实例内存 TTL 清理；`watch` 最新人数分发。
- 连接即加入房间；下发 `hello(sid, ttl, count)`；人数变化时 `sync(count)`。

**快速开始**
- 运行：`RUST_LOG=info PORT=8080 cargo run`
- 本地验证：打开 `hack/test.html`，连接 `ws://localhost:8080/v1/ws?room=demo`。

**环境变量**
- `PORT`：默认 `8080`
- `PRESENCE_TTL`：生存时长（秒，默认 `30`）
- `PING_INTERVAL`：服务器 Ping 间隔（秒，`>0` 开启）
- `ALLOWED_ORIGINS`：逗号分隔来源白名单（为空则不限制）

**Docker Compose（推荐）**
- 准备环境：`cp .env.example .env`（按需修改端口与环境变量）
- 本地构建并运行：`docker compose up -d --build`
- 查看日志：`docker compose logs -f activenow`
- 停止并移除：`docker compose down`

使用 GHCR 预构建镜像：
- 打开 `docker-compose.yml`，按注释操作：
  1) 注释掉 `build:` 块；
  2) 取消注释 `image: ghcr.io/...` 行；
  3) 在 `.env` 中设置 `GHCR_OWNER`/`IMAGE_NAME`/`TAG`（默认 `TAG=main`）。
  然后执行：`docker compose up -d`

**GitHub Packages（GHCR）自动构建与推送**
- 已内置工作流：`.github/workflows/docker.yml`
  - 触发：推送到 `main` 分支、推送 `v*`/`release-*` 标签，或手动 `workflow_dispatch`
- 产物：`ghcr.io/<OWNER>/activenow:<tag>`（由 `docker/metadata-action` 生成多种标签：分支、语义化版本、SHA）
- 权限：`packages: write` 使用 `GITHUB_TOKEN` 登录 GHCR

**使用 GHCR 镜像**
- 拉取最新：`docker pull ghcr.io/<OWNER>/activenow:main`
- 指定版本：`docker pull ghcr.io/<OWNER>/activenow:v1.0.0`
- 运行：`docker run --rm -p 8080:8080 ghcr.io/<OWNER>/activenow:main`

说明：若仓库为私有，先登录 GHCR：
`echo <PAT> | docker login ghcr.io -u <YOUR_GH_USERNAME> --password-stdin`
（PAT 至少需要 `read:packages`，推送需 `write:packages`）

**WS 接口**
- URL：`GET /v1/ws?room=<room>`（升级为 WebSocket）
- 客户端 → 服务端：`{"type":"hb"}`（心跳）
- 服务端 → 客户端：
  - `{"type":"hello","sid","ttl","count"}`
  - `{"type":"sync","count"}`（仅人数变化时发送）

**实现要点**
- 使用 tokio `watch` 分发“最新人数”，新订阅者可立即获得当前值；
- 使用单调时间 `Instant` 判断 TTL，避免系统时钟变更影响；
- 每秒清理超时会话；房间人数为 0 时自动回收；
- 可选 `Origin` 白名单与服务器 Ping 兜底。
