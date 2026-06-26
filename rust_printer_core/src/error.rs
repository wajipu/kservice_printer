//! 打印核心统一错误类型。
//!
//! 将 ESC/POS 驱动错误、模板解析失败、编码问题、图片渲染错误和设备发现错误
//! 统一收敛到 `PrinterError`。`#[error("...")]` 里的文案会经 Flutter/Dart
//! FFI 层返回给调用方，因此这里保持用户可读的中文错误信息。

use escpos::errors::PrinterError as EscposError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum PrinterError {
    #[error("模板 JSON 解析失败: {0}")]
    InvalidTemplate(String),
    #[error("数据 JSON 解析失败: {0}")]
    InvalidData(String),
    #[error("Handlebars 渲染失败: {0}")]
    Render(String),
    #[error("文本编码失败: {0}")]
    Encode(String),
    #[error("图片渲染失败: {0}")]
    ImageRender(String),
    #[error("图片数据无效: {0}")]
    InvalidImageData(String),
    #[error("标签指令渲染失败: {0}")]
    LabelRender(String),
    #[error("原始 hex 指令解析失败: {0}")]
    InvalidRawHex(String),
    #[error("网络发现失败: {0}")]
    Discovery(String),
    #[error("钱箱控制失败: {0}")]
    CashDrawer(String),
    #[error("ESC/POS 驱动错误: {0}")]
    Escpos(String),
    #[error("连接打印机失败: {0}")]
    Connect(String),
}

impl From<EscposError> for PrinterError {
    fn from(e: EscposError) -> Self {
        PrinterError::Escpos(e.to_string())
    }
}
