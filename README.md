**ActiveNow（Rust，最小 WS 在线人数）**

- 房间粒度在线人数统计；仅 WebSocket；单实例内存 TTL 清理；`watch` 最新人数分发。
- 连接即加入房间；下发 `hello(sid, ttl, count)`；人数变化时 `sync(count)`。

**快速开始（源码运行）**
- 运行：`RUST_LOG=info PORT=8080 cargo run`
- 本地验证：打开 `hack/test.html`，连接 `ws://localhost:8080/v1/ws?room=demo`。

**环境变量**
- `PORT`：默认 `8080`
- `PRESENCE_TTL`：生存时长（秒，默认 `30`）
- `PING_INTERVAL`：服务器 Ping 间隔（秒，`>0` 开启）
- `ALLOWED_ORIGINS`：逗号分隔来源白名单（为空则不限制）

**二进制构建与下载（已去除 Docker）**
- 已完全移除 Docker 与 Compose 相关内容与工作流。
- 预发布自动化：推送到 `main` 将自动计算下一版 `v<package>-pre.N`、创建 GitHub Pre-release，并上传各平台二进制（见 `.github/workflows/binaries.yml`）。
  - 版本基于 `Cargo.toml` 中的 `version`，仅递增预发布序号 `N`；无需手动打标签。
- 本地构建发布版：
  - `cargo build --release`
  - 运行：`RUST_LOG=info PORT=8080 ./target/release/activenow`

**使用 .env（可选）**
- 提供 `.env.example`，复制为 `.env` 后按需修改：
  - Mac/Linux：`set -a && source .env && set +a && ./target/release/activenow`
  - Windows PowerShell（临时会话）：`Get-Content .env | foreach { if ($_ -match '^(?<k>[^#=]+)=(?<v>.*)$') { $env:$($Matches.k) = $Matches.v } } ; ./target/release/activenow`



**WS 接口**
- URL：`GET /v1/ws?room=<room>`（升级为 WebSocket）
- 客户端 → 服务端：`{"type":"hb"}`（心跳）
- 服务端 → 客户端：
  - `{"type":"hello","sid","ttl","count"}`
  - `{"type":"sync","count"}`（仅人数变化时发送）

**聚合接口**
- URL：`GET /v1/rooms/active?limit=10`
- 返回：`[{ "room": string, "count": number, "path": string, "title": string }]`
- 说明：当前未存储额外元数据，`path/title` 暂与 `room` 相同，便于前端展示。

**实现要点**
- 使用 tokio `watch` 分发“最新人数”，新订阅者可立即获得当前值；
- 使用单调时间 `Instant` 判断 TTL，避免系统时钟变更影响；
- 每秒清理超时会话；房间人数为 0 时自动回收；
- 可选 `Origin` 白名单与服务器 Ping 兜底。
