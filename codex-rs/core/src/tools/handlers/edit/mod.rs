//! Edit tool - 互斥实现简单/智能文件编辑
//!
//! 提供两种编辑模式：
//! - Simple: 精确字符串匹配 + 简单 LLM 纠错
//! - Smart: 三层匹配策略 + 语义感知纠错
//!
//! 由配置决定运行时使用哪种实现，对 LLM 透明（同名工具 "edit"）

pub mod common;
pub mod simple;
pub mod smart;

// 导出两个 Handler
pub use simple::EditHandler;
pub use smart::SmartEditHandler;

/// Edit 工具配置
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct EditConfig {
    /// 使用智能编辑（默认 true）
    #[serde(default = "default_use_smart")]
    pub use_smart: bool,
}

impl Default for EditConfig {
    fn default() -> Self {
        Self {
            use_smart: true, // 默认使用智能版
        }
    }
}

fn default_use_smart() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_edit_config_default() {
        let config = EditConfig::default();
        assert!(config.use_smart);
    }

    #[test]
    fn test_edit_config_serde() {
        let config = EditConfig { use_smart: false };
        let json = serde_json::to_string(&config).unwrap();
        let parsed: EditConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(config, parsed);
    }
}
