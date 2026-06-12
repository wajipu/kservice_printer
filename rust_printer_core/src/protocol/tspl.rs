//! TSPL（TSC Printer Language）协议实现。
//!
//! 这里支持两条渲染路径：
//! - **TSPL 指令流**（`tspl` / `tsc` 编码）：将文本、条码和二维码元素映射为
//!   原生 TSPL 指令（`TEXT`、`BARCODE`、`QRCODE`），并由 `TsplRenderState`
//!   维护当前坐标。
//! - **TSPL 位图图片**（`tspl-image` / `tspl-bitmap`）：先用 `cosmic-text`
//!   把整张标签合成为灰度图，再封装成单条 `BITMAP` 指令。适合打印机固件字库
//!   不支持的文字，例如阿拉伯语、维吾尔语。

use handlebars::{no_escape, Handlebars};
use serde_json::Value;

use cosmic_text::{Attrs, Buffer, Color, Family, FontSystem, Metrics, Shaping, SwashCache};
use image::{GrayImage, Luma};
use qrcode::QrCode;

use crate::error::PrinterError;
use crate::render::encoding::encode_printer_text;
use crate::render::text_layout::{format_columns, format_row};
use crate::render::value::{hex_decode, render_value, value_ref};
use crate::template::{Align, BarcodeKind, Element, Template, TextSize};

const TSPL_DOTS_PER_MM: f32 = 8.0;
const TSPL_MARGIN_X: i32 = 24;
const TSPL_MARGIN_Y: i32 = 24;
const TSPL_TEXT_LINE_HEIGHT: i32 = 34;

pub(crate) fn is_tspl_template(template: &Template) -> bool {
    let normalized = template
        .encoding
        .trim()
        .to_ascii_lowercase()
        .replace(['-', '_'], "");
    matches!(normalized.as_str(), "tspl" | "tsc")
}

pub(crate) fn is_tspl_image_template(template: &Template) -> bool {
    let normalized = template
        .encoding
        .trim()
        .to_ascii_lowercase()
        .replace(['-', '_'], "");
    matches!(
        normalized.as_str(),
        "tsplimage" | "tsplbitmap" | "tscimage" | "tscbitmap"
    )
}

pub(crate) fn render_template_as_tspl_bytes(
    template: &Template,
    data: &Value,
) -> Result<Vec<u8>, PrinterError> {
    let mut handlebars = Handlebars::new();
    handlebars.register_escape_fn(no_escape);
    let mut state = TsplRenderState::new(template);

    for element in &template.elements {
        render_tspl_element(&mut state, element, data, &handlebars, template.width)?;
    }

    // PRINT 1,1 表示打印 1 张、1 批；它必须放在最后，
    // TSPL 解释器遇到 PRINT 后才会真正开始打印。
    state.commands.push("PRINT 1,1".to_string());
    let script = format!("{}\r\n", state.commands.join("\r\n"));
    encode_printer_text(&script, "gbk")
}

pub(crate) fn render_template_as_tspl_image_bytes(
    template: &Template,
    data: &Value,
) -> Result<Vec<u8>, PrinterError> {
    let mut handlebars = Handlebars::new();
    handlebars.register_escape_fn(no_escape);
    let image = render_template_to_label_image(template, data, &handlebars)?;
    let bitmap = pack_tspl_bitmap(&image);
    let width_bytes = image.width().div_ceil(8);

    let mut bytes = encode_printer_text(
        &format!("{}\r\n", tspl_setup_commands(template).join("\r\n")),
        "gbk",
    )?;
    bytes.extend_from_slice(format!("BITMAP 0,0,{},{},0,", width_bytes, image.height()).as_bytes());
    bytes.extend_from_slice(&bitmap);
    bytes.extend_from_slice(b"\r\nPRINT 1,1\r\n");
    Ok(bytes)
}

// ---------- TSPL 指令生成 ----------

fn tspl_setup_commands(template: &Template) -> Vec<String> {
    let reference_x = template.label_reference_x.unwrap_or(0);
    let reference_y = template.label_reference_y.unwrap_or(0);
    let mut commands = vec![
        format!(
            "SIZE {} mm,{} mm",
            format_tspl_mm(tspl_label_width_mm(template)),
            format_tspl_mm(template.label_height_mm.unwrap_or(40.0))
        ),
        format!(
            "GAP {} mm,0 mm",
            format_tspl_mm(template.label_gap_mm.unwrap_or(2.0))
        ),
        format!(
            "DENSITY {}",
            template.label_density.unwrap_or(8).clamp(0, 15)
        ),
        format!("SPEED {}", template.label_speed.unwrap_or(4).clamp(1, 6)),
        "DIRECTION 1".to_string(),
        format!("REFERENCE {reference_x},{reference_y}"),
    ];
    if let Some(shift) = template.label_shift_dots.filter(|value| *value != 0) {
        commands.push(format!("SHIFT {}", shift.clamp(-203, 203)));
    }
    if template.label_home_before_print.unwrap_or(false) {
        commands.push("HOME".to_string());
    }
    commands.push("CLS".to_string());
    commands
}

struct TsplRenderState {
    commands: Vec<String>,
    y: i32,
    label_width_dots: i32,
}

impl TsplRenderState {
    fn new(template: &Template) -> Self {
        // TSPL 常见 203 DPI 设备按 8 dots/mm 计算坐标；
        // 指令里的坐标单位是点，所以这里先把毫米换算成点。
        let width_mm = tspl_label_width_mm(template);
        Self {
            commands: tspl_setup_commands(template),
            y: TSPL_MARGIN_Y,
            label_width_dots: (width_mm * TSPL_DOTS_PER_MM).round() as i32,
        }
    }

    fn push_text(&mut self, text: &str, align: Align, bold: bool, size: TextSize) {
        let text = text.trim();
        if text.is_empty() {
            return;
        }

        let scale = if matches!(size, TextSize::Double) {
            2
        } else {
            1
        };
        let line_height = TSPL_TEXT_LINE_HEIGHT * scale;
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() {
                self.y += line_height;
                continue;
            }
            let estimated_width = estimate_tspl_text_width(line, scale);
            let x = match align {
                Align::Left => TSPL_MARGIN_X,
                Align::Center => ((self.label_width_dots - estimated_width) / 2).max(0),
                Align::Right => (self.label_width_dots - TSPL_MARGIN_X - estimated_width).max(0),
            };
            self.commands.push(format!(
                "TEXT {},{},\"TSS24.BF2\",0,{},{},\"{}\"",
                x,
                self.y,
                scale,
                if bold { scale + 1 } else { scale },
                escape_tspl_text(line)
            ));
            self.y += line_height;
        }
    }

    fn push_bar(&mut self) {
        let width = (self.label_width_dots - TSPL_MARGIN_X * 2).max(8);
        self.commands
            .push(format!("BAR {},{},{},2", TSPL_MARGIN_X, self.y, width));
        self.y += 12;
    }

    fn push_qrcode(&mut self, value: &str, size: u8, align: Align, x: Option<i32>, y: Option<i32>) {
        let value = value.trim();
        if value.is_empty() {
            return;
        }

        let qr_size = i32::from(size.clamp(1, 10)) * 28;
        let x = x.unwrap_or_else(|| match align {
            Align::Left => TSPL_MARGIN_X,
            Align::Center => ((self.label_width_dots - qr_size) / 2).max(0),
            Align::Right => (self.label_width_dots - TSPL_MARGIN_X - qr_size).max(0),
        });
        let y = y.unwrap_or(self.y);
        self.commands.push(format!(
            "QRCODE {},{},L,{},A,0,\"{}\"",
            x,
            y,
            size.clamp(1, 10),
            escape_tspl_text(value)
        ));
        if y == self.y {
            self.y += qr_size + 12;
        }
    }

    fn push_barcode(&mut self, value: &str, system: BarcodeKind, align: Align) {
        let value = value.trim();
        if value.is_empty() {
            return;
        }

        let barcode_type = tspl_barcode_type(system);
        let estimated_width = (value.chars().count() as i32 * 16).max(120);
        let x = match align {
            Align::Left => TSPL_MARGIN_X,
            Align::Center => ((self.label_width_dots - estimated_width) / 2).max(0),
            Align::Right => (self.label_width_dots - TSPL_MARGIN_X - estimated_width).max(0),
        };
        self.commands.push(format!(
            "BARCODE {},{},\"{}\",64,1,0,2,2,\"{}\"",
            x,
            self.y,
            barcode_type,
            escape_tspl_text(value)
        ));
        self.y += 92;
    }
}

// ---------- 元素渲染（TSPL 指令流） ----------

fn render_tspl_element(
    state: &mut TsplRenderState,
    element: &Element,
    data: &Value,
    handlebars: &Handlebars,
    line_width: usize,
) -> Result<(), PrinterError> {
    match element {
        Element::Text {
            value,
            align,
            bold,
            size,
        } => {
            let text = render_value(handlebars, value, data)?;
            state.push_text(&text, *align, *bold, *size);
        }
        Element::Row { left, right, bold } => {
            let l = render_value(handlebars, left, data)?;
            let r = render_value(handlebars, right, data)?;
            for line in format_row(&l, &r, line_width) {
                state.push_text(&line, Align::Left, *bold, TextSize::Normal);
            }
        }
        Element::Columns { columns } => {
            let mut items = Vec::new();
            let bold = columns.iter().any(|col| col.bold);
            for col in columns {
                let value = render_value(handlebars, &col.value, data)?;
                items.push((value, col.width, col.align));
            }
            for line in format_columns(&items) {
                state.push_text(&line, Align::Left, bold, TextSize::Normal);
            }
        }
        Element::Divider { .. } => state.push_bar(),
        Element::Feed { lines } => {
            state.y += i32::from(*lines) * TSPL_TEXT_LINE_HEIGHT;
        }
        Element::Cut => {}
        Element::Repeat { path, elements } => {
            if let Some(Value::Array(items)) = value_ref(data, path) {
                for item in items {
                    for child in elements {
                        render_tspl_element(state, child, item, handlebars, line_width)?;
                    }
                }
            }
        }
        Element::Raw { hex } => {
            let bytes = hex_decode(hex)?;
            let command = String::from_utf8_lossy(&bytes).trim().to_string();
            if !command.is_empty() {
                state.commands.push(command);
            }
        }
        Element::QrCode {
            value,
            size,
            align,
            x,
            y,
        } => {
            let data = render_value(handlebars, value, data)?;
            state.push_qrcode(&data, *size, *align, *x, *y);
        }
        Element::Barcode {
            value,
            system,
            align,
        } => {
            let data = render_value(handlebars, value, data)?;
            state.push_barcode(&data, *system, *align);
        }
        Element::Image { .. } => {
            return Err(PrinterError::LabelRender(
                "TSPL 标签模板暂不支持 image 元素，请改用文本、条码或二维码".into(),
            ));
        }
    }
    Ok(())
}

// ---------- 标签图片渲染（tspl-image/bitmap） ----------

fn render_template_to_label_image(
    template: &Template,
    data: &Value,
    handlebars: &Handlebars,
) -> Result<GrayImage, PrinterError> {
    let width = (tspl_label_width_mm(template) * TSPL_DOTS_PER_MM).round() as u32;
    let height = (template.label_height_mm.unwrap_or(40.0) * TSPL_DOTS_PER_MM).round() as u32;
    let mut image = GrayImage::from_pixel(width.max(8), height.max(8), Luma([255]));
    let mut y = TSPL_MARGIN_Y;
    render_tspl_image_elements(
        &mut image,
        &mut y,
        &template.elements,
        data,
        handlebars,
        template,
    )?;
    Ok(image)
}

fn render_tspl_image_elements(
    image: &mut GrayImage,
    y: &mut i32,
    elements: &[Element],
    data: &Value,
    handlebars: &Handlebars,
    template: &Template,
) -> Result<(), PrinterError> {
    for element in elements {
        match element {
            Element::Text {
                value,
                align,
                bold,
                size,
            } => {
                let text = render_value(handlebars, value, data)?;
                let font_size = if matches!(size, TextSize::Double) {
                    template.font_size.unwrap_or(44.0).max(28.0)
                } else {
                    template.font_size.unwrap_or(26.0)
                };
                let line_height = (font_size * 1.25).ceil() as i32;
                for line in text.lines() {
                    let line = line.trim();
                    if line.is_empty() {
                        *y += line_height;
                        continue;
                    }
                    draw_label_text(
                        image,
                        line,
                        LabelTextSpec {
                            x: TSPL_MARGIN_X,
                            y: *y,
                            width: image.width().saturating_sub((TSPL_MARGIN_X * 2) as u32),
                            font_size,
                            align: *align,
                            bold: *bold,
                            font_family: template.font_family.as_deref(),
                        },
                    );
                    *y += line_height;
                }
            }
            Element::Row { left, right, bold } => {
                let left = render_value(handlebars, left, data)?;
                let right = render_value(handlebars, right, data)?;
                let font_size = template.font_size.unwrap_or(24.0);
                let line_height = (font_size * 1.35).ceil() as i32;
                draw_label_text(
                    image,
                    &left,
                    LabelTextSpec {
                        x: TSPL_MARGIN_X,
                        y: *y,
                        width: image.width().saturating_sub((TSPL_MARGIN_X * 2) as u32),
                        font_size,
                        align: Align::Left,
                        bold: *bold,
                        font_family: template.font_family.as_deref(),
                    },
                );
                draw_label_text(
                    image,
                    &right,
                    LabelTextSpec {
                        x: TSPL_MARGIN_X,
                        y: *y,
                        width: image.width().saturating_sub((TSPL_MARGIN_X * 2) as u32),
                        font_size,
                        align: Align::Right,
                        bold: *bold,
                        font_family: template.font_family.as_deref(),
                    },
                );
                *y += line_height;
            }
            Element::Columns { columns } => {
                let mut items = Vec::new();
                let bold = columns.iter().any(|col| col.bold);
                for col in columns {
                    let value = render_value(handlebars, &col.value, data)?;
                    items.push((value, col.width, col.align));
                }
                for line in format_columns(&items) {
                    draw_label_text(
                        image,
                        &line,
                        LabelTextSpec {
                            x: TSPL_MARGIN_X,
                            y: *y,
                            width: image.width().saturating_sub((TSPL_MARGIN_X * 2) as u32),
                            font_size: template.font_size.unwrap_or(24.0),
                            align: Align::Left,
                            bold,
                            font_family: template.font_family.as_deref(),
                        },
                    );
                    *y += TSPL_TEXT_LINE_HEIGHT;
                }
            }
            Element::Divider { .. } => {
                draw_horizontal_bar(image, TSPL_MARGIN_X, *y, 2);
                *y += 12;
            }
            Element::Feed { lines } => {
                *y += i32::from(*lines) * TSPL_TEXT_LINE_HEIGHT;
            }
            Element::Cut
            | Element::Raw { .. }
            | Element::Barcode { .. }
            | Element::Image { .. } => {}
            Element::Repeat { path, elements } => {
                if let Some(Value::Array(items)) = value_ref(data, path) {
                    for item in items {
                        render_tspl_image_elements(image, y, elements, item, handlebars, template)?;
                    }
                }
            }
            Element::QrCode {
                value,
                size,
                align,
                x,
                y: fixed_y,
            } => {
                let value = render_value(handlebars, value, data)?;
                draw_qr_code_on_label(image, &value, *size, *align, *x, *fixed_y, y)?;
            }
        }
    }
    Ok(())
}

struct LabelTextSpec<'a> {
    x: i32,
    y: i32,
    width: u32,
    font_size: f32,
    align: Align,
    bold: bool,
    font_family: Option<&'a str>,
}

struct TextDrawSpec<'a> {
    x: i32,
    y: i32,
    width: u32,
    font_size: f32,
    line_height: f32,
    font_family: Option<&'a str>,
}

fn draw_label_text(image: &mut GrayImage, text: &str, spec: LabelTextSpec<'_>) {
    let line_height = (spec.font_size * 1.35).ceil();
    let estimated_width = estimate_bitmap_text_width(text, spec.font_size);
    let draw_x = match spec.align {
        Align::Left => spec.x,
        Align::Center => spec.x + ((spec.width as i32 - estimated_width) / 2).max(0),
        Align::Right => spec.x + (spec.width as i32 - estimated_width).max(0),
    };
    draw_text_at(
        image,
        text,
        TextDrawSpec {
            x: draw_x,
            y: spec.y,
            width: spec.width,
            font_size: spec.font_size,
            line_height,
            font_family: spec.font_family,
        },
    );
    if spec.bold {
        draw_text_at(
            image,
            text,
            TextDrawSpec {
                x: draw_x + 1,
                y: spec.y,
                width: spec.width,
                font_size: spec.font_size,
                line_height,
                font_family: spec.font_family,
            },
        );
    }
}

fn draw_text_at(image: &mut GrayImage, text: &str, spec: TextDrawSpec<'_>) {
    let mut font_system = FontSystem::new();
    let mut swash_cache = SwashCache::new();
    let metrics = Metrics::new(spec.font_size, spec.line_height);
    let mut buffer = Buffer::new(&mut font_system, metrics);
    buffer.set_size(
        &mut font_system,
        Some(spec.width as f32),
        Some(spec.line_height * 2.0),
    );
    let mut attrs = Attrs::new();
    if let Some(font_family) = spec
        .font_family
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        attrs = attrs.family(Family::Name(font_family));
    }
    buffer.set_text(&mut font_system, text, &attrs, Shaping::Advanced, None);
    buffer.shape_until_scroll(&mut font_system, false);
    let image_width = image.width() as i32;
    let image_height = image.height() as i32;
    buffer.draw(
        &mut font_system,
        &mut swash_cache,
        Color::rgb(0, 0, 0),
        |glyph_x, glyph_y, w, h, color| {
            let alpha = (color.0 >> 24) as u8;
            if alpha == 0 {
                return;
            }
            for dy in 0..h {
                for dx in 0..w {
                    let px = spec.x + glyph_x + dx as i32;
                    let py = spec.y + glyph_y + dy as i32;
                    if px < 0 || py < 0 || px >= image_width || py >= image_height {
                        continue;
                    }
                    let current = image.get_pixel(px as u32, py as u32)[0];
                    let blended = 255u16.saturating_sub(alpha as u16);
                    image.put_pixel(px as u32, py as u32, Luma([current.min(blended as u8)]));
                }
            }
        },
    );
}

fn draw_horizontal_bar(image: &mut GrayImage, x: i32, y: i32, height: i32) {
    let start_x = x.max(0) as u32;
    let end_x = image.width().saturating_sub(x.max(0) as u32).max(start_x);
    for py in y.max(0) as u32..(y + height).max(0) as u32 {
        if py >= image.height() {
            break;
        }
        for px in start_x..end_x {
            image.put_pixel(px, py, Luma([0]));
        }
    }
}

fn draw_qr_code_on_label(
    image: &mut GrayImage,
    value: &str,
    size: u8,
    align: Align,
    x: Option<i32>,
    y: Option<i32>,
    flow_y: &mut i32,
) -> Result<(), PrinterError> {
    let value = value.trim();
    if value.is_empty() {
        return Ok(());
    }
    let code =
        QrCode::new(value.as_bytes()).map_err(|e| PrinterError::ImageRender(e.to_string()))?;
    let scale = i32::from(size.clamp(1, 8));
    let quiet_modules = 4i32;
    let modules = code.width() as i32;
    let qr_size = (modules + quiet_modules * 2) * scale;
    let draw_x = x.unwrap_or_else(|| match align {
        Align::Left => TSPL_MARGIN_X,
        Align::Center => ((image.width() as i32 - qr_size) / 2).max(0),
        Align::Right => (image.width() as i32 - TSPL_MARGIN_X - qr_size).max(0),
    });
    let draw_y = y.unwrap_or(*flow_y);
    for module_y in 0..modules {
        for module_x in 0..modules {
            if code[(module_x as usize, module_y as usize)] != qrcode::types::Color::Dark {
                continue;
            }
            let px = draw_x + (module_x + quiet_modules) * scale;
            let py = draw_y + (module_y + quiet_modules) * scale;
            fill_rect(image, px, py, scale, scale);
        }
    }
    if y.is_none() {
        *flow_y += qr_size + 12;
    }
    Ok(())
}

fn fill_rect(image: &mut GrayImage, x: i32, y: i32, width: i32, height: i32) {
    for py in y.max(0)..(y + height).max(0) {
        if py >= image.height() as i32 {
            break;
        }
        for px in x.max(0)..(x + width).max(0) {
            if px >= image.width() as i32 {
                break;
            }
            image.put_pixel(px as u32, py as u32, Luma([0]));
        }
    }
}

fn pack_tspl_bitmap(image: &GrayImage) -> Vec<u8> {
    let width_bytes = image.width().div_ceil(8);
    let mut bytes = Vec::with_capacity((width_bytes * image.height()) as usize);
    for y in 0..image.height() {
        for byte_x in 0..width_bytes {
            let mut byte = 0u8;
            for bit in 0..8 {
                let x = byte_x * 8 + bit;
                if x < image.width() && image.get_pixel(x, y)[0] < 160 {
                    byte |= 0x80 >> bit;
                }
            }
            bytes.push(byte);
        }
    }
    bytes
}

// TSPL 位图路径里的粗略像素宽度估算。
// ASCII 按 0.56 倍字号估算，中文等全宽字符按 1 倍字号估算；
// 这样无需做昂贵的像素级测量，也足够用于对齐定位。
fn estimate_bitmap_text_width(value: &str, font_size: f32) -> i32 {
    let units = value
        .chars()
        .map(|ch| if ch.is_ascii() { 0.56 } else { 1.0 })
        .sum::<f32>();
    (units * font_size).ceil() as i32
}

// ---------- TSPL 辅助函数 ----------

fn tspl_label_width_mm(template: &Template) -> f32 {
    template.label_width_mm.unwrap_or_else(|| {
        if template.width <= 40 {
            58.0
        } else {
            template.width as f32
        }
    })
}

fn format_tspl_mm(value: f32) -> String {
    if (value.fract()).abs() < f32::EPSILON {
        format!("{}", value as i32)
    } else {
        format!("{value:.1}")
    }
}

fn escape_tspl_text(value: &str) -> String {
    value
        .chars()
        .map(|ch| match ch {
            '"' => '\'',
            '\r' | '\n' | '\t' => ' ',
            _ => ch,
        })
        .collect()
}

// TSPL TEXT 指令定位用的粗略宽度估算。
// ASCII 每字符按 12 点，CJK 每字符按 24 点；这个估算偏保守，
// 可以降低多数 TSPL 打印机上文字贴边或越界的概率。
fn estimate_tspl_text_width(value: &str, scale: i32) -> i32 {
    let width = value
        .chars()
        .map(|ch| if ch.is_ascii() { 12 } else { 24 })
        .sum::<i32>();
    width * scale.max(1)
}

fn tspl_barcode_type(system: BarcodeKind) -> &'static str {
    match system {
        BarcodeKind::Ean13 => "EAN13",
        BarcodeKind::Ean8 => "EAN8",
        BarcodeKind::Code39 => "39",
        BarcodeKind::Codabar => "CODA",
        BarcodeKind::Itf => "ITF",
        BarcodeKind::Upca => "UPCA",
        BarcodeKind::Upce => "UPCE",
    }
}
