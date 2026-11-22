# 网易云 api-enhanced（可选本地目录）

默认情况下 Chordial 会**自动下载官方 api-enhanced release 二进制**，不需要
往本目录放任何东西。

本目录只在你想使用自己构建的 Node 版本时使用：

```text
resources/netease-api/
  app.js              # 入口；也支持 server.js / index.js
  package.json
  node_modules/...
  node.exe / node     # 可选；没有则使用 PATH 上的 node
```

存在入口文件时，`netease.mode = "auto"` 会优先启动它；也可以用
`netease.managed.app_dir` 或 `CHORDIAL_NETEASE_APP` 指向任意目录。

一键部署相关配置：

```json
{
  "netease": {
    "mode": "auto",
    "managed": {
      "auto_install": true,
      "install_dir": null,
      "api_version": null,
      "api_sha256": null,
      "binary_path": null,
      "port": null,
      "keep_warm": false
    }
  }
}
```

- `auto_install`：没有本地入口时自动下载官方二进制。
- `install_dir`：安装目录；默认平台数据目录下的 `chordial/netease-api`。
- `api_version`：固定 release tag；默认最新。
- `api_sha256`：额外固定校验值。
- `binary_path`：直接指定已下载的官方二进制。

Chordial 只负责下载、校验、启动和生命周期；api-enhanced 本体仍属于上游项目。
