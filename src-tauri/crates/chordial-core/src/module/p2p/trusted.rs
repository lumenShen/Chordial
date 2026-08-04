//! 可信设备管理 — 手动/自动添加、移除、列出可信设备。

use crate::module::p2p::manager::P2pManager;
use crate::module::p2p::protocol::Permission;
use crate::module::p2p::types::{TrustedDevice, TRUSTED_CONFIG_KEY, now_unix_secs};

impl P2pManager {
    /// 内部方法：握手成功后自动存为可信设备（若尚未存在）。
    pub(crate) fn add_trusted_internal(
        &self,
        instance_id: String,
        name: String,
        addr: String,
        permission: Permission,
    ) {
        let mut g = self.inner.write();
        if g.trusted_devices.iter().any(|t| t.instance_id == instance_id) {
            return; // 已存在
        }
        let device = TrustedDevice {
            instance_id: instance_id.clone(),
            name,
            addr,
            permission,
            added_at: now_unix_secs(),
        };
        g.trusted_devices.push(device.clone());
        let devices = g.trusted_devices.clone();
        drop(g);
        let _ = self.config.set(TRUSTED_CONFIG_KEY, &devices);
        eprintln!("[p2p] 新增可信设备: {instance_id}");
    }

    /// 手动添加可信设备。
    pub fn add_trusted(&self, device: TrustedDevice) -> Result<(), String> {
        let mut g = self.inner.write();
        if g.trusted_devices.iter().any(|t| t.instance_id == device.instance_id) {
            return Err("该设备已在可信列表中".into());
        }
        g.trusted_devices.push(device.clone());
        let devices = g.trusted_devices.clone();
        drop(g);
        self.config.set(TRUSTED_CONFIG_KEY, &devices)
    }

    /// 移除可信设备。
    pub fn remove_trusted(&self, instance_id: &str) -> Result<bool, String> {
        let mut g = self.inner.write();
        let before = g.trusted_devices.len();
        g.trusted_devices.retain(|t| t.instance_id != instance_id);
        let removed = g.trusted_devices.len() < before;
        let devices = g.trusted_devices.clone();
        drop(g);
        if removed {
            self.config.set(TRUSTED_CONFIG_KEY, &devices)?;
        }
        Ok(removed)
    }

    /// 列出所有可信设备。
    pub fn list_trusted(&self) -> Vec<TrustedDevice> {
        self.inner.read().trusted_devices.clone()
    }
}