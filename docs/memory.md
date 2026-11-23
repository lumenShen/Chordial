# 运行内存观测与优化

Chordial 提供三层内存观测，覆盖「总量 → 归因 → 时间线」：

| 层 | 入口 | 数据 |
| --- | --- | --- |
| 全局总量 | 设置 → 存储 → **运行内存** | 进程常驻 RSS、峰值、虚拟地址空间；堆活跃/峰值；分配/释放次数 |
| 归因 | 同一区块的分项 | 音频流缓冲、封面纹理、歌词模型、背景光栅、本地音乐库 |
| 时间线 | 设置 → 通用 → **画面性能浮窗** | FPS / 1% Low / CPU / 常驻内存 / 堆活跃，500ms 刷新 |

数据面实现于 `crates/chordal-core/src/mem.rs`：

- `TrackingAllocator`：`#[global_allocator]` 包装系统分配器，原子计数堆活跃、
  峰值与分配次数。单次分配只多一次原子加法。
- `process_memory()`：Windows 走 `K32GetProcessMemoryInfo`（Working Set），
  Linux 读 `/proc/self/statm` + `VmHWM`，macOS 用 `getrusage` 的峰值。
- `process_cpu_time()`：用户态 + 内核态累计 CPU 时间，供浮窗做差分。
- `Gauge`：业务侧主动登记的已知大块内存，避免只有总量无法定位。
- 采样线程：默认 2s 一次，写入容量固定的环形账本
  （`SAMPLE_CAPACITY = 300`，约 10 分钟），观测层自身内存有上界。

## 环境变量

| 变量 | 默认 | 作用 |
| --- | --- | --- |
| `CHORDIAL_MEM_INTERVAL_MS` | `2000` | 采样间隔；`0` 关闭采样线程 |
| `CHORDIAL_MEM_TRACE` | 关闭 | 设为任意值后每次采样写一条 `chordial::mem` INFO 日志 |
| `CHORDIAL_MEM_WARN_MB` | `2048` | 常驻内存告警阈值（MiB），超过后每 60s 告警一次；`0` 关闭 |

日志示例：

```text
RUST_LOG=chordial::mem=info CHORDIAL_MEM_TRACE=1 chordial
```

```
mem.sample rss_mb=187 peak_rss_mb=214 live_kb=48210 peak_live_kb=61120
           audio_stream_kb=23840 cover_kb=1024 lyrics_kb=96
           background_kb=1024 library_kb=12840
```

## 已落地的内存优化

1. **切歌递归滞留（最大项）**：播放线程原先在 `play_item` 内递归处理新的
   `Play` 命令，旧曲目的 `decoder` 与 `HttpMediaSource`（整首已下载字节）
   会一直压在调用栈上，直到最新一首结束。现在 `play_item` 返回
   `Some(next)` 由外层迭代续播，切歌瞬间释放上一首的全部缓冲。
2. **HTTP 流精确预留**：拿到 `Content-Length` 后按总长精确预留（上限
   64 MiB），消除 `Vec` 倍增扩容最多一倍的容量冗余；无长度的 chunked 流
   在下载完成后 `shrink_to_fit`。
3. **封面解码上限**：最长边限制从 8192 收紧到 4096，解码临时分配上限从
   320 MiB 收紧到 128 MiB。最终纹理本来就只有 512px，超限封面直接拒绝，
   避免为了无收益的像素付出数百 MiB 峰值。
4. **去掉整图 RGBA 副本**：AMLL 视图原先同时保留一份 512² RGBA（约 1 MiB）
   用于切换背景渲染器。现在只缓存 32×32 调色纹理（4 KiB），切换时重烘焙。
5. **背景双缓冲销毁**：`MeshGradientRenderer::dispose` 现在同时收缩
   `output` / `scratch`，退出沉浸层后不再保留旧分辨率的大缓冲。
6. **背景帧缓冲回收复用**：软件光栅每帧交出一块整帧 BGRA 给 `RenderImage`，
   旧帧从 sprite atlas 移除后，像素缓冲会交回渲染器（最多缓存 2 块同尺寸），
   下一帧直接复用。30fps 下按 1–2 MiB/帧计算，相当于消除约 30–60 MiB/s
   的分配/清零抖动，只保留每帧 `fill(0)`。

## 一次实测（debug 构建，Windows）

隔离配置下启动并打开设置「存储」页，500ms 采样：

```text
rss_mb=97 peak_rss_mb=101 live_kb=7507 peak_live_kb=8333
audio_stream_kb=0 cover_kb=0 lyrics_kb=0 background_kb=0 library_kb=0
```

空闲 10 分钟内 RSS 与堆活跃保持水平线，分配次数持续增长但释放次数同步
增长——说明是短生命周期分配（gpui 文本/布局），不是滞留对象。

## 排查建议

- 常驻内存持续上涨：先看设置页「音频流缓冲」——正常应约等于当前曲目大小；
  若快速切歌后仍线性增长，检查是否又引入了持有 `HttpMediaSource` 的递归/环。
- 堆峰值远高于当前值：多为封面解码或背景光栅的瞬时缓冲，对照上面第 3/4 项。
- 本地音乐库很大时，「本地音乐库」分项随曲目数线性增长，属于预期；
  可缩小扫描根目录或拆分库。
