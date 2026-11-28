import org.jetbrains.kotlin.gradle.dsl.JvmTarget
import java.util.Properties

plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.android)
}

// ---------------------------------------------------------------------------
// Rust 交叉编译接线
//
// Rust 侧 crates/chordal-android 产出 cdylib（libchordial_android.so），
// Gradle 在打包前调用 `cargo ndk` 生成 jniLibs，再交给 AGP 合并。
//
// ABI 可用 -Pchordial.abis=arm64-v8a,x86_64 覆盖：模拟器调试只编 x86_64
// 能显著缩短构建时间，真机则用 arm64-v8a。
// ---------------------------------------------------------------------------
val workspaceRoot: File = rootProject.projectDir.parentFile
val abis: List<String> =
    (providers.gradleProperty("chordial.abis").orNull ?: "arm64-v8a,x86_64")
        .split(',')
        .map(String::trim)
        .filter(String::isNotEmpty)

/** SDK 位置：local.properties 优先，其次环境变量（与 AGP 的解析顺序一致）。 */
fun resolveSdkDir(): File {
    val local = rootProject.file("local.properties")
    if (local.isFile) {
        val props = Properties()
        local.inputStream().use(props::load)
        props.getProperty("sdk.dir")?.let { return File(it) }
    }
    val env = System.getenv("ANDROID_HOME") ?: System.getenv("ANDROID_SDK_ROOT")
        ?: error("未找到 Android SDK：请在 android/local.properties 写 sdk.dir，或设置 ANDROID_HOME")
    return File(env)
}

val sdkDir: File = resolveSdkDir()
val ndkVersion: String =
    providers.gradleProperty("chordial.ndkVersion").orNull
        ?: sdkDir.resolve("ndk").listFiles()
            ?.filter { it.isDirectory }
            ?.maxByOrNull { it.name }
            ?.name
        ?: error("$sdkDir/ndk 下没有已安装的 NDK")

fun registerCargoTask(name: String, profile: String, outputDir: File) =
    tasks.register<Exec>(name) {
        group = "rust"
        description = "用 cargo ndk 编译 chordal-android（$profile）"
        workingDir = workspaceRoot
        inputs.dir(workspaceRoot.resolve("crates"))
        inputs.dir(workspaceRoot.resolve("vendor/gpui/src"))
        inputs.dir(workspaceRoot.resolve("vendor/blade-graphics/src"))
        inputs.files(
            workspaceRoot.resolve("Cargo.toml"),
            workspaceRoot.resolve("Cargo.lock"),
            workspaceRoot.resolve("vendor/gpui/Cargo.toml"),
            workspaceRoot.resolve("vendor/blade-graphics/Cargo.toml"),
        )
        outputs.dir(outputDir)
        doFirst {
            val args = buildList {
                add("ndk")
                // 与 minSdk 对齐：AAudio 等平台库只在 API 26+ 的 stub 目录里。
                addAll(listOf("--platform", "26"))
                abis.forEach {
                    add("-t")
                    add(it)
                }
                addAll(
                    listOf(
                        "-o",
                        outputDir.absolutePath,
                        "build",
                        "-p",
                        "chordal-android",
                        "--lib",
                    ),
                )
                if (profile == "release") add("--release")
            }
            // 直接用 AGP 同源的 SDK/NDK，避免依赖开发者 shell 里的环境变量。
            environment("ANDROID_HOME", sdkDir.absolutePath)
            environment("ANDROID_SDK_ROOT", sdkDir.absolutePath)
            environment("ANDROID_NDK_HOME", sdkDir.resolve("ndk/$ndkVersion").absolutePath)
            commandLine(listOf("cargo") + args)
        }
        doLast {
            // debug 构建的 .so 带完整 DWARF（约 600 MB），直接打进 APK 会让安装
            // 极慢。这里只去 DWARF、保留 symtab：panic 栈仍能解析出函数名，
            // 需要行号时用 target/<triple>/debug 下未裁剪的原始产物。
            val hostTag = when {
                System.getProperty("os.name").startsWith("Windows", ignoreCase = true) ->
                    "windows-x86_64"
                System.getProperty("os.name").startsWith("Mac", ignoreCase = true) ->
                    "darwin-x86_64"
                else -> "linux-x86_64"
            }
            val stripName = if (hostTag.startsWith("windows")) "llvm-strip.exe" else "llvm-strip"
            val strip = sdkDir.resolve(
                "ndk/$ndkVersion/toolchains/llvm/prebuilt/$hostTag/bin/$stripName",
            )
            if (!strip.isFile) {
                logger.warn("llvm-strip 不存在（$strip），APK 将包含未裁剪的 .so")
                return@doLast
            }
            abis.forEach { abi ->
                val lib = outputDir.resolve(abi).resolve("libchordial_android.so")
                if (lib.isFile) {
                    val process = ProcessBuilder(
                        strip.absolutePath,
                        "--strip-debug",
                        lib.absolutePath,
                    ).inheritIO().start()
                    check(process.waitFor() == 0) { "llvm-strip failed for $lib" }
                }
            }
        }
    }

val debugJniLibs = layout.buildDirectory.dir("rustJniLibs/debug").get().asFile
val releaseJniLibs = layout.buildDirectory.dir("rustJniLibs/release").get().asFile
val cargoDebug = registerCargoTask("cargoBuildDebug", "debug", debugJniLibs)
val cargoRelease = registerCargoTask("cargoBuildRelease", "release", releaseJniLibs)

android {
    namespace = "com.chordial.app"
    compileSdk = 36
    ndkVersion = ndkVersion

    defaultConfig {
        applicationId = "com.chordial.app"
        minSdk = 26
        targetSdk = 35
        versionCode = 1
        versionName = "2.0.0"
        ndk {
            abiFilters += abis
        }
    }

    sourceSets {
        getByName("debug").jniLibs.srcDir(debugJniLibs)
        getByName("release").jniLibs.srcDir(releaseJniLibs)
    }

    buildTypes {
        debug {
            isMinifyEnabled = false
        }
        release {
            isMinifyEnabled = false
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                "proguard-rules.pro",
            )
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}

kotlin {
    compilerOptions {
        jvmTarget = JvmTarget.JVM_17
    }
}

// .so 必须先于资源合并/打包生成。
// AGP 8.x 的变体任务在配置后才惰性注册，这里用 matching 延迟绑定。
tasks.matching { it.name == "preDebugBuild" }.configureEach { dependsOn(cargoDebug) }
tasks.matching { it.name == "preReleaseBuild" }.configureEach { dependsOn(cargoRelease) }
tasks.matching { it.name == "mergeDebugJniLibFolders" }.configureEach { dependsOn(cargoDebug) }
tasks.matching { it.name == "mergeReleaseJniLibFolders" }.configureEach { dependsOn(cargoRelease) }

dependencies {
    implementation(libs.androidx.core.ktx)
    implementation(libs.androidx.media)
}
