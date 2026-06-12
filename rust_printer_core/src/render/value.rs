//! Handlebars 渲染和 JSON 数据访问工具。
//!
//! `render_value` 负责对模板片段做 Handlebars 插值。`value_ref` 按点分路径
//! 在 JSON 树里取值，同时支持对象字段和数组下标。`hex_decode` 用于解析
//! `"raw"` 模板元素里的空格分隔十六进制字节。

use handlebars::Handlebars;
use serde_json::Value;

use crate::error::PrinterError;

pub(crate) fn render_value(
    handlebars: &Handlebars,
    tmpl: &str,
    data: &Value,
) -> Result<String, PrinterError> {
    handlebars
        .render_template(tmpl, data)
        .map_err(|e| PrinterError::Render(e.to_string()))
}

pub(crate) fn value_ref<'a>(data: &'a Value, path: &str) -> Option<&'a Value> {
    let mut current = data;
    for part in path.split('.') {
        match current {
            Value::Object(map) => current = map.get(part)?,
            Value::Array(list) => current = part.parse::<usize>().ok().and_then(|i| list.get(i))?,
            _ => return None,
        }
    }
    Some(current)
}

pub(crate) fn hex_decode(hex_str: &str) -> Result<Vec<u8>, PrinterError> {
    let normalized = hex_str
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>();
    hex::decode(normalized).map_err(|e| PrinterError::InvalidRawHex(e.to_string()))
}
