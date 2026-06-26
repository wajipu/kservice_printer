//! ZPL (Zebra Programming Language) protocol rendering.

use handlebars::{no_escape, Handlebars};
use serde_json::Value;

use crate::error::PrinterError;
use crate::protocol::tspl::{pack_label_bitmap, render_template_to_label_image};
use crate::render::text_layout::{format_columns, format_row};
use crate::render::value::{hex_decode, render_value, value_ref};
use crate::template::{Align, BarcodeKind, Element, Template, TextSize};

const ZPL_DOTS_PER_MM: f32 = 8.0;
const ZPL_MARGIN_X: i32 = 24;
const ZPL_MARGIN_Y: i32 = 24;
const ZPL_TEXT_LINE_HEIGHT: i32 = 34;

pub(crate) fn is_zpl_template(template: &Template) -> bool {
    let normalized = template
        .encoding
        .trim()
        .to_ascii_lowercase()
        .replace(['-', '_'], "");
    matches!(normalized.as_str(), "zpl" | "zpl2" | "zebra")
}

pub(crate) fn is_zpl_image_template(template: &Template) -> bool {
    let normalized = template
        .encoding
        .trim()
        .to_ascii_lowercase()
        .replace(['-', '_'], "");
    matches!(
        normalized.as_str(),
        "zplimage" | "zplbitmap" | "zplraster" | "zebraimage" | "zebrabitmap" | "zebraraster"
    )
}

pub(crate) fn render_template_as_zpl_bytes(
    template: &Template,
    data: &Value,
) -> Result<Vec<u8>, PrinterError> {
    let mut handlebars = Handlebars::new();
    handlebars.register_escape_fn(no_escape);
    let mut state = ZplRenderState::new(template);

    for element in &template.elements {
        render_zpl_element(&mut state, element, data, &handlebars, template.width)?;
    }

    state.commands.push("^PQ1".to_string());
    state.commands.push("^XZ".to_string());
    Ok(format!("{}\n", state.commands.join("\n")).into_bytes())
}

pub(crate) fn render_template_as_zpl_image_bytes(
    template: &Template,
    data: &Value,
) -> Result<Vec<u8>, PrinterError> {
    let mut handlebars = Handlebars::new();
    handlebars.register_escape_fn(no_escape);
    let image = render_template_to_label_image(template, data, &handlebars)?;
    let bitmap = pack_label_bitmap(&image);
    let row_bytes = image.width().div_ceil(8);
    let total_bytes = bitmap.len();
    let image_hex = hex::encode_upper(bitmap);

    let mut commands = zpl_setup_commands(template);
    commands.push(format!(
        "^FO0,0^GFA,{total_bytes},{total_bytes},{row_bytes},{image_hex}^FS"
    ));
    commands.push("^PQ1".to_string());
    commands.push("^XZ".to_string());
    Ok(format!("{}\n", commands.join("\n")).into_bytes())
}

struct ZplRenderState {
    commands: Vec<String>,
    y: i32,
    label_width_dots: i32,
}

impl ZplRenderState {
    fn new(template: &Template) -> Self {
        let width_mm = zpl_label_width_mm(template);
        Self {
            commands: zpl_setup_commands(template),
            y: ZPL_MARGIN_Y,
            label_width_dots: (width_mm * ZPL_DOTS_PER_MM).round() as i32,
        }
    }

    fn push_text(&mut self, text: &str, align: Align, bold: bool, size: TextSize) {
        let font_height = if matches!(size, TextSize::Double) {
            56
        } else {
            30
        };
        let font_width = if bold { font_height + 4 } else { font_height };
        let line_height = if matches!(size, TextSize::Double) {
            68
        } else {
            ZPL_TEXT_LINE_HEIGHT
        };

        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() {
                self.y += line_height;
                continue;
            }
            let estimated_width = estimate_zpl_text_width(line, font_width);
            let x = match align {
                Align::Left => ZPL_MARGIN_X,
                Align::Center => ((self.label_width_dots - estimated_width) / 2).max(0),
                Align::Right => (self.label_width_dots - ZPL_MARGIN_X - estimated_width).max(0),
            };
            self.commands.push(format!(
                "^FO{},{}^A0N,{font_height},{font_width}^FH\\^FD{}^FS",
                x,
                self.y,
                escape_zpl_field_data(line)
            ));
            self.y += line_height;
        }
    }

    fn push_bar(&mut self) {
        let width = (self.label_width_dots - ZPL_MARGIN_X * 2).max(8);
        self.commands
            .push(format!("^FO{},{}^GB{width},2,2^FS", ZPL_MARGIN_X, self.y));
        self.y += 12;
    }

    fn push_qrcode(&mut self, value: &str, size: u8, align: Align, x: Option<i32>, y: Option<i32>) {
        let value = value.trim();
        if value.is_empty() {
            return;
        }

        let magnification = size.clamp(1, 10);
        let qr_size = i32::from(magnification) * 32;
        let x = x.unwrap_or_else(|| match align {
            Align::Left => ZPL_MARGIN_X,
            Align::Center => ((self.label_width_dots - qr_size) / 2).max(0),
            Align::Right => (self.label_width_dots - ZPL_MARGIN_X - qr_size).max(0),
        });
        let y = y.unwrap_or(self.y);
        self.commands.push(format!(
            "^FO{},{}^BQN,2,{magnification}^FH\\^FDLA,{}^FS",
            x,
            y,
            escape_zpl_field_data(value)
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

        let estimated_width = (value.chars().count() as i32 * 18).max(120);
        let x = match align {
            Align::Left => ZPL_MARGIN_X,
            Align::Center => ((self.label_width_dots - estimated_width) / 2).max(0),
            Align::Right => (self.label_width_dots - ZPL_MARGIN_X - estimated_width).max(0),
        };
        self.commands.push(format!(
            "^FO{},{}{}^FH\\^FD{}^FS",
            x,
            self.y,
            zpl_barcode_command(system),
            escape_zpl_field_data(value)
        ));
        self.y += 96;
    }
}

fn render_zpl_element(
    state: &mut ZplRenderState,
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
            let left = render_value(handlebars, left, data)?;
            let right = render_value(handlebars, right, data)?;
            for line in format_row(&left, &right, line_width) {
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
            state.y += i32::from(*lines) * ZPL_TEXT_LINE_HEIGHT;
        }
        Element::Cut => {}
        Element::Repeat { path, elements } => {
            if let Some(Value::Array(items)) = value_ref(data, path) {
                for item in items {
                    for child in elements {
                        render_zpl_element(state, child, item, handlebars, line_width)?;
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
                "ZPL 标签模板暂不支持 image 元素，请改用 zpl-image 整张标签图片模式".into(),
            ));
        }
    }
    Ok(())
}

fn zpl_setup_commands(template: &Template) -> Vec<String> {
    let width_dots = (zpl_label_width_mm(template) * ZPL_DOTS_PER_MM).round() as i32;
    let height_dots = (template.label_height_mm.unwrap_or(40.0) * ZPL_DOTS_PER_MM).round() as i32;
    let reference_x = template.label_reference_x.unwrap_or(0);
    let reference_y = template.label_reference_y.unwrap_or(0);
    let mut commands = vec![
        "^XA".to_string(),
        "^CI28".to_string(),
        format!("^PW{}", width_dots.max(8)),
        format!("^LL{}", height_dots.max(8)),
        format!("^LH{reference_x},{reference_y}"),
    ];
    if let Some(density) = template.label_density {
        commands.push(format!("^MD{}", density.clamp(0, 30)));
    }
    if let Some(speed) = template.label_speed {
        commands.push(format!("^PR{}", speed.clamp(1, 14)));
    }
    if let Some(shift) = template.label_shift_dots.filter(|value| *value != 0) {
        commands.push(format!("^LS{}", shift.clamp(-9999, 9999)));
    }
    commands
}

fn zpl_label_width_mm(template: &Template) -> f32 {
    template.label_width_mm.unwrap_or({
        if template.width <= 40 {
            58.0
        } else {
            template.width as f32
        }
    })
}

fn escape_zpl_field_data(value: &str) -> String {
    let mut output = String::new();
    for ch in value.chars() {
        match ch {
            '^' => output.push_str("\\5E"),
            '~' => output.push_str("\\7E"),
            '\\' => output.push_str("\\5C"),
            '\r' | '\n' | '\t' => output.push(' '),
            ch if ch.is_control() => output.push(' '),
            _ => output.push(ch),
        }
    }
    output
}

fn estimate_zpl_text_width(value: &str, font_width: i32) -> i32 {
    let units = value
        .chars()
        .map(|ch| if ch.is_ascii() { 0.62 } else { 1.0 })
        .sum::<f32>();
    (units * font_width as f32).ceil() as i32
}

fn zpl_barcode_command(system: BarcodeKind) -> &'static str {
    match system {
        BarcodeKind::Ean13 => "^BEN,80,Y,N",
        BarcodeKind::Ean8 => "^B8N,80,Y,N",
        BarcodeKind::Code39 => "^B3N,N,80,Y,N",
        BarcodeKind::Codabar => "^BKN,N,80,Y,N",
        BarcodeKind::Itf => "^B2N,80,Y,N,N",
        BarcodeKind::Upca => "^BUN,80,Y,N",
        BarcodeKind::Upce => "^B9N,80,Y,N",
    }
}
