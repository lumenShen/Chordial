# Chordial 2.0

> GPUI + Rust 的桌面音乐播放器：**多音源可载可卸 · 本地库 · 多源聚合搜索 · AMLL 等价歌词**。

Chordial 2.0 是一套以 gpui（Rust 原生 GPU UI 框架）为前端的音乐软件架构。
核心是"音乐源适配器"体系：本地音乐库与自部署的
[网易云 api-enhanced](https://github.com/NeteaseCloudMusicApiEnhanced/api-enhanced)
服务都是可插拔的适配器，运行时可以随时加载 / 卸载，聚合搜索会把所有已加载
源的结果并发 fan-out、单遍合并去重。

## 架构

```
crates/
├── chordal-core      # 领域核心（无 UI 依赖）
│   ├── model.rs      #   Track / Collection / Account / Lyrics / 搜索模型
│   ├── source.rs     #   MusicSource / CatalogSource / AccountSource 契约
│   ├── registry.rs   #   SourceRegistry：注册/加载/卸载 + 并发聚合搜索（单源超时隔离）
│   ├── perf.rs       #   性能检查点：环形账本 + 预算告警 + 外接 profiler 钩子
│   └── error.rs
├── chordal-cache     # 磁盘对象缓存池
│                     #   {source}/{kind}/{id} 命名空间 · 默认 5 GiB LRU
│                     #   内容摘要比对（未变化不重写）· 源卸载按 namespace 回收
├── chordal-local     # 适配器 ①：本地音乐库
│                     #   walkdir 单遍扫描 + mtime/len 增量指纹 + symphonia 标签
│                     #   扫描期派生专辑/歌手索引（合成稳定 id）+ 同曲查找
│                     #   O(1) 主键查找 + 预归一化 search_key 子串过滤
├── chordal-netease   # 适配器 ②：网易云（api-enhanced HTTP 服务）
│                     #   /search /song/detail /song/url/v1 /lyric
│                     #   扫码登录 + 用户歌单 + 歌单/专辑/歌手集合页
│                     #   共享 tokio runtime（2 worker）+ 直链 LRU（256, O(1)）
│                     #   stale-while-revalidate：详情/歌词/直链/集合先缓存后刷新
├── chordal-player      # 播放引擎
│   ├── decode.rs       #   symphonia → f32 交错样本（mp3/flac/ogg/wav/aac）
│   ├── http_stream.rs  #   HTTP 渐进式流 → 可 seek 的 MediaSource（边下边播）
│   ├── backend.rs      #   AudioSink：dlopen ALSA（零构建依赖）+ 无头后端
│   ├── analysis.rs     #   可配置频段能量分析（背景鼓点，25Hz 更新）
│   └── player.rs       #   单线程状态机：命令 mpsc / 状态快照 / 事件轮询
├── chordial-amll-gpui  # AMLL 等价歌词组件（独立实现，见 crates/.../README.md）
│   ├── parser.rs       #   LRC / ESLRC 解析（单遍，AMLL 时间语义）
│   ├── document.rs     #   setLyricLines 等价：提前 1s / 背景行合并 / 对唱
│   ├── spring.rs       #   解析弹簧（帧率无关、O(1) 求值）
│   ├── player.rs       #   热行/缓冲行滚动状态机 + 间奏检测（纯逻辑）
│   ├── text.rs         #   gpui 文本整形、逐词几何、强调分组、按行 LRU（≤256 行）
│   ├── render.rs       #   逐字形绘制 / 逐词扫光 / 逐词上浮 / 强调词逐字+辉光 / 呼吸点
│   └── background.rs   #   Mesh Gradient（BHP 网格 CPU 光栅）+ CSS + None
└── chordal-ui          # gpui 前端
    ├── app.rs          #   三栏（侧栏 / 曲目板 / 正在播放）/ 悬浮 Dock / 沉浸歌词
    ├── input.rs        #   支持中文 IME 的单行输入框（EntityInputHandler 全实现）
    ├── lyrics.rs       #   Lyrics → AMLL 模型、封面下载/解码（后台线程）
    └── ...
```

### 可载可卸的适配器

`SourceRegistry` 只认识 `MusicSource` trait 与 `SourceDescriptor`（静态信息 +
延迟构建工厂）。卸载 = 摘除实例并调用 `shutdown()`；本地源实例常驻内存，
重载时索引零丢失。新增一个源 = 实现四个 async 方法 + 注册描述符，UI 与聚合
搜索自动感知（`version` 计数器）。

### 网易云账号与集合页

- 侧栏「登录网易云」支持扫码（`/login/qr/key|create|check`）与手机号 +
  短信验证码（`/captcha/sent`、`/login/cellphone`）；成功后 Cookie 写入配置，
  账号头像/昵称显示在侧栏。启动时用 `/user/account` 静默校验，失效自动回到未登录。
- 登录后侧栏出现「我的歌单」（`/user/playlist`），点击进入歌单页；
  曲目信息旁的「专辑 / 歌手」进入 `/album` / `/artist/*` 集合页。
- 搜索面板提供「歌曲 / 专辑 / 歌手 / 歌单」四类；集合搜索对本地与网易云
  `catalog.search_collections` 并发 fan-out，按 `(类型, 标题, 创建者)` 去重。
- 集合页统一使用 `CollectionPage`（摘要 + 曲目 + 歌手专辑），详情/歌单/
  专辑/歌手数据都走对象缓存；登出按 `{source}/account/` 前缀回收账号级缓存，
  卸载源则回收整个 `{source}/` 命名空间。

### 聚合搜索

```
query ──┬──> source A (timeout 6s) ──┐
        ├──> source B (timeout 6s) ──┼──> round-robin 交错合并（单遍 O(total)）
        └──> ...                    ┘    + dedup_key 哈希去重（O(1)/首）
```

任一源超时 / 报错不影响其它源；合并结果按源优先级交错，同曲（同名同主艺人，
构造期预计算 `dedup_key`）只保留一份；同一源内的同名不同版本不会互相吞掉。

### 歌词

播放曲目时后台并行加载 `song/detail`（封面/时长）与 `/lyric`（LRC + YRC 逐词），
转换后交给 `LyricPlayerView`。逐词歌词自动进入 AMLL 的逐词扫光模式，并叠加
逐词上浮（行离场倒放）与强调词逐字缩放/位移/真高斯辉光；纯 LRC 则按行高亮。
歌词不是独立页面：点击悬浮 Dock 或右栏的歌词入口进入**全窗口沉浸层**（AMLL player
等价对象；传输控件的回弹/双箭头入场/音量喇叭弹跳与 AMLL `MediaButton` 对齐），
背景使用专辑封面驱动的 AMLL Mesh Gradient（可切纯色），
背景网格会跟随可配置频段（默认 4000–10000 Hz）的能量跳动；
页面尺寸/帧率/渲染比例/静态模式均可配置。本地源支持同名 `.lrc`/`.txt`
边车歌词（UTF-8 / UTF-16 / GBK 自动探测）。

## 性能设计（本仓库的硬约束）

- **检查点系统**（`chordal-core/src/perf.rs`）：所有热点路径埋 `PerfScope`
  （合并、扫描、过滤、HTTP 请求、解码 warmup、seek、UI 渲染 pass）。
  每个检查点有微秒预算，超预算 `tracing::warn`；账本是定容环形缓冲
  （观测层自身 O(1) 内存）。`RUST_LOG=debug` 可看全部事件。
- **复杂度护栏测试**（`crates/*/tests/perf_scaling.rs`）：用检查点计时数据
  对 2k/8k/32k（搜索合并）、1k/8k/64k（本地过滤）等三档数据集断言
  "每条目耗时"不超线性增长（4× 数据 → per-item ≤ 2.5×，二次复杂度约 4×），
  把 O(n²)/O(n³) 拦在测试期。
- 关键路径复杂度承诺：
  - 多源合并 + 去重：**O(total)** 单遍（预分配 HashSet、无 rehash）
  - 本地扫描：**O(文件数)** 单遍，增量重扫仅读变更文件的标签
  - 本地搜索：预归一化键上单次 `contains`，命中达到 limit 即提前止步
  - 主键查找：HashMap **O(1)**
  - 直链 LRU：HashMap + 惰性淘汰队列，**均摊 O(1)**，容量恒定
  - UI：`uniform_list` 只渲染可视行；播放状态 200ms 轮询、变化才重绘
  - 歌词：每帧只整形/绘制 `visible_range()`（含 300px overscan），
    状态机 `update` 只遍历可见 + 未收敛行；文本整形按行 LRU（≤256 行）
  - 歌词背景：光栅化像素硬上限 200k，缓冲区跨帧复用，30FPS 默认
  - 播放循环：逐 packet O(帧数)，音量内联乘法，状态写锁 100ms 节流

## 环境要求

- Rust ≥ 1.90（edition 2024）
- Linux：`libxcb.so.1`、`libxkbcommon.so.0`、`libxkbcommon-x11.so.0` 运行时库
  （无需 -devel 包；`.cargo/config.toml` 通过 `-L native` 指向无版本符号链接）
- Windows：WebView2 运行时（Win10/11 自带）；音频走原生 **WASAPI** 共享模式，
  无额外依赖
- 播放：
  - Linux：`libasound.so.2`（Fedora 默认自带；后端运行时 dlopen，缺库时明确报错）
  - Windows：系统默认渲染端点（WASAPI，`AUTOCONVERTPCM` 自动重采样）
  - 其他平台：无输出后端，自动降级为静音（NullSink）
- 网易云源：自部署 [api-enhanced](https://github.com/NeteaseCloudMusicApiEnhanced/api-enhanced)
  （默认 `http://127.0.0.1:3000`）

> 注：`.cargo/config.toml` 是**本机可选配置，不入库**。若仓库放在 NTFS/FUSE
> 挂载盘，请自行创建该文件把 `build.target-dir` 指到本地磁盘（例如
> `~/.cache/chordial-target`），避免链接器 EOVERFLOW；正常 ext4/btrfs 无需此步。

## 使用

```bash
cargo run --release -p chordal-ui
```

配置文件 `~/.config/chordial/config.json`（首次运行自动生成）：

```json
{
  "local_roots": ["/home/you/Music"],
  "netease": {
    "base_url": "http://127.0.0.1:3000",
    "cookie": null,
    "level": "ExHigh",
    "mode": "auto",
    "managed": {
      "node_path": null,
      "app_dir": null,
      "port": null,
      "keep_warm": false,
      "startup_timeout_ms": 15000
    }
  },
  "player_device": null,
  "search_timeout_ms": 6000,
  "cache_dir": "~/.cache/chordial/objects",
  "cache_limit_bytes": 5368709120,
  "download_dir": "/home/you/Music/Chordial Downloads",
  "source_preference": "auto",
  "beat_band_low_hz": 4000.0,
  "beat_band_high_hz": 10000.0
}
```

`cache_limit_bytes` 默认 5 GiB。`source_preference` 取 `auto`（本地音质更高才播本地）、
`prefer_local`、`prefer_online`。缓存目录、上限、下载目录与播放来源都可以在侧栏
「设置」里修改；`cache_dir` 修改后立即切换（旧目录内容保留，需手动清理）。
沉浸歌词背景的鼓点监听频段（默认 4000–10000 Hz）同样可在「设置」里自定义，
播放线程按该频段能量驱动背景网格的缩放/旋转跳动（响应延迟约 40ms）。

### 对象缓存与下载

- 封面、歌词、单曲详情、临时直链都进入 `chordal-cache` 磁盘对象池：
  key = `{source}/{kind}/{id}`，默认上限 5 GiB，超限按 LRU 淘汰。
- 读取策略是 stale-while-revalidate：先返回本地缓存，再在后台拉取并比对
  内容摘要/ETag；未变化只更新元信息，不重写对象。直链带 10 分钟 TTL、
  临近过期自动续期。
- 卸载/注销某个音乐源时按 namespace 回收该源的全部缓存对象
  （`NeteaseSource::shutdown`）。
- 「下载」写入独立的 `download_dir`（`downloads.json` 索引），**不计入缓存配额**；
  之后播放同一曲目直接使用本地文件。
- 在线曲目播放前会按「同名 + 主艺人 + 同专辑」查找本地文件，并用
  档位/码率比较音质；本地更高才播本地，否则在线。已下载文件优先。

### 网易云托管模式（Node sidecar）

`netease.mode`：

- `auto`（默认）：找到 `resources/netease-api/app.js`（或 `server.js` /
  `index.js`）时由 Chordial 启动 Node；找不到则回退 `base_url` 外部服务。
- `external`：只连接用户自己运行的 api-enhanced。
- `managed`：强制由 Chordial 启动 Node，找不到入口会给出可操作错误。

目录约定（放在可执行文件同级，或开发期仓库根目录）：

```text
resources/netease-api/
  app.js              # 或 server.js / index.js
  node_modules/...
  node.exe / node     # 可选；没有则用 PATH 上的 node
```

也可以用 `managed.app_dir` 指向任意 api-enhanced 目录，或设置
`CHORDIAL_NETEASE_APP`。Chordial **不内置 api-enhanced 本体**；需要用户
自行把服务源码/构建产物放到上述目录（或使用外部服务）。

进程行为：

- 首次需要网易云请求时启动，优先 `managed.port`，否则自动挑 `127.0.0.1`
  空闲端口；子进程只监听回环地址。
- 轮询 `/login/status` 直到返回 JSON（默认 15s 超时）；失败会带上最近日志。
- 源卸载时停止进程（`managed.keep_warm = true` 可保留）；进程重启换端口时
  动态更新请求 base URL；`kill_on_drop` 兜底防止残留。
- 日志尾部会脱敏 `MUSIC_U` / `__csrf`。

快捷键：`Ctrl+K` 聚焦搜索 · `Enter` 搜索 · `Ctrl+Space` 播放/暂停 ·
`Ctrl+←/→` 上一/下一首 · `Ctrl+R` 重扫本地库 · `Ctrl+B` 侧栏 · `Ctrl+Q` 退出。

## 测试

```bash
cargo test --workspace            # 单元 + 集成 + 复杂度护栏
cargo test -p chordal-core --release perf_scaling   # 护栏测试建议 release 跑
cargo test -p chordial-amll-gpui --release          # 歌词解析/布局/更新的复杂度护栏
```

播放管线 e2e（`chordal-player/tests/playback_e2e.rs`）生成 WAV 经
RealtimeNullSink 播放，断言暂停/恢复/seek/播完事件 —— 无声卡环境可跑。

## License

MIT。`chordial-amll-gpui` 为 AMLL（GPL-3.0）的行为等价独立重实现，
详见 `NOTICE` 与 crate README。
