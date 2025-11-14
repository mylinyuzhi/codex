//! 共享工具模块
//!
//! 提供文件操作和文本处理工具，供 simple 和 smart 使用

pub mod file_ops;
pub mod text_utils;

// 重导出常用功能
pub use file_ops::detect_line_ending;
pub use file_ops::hash_content;
pub use file_ops::restore_trailing_newline;
pub use text_utils::exact_match_count;
pub use text_utils::safe_literal_replace;
pub use text_utils::unescape_string;
