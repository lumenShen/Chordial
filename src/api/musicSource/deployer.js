/**
 * 音乐源服务快速部署工具 API — 后端 source_deployer_* 命令
 *
 * 提供 git clone → npm install → npm start 的全链路自动化部署，
 * 让用户一键在本地拉起 NeteaseCloudMusicApi / KuGouMusicApi / QQMusicApi 服务，
 * 并可设置程序启动时自动运行。
 *
 * 部署完成后可通过 `getBaseUrl` 拿到 `http://localhost:<port>`，
 * 再调用对应来源（netease/kugou/qq）的 `updateConfig` 写回 `base_url`。
 */

import { transport } from '@/api/transport';

// ══════════════════════════════════════════════════════════════════════════════
// 类型定义（JSDoc）
// ══════════════════════════════════════════════════════════════════════════════

/**
 * 可部署的音乐源服务规格（后端固定信息）。
 * @typedef {Object} DeployableSourceSpec
 * @property {string} id - 来源 ID（`"netease"` / `"kugou"` / `"qq"`）
 * @property {string} display_name - 展示名（如 `"网易云音乐"`）
 * @property {string} git_url - GitHub 仓库地址
 * @property {string} start_command - 启动命令（通常 `"npm"`）
 * @property {string[]} start_args - 启动参数（如 `["start"]`）
 * @property {string} install_command - 安装依赖命令（通常 `"npm"`）
 * @property {string[]} install_args - 安装参数（如 `["install"]`）
 * @property {string} description - 项目描述
 * @property {number} default_port - 默认监听端口
 */

/**
 * 部署配置（持久化到 PersistentStore）。
 * @typedef {Object} DeployConfig
 * @property {string} install_path - 项目克隆到的本地路径
 * @property {string} last_status - 上次状态（snake_case 字符串）
 * @property {string} last_error - 上次错误信息
 * @property {number} last_started_at - 上次启动时间（Unix 秒）
 * @property {number} last_stopped_at - 上次停止时间（Unix 秒）
 * @property {number} port - 监听端口（0 = 使用 default_port）
 * @property {boolean} enabled - 是否启用快速部署
 * @property {boolean} auto_start - 程序启动时自动运行此服务
 * @property {boolean} auto_configure_base_url - 服务启动后自动配置对应来源的 base_url
 */

/**
 * 运行时进程信息（非持久化）。
 * @typedef {Object} DeployRuntimeInfo
 * @property {string} status - 当前状态
 * @property {number|null} pid - 进程 PID（未运行时为 null）
 * @property {string} base_url - 服务的 base_url
 * @property {string[]} recent_logs - 最近日志行（最多 200 条）
 */

/**
 * 完整的部署信息（前端列表展示用）。
 * @typedef {Object} SourceDeployInfo
 * @property {DeployableSourceSpec} spec - 来源规格
 * @property {DeployConfig} config - 持久化配置
 * @property {DeployRuntimeInfo} runtime - 运行时状态
 */

/**
 * 工具链检测结果。
 * @typedef {Object} ToolchainCheck
 * @property {boolean} git_available
 * @property {boolean} node_available
 * @property {boolean} npm_available
 * @property {string} git_version
 * @property {string} node_version
 * @property {string} npm_version
 */

// ══════════════════════════════════════════════════════════════════════════════
// 查询
// ══════════════════════════════════════════════════════════════════════════════

/**
 * 列出所有可部署来源的完整信息（规格 + 配置 + 运行时状态）。
 * @returns {Promise<SourceDeployInfo[]>}
 */
export async function listSources() {
  return transport.command('source_deployer_list');
}

/**
 * 获取单个来源的完整信息。
 * @param {string} sourceId - 来源 ID（`"netease"` / `"kugou"` / `"qq"`）
 * @returns {Promise<SourceDeployInfo>}
 */
export async function getSource(sourceId) {
  return transport.command('source_deployer_get', { sourceId });
}

/**
 * 获取单个来源的部署配置。
 * @param {string} sourceId
 * @returns {Promise<DeployConfig>}
 */
export async function getConfig(sourceId) {
  return transport.command('source_deployer_get_config', { sourceId });
}

/**
 * 获取最近 N 条日志。未运行时返回空数组。
 * @param {string} sourceId
 * @param {number} [n=200] - 返回的日志条数
 * @returns {Promise<string[]>}
 */
export async function getLogs(sourceId, n = 200) {
  return transport.command('source_deployer_get_logs', { sourceId, n });
}

/**
 * 获取某来源服务的 base_url（`http://localhost:<port>`）。
 * 用于部署成功后自动配置对应来源（netease/kugou/qq）的 base_url。
 * @param {string} sourceId
 * @returns {Promise<string|null>}
 */
export async function getBaseUrl(sourceId) {
  return transport.command('source_deployer_get_base_url', { sourceId });
}

// ══════════════════════════════════════════════════════════════════════════════
// 工具链检测
// ══════════════════════════════════════════════════════════════════════════════

/**
 * 检测 git / node / npm 是否可用及版本号。
 * @returns {Promise<ToolchainCheck>}
 */
export async function checkToolchain() {
  return transport.command('source_deployer_check_toolchain');
}

// ══════════════════════════════════════════════════════════════════════════════
// 配置更新
// ══════════════════════════════════════════════════════════════════════════════

/**
 * 更新来源的部署配置（端口 / auto_start / auto_configure_base_url / enabled）。
 * @param {string} sourceId
 * @param {DeployConfig} config
 * @returns {Promise<void>}
 */
export async function updateConfig(sourceId, config) {
  return transport.command('source_deployer_update_config', { sourceId, config });
}

/**
 * 切换 `auto_start` 标志（程序启动时自动运行）。
 * @param {string} sourceId
 * @param {boolean} autoStart
 * @returns {Promise<void>}
 */
export async function setAutoStart(sourceId, autoStart) {
  return transport.command('source_deployer_set_auto_start', { sourceId, autoStart });
}

/**
 * 切换 `enabled` 标志（关闭后忽略 auto_start）。
 * @param {string} sourceId
 * @param {boolean} enabled
 * @returns {Promise<void>}
 */
export async function setEnabled(sourceId, enabled) {
  return transport.command('source_deployer_set_enabled', { sourceId, enabled });
}

// ══════════════════════════════════════════════════════════════════════════════
// 部署操作（异步，可能耗时较长）
// ══════════════════════════════════════════════════════════════════════════════

/**
 * 克隆项目（`git clone --depth 1` 浅克隆）。
 * @param {string} sourceId
 * @returns {Promise<void>}
 */
export async function cloneSource(sourceId) {
  return transport.command('source_deployer_clone', { sourceId });
}

/**
 * 拉取最新代码（`git pull`）。仅在已克隆且服务停止时可用。
 * @param {string} sourceId
 * @returns {Promise<void>}
 */
export async function pullSource(sourceId) {
  return transport.command('source_deployer_pull', { sourceId });
}

/**
 * 安装依赖（`npm install`）。仅在已克隆且服务停止时可用。
 * @param {string} sourceId
 * @returns {Promise<void>}
 */
export async function installSource(sourceId) {
  return transport.command('source_deployer_install', { sourceId });
}

/**
 * 启动服务（PORT 环境变量 + kill_on_drop）。
 * @param {string} sourceId
 * @returns {Promise<void>}
 */
export async function startSource(sourceId) {
  return transport.command('source_deployer_start', { sourceId });
}

/**
 * 停止服务（发送 stop 信号 → wait_task kill child）。
 * 幂等：未运行时也返回 Ok。
 * @param {string} sourceId
 * @returns {Promise<void>}
 */
export async function stopSource(sourceId) {
  return transport.command('source_deployer_stop', { sourceId });
}

/**
 * 移除部署：停止服务（若运行中）+ 删除项目目录 + 清除配置。
 * @param {string} sourceId
 * @returns {Promise<void>}
 */
export async function removeSource(sourceId) {
  return transport.command('source_deployer_remove', { sourceId });
}

// ══════════════════════════════════════════════════════════════════════════════
// 一键部署流程（前端组合命令）
// ══════════════════════════════════════════════════════════════════════════════

/**
 * 一键部署并启动：clone → install → start。
 *
 * 调用方应捕获每步错误并展示给用户。
 *
 * @param {string} sourceId
 * @param {Object} [callbacks] - 进度回调
 * @param {(step: 'clone'|'install'|'start') => void} [callbacks.onStep]
 * @returns {Promise<void>}
 */
export async function deployAndStart(sourceId, callbacks = {}) {
  const { onStep } = callbacks;
  if (onStep) onStep('clone');
  await cloneSource(sourceId);
  if (onStep) onStep('install');
  await installSource(sourceId);
  if (onStep) onStep('start');
  await startSource(sourceId);
}
