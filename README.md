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
│   ├── render.rs       #   逐字形绘制 / 整行扫光 / 逐词上浮 / 强调词逐字+辉光 / 呼吸点
│   └── background.rs   #   Mesh Gradient（BHP 网格 CPU 光栅）+ CSS + None
└── chordal-ui          # gpui 前端
    ├── app.rs          #   三栏 / 悬浮 Dock / 共用 AMLL 播放器列 / 宽屏沉浸层
    ├── player_page.rs  #   竖屏页面模型与队列纵向过渡
    ├── layout_morph.rs #   宽屏/竖屏共享元素 FLIP 几何与补间
    ├── eq_page.rs      #   均衡器桌面控制台 / 紧凑布局
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
转换后交给 `LyricPlayerView`。逐词歌词自动进入整行扫光模式（一条渐变前沿
连续跨字/词推进），并叠加逐词上浮（行离场倒放）与强调词逐字缩放/位移/真高斯辉光；
纯 LRC 则按行高亮。
歌词不是独立页面：点击悬浮 Dock 或右栏的歌词入口进入**全窗口沉浸层**（AMLL player
等价对象；传输控件的回弹/双箭头入场/音量喇叭弹跳与 AMLL `MediaButton` 对齐），
背景使用专辑封面驱动的 AMLL Mesh Gradient（可切纯色），
背景网格会跟随可配置频段（默认 4000–10000 Hz）的瞬态能量做呼吸缩放、
旋转与提亮；包络带自适应基线，持续噪声不会压掉鼓点脉冲；
页面尺寸/帧率/渲染比例/静态模式均可配置。本地源支持同名 `.lrc`/`.txt`
边车歌词（UTF-8 / UTF-16 / GBK 自动探测）。

### 播放页（F2 / 沉浸）

参考视频的逐帧分析与实现说明见 [`docs/portrait-player.md`](docs/portrait-player.md)。

宽屏与竖屏共用同一根 AMLL 播放器列（`amll_player_column`）：封面、曲名/艺人、
进度、传输、音量使用**同一个列宽**，不再有两套 UI。布局只由「歌词是否可见」
决定（`layout_one_active`）：

- **布局 1（宽屏/类平板 + 歌词可见）**：播放器列在左侧，歌词在右列。
- **布局 2（竖屏 / 无歌词 / 收起歌词）**：不显示歌词文字，播放器列水平居中；
  封面、信息、进度、传输、音量、底栏全部同宽。窗口过矮时先缩封面，
  保证控件区最小高度。横向窗口没有歌词时也走这一套。
- **统一底栏**：全屏 / 队列 /（歌词）/ 更多 / 关闭，两套布局由同一个构建函数生成。
- **「…」抽屉**：从底部滑入，收纳睡眠定时（含 15/30/60 分钟二级页）、均衡器、
  歌词开关、队列、音频信息、艺术家与退出播放页；布局 2 的次级入口不再散落在底栏。
- **队列**：纵向滑入；封面与曲名走 `layout_morph` 的共享元素 FLIP，从大图连续
  缩放到页头，而不是整页硬切。
- **Dock⇄全屏**：进入沉浸层时底部 Dock 作为共享布局的第一帧放大成整页，
  封面/曲名飞到沉浸页对应位置；顶部小白条（及其周围的空白区）向下拖动时，
  整页按同一套共享几何收向 Dock，松手由解析弹簧决定回弹或收进 Dock 退出
  （`sheet_drag.rs`）。
- 背景是封面驱动的 AMLL Mesh Gradient；无论界面明暗都强制使用 Mesh，避免
  沉浸/竖屏页出现纯黑底。

### 参数均衡器（F3）

- 两套自适应布局：横屏桌面端为「预设导航 + 频响图/频段推子 + 滤波器检视器」的
  Studio 控制台；竖屏或窄窗为「频响主画布 + 横向频段芯片 + 底部触控检视器 + Tab」。
- 20Hz–20kHz / ±24dB 频响图；点击空白添加峰值频段，拖动控制点同时改频率与
  增益，右键删除；最多 32 段。
- 频段参数：峰值/低架/高架/低通/高通、频率、增益、Q；单频段启用/旁路；
  输入/输出增益使用 0dB 居中的双极性滑杆。
- 预设：V4、AutoEQ（从 `jaakkopasanen/AutoEq` 仓库同步 6000+ 条真实耳机档案，
  搜索/分类、索引与曲线本地缓存、离线可用）、12 条内置预设、个人配置
  （「另存为」）；桌面端先预览曲线再应用，紧凑端单击即应用；全部走同一套
  `EqSettings`。
- 播放线程内实时处理：RBJ 双二阶滤波器链、每声道独立状态、系数平滑与 5ms
  增益斜坡；UI 曲线与播放使用同一组系数。配置持久化到 `config.json` 的
  `equalizer` / `eq_saved_presets`（400ms 防抖）。

## 性能设计（本仓库的硬约束）

- **300Hz 目标**：在 300Hz 及以上显示器上，所有连续动画与交互跟随显示
  刷新率，不设 60/120fps 上限。主线程单帧（draw + prepaint/paint + scene
  提交 + present 调用）目标为 p50 ≤ 1.5ms、p99 ≤ 3.0ms、max ≤ 3.33ms；
  连续动画 10s 不应出现超过 3.33ms 的帧。
- **帧调度**：歌词动画在 `LyricCanvas` 绘制时调用
  `window.request_animation_frame()`；控件/页面过渡在 `ChordialApp::render`
  中按动画状态请求下一帧。没有固定 16ms 定时泵；动画结束后立即停止。
  仅背景 Mesh 流动时按背景光栅帧率（默认 30fps）请求，不空转 300Hz。
- **重绘范围**：主界面按侧栏/列表/播放面板/Dock 分块进入 `batch_layer`，
  同一块的路径（图标、EQ 曲线）合并为一次绘制；歌词只重绘自身视图。
  竖屏播放队列与 EQ 预设列表使用 `uniform_list` 虚拟化；曲目行文本在列表
  快照变化时预计算，render 期间不再 `format!`/`join`。EQ 打开时跳过被
  面板完全遮挡的主列表/正在播放；频响曲线采样按设置与几何缓存，滚动帧
  不再重算 biquad。
- **输入合并**：Windows 平台每帧只派发最后一次鼠标移动、合并当帧滚轮/
  触控板增量；滑杆拖动只通知所属视图，不再 `refresh_windows()` 让所有
  缓存失效。
- **滚动**：滚轮/触控板事件按帧合并；可在「设置 → 通用 → 滚动」开启
  `平滑滚动`（或写 `config.json` 的 `smooth_scroll`，默认 `false`），
  作用于主列表、集合结果、侧栏、正在播放、设置页、抽屉、竖屏队列与
  EQ 页（耳机/频段/预设等全部滚动容器）。平滑与帧率无关（45ms 时间
  常数），300Hz 下每帧推进，结束后停止请求帧；程序化跳转（切换歌单、
  滚动到当前曲目）会重新锚定目标，不会被未完成的动画拉回。
- **帧预算观测**：`GPUI_FRAME_STATS=1 GPUI_TARGET_HZ=300` 会每秒输出
  draw/present/total 的 p50/p95/p99/max 与 miss 数；没有 300Hz 显示器时
  用 `GPUI_BENCH_HZ=300` 按 3.33ms 节奏压测，`CHORDIAL_BENCH_ANIM=1`
  强制持续重绘，`CHORDIAL_START_IMMERSIVE=1` 直接进入沉浸页，
  `CHORDIAL_START_SETTINGS=1` / `CHORDIAL_START_EQ=1` 直接打开设置 / 均衡器；
  `CHORDIAL_EQ_PROBE=rows` 或 `preview` 把 EQ 预设行 / 预览面板替换为空块，
  用于拆分帧成本（仅压测）。
  `CHORDIAL_CONFIG_DIR`/`CHORDIAL_CONFIG` 可隔离配置做可重复压测；
  `CHORDIAL_AUTOPLAY=1` 在本地库扫描完成后自动播放第一首并进入沉浸页，
  用于真实歌词 / 播放路径验证。
- **实测参考**（release，Windows，RTX 5060，`GPUI_BENCH_HZ=300` 合成节奏，
  1455×929 逻辑窗口，统计窗口为最近 240 帧）：
  - 真实本地 WAV + LRC 播放（沉浸歌词）：299.2Hz，draw p50 0.40ms /
    p99 1.6–2.2ms，total p99 ~2.0ms，miss ~0.3%；
  - 桌面主界面 300 曲目列表连续平滑滚动：299.5Hz，prepaint p50 1.03ms、
    paint p50 0.28ms、present p50 0.10ms，total p50 1.52ms / p95 2.35ms /
    p99 2.65ms，0 miss；
  - 均衡器 AutoEQ 列表连续平滑滚动（索引 6000+ 条，视图虚拟化，仅渲染
    可视行）：299.4Hz，total p50 2.2ms / p95 2.8ms / p99 3.7ms（被 EQ
    面板完全遮挡的主列表/正在播放不再绘制；预览曲线的 biquad 采样已按
    设置与画布几何缓存，p99 的剩余波动来自右侧预览面板的元素重建，
    是当前唯一略高于 3.0ms 目标的表面）；
  - 设置中心滚动/交互：total p50 1.87ms / p99 2.34ms。
  以上为“每帧都重绘”的最坏情况；空闲或仅局部交互时成本更低。
  侧栏歌单区使用版本化保留缓存（`gpui::retained`）：数据、滚动偏移与
  鼠标位置未变时直接复用上一帧的布局/绘制范围。
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
      "binary_path": null,
      "auto_install": true,
      "install_dir": null,
      "api_version": null,
      "api_sha256": null,
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
  "beat_band_high_hz": 10000.0,
  "beat_strength": 0.5
}
```

`cache_limit_bytes` 默认 5 GiB。`source_preference` 取 `auto`（本地音质更高才播本地）、
`prefer_local`、`prefer_online`。缓存目录、上限、下载目录与播放来源都可以在侧栏
「设置」里修改；`cache_dir` 修改后立即切换（旧目录内容保留，需手动清理）。
沉浸歌词背景的鼓点监听频段（默认 4000–10000 Hz）与视觉效果强度
（`beat_strength`，0–150%，默认 50%）同样可在「设置 → 播放 → 背景律动」里调整，
播放线程按该频段的瞬态能量驱动背景网格的呼吸缩放/旋转/提亮（响应延迟约 40ms）。

### 对象缓存与下载

- 封面、歌词、单曲详情、临时直链都进入 `chordal-cache` 磁盘对象池：
  key = `{source}/{kind}/{id}`，默认上限 5 GiB，超限按 LRU 淘汰。
- 读取策略是 stale-while-revalidate：先返回本地缓存，再在后台拉取并比对
  内容摘要/ETag；未变化只更新元信息，不重写对象。直链带 10 分钟 TTL、
  临近过期自动续期。
- 卸载/注销某个音乐源时按 namespace 回收该源的全部缓存对象
  （`NeteaseSource::shutdown`）。
- 「下载」写入独立的 `download_dir`（`downloads.json` 索引），**不计入缓存配额**；
  之后播放同一曲目直接使用本地文件。音频落盘时会写回标题/艺人/专辑/序号/年份、
  内嵌歌词与封面（mp3/flac/m4a/wav 等由 `lofty` 统一处理），并生成同名
  `.lrc` 与 `<文件名>.cover.<ext>` 边车；旧版本下载缺少标签时会在服务就绪后
  自动补写，无需重新下载。
- 在线曲目播放前会按「同名 + 主艺人 + 同专辑」查找本地文件，并用
  档位/码率比较音质；本地更高才播本地，否则在线。已下载文件优先。

### 网易云托管模式（一键部署）

`netease.mode`：

- `auto`（默认）：优先已有本地资产/外部服务；都没有时自动下载官方
  发布二进制并启动。
- `external`：只连接用户自己运行的 api-enhanced，不下载、不启动任何进程。
- `managed`：强制使用 Chordial 管理的 api-enhanced；没有则自动下载。

**首次运行会自动完成**：

1. 查询 GitHub `NeteaseCloudMusicApiEnhanced/api-enhanced` 最新 release；
2. 按平台选择资产（`ncm-api-win-x64.exe` / `ncm-api-linux-x64` /
   `ncm-api-macos-x64`）；
3. 下载到平台数据目录（可用 `managed.install_dir` 覆盖），
   校验 release 提供的 SHA-256 后原子安装；
4. 在 `127.0.0.1` 空闲端口启动，轮询 `/login/status` 等待就绪。

不需要安装 Node；官方 release 是独立可执行文件。若想使用自己构建的
Node 版本，把 api-enhanced 源码放到 `resources/netease-api/`（或配置
`managed.app_dir`），存在 `app.js` / `server.js` / `index.js` 时优先用它，
并可使用 `managed.node_path` 指定 Node。

其他配置：

- `managed.auto_install = false`：禁止自动下载，只使用已有目录/外部服务。
- `managed.api_version`：固定 release tag；默认 `latest`。
- `managed.api_sha256`：额外固定 SHA-256；与 release 摘要不一致会拒绝安装。
- `managed.binary_path`：直接指定官方 release 二进制。
- `managed.port`：固定端口；默认自动挑空闲端口。
- `managed.keep_warm`：源卸载后保留进程；应用退出时始终停止。
- `managed.startup_timeout_ms`：启动就绪超时。

进程行为：

- 源卸载时停止进程（`keep_warm` 可保留）；进程重启换端口时动态更新请求
  base URL；`kill_on_drop` / `Drop` 兜底防止残留。
- 启动/下载在后台任务中进行，UI 请求超时中断不会中断安装。
- 日志尾部会脱敏 `MUSIC_U` / `__csrf`；失败错误会带最近日志。

快捷键：`Ctrl+K` 聚焦搜索 · `Enter` 搜索 · `Ctrl+Space` 播放/暂停 ·
`Ctrl+←/→` 上一/下一首 · `Ctrl+R` 重扫本地库 · `Ctrl+B` 侧栏 · `Ctrl+Q` 退出 ·
`F2` 沉浸/竖屏 · `F3` 均衡器 · `F5` 播放第一首 · `F9` 设置。

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
