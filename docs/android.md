# Android 构建与模拟器调试

Chordial 的 Android 形态是一个 **Kotlin 壳 + Rust cdylib**：

```
MainActivity (SurfaceView + Choreographer)
        │  JNI
        ▼
libchordial_android.so  (crates/chordal-android)
        │
        ▼
chordal-ui::run_android → gpui Android 平台层（Blade/Vulkan + ANativeWindow）
```

Kotlin 只负责窗口、Surface 生命周期、输入与帧节拍；UI、业务与音频解码全部
复用桌面端同一套 Rust 代码（`crates/chordal-ui`）。

## 1. 前置条件

| 组件 | 版本 / 说明 |
| --- | --- |
| Rust | 1.90+，并安装 android target |
| cargo-ndk | 4.x（`cargo install cargo-ndk`） |
| JDK | **17–21**（AGP 8.13 不支持 JDK 25；脚本会自动回退到 Android Studio JBR） |
| Android SDK | platform `android-36`（`compileSdk = 36`） |
| NDK | 任意已安装版本；Gradle 自动取 `$SDK/ndk/` 下最高版本，也可 `-Pchordial.ndkVersion=` 指定 |
| Gradle | 仓库自带 wrapper（8.14.3） |

```bash
rustup target add aarch64-linux-android x86_64-linux-android
cargo install cargo-ndk
sdkmanager "platforms;android-36" "ndk;29.0.13846066"
```

`android/local.properties` 写入 SDK 路径（不入库；Windows 上请用正斜杠，避免
Properties 把反斜杠当转义符）：

```properties
sdk.dir=C:/Users/you/AppData/Local/Android/Sdk
```

## 2. 构建

```bash
# 模拟器（默认 debug + x86_64）
./android/scripts/build.sh

# 真机 release（arm64）
./android/scripts/build.sh release arm64-v8a

# 多 ABI
./android/scripts/build.sh release arm64-v8a x86_64
```

直接调 Gradle 等价于：

```bash
./android/gradlew -p android :app:assembleDebug -Pchordial.abis=x86_64
```

产物：

- APK：`android/app/build/outputs/apk/<variant>/app-<variant>.apk`
- Rust 动态库：`android/app/build/rustJniLibs/<variant>/<abi>/libchordial_android.so`

debug 的 Rust `.so` 带完整 DWARF 时约 600 MB。Gradle 任务在 cargo 结束后
自动调用 NDK `llvm-strip --strip-debug`（保留符号表，panic 栈仍可解析函数名），
产物降到约 53 MB。需要行号时用 `target/<triple>/debug/libchordial_android.so`
（未裁剪的原始文件）做 `llvm-addr2line`。

### 构建管线

`android/app/build.gradle.kts` 里注册了两个 Exec 任务：

| Gradle 任务 | 行为 |
| --- | --- |
| `cargoBuildDebug` | `cargo ndk -t <abis> -o build/rustJniLibs/debug build -p chordal-android --lib` |
| `cargoBuildRelease` | 同上 + `--release` |

`preDebugBuild` / `preReleaseBuild`、`merge*JniLibFolders` 依赖对应任务，因此
`assemble*` 会先自动交叉编译。SDK/NDK 路径从 `local.properties` 或
`ANDROID_HOME` 解析后注入 `ANDROID_HOME` / `ANDROID_NDK_HOME`，不依赖开发者
shell 里已有的环境变量。

ABI 通过 `-Pchordial.abis=a,b` 控制，同时作用于 `abiFilters` 与 cargo 的
`-t` 参数，两边不会漂移。

## 3. 模拟器调试

### 创建 AVD

```bash
avdmanager create avd -n chordial_api35 \
  -k "system-images;android-35;google_apis;x86_64" -d pixel_7
```

gpui 走 **Vulkan**，所以 AVD 必须开 GPU：

```ini
# ~/.android/avd/chordial_api35.avd/config.ini
hw.gpu.enabled = yes
hw.gpu.mode = host
hw.ramSize = 4096
hw.keyboard = yes
```

> **必须用 `-gpu host`**。`swiftshader_indirect` 的 Vulkan 只暴露核心 1.2、
> 没有 dynamic rendering / descriptor indexing，Blade 无法初始化（会
> `NoSupportedDeviceFound`）。宿主 GPU 走 gfxstream，报告 Vulkan 1.3，
> 功能完整。
>
> 另外，Vulkan 1.3 会把 inline uniform block / dynamic rendering 等扩展
> 提升为核心，驱动可能不再枚举扩展名。仓库已 vendor `blade-graphics` 并放宽
> 该判断（见 `vendor/blade-graphics/PATCHES.md`），否则会误判为不支持。

### 一键运行

```bash
./android/scripts/run.sh              # 启动 AVD（如需）→ 构建 → 安装 → 启动 → logcat
./android/scripts/run.sh --no-follow  # 只安装启动
./android/scripts/log.sh --file       # 读应用私有目录里的 chordial.log
```

手动操作：

```bash
adb install -r -t android/app/build/outputs/apk/debug/app-debug.apk
adb shell am start -n com.chordial.app/.MainActivity
adb logcat -v time RustStdoutStderr:V AndroidRuntime:E '*:S'
adb exec-out screencap -p > shot.png
```

> gpui 渲染在一个 `SurfaceView` 里，`uiautomator dump` 拿不到控件树；
> 交互调试用截图 + `adb shell input tap/swipe`。

## 4. 存储与路径

Android 没有 XDG 目录，`dirs` 会全部返回 `None`。`MainActivity` 在
`NativeBridge.start` 之前调用 `setStorageDirs(filesDir, cacheDir)`，Rust 侧写入
环境变量并建目录：

| 环境变量 | 实际路径 | 用途 |
| --- | --- | --- |
| `CHORDIAL_CONFIG_DIR` | `files/config.json` | 配置、登录 Cookie、播放偏好 |
| `CHORDIAL_LOG_DIR` | `files/logs/` | `chordial.log` + 崩溃报告 |
| `CHORDIAL_MUSIC_DIR` | `files/music/` | 本地音乐库默认根目录 |
| `CHORDIAL_DOWNLOAD_DIR` | `files/downloads/` | 下载落盘 |
| `CHORDIAL_CACHE_DIR` | `cache/chordial/objects/` | 封面/歌词/直链对象缓存 |

往模拟器里塞几首歌：

```bash
adb push song.flac /sdcard/Download/
adb shell run-as com.chordial.app mkdir -p files/music
adb shell run-as com.chordial.app cp /sdcard/Download/song.flac files/music/
```

`files/` 是应用私有目录，`run-as` 无需 root 即可读写与查看日志。

## 5. 排障

| 现象 | 原因 / 处理 |
| --- | --- |
| `Unsupported class file major version` | Gradle 跑在 JDK 25 上；脚本会自动选 JBR 21，手动跑 Gradle 时 `export JAVA_HOME=<Android Studio>/jbr` |
| `local.properties` 的 SDK 路径变成 `C:Users...` | Properties 转义；用正斜杠 `C:/...` |
| `Task with name 'preDebugBuild' not found` | AGP 惰性注册变体任务；用 `tasks.matching{...}.configureEach` 而不是 `tasks.named` |
| `cargo ndk panicked: unknown package` | `-p` 必须写 **包名** `chordal-android`，不是库名 `chordial_android` |
| `NoSupportedDeviceFound` | 设备是 Vulkan 1.3、扩展已提升为核心但未枚举；已由 `vendor/blade-graphics` 补丁处理。若仍失败，确认用的是 `-gpu host` |
| `Unable to load cmd_begin_rendering_khr` | Vulkan 1.3 只导出核心 `vkCmdBeginRendering`；已由 Blade 补丁分派核心入口 |
| `Unable to load wait_semaphores_khr` | 同上，`vkWaitSemaphores` 已提升为核心 1.2 |
| 字体解析 panic（`.SystemUIFont`） | fontdb 不扫描 Android 系统字体；`vendor/gpui` 已手动加载 `/system/fonts` |
| 界面能显示但点击无反应 | 触摸坐标需扣除 SurfaceView 的窗口偏移并除以 density；见 `MainActivity.onTouchEvent` 与 gpui `on_touch` |
| `Cannot find AVD system path` | AVD `config.ini` 的 `image.sysdir.1` 不要带 `Sdk\` 前缀；或显式设 `ANDROID_SDK_ROOT` |
| APK 体积异常（如 611 MB） | 上一次大 `.so` 留在增量 APK 里的稀疏空洞；删掉 `app-debug.apk` 重新打包 |
| 安装很慢 | debug `.so` 带完整调试符号；Gradle 任务会自动 `llvm-strip --strip-debug`（约 53 MB） |

## 6. 已验证状态

在 `chordial_api35`（x86_64 / API 35 / `-gpu host`）上验证：

- Rust 库加载、Vulkan 设备创建、Surface 交换链正常；
- 首帧渲染出完整界面（底部 Tab、空库引导、管理音源页）；
- 点击 / Tab 切换 / 系统返回键均生效；
- 应用私有目录、日志与崩溃报告落盘正常；
- 本地库扫描、AAudio 输出后端（API 26 低延迟共享流）已接入；
- 模拟器需要带音频启动才能听到声音：不要加 `-no-audio`。

崩溃报告位置：`files/logs/crash-latest.log`（Rust panic）与
`fatal-*.log`（原生信号）。

## 6. 已知缺口

- **本地音乐库**：默认根目录是应用私有 `files/music`；尚未接 MediaStore，
  也没有 SAF 目录选择器。
- **输入法**：`NativeBridge.imeInput` 已打通到 gpui，但 Kotlin 侧还没有
  `InputConnection`，中文输入需要补。
- **后台播放**：`PlaybackService` 是空实现，`FOREGROUND_SERVICE_MEDIA_PLAYBACK`
  权限已声明但尚未启动前台服务。
- **release 签名**：未配置 `signingConfigs`，release APK 只能本地 `debug` 签名安装。
