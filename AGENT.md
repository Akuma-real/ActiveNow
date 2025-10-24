# ActiveNow AGENT（精简：仅在线总人数）

本项目现已精简为“仅统计在线总人数”的 WebSocket 服务，并提供一个只读 HTTP 接口查询当前在线值。无房间、无 TTL、无心跳协议与来源白名单逻辑。

---

## 功能概览

- WebSocket 主动推送在线人数变更；首包下发当前人数。
- 支持通过 `socket_session_id` 进行会话去重（同一用户多标签页仅计 1）。
- HTTP 只读接口返回当前在线人数。

---

## 接口

- WebSocket（推送）
  - 路径：`GET /ws`（兼容：`/v1/ws`、`/v1/ws/web`、`/web`）
  - 查询（可选）：`socket_session_id=<稳定ID>`
  - 首包：`{"type":"hello","sid":"...","count":N}`
  - 变更：`{"type":"sync","count":N}`
  - 客户端可发送：`{"type":"updateSid","session_id":"<稳定ID>"}` 更新去重标识

- HTTP（查询）
  - 路径：`GET /v1/metrics/online`
  - 响应：`{"online":N}`

---

## 运行与配置

- 运行：`RUST_LOG=info PORT=8080 cargo run`
- 环境变量：
  - `PORT`：监听端口，默认 `8080`
  - `PING_INTERVAL`：服务器 Ping 间隔（秒）；`>0` 开启，默认关闭
  - `ALLOWED_ORIGINS`（可选）：允许的来源白名单，逗号分隔；支持完整 Origin/域名/域名:端口/后缀通配（如 `*.example.com`）。配置后，缺失或不匹配的 `Origin` 将被拒绝。

---

## 实现要点

- 在线人数由全局 `watch` 通道（`online_tx/online_rx`）维护，所有连接共享。
- 会话去重：优先取请求头 `x-socket-session-id`，否则取查询 `socket_session_id`；连接后也可通过 `updateSid` 更新。
- 首包发送 `hello{ sid, count }`，其后人数变化时发送 `sync{ count }`。
- 可选服务器 Ping（由 `PING_INTERVAL` 控制）。

---

## 目录结构

- `src/main.rs`：进程入口、路由装配、日志输出
- `src/gateway.rs`：WS 接入、消息编解码、在线人数分发
- `src/meta.rs`：会话元数据存储（内存实现），仅保留必要接口
- `src/id.rs`：会话 `sid` 生成

（已删除：房间/TTL/心跳/事件相关文件与逻辑）

---

## 验收建议

1) 打开两个浏览器标签页连接 `/ws`，观察收到 `hello` 与随后 `sync` 的 `count` 变化；
2) 关闭其中一个标签页，另一个应收到 `sync`，`count` 递减；
3) `curl http://localhost:8080/v1/metrics/online` 应返回当前在线值。

---

## 约束与注意事项

- 当前实现不包含房间、TTL 清理与来源白名单；如需恢复相关能力，请在 `gateway` 与 `meta` 层面扩展。
- 当前实现仅支持单实例内存后端；如需跨实例共享，请在未来引入集中式存储再行扩展。
