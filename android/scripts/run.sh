#!/usr/bin/env bash
# 构建、安装并在模拟器/真机上启动 Chordial，然后跟随日志。
#
# 用法：
#   ./android/scripts/run.sh              # 模拟器：没启动就拉起 AVD
#   ./android/scripts/run.sh --no-follow  # 启动后直接返回
#   CHORDIAL_AVD=other_avd ./android/scripts/run.sh
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
android_dir="$(cd "$here/.." && pwd)"
sdk="${ANDROID_HOME:-${ANDROID_SDK_ROOT:-$LOCALAPPDATA/Android/Sdk}}"
adb="$sdk/platform-tools/adb.exe"
[[ -x "$adb" ]] || adb="$sdk/platform-tools/adb"
avd="${CHORDIAL_AVD:-chordial_api35}"
app_id="com.chordial.app"
follow=true
[[ "${1:-}" == "--no-follow" ]] && follow=false

"$adb" start-server >/dev/null

has_emulator() {
  "$adb" devices | awk '/^emulator-[0-9]+[[:space:]]+device/{found=1} END{exit !found}'
}

if ! has_emulator; then
  echo "==> 启动模拟器 $avd（GPU: host）"
  "$sdk/emulator/emulator.exe" -avd "$avd" -gpu host -no-audio -no-boot-anim \
    >/tmp/chordial-emulator.log 2>&1 &
  "$adb" wait-for-device
fi

echo "==> 等待系统启动"
for _ in $(seq 1 180); do
  if [[ "$("$adb" shell getprop sys.boot_completed 2>/dev/null | tr -d '\r')" == "1" ]]; then
    break
  fi
  sleep 2
done

target_abi="$("$adb" shell getprop ro.product.cpu.abi | tr -d '\r')"
case "$target_abi" in
  x86_64) abi="x86_64" ;;
  arm64-v8a) abi="arm64-v8a" ;;
  *) abi="arm64-v8a" ;;
esac

"$here/build.sh" debug "$abi"

apk="$android_dir/app/build/outputs/apk/debug/app-debug.apk"
apk_path="$apk"
command -v cygpath >/dev/null 2>&1 && apk_path="$(cygpath -w "$apk")"

echo "==> 安装 $apk"
"$adb" install -r -t "$apk_path"

echo "==> 启动 $app_id"
"$adb" shell am force-stop "$app_id" || true
"$adb" logcat -c
"$adb" shell am start -n "$app_id/.MainActivity"

if [[ "$follow" == true ]]; then
  echo "==> logcat（Ctrl-C 退出）"
  exec "$here/log.sh"
fi
