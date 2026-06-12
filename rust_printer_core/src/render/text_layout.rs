//! 小票文本布局工具，供 ESC/POS、图片模式和 TSPL 渲染共同复用。
//!
//! 这里集中处理布局，是因为布局逻辑和最终输出协议无关：原生 ESC/POS 文本、
//! ESC/POS 图片模式、TSPL 标签都需要相同的列网格、左右行排版和 CJK 宽度计算。
//! `escpos` crate 只提供整行硬件对齐（`ESC a n`）和基础换行，不理解多列、
//! 同一行左/右混排，也不会按中文双宽字符计算显示宽度。把这套算法放在一处，
//! 可以避免三个渲染器各自复制一份小票排版逻辑。

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::template::Align;

/// 构造小票里的左右行，例如“项目名 vs 金额”。
///
/// `escpos` 的 `justify()` 只能设置整行对齐，不能在同一物理行里同时放左对齐文本
/// 和右对齐文本。这里用三种策略补齐这个能力：
///   1. 两边都放得下：中间补空格。
///   2. 右侧小于整行宽度：截断左侧，并保留一个空格分隔。
///   3. 否则：拆成上下两行。
pub(crate) fn format_row(left: &str, right: &str, line_width: usize) -> Vec<String> {
    if line_width == 0 {
        return vec![format!("{left}{right}")];
    }

    let left_width = display_width(left);
    let right_width = display_width(right);
    if left_width + right_width <= line_width {
        return vec![format!(
            "{left}{}{right}",
            " ".repeat(line_width - left_width - right_width)
        )];
    }

    if right_width < line_width {
        let fitted_left = fit_text(left, line_width - right_width - 1, Align::Left);
        return vec![format!("{fitted_left} {right}")];
    }

    vec![
        fit_text(left, line_width, Align::Left),
        fit_text(right, line_width, Align::Right),
    ]
}

/// 多列网格：每列都有独立宽度、对齐方式和换行。
///
/// 每列先按自己的宽度单独换行，再把所有列的第 n 行横向拼成一行。
/// 这样长商品名换行时不会把数量、金额列挤出小票宽度；这类业务列布局
/// 不是 `escpos` 的整行 `justify()` 能表达的。
pub(crate) fn format_columns(columns: &[(String, usize, Align)]) -> Vec<String> {
    if columns.is_empty() {
        return Vec::new();
    }

    let wrapped = columns
        .iter()
        .map(|(value, width, _)| wrap_text_to_width(value, *width))
        .collect::<Vec<_>>();
    let row_count = wrapped.iter().map(Vec::len).max().unwrap_or(0);
    let mut rows = Vec::new();

    for row_index in 0..row_count {
        let mut row = String::new();
        for (column_index, (_, width, align)) in columns.iter().enumerate() {
            let value = wrapped[column_index]
                .get(row_index)
                .map(String::as_str)
                .unwrap_or("");
            row.push_str(&fit_text(value, *width, *align));
        }
        if !row.trim().is_empty() {
            rows.push(row);
        }
    }

    rows
}

pub(crate) fn fit_text(value: &str, width: usize, align: Align) -> String {
    let fitted = truncate_to_width(value, width);
    let padding = width.saturating_sub(display_width(&fitted));
    match align {
        Align::Left => format!("{fitted}{}", " ".repeat(padding)),
        Align::Right => format!("{}{fitted}", " ".repeat(padding)),
        Align::Center => {
            let left = padding / 2;
            let right = padding - left;
            format!("{}{fitted}{}", " ".repeat(left), " ".repeat(right))
        }
    }
}

/// 重复字符直到填满指定显示宽度，通常用于生成 `----` 这类分隔线。
///
/// 如果传入零宽字符（组合符号、部分 emoji 等），退回到 `-`；
/// 重复零宽字符不会形成可见分隔线。
pub(crate) fn repeat_to_width(ch: char, width: usize) -> String {
    let char_width = UnicodeWidthChar::width(ch).unwrap_or(0);
    if width == 0 {
        return String::new();
    }
    if char_width == 0 {
        return "-".repeat(width);
    }

    let mut result = String::new();
    let mut used = 0;
    while used + char_width <= width {
        result.push(ch);
        used += char_width;
    }
    result.push_str(&" ".repeat(width - used));
    result
}

pub(crate) fn display_width(value: &str) -> usize {
    UnicodeWidthStr::width(value)
}

/// 按显示列宽做字符级换行。
///
/// 小票宽度很窄（通常 32 到 48 列），按英文单词换行会浪费空间。
/// 这里按字符粒度换行，并保留零宽字符（如 ANSI 控制片段）在当前行内，
/// 同时正确处理中文等 CJK 双宽字符的边界。
fn wrap_text_to_width(value: &str, width: usize) -> Vec<String> {
    if width == 0 || value.is_empty() {
        return vec![String::new()];
    }

    let mut lines = Vec::new();
    for source_line in value.lines() {
        let mut current = String::new();
        let mut used = 0;
        for ch in source_line.chars() {
            let char_width = UnicodeWidthChar::width(ch).unwrap_or(0);
            if char_width == 0 {
                current.push(ch);
                continue;
            }
            if char_width > width {
                if !current.is_empty() {
                    lines.push(current);
                    current = String::new();
                    used = 0;
                }
                continue;
            }
            if used + char_width > width {
                lines.push(current);
                current = String::new();
                used = 0;
            }
            current.push(ch);
            used += char_width;
        }
        lines.push(current);
    }

    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

fn truncate_to_width(value: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }

    let mut result = String::new();
    let mut used = 0;
    for ch in value.chars() {
        let char_width = UnicodeWidthChar::width(ch).unwrap_or(0);
        if used + char_width > width {
            break;
        }
        result.push(ch);
        used += char_width;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::encoding::normalize_text_for_encoding;

    #[test]
    fn formats_row_to_receipt_width() {
        let lines = format_row("合计", "¥88.00", 16);

        assert_eq!(lines.len(), 1);
        assert_eq!(display_width(&lines[0]), 16);
        assert!(lines[0].starts_with("合计"));
        assert!(lines[0].ends_with("¥88.00"));
    }

    #[test]
    fn fits_columns_with_cjk_width_and_alignment() {
        let name = fit_text("牛肉饭", 8, Align::Left);
        let amount = fit_text("¥58.00", 8, Align::Right);
        let line = format!("{name}{amount}");

        assert_eq!(display_width(&name), 8);
        assert_eq!(amount, "  ¥58.00");
        assert_eq!(display_width(&line), 16);
    }

    #[test]
    fn formats_columns_with_gbk_currency_width() {
        let amount = normalize_text_for_encoding("¥58.00", "gbk");
        let lines = format_columns(&[
            ("招牌牛肉饭".to_string(), 16, Align::Left),
            ("2".to_string(), 6, Align::Right),
            (amount, 10, Align::Right),
        ]);

        assert_eq!(lines.len(), 1);
        assert_eq!(display_width(&lines[0]), 32);
        assert!(lines[0].ends_with("￥58.00"));
    }

    #[test]
    fn wraps_long_column_values_without_moving_amount() {
        let lines = format_columns(&[
            ("超长招牌牛肉饭大份".to_string(), 12, Align::Left),
            ("2".to_string(), 6, Align::Right),
            ("￥58.00".to_string(), 10, Align::Right),
        ]);

        assert_eq!(lines.len(), 2);
        assert_eq!(display_width(&lines[0]), 28);
        assert_eq!(display_width(&lines[1]), 28);
        assert!(lines[0].contains("￥58.00"));
        assert!(!lines[1].contains("￥58.00"));
    }

    #[test]
    fn wraps_note_columns_with_hanging_indent() {
        let lines = format_columns(&[
            ("  备注：".to_string(), 8, Align::Left),
            ("不要洋葱不要香菜需要分开打包".to_string(), 12, Align::Left),
        ]);

        assert_eq!(lines.len(), 3);
        assert_eq!(display_width(&lines[0]), 20);
        assert_eq!(display_width(&lines[1]), 20);
        assert_eq!(display_width(&lines[2]), 20);
        assert!(lines[0].starts_with("  备注："));
        assert!(lines[1].starts_with("        "));
        assert!(lines[2].starts_with("        "));
    }

    #[test]
    fn truncates_long_column_without_overflowing_width() {
        let value = fit_text("超长商品名称", 8, Align::Left);

        assert_eq!(display_width(&value), 8);
        assert_eq!(value, "超长商品");
    }
}
