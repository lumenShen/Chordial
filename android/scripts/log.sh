#!/usr/bin/env bash
# 查看 Chordial 的 Android 日志。
#
#   ./android/scripts/log.sh          # 跟随 logcat（Rust stdout / panic / 启动错误）
#   ./android/scripts/log.sh --file   # 读取应用私有目录里的 chordial.log
#   ./android/scripts/log.sh --clear  # 清空 logcat
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
sdk="${ANDROID_HOME:-${ANDROID_SDK_ROOT:-$LOCALAPPDATA/Android/Sdk}}"
adb="$sdk/platform-tools/adb.exe"
[[ -x "$adb" ]] || adb="$sdk/platform-tools/adb"
app_id="com.chordial.app"

case "${1:-}" in
  --clear)
    "$adb" logcat -c
    ;;
  --file)
    # Rust 侧 tracing 同时写入 files/logs/chordial.log（run-as 不需要 root）。
    # 整个远端命令必须作为一个参数传给 adb shell，否则 sh -c 会在设备端被拆开。
    "$adb" shell "run-as $app_id sh -c 'tail -n 300 files/logs/chordial.log'"
    ;;
  *)
    # Rust 的 println!/eprintln! 在 Android 上落到 RustStdoutStderr；
    # panic 也走 stderr。gpui 的 log 目标与 Android 运行时错误一并保留。
    exec "$adb" logcat -v time \
      RustStdoutStderr:V RustPanic:V AndroidRuntime:E DEBUG:V libc:V \
      chorus:V gpui:V '*:S'
    ;;
esac
