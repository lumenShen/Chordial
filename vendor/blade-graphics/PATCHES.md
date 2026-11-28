# blade-graphics 本地补丁

上游版本：`blade-graphics 0.7.1`（crates.io）。

## 补丁 1：Vulkan 核心版本提升后的扩展判断

**问题**：`inspect_adapter` 只比对 `vkEnumerateDeviceExtensionProperties`
返回的扩展名。符合规范的 Vulkan 驱动在扩展被提升进核心后可以不枚举该名字：

| 扩展 | 提升版本 |
| --- | --- |
| `VK_KHR_descriptor_update_template` | 1.1 |
| `VK_KHR_timeline_semaphore` | 1.2 |
| `VK_EXT_inline_uniform_block` | 1.3 |
| `VK_KHR_dynamic_rendering` | 1.3 |

Android 模拟器（gfxstream + 宿主 GPU）报告 Vulkan 1.3，上述扩展名均不出现在
扩展列表里，但对应 feature 都是 TRUE。旧逻辑因此返回
`NoSupportedDeviceFound`，gpui 在 `BladeContext::new()` 直接 panic：
`failed to initialize Android Vulkan context: NoSupportedDeviceFound`。

**改动**（`src/vulkan/init.rs`）：

1. 新增 `promoted_device_extension_version` 与 `device_extension_available`：
   扩展被枚举 **或** 设备 API 版本 ≥ 提升版本即视为可用。
2. 适配器筛选改用 `device_extension_available`。
3. 创建设备时**只**把驱动真正枚举到的扩展写进 `enabled_extension_names`。
   注意“功能可用”不等于“扩展名可启用”：对已提升的扩展再启用会返回
   `VK_ERROR_EXTENSION_NOT_PRESENT`（第一版补丁在这里踩过一次）。
4. 创建设备前输出 `Enabling device extensions: [...]`，便于排查。

feature 结构体仍沿用 `VkPhysicalDeviceInlineUniformBlockFeaturesEXT` 等
KHR/EXT 形式；在核心版本满足时这些结构是核心结构的别名，可直接链入
`VkDeviceCreateInfo::pNext`（已用探针在模拟器上验证
`inline_uniform_block=1 / max_size=256`）。

补丁只放宽“能否用”，不改变渲染路径，桌面端行为不变。

## 补丁 2：Android 面格式回退

**问题**：`ColorSpace::Srgb` 固定选 `B8G8R8A8_UNORM`，Android 的 WSI
（模拟器 gfxstream / 多数手机）只提供 `R8G8B8A8_*`，于是
`vkCreateSwapchainKHR` 使用不支持的格式。

**改动**（`src/vulkan/surface.rs`）：BGRA 不在支持列表时按
`R8G8B8A8_UNORM` → `R8G8B8A8_SRGB` 回退（优先 `SRGB_NONLINEAR` 色彩空间），
并在 “Retaining current format” 分支补齐 RGBA8 的色彩空间映射。
桌面仍命中首选 BGRA，行为不变。

## 补丁 3：动态渲染改用核心入口

**问题**：`VK_KHR_dynamic_rendering` 提升到 Vulkan 1.3 后，驱动可能只导出核心名
`vkCmdBeginRendering` / `vkCmdEndRendering`，不导出 KHR 别名。ash 的
`khr::dynamic_rendering::Device` 在首次调用时会因函数指针为空而 panic
（`Unable to load cmd_begin_rendering_khr`）。

**改动**：
- `src/vulkan/mod.rs`：`Device.dynamic_rendering` 改为 `enum DynamicRendering
  { Core, Khr(..) }`；
- `src/vulkan/init.rs`：用 `vkGetDeviceProcAddr("vkCmdBeginRendering")` 探测，
  有核心入口就用 `Core`，否则回退 KHR 加载器；
- `src/vulkan/command.rs`：begin/end rendering 按枚举分派。

Vulkan 1.2 及更早的驱动仍走 KHR 路径，行为不变。

## 补丁 4：时间线信号量改用核心入口

**问题**：`VK_KHR_timeline_semaphore` 在 Vulkan 1.2 成为核心，驱动可能不导出
`vkWaitSemaphoresKHR`；`wait_for` 调用 KHR 加载器时 panic
（`Unable to load wait_semaphores_khr`）。

**改动**：`Device.timeline_semaphore` 改为 `enum TimelineSemaphore { Core, Khr }`；
`vkGetDeviceProcAddr("vkWaitSemaphores")` 可用时走核心入口，否则保持 KHR。
