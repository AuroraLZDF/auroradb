//! AuroraDB 专业版功能。
//!
//! 模块迁移进行中：当前为工作区骨架，后续依次迁入 SSH 隧道、结构对比、
//! 实例监控、备份还原、ER 图、全局搜索与执行计划分析。

pub mod backup;
pub mod er_diagram;
pub mod explain;
pub mod global_search;
pub mod monitor;
pub mod schema_diff;
pub mod tunnel;
