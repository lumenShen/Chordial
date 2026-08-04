//! 可部署来源的固定规格（代码内硬编码，按 ID 升序）。

use super::types::DeployableSourceSpec;

/// 返回所有可部署来源的固定规格（按 ID 升序）。
///
/// 默认端口已错开，避免同时启动多服务时端口冲突：
/// - netease :3000 | kugou :3001
///
/// `start_command` 直接用 `node` 而非 `npm start`：避免 Windows 上 `cmd /c npm.cmd`
/// 转发到 node 后立即退出的行为（误判为服务退出）。
pub fn list_deployable_sources() -> Vec<DeployableSourceSpec> {
    vec![
        DeployableSourceSpec {
            id: "netease".into(),
            display_name: "网易云音乐".into(),
            git_url: "https://github.com/NeteaseCloudMusicApiEnhanced/api-enhanced.git".into(),
            mirror_url: Some("https://ghproxy.com/https://github.com/NeteaseCloudMusicApiEnhanced/api-enhanced.git".into()),
            start_command: "node".into(),
            start_args: vec!["app.js".into()],
            install_command: "npm".into(),
            install_args: vec!["install".into()],
            description: "NeteaseCloudMusicApi Enhanced — 提供网易云搜索/下载/扫码登录".into(),
            default_port: 3000,
        },
        DeployableSourceSpec {
            id: "kugou".into(),
            display_name: "酷狗音乐".into(),
            git_url: "https://github.com/MakcRe/KuGouMusicApi.git".into(),
            mirror_url: Some("https://ghproxy.com/https://github.com/MakcRe/KuGouMusicApi.git".into()),
            start_command: "node".into(),
            start_args: vec!["app.js".into()],
            install_command: "npm".into(),
            install_args: vec!["install".into()],
            description: "KuGouMusicApi — 提供酷狗音乐搜索/下载/扫码登录".into(),
            default_port: 3001,
        },
    ]
}

/// 按 ID 查找固定规格。
pub fn find_spec(source_id: &str) -> Option<DeployableSourceSpec> {
    list_deployable_sources()
        .into_iter()
        .find(|s| s.id == source_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spec_ids() {
        let specs = list_deployable_sources();
        assert_eq!(specs.len(), 2);
        let ids: Vec<_> = specs.iter().map(|s| s.id.as_str()).collect();
        assert!(ids.contains(&"netease"));
        assert!(ids.contains(&"kugou"));
    }
}