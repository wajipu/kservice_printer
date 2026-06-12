//! 打印文本编码：把 Rust 字符串转成打印机可接收的字节。
//!
//! 常见 ESC/POS 热敏机打印中文时通常需要 GBK（或 GB2312/CP936），而不是
//! 直接 UTF-8。`encode_printer_text` 负责按模板指定编码输出字节；
//! `normalize_text_for_encoding` 处理一些固件依赖的字符替换，例如把半角
//! `¥` 换成更容易被 GBK 字库正确映射的全角 `￥`。

use encoding_rs::GBK;

use crate::error::PrinterError;

pub(crate) fn encode_printer_text(text: &str, encoding: &str) -> Result<Vec<u8>, PrinterError> {
    let normalized_encoding = encoding.trim().to_ascii_lowercase().replace('-', "");
    if normalized_encoding == "utf8" {
        return Ok(text.as_bytes().to_vec());
    }

    if matches!(normalized_encoding.as_str(), "gbk" | "gb2312" | "cp936") {
        let normalized = normalize_text_for_encoding(text, encoding);
        let (encoded, _, had_errors) = GBK.encode(&normalized);
        if had_errors {
            return Err(PrinterError::Encode("GBK 不支持部分字符".into()));
        }
        return Ok(encoded.into_owned());
    }

    Err(PrinterError::Encode(format!(
        "不支持的文本编码: {encoding}"
    )))
}

pub(crate) fn normalize_text_for_encoding(text: &str, encoding: &str) -> String {
    let normalized_encoding = encoding.trim().to_ascii_lowercase().replace('-', "");
    if matches!(normalized_encoding.as_str(), "gbk" | "gb2312" | "cp936") {
        text.replace('¥', "￥")
    } else {
        text.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_cjk_text_as_gbk_for_escpos_printers() {
        let bytes = encode_printer_text("牛肉饭 ¥58.00", "gbk").unwrap();

        assert_eq!(
            bytes,
            vec![
                0xc5, 0xa3, 0xc8, 0xe2, 0xb7, 0xb9, b' ', 0xa3, 0xa4, b'5', b'8', b'.', b'0', b'0'
            ]
        );
    }

    #[test]
    fn encodes_utf8_text_when_requested() {
        let bytes = encode_printer_text("牛肉饭", "utf-8").unwrap();

        assert_eq!(bytes, "牛肉饭".as_bytes());
    }

    #[test]
    fn rejects_unsupported_text_encoding() {
        let result = encode_printer_text("hello", "shift_jis");

        assert!(matches!(result, Err(PrinterError::Encode(_))));
    }
}
