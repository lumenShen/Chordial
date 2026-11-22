# 网易云 api-enhanced sidecar

把可运行的 [api-enhanced](https://github.com/NeteaseCloudMusicApiEnhanced/api-enhanced)
放在本目录，Chordial 的 `netease.mode = "managed"`（或 `auto` 检测到入口时）
会启动它。

期望结构：

```text
resources/netease-api/
  app.js              # 入口；也支持 server.js / index.js
  package.json
  node_modules/...
  node.exe / node     # 可选；没有则使用 PATH 上的 node
```

打包时把本目录放到可执行文件同级：

```text
Chordial.exe
resources/netease-api/app.js
```

开发期也可以放在仓库根目录的 `vendor/netease-api/`，或用环境变量
`CHORDIAL_NETEASE_APP` / 配置 `netease.managed.app_dir` 指定。

Chordial 只负责：

- 挑 `127.0.0.1` 空闲端口（或 `managed.port`）
- 以 `HOST=127.0.0.1 PORT=<port>` 启动 `node app.js`
- 轮询 `/login/status` 等待就绪
- 源卸载时停止进程（`managed.keep_warm` 可保留）
- 进程重启换端口时自动更新请求地址

Chordial 不内置 api-enhanced 本体，也不代替你更新它。
