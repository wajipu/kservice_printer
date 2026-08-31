//! 图片模式小票渲染。
//!
//! 当模板编码为 `"image"` 或 `"bitmap"` 时，先用 `cosmic-text` 排版文本，
//! 再合成为灰度图片，最后通过 ESC/POS 光栅图片指令发送给打印机。
//! 这条路径用于处理 ESC/POS 内置字库无法正确渲染的复杂文字，例如阿拉伯语、
//! 维吾尔语。`TempImageFile` 负责在生命周期结束时清理临时 PNG。

use base64::Engine as _;
use cosmic_text::{Attrs, Buffer, Color, Family, FontSystem, Metrics, Shaping, SwashCache};
use escpos::driver::Driver;
use escpos::printer::Printer;
use escpos::utils::*;
use handlebars::Handlebars;
use image::{GrayImage, Luma};
use serde_json::Value;
use std::collections::BTreeSet;
use std::sync::{Mutex, OnceLock};

use crate::error::PrinterError;
use crate::render::text_layout::{format_columns, format_row, repeat_to_width};
use crate::render::value::{render_value, value_ref};
use crate::template::{Element, Template};

struct ImageRendererState {
    font_system: FontSystem,
    swash_cache: SwashCache,
    loaded_font_paths: BTreeSet<String>,
}

static IMAGE_RENDERER: OnceLock<Mutex<ImageRendererState>> = OnceLock::new();

fn image_renderer() -> &'static Mutex<ImageRendererState> {
    IMAGE_RENDERER.get_or_init(|| {
        Mutex::new(ImageRendererState {
            font_system: FontSystem::new(),
            swash_cache: SwashCache::new(),
            loaded_font_paths: BTreeSet::new(),
        })
    })
}

pub(crate) fn with_image_renderer<T>(
    callback: impl FnOnce(&mut FontSystem, &mut SwashCache) -> Result<T, PrinterError>,
) -> Result<T, PrinterError> {
    let mut renderer = image_renderer()
        .lock()
        .map_err(|_| PrinterError::ImageRender("图片字体渲染器锁已损坏".into()))?;
    let ImageRendererState {
        font_system,
        swash_cache,
        ..
    } = &mut *renderer;
    callback(font_system, swash_cache)
}

pub(crate) fn configure_image_fonts(font_paths: &[String]) -> Result<usize, PrinterError> {
    let mut renderer = image_renderer()
        .lock()
        .map_err(|_| PrinterError::ImageRender("图片字体渲染器锁已损坏".into()))?;
    let mut loaded = 0usize;
    for font_path in font_paths {
        let normalized = font_path.trim();
        if normalized.is_empty() || renderer.loaded_font_paths.contains(normalized) {
            continue;
        }
        let bytes = std::fs::read(normalized).map_err(|error| {
            PrinterError::ImageRender(format!("读取字体 {normalized}: {error}"))
        })?;
        renderer.font_system.db_mut().load_font_data(bytes);
        renderer.loaded_font_paths.insert(normalized.to_string());
        loaded += 1;
    }
    Ok(loaded)
}

pub(crate) struct TempImageFile {
    path: String,
}

impl TempImageFile {
    pub(crate) fn new(path: String) -> Self {
        Self { path }
    }

    pub(crate) fn path(&self) -> &str {
        &self.path
    }
}

impl Drop for TempImageFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

pub(crate) fn render_template_as_image<D: Driver>(
    printer: &mut Printer<D>,
    template: &Template,
    data: &Value,
    handlebars: &Handlebars,
) -> Result<(), PrinterError> {
    let mut lines = Vec::new();
    collect_image_lines(
        &template.elements,
        data,
        handlebars,
        template.width,
        &mut lines,
    )?;
    let temp_image = TempImageFile::new(render_lines_to_image(&lines, template)?);
    printer.justify(JustifyMode::CENTER)?;
    printer.bit_image_option(
        temp_image.path(),
        image_bit_option(temp_image.path(), receipt_pixel_width(template.width), None)?,
    )?;
    printer.feed()?;
    printer.justify(JustifyMode::LEFT)?;
    Ok(())
}

pub(crate) fn render_template_image_base64(
    template: &Template,
    data: &Value,
    handlebars: &Handlebars,
) -> Result<Value, PrinterError> {
    let mut lines = Vec::new();
    collect_image_lines(
        &template.elements,
        data,
        handlebars,
        template.width,
        &mut lines,
    )?;
    let temp_image = TempImageFile::new(render_lines_to_image(&lines, template)?);
    let bytes = std::fs::read(temp_image.path())
        .map_err(|error| PrinterError::ImageRender(error.to_string()))?;
    let (width, height) = image::image_dimensions(temp_image.path())
        .map_err(|error| PrinterError::ImageRender(error.to_string()))?;
    Ok(serde_json::json!({
        "imageBase64": base64::engine::general_purpose::STANDARD.encode(bytes),
        "width": width,
        "height": height,
    }))
}

fn collect_image_lines(
    elements: &[Element],
    data: &Value,
    handlebars: &Handlebars,
    line_width: usize,
    lines: &mut Vec<String>,
) -> Result<(), PrinterError> {
    for element in elements {
        match element {
            Element::Text { value, .. } => {
                let text = render_value(handlebars, value, data)?;
                if text.is_empty() {
                    continue;
                }
                lines.extend(text.lines().map(ToOwned::to_owned));
            }
            Element::Row { left, right, .. } => {
                let left = render_value(handlebars, left, data)?;
                let right = render_value(handlebars, right, data)?;
                lines.extend(format_row(&left, &right, line_width));
            }
            Element::Columns { columns } => {
                let mut items = Vec::new();
                for col in columns {
                    let value = render_value(handlebars, &col.value, data)?;
                    items.push((value, col.width, col.align));
                }
                lines.extend(format_columns(&items));
            }
            Element::Divider { ch } => {
                let token = ch.chars().next().unwrap_or('-');
                lines.push(repeat_to_width(token, line_width));
            }
            Element::Feed { lines: count } => {
                for _ in 0..*count {
                    lines.push(String::new());
                }
            }
            Element::Cut
            | Element::Raw { .. }
            | Element::QrCode { .. }
            | Element::Barcode { .. } => {}
            Element::Repeat { path, elements } => {
                if let Some(Value::Array(items)) = value_ref(data, path) {
                    for item in items {
                        collect_image_lines(elements, item, handlebars, line_width, lines)?;
                    }
                }
            }
            Element::Image { .. } => {}
        }
    }
    Ok(())
}

fn render_lines_to_image(lines: &[String], template: &Template) -> Result<String, PrinterError> {
    let text = lines.join("\n");
    let width = receipt_pixel_width(template.width);
    // 58mm 小票（384px 宽）默认 24px，80mm 小票（576px 宽）默认 26px；
    // clamp 防止外部传入过小或过大的字号导致图片不可读。
    let font_size = template
        .font_size
        .unwrap_or(if width <= 384 { 24.0 } else { 26.0 })
        .clamp(12.0, 72.0);
    // 1.35 倍行高给中文等 CJK 文本留出可读的行距，避免字形上下贴得太紧。
    let line_height = (font_size * 1.35_f32).ceil();
    let padding = 12u32;
    let height = ((lines.len().max(1) as f32 * line_height).ceil() as u32) + padding * 2;
    let mut image = GrayImage::from_pixel(width, height, Luma([255]));
    let mut renderer = image_renderer()
        .lock()
        .map_err(|_| PrinterError::ImageRender("图片字体渲染器锁已损坏".into()))?;
    let ImageRendererState {
        font_system,
        swash_cache,
        ..
    } = &mut *renderer;
    let metrics = Metrics::new(font_size, line_height);
    let mut buffer = Buffer::new(font_system, metrics);

    buffer.set_size(
        font_system,
        Some((width - padding * 2) as f32),
        Some(height as f32),
    );
    let mut attrs = Attrs::new();
    if let Some(font_family) = template.font_family.as_deref().map(str::trim) {
        if !font_family.is_empty() {
            attrs = attrs.family(Family::Name(font_family));
        }
    }
    buffer.set_text(font_system, &text, &attrs, Shaping::Advanced, None);
    buffer.shape_until_scroll(font_system, false);
    buffer.draw(
        font_system,
        swash_cache,
        Color::rgb(0, 0, 0),
        |x, y, w, h, color| {
            let alpha = (color.0 >> 24) as u8;
            if alpha == 0 {
                return;
            }
            for dy in 0..h {
                for dx in 0..w {
                    let px = x + dx as i32 + padding as i32;
                    let py = y + dy as i32 + padding as i32;
                    if px < 0 || py < 0 || px >= width as i32 || py >= height as i32 {
                        continue;
                    }
                    let current = image.get_pixel(px as u32, py as u32)[0];
                    let blended = 255u16.saturating_sub(alpha as u16);
                    image.put_pixel(px as u32, py as u32, Luma([current.min(blended as u8)]));
                }
            }
        },
    );

    let path = std::env::temp_dir().join(format!(
        "kservice-printer-receipt-{}-{}.png",
        std::process::id(),
        unique_temp_suffix()
    ));
    image
        .save(&path)
        .map_err(|e| PrinterError::ImageRender(e.to_string()))?;
    Ok(path.to_string_lossy().into_owned())
}

pub(crate) fn image_bit_option(
    path: &str,
    max_width: u32,
    max_height: Option<u32>,
) -> Result<BitImageOption, PrinterError> {
    let (width, height) =
        image::image_dimensions(path).map_err(|e| PrinterError::ImageRender(e.to_string()))?;
    image_bit_option_for_dimensions(width, height, max_width, max_height)
}

pub(crate) fn image_bytes_bit_option(
    bytes: &[u8],
    max_width: u32,
    max_height: Option<u32>,
) -> Result<BitImageOption, PrinterError> {
    let image =
        image::load_from_memory(bytes).map_err(|e| PrinterError::ImageRender(e.to_string()))?;
    image_bit_option_for_dimensions(image.width(), image.height(), max_width, max_height)
}

fn image_bit_option_for_dimensions(
    width: u32,
    height: u32,
    max_width: u32,
    max_height: Option<u32>,
) -> Result<BitImageOption, PrinterError> {
    let target_width = max_width.min(width).max(8);
    let scaled_height = if width == 0 {
        height
    } else {
        (height as u64 * target_width as u64).div_ceil(width as u64) as u32
    };
    let target_height = max_height.unwrap_or(scaled_height).max(8);
    BitImageOption::new(
        Some(round_up_to_multiple_of_8(target_width)),
        Some(round_up_to_multiple_of_8(target_height)),
        BitImageSize::Normal,
    )
    .map_err(PrinterError::from)
}

fn round_up_to_multiple_of_8(value: u32) -> u32 {
    value.saturating_add(7) / 8 * 8
}

pub(crate) fn decode_image_base64(value: &str) -> Result<Vec<u8>, PrinterError> {
    let payload = value
        .split_once(',')
        .map(|(_, payload)| payload)
        .unwrap_or(value)
        .trim();
    base64::engine::general_purpose::STANDARD
        .decode(payload)
        .map_err(|e| PrinterError::InvalidImageData(e.to_string()))
}

fn receipt_pixel_width(line_width: usize) -> u32 {
    if line_width <= 32 {
        384
    } else {
        576
    }
}

pub(crate) fn unique_temp_suffix() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temp_image_guard_removes_file_on_drop() {
        let path = std::env::temp_dir().join(format!(
            "kservice-printer-receipt-drop-test-{}.png",
            std::process::id()
        ));
        std::fs::write(&path, b"temporary").unwrap();

        {
            let _guard = TempImageFile::new(path.to_string_lossy().into_owned());
            assert!(path.exists());
        }

        assert!(!path.exists());
    }
}
