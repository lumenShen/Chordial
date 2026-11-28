#!/usr/bin/env bash
# 构建 Chordial Android APK。
#
# 用法：
#   ./android/scripts/build.sh                     # debug + x86_64（模拟器）
#   ./android/scripts/build.sh release             # release + arm64-v8a（真机）
#   ./android/scripts/build.sh debug x86_64 arm64-v8a
#
# 说明：Rust 侧由 Gradle 任务 cargoBuildDebug/cargoBuildRelease 调用
# `cargo ndk` 交叉编译，产物落在 app/build/rustJniLibs/<variant>/<abi>/。
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
android_dir="$(cd "$here/.." && pwd)"

variant="debug"
if [[ $# -gt 0 && ( "$1" == "debug" || "$1" == "release" ) ]]; then
  variant="$1"
  shift
fi

if [[ $# -gt 0 ]]; then
  abis="$(IFS=,; echo "$*")"
elif [[ "$variant" == "debug" ]]; then
  # 模拟器是 x86_64；只编一个 ABI 能省掉一半以上构建时间。
  abis="x86_64"
else
  abis="arm64-v8a"
fi

# AGP 8.13 支持 JDK 17–21。系统默认 JDK 25 会直接失败，这里按版本号
# 回退到 Android Studio 自带的 JBR（存在时）。
java_major() {
  "$1/bin/java" -version 2>&1 | sed -n 's/.*version "\([0-9][0-9]*\).*/\1/p' | head -1
}

java_ok=false
if [[ -n "${JAVA_HOME:-}" && -x "${JAVA_HOME}/bin/java" ]]; then
  major="$(java_major "$JAVA_HOME")"
  if [[ "$major" -ge 17 && "$major" -le 21 ]]; then
    java_ok=true
  else
    echo "==> 忽略 JAVA_HOME=$JAVA_HOME（JDK $major，需要 17–21）"
  fi
fi
if [[ "$java_ok" == false ]]; then
  for candidate in \
    "/c/Program Files/Android/Android Studio/jbr" \
    "${LOCALAPPDATA:-}/Programs/Android Studio/jbr" \
    "/Applications/Android Studio.app/Contents/jbr/Contents/Home"; do
    if [[ -x "$candidate/bin/java" ]]; then
      major="$(java_major "$candidate")"
      if [[ "$major" -ge 17 && "$major" -le 21 ]]; then
        export JAVA_HOME="$candidate"
        java_ok=true
        break
      fi
    fi
  done
fi
if [[ "$java_ok" == false ]]; then
  echo "错误：需要 JDK 17–21（AGP 8.13 不支持 JDK 25）。请设置 JAVA_HOME。" >&2
  exit 1
fi

task="assemble${variant^}"
echo "==> $task (ABI: $abis, JAVA_HOME: $JAVA_HOME)"
"$android_dir/gradlew" -p "$android_dir" ":app:$task" \
  -Pchordial.abis="$abis" --console=plain

apk="$android_dir/app/build/outputs/apk/$variant/app-$variant.apk"
if [[ -f "$apk" ]]; then
  echo "==> APK: $apk"
fi
