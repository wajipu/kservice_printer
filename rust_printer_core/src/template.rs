//! 模板结构：小票和标签 JSON 模板的反序列化模型。
//!
//! 模板描述字符列宽、文本编码、TSPL 标签纸张参数，以及有序元素列表
//!（text、row、columns、divider、feed、cut、repeat、raw、qrcode、barcode、image）。
//! `paperSize` 这种 58/80mm 简写会在解析阶段转换为小票字符列宽，
//! 方便后续渲染层只处理统一的 `width`。

use serde::Deserialize;
use serde_json::Value;

use crate::error::PrinterError;

#[derive(Debug, Deserialize)]
pub(crate) struct Template {
    #[serde(default = "default_width")]
    pub(crate) width: usize,
    #[serde(default = "default_encoding")]
    pub(crate) encoding: String,
    #[serde(default, alias = "fontFamily")]
    pub(crate) font_family: Option<String>,
    #[serde(default, alias = "fontSize")]
    pub(crate) font_size: Option<f32>,
    #[serde(default, alias = "labelWidthMm")]
    pub(crate) label_width_mm: Option<f32>,
    #[serde(default, alias = "labelHeightMm")]
    pub(crate) label_height_mm: Option<f32>,
    #[serde(default, alias = "labelGapMm")]
    pub(crate) label_gap_mm: Option<f32>,
    #[serde(default, alias = "labelDensity")]
    pub(crate) label_density: Option<u8>,
    #[serde(default, alias = "labelSpeed")]
    pub(crate) label_speed: Option<u8>,
    #[serde(default, alias = "labelHomeBeforePrint")]
    pub(crate) label_home_before_print: Option<bool>,
    #[serde(default, alias = "labelReferenceX")]
    pub(crate) label_reference_x: Option<i32>,
    #[serde(default, alias = "labelReferenceY")]
    pub(crate) label_reference_y: Option<i32>,
    #[serde(default, alias = "labelShiftDots")]
    pub(crate) label_shift_dots: Option<i32>,
    #[serde(default)]
    pub(crate) elements: Vec<Element>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub(crate) enum Element {
    #[serde(rename = "text")]
    Text {
        value: String,
        #[serde(default)]
        align: Align,
        #[serde(default)]
        bold: bool,
        #[serde(default)]
        size: TextSize,
    },
    #[serde(rename = "row")]
    Row {
        left: String,
        right: String,
        #[serde(default)]
        bold: bool,
    },
    #[serde(rename = "columns")]
    Columns { columns: Vec<Column> },
    #[serde(rename = "divider")]
    Divider {
        #[serde(default = "default_divider_char")]
        ch: String,
    },
    #[serde(rename = "feed")]
    Feed {
        #[serde(default = "default_feed_lines")]
        lines: u8,
    },
    #[serde(rename = "cut")]
    Cut,
    #[serde(rename = "repeat")]
    Repeat {
        path: String,
        elements: Vec<Element>,
    },
    #[serde(rename = "raw")]
    Raw { hex: String },
    #[serde(rename = "qrcode")]
    QrCode {
        value: String,
        #[serde(default = "default_qrcode_size")]
        size: u8,
        #[serde(default)]
        align: Align,
        #[serde(default)]
        x: Option<i32>,
        #[serde(default)]
        y: Option<i32>,
    },
    #[serde(rename = "barcode")]
    Barcode {
        value: String,
        #[serde(default)]
        system: BarcodeKind,
        #[serde(default)]
        align: Align,
    },
    #[serde(rename = "image")]
    Image {
        #[serde(default)]
        path: Option<String>,
        #[serde(default)]
        base64: Option<String>,
        #[serde(default = "default_image_max_width")]
        max_width: u32,
        #[serde(default)]
        max_height: Option<u32>,
        #[serde(default)]
        align: Align,
    },
}

#[derive(Debug, Deserialize)]
pub(crate) struct Column {
    pub(crate) value: String,
    #[serde(default = "default_column_width")]
    pub(crate) width: usize,
    #[serde(default)]
    pub(crate) align: Align,
    #[serde(default)]
    pub(crate) bold: bool,
}

#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Align {
    #[default]
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum TextSize {
    #[default]
    Normal,
    Double,
}

#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum BarcodeKind {
    #[default]
    Ean13,
    Ean8,
    Code39,
    Codabar,
    Itf,
    Upca,
    Upce,
}

pub(crate) fn parse_template(template_json: &str) -> Result<Template, PrinterError> {
    let mut value: Value = serde_json::from_str(template_json)
        .map_err(|e| PrinterError::InvalidTemplate(e.to_string()))?;
    normalize_template_paper_size(&mut value)?;
    serde_json::from_value(value).map_err(|e| PrinterError::InvalidTemplate(e.to_string()))
}

fn default_width() -> usize {
    48
}

fn default_encoding() -> String {
    "gbk".into()
}

fn default_divider_char() -> String {
    "-".into()
}

fn default_feed_lines() -> u8 {
    3
}

fn default_column_width() -> usize {
    12
}

fn default_qrcode_size() -> u8 {
    5
}

fn default_image_max_width() -> u32 {
    192
}

fn normalize_template_paper_size(value: &mut Value) -> Result<(), PrinterError> {
    let Value::Object(template) = value else {
        return Ok(());
    };

    if template.contains_key("width") {
        return Ok(());
    }

    let Some(paper_size) = template
        .get("paperSize")
        .or_else(|| template.get("paper_size"))
        .cloned()
    else {
        return Ok(());
    };

    let width = match paper_size {
        Value::Number(number) if number.as_u64() == Some(58) => 32,
        Value::Number(number) if number.as_u64() == Some(80) => 48,
        Value::String(value) => match normalize_paper_size_label(&value).as_str() {
            "58" => 32,
            "80" => 48,
            _ => {
                return Err(PrinterError::InvalidTemplate(format!(
                    "不支持的 paperSize: {value}"
                )));
            }
        },
        other => {
            return Err(PrinterError::InvalidTemplate(format!(
                "paperSize 必须是 58、80、58mm 或 80mm，当前为 {other}"
            )));
        }
    };

    template.insert("width".to_string(), serde_json::json!(width));
    Ok(())
}

fn normalize_paper_size_label(value: &str) -> String {
    value
        .trim()
        .to_ascii_lowercase()
        .replace([' ', '-', '_'], "")
        .replace("毫米", "mm")
        .replace("小票", "")
        .trim_end_matches("mm")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_paper_size_from_template_json() {
        let template58 = parse_template(
            &json!({
                "paperSize": 58,
                "elements": []
            })
            .to_string(),
        )
        .unwrap();
        let template80 = parse_template(
            &json!({
                "paper_size": "80mm",
                "elements": []
            })
            .to_string(),
        )
        .unwrap();
        let template58_label = parse_template(
            &json!({
                "paperSize": "58 小票",
                "fontFamily": "Noto Sans Arabic",
                "fontSize": 28,
                "elements": []
            })
            .to_string(),
        )
        .unwrap();
        let explicit_width = parse_template(
            &json!({
                "paperSize": 58,
                "width": 48,
                "elements": []
            })
            .to_string(),
        )
        .unwrap();

        assert_eq!(template58.width, 32);
        assert_eq!(template80.width, 48);
        assert_eq!(template58_label.width, 32);
        assert_eq!(
            template58_label.font_family.as_deref(),
            Some("Noto Sans Arabic")
        );
        assert_eq!(template58_label.font_size, Some(28.0));
        assert_eq!(explicit_width.width, 48);
    }

    #[test]
    fn rejects_unknown_paper_size() {
        let result = parse_template(
            &json!({
                "paperSize": 76,
                "elements": []
            })
            .to_string(),
        );

        assert!(matches!(result, Err(PrinterError::InvalidTemplate(_))));
    }
}
