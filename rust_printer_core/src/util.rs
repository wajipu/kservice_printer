//! crate 内部共享的小工具函数。
//!
//! `into_response` 将 `Result` 包装成 Flutter FFI 层消费的 JSON 响应字符串。
//! `justify_mode` 把模板里的对齐方式映射到 ESC/POS 对齐枚举。
//! `has_cut_element` 递归检查模板树里是否显式包含 `Cut`，用于决定图片模式
//! 渲染完成后是否额外发送切纸指令。

use escpos::utils::JustifyMode;
use serde_json::{json, Value};

use crate::error::PrinterError;
use crate::template::{Align, Element};

pub(crate) fn into_response(result: Result<Value, PrinterError>) -> String {
    match result {
        Ok(value) => json!({ "ok": true, "result": value }).to_string(),
        Err(err) => json!({ "ok": false, "error": err.to_string() }).to_string(),
    }
}

pub(crate) fn justify_mode(align: Align) -> JustifyMode {
    match align {
        Align::Left => JustifyMode::LEFT,
        Align::Center => JustifyMode::CENTER,
        Align::Right => JustifyMode::RIGHT,
    }
}

pub(crate) fn has_cut_element(elements: &[Element]) -> bool {
    elements.iter().any(|element| match element {
        Element::Cut => true,
        Element::Repeat { elements, .. } => has_cut_element(elements),
        _ => false,
    })
}
