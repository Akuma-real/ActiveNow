**ActiveNow（精简版：仅在线人数）**

- 仅提供“当前在线人数”功能；无房间、无 Presence 事件、无聚合统计。
- 通过 WebSocket 主动推送人数变化；提供一个 HTTP 只读接口获取当前在线值。

**快速开始**
- 运行：`RUST_LOG=info PORT=8080 cargo run`

**环境变量**
- `PORT`：默认 `8080`
- `PING_INTERVAL`：服务器 Ping 间隔（秒，`>0` 开启）
- `ALLOWED_ORIGINS`：允许的来源白名单（逗号分隔）。支持：
  - 完整 Origin：`https://example.com:443`
  - 域名或域名:端口：`example.com`、`example.com:3000`
  - 通配后缀：`*.example.com` 或 `.example.com`
  - 特殊：`*` 表示放行所有（不推荐）
  - 说明：仅对浏览器请求有效；非浏览器可无 `Origin` 头，若配置白名单且缺失 `Origin` 将被拒绝。

**接口**
- WebSocket：`GET /ws`（兼容别名：`/v1/ws`、`/v1/ws/web`、`/web`）
  - 查询参数（可选）：`socket_session_id=<稳定ID>`，用于同一用户多标签页合并为 1 个会话。
  - 首包：`{"type":"hello","sid":"...","count":N}`
  - 推送：`{"type":"sync","count":N}`（在线人数变化时）
  - 客户端可在连接后发送：`{"type":"updateSid","session_id":"<稳定ID>"}` 更新去重标识。
- HTTP：`GET /v1/metrics/online`
  - 响应：`{"online":N}`
  - 示例：`curl -s http://localhost:8080/v1/metrics/online`

**浏览器示例**
```html
<script>
const sid = localStorage.sid || (localStorage.sid = crypto.randomUUID());
const ws = new WebSocket(`ws://${location.host}/ws?socket_session_id=${encodeURIComponent(sid)}`);
ws.onmessage = (e) => {
  const msg = JSON.parse(e.data);
  if (msg.type === 'hello' || msg.type === 'sync') {
    console.log('online:', msg.count);
  }
};
</script>
```

**实现说明**
- 使用 `watch` 通道维护与分发在线人数，所有连接共享同一计数源。
- 通过（可选）`socket_session_id` 将同一用户的多连接视作 1 个会话；断开时自动扣减。
 - 可选服务器 Ping（由 `PING_INTERVAL` 控制）。
