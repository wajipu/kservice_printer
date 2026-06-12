//! 打印核心：基于 JSON 模板渲染 ESC/POS 小票和 TSPL 标签。
//!
//! 对外入口接收模板 JSON 和数据 JSON，然后按模板类型分发到对应渲染路径：
//! 原生 ESC/POS 文本、TSPL 指令流或先合成图片再走 ESC/POS。`api/` 和
//! `frb_generated` 负责 FFI 桥接，其它模块分别处理模板结构、文本布局、
//! 编码、设备发现和协议字节生成。

#![allow(unexpected_cfgs)]
#![allow(clippy::not_unsafe_ptr_arg_deref)]

mod api;
mod discovery;
mod engine;
mod error;
pub mod frb_generated;
mod protocol;
mod render;
mod template;
mod util;
#[cfg(target_os = "windows")]
mod windows_usbprint;
