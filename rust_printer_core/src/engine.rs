use base64::Engine as _;
use cosmic_text::{Attrs, Buffer, Color, Family, FontSystem, Metrics, Shaping, SwashCache};
use encoding_rs::GBK;
use escpos::driver::{Driver, SerialPortDriver, UsbDriver};
use escpos::errors::PrinterError as EscposError;
use escpos::printer::Printer;
use escpos::utils::*;
use handlebars::Handlebars;
use image::{GrayImage, Luma};
use rusb::UsbContext;
use serde::Deserialize;
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use thiserror::Error;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::api::printer::PrinterConnection;

// ---- 自定义 VecDriver：捕获打印字节，不做网络发送 ----

struct VecDriver {
    name: String,
    buf: Arc<Mutex<Vec<u8>>>,
}

impl VecDriver {
    fn new() -> (Self, Arc<Mutex<Vec<u8>>>) {
        let buf = Arc::new(Mutex::new(Vec::new()));
        (
            Self {
                name: "vec".into(),
                buf: buf.clone(),
            },
            buf,
        )
    }
}

impl Driver for VecDriver {
    fn name(&self) -> String {
        self.name.clone()
    }
    fn write(&self, data: &[u8]) -> std::result::Result<(), EscposError> {
        self.buf.lock().unwrap().extend_from_slice(data);
        Ok(())
    }
    fn read(&self, _buf: &mut [u8]) -> std::result::Result<usize, EscposError> {
        Ok(0)
    }
    fn flush(&self) -> std::result::Result<(), EscposError> {
        Ok(())
    }
}

struct CountingDriver<D: Driver> {
    inner: D,
    bytes: Arc<Mutex<usize>>,
}

impl<D: Driver> CountingDriver<D> {
    fn new(inner: D) -> (Self, Arc<Mutex<usize>>) {
        let bytes = Arc::new(Mutex::new(0));
        (
            Self {
                inner,
                bytes: bytes.clone(),
            },
            bytes,
        )
    }
}

impl<D: Driver> Driver for CountingDriver<D> {
    fn name(&self) -> String {
        self.inner.name()
    }
    fn write(&self, data: &[u8]) -> std::result::Result<(), EscposError> {
        self.inner.write(data)?;
        *self.bytes.lock().unwrap() += data.len();
        Ok(())
    }
    fn read(&self, buf: &mut [u8]) -> std::result::Result<usize, EscposError> {
        self.inner.read(buf)
    }
    fn flush(&self) -> std::result::Result<(), EscposError> {
        self.inner.flush()
    }
}

// ---- AnyDriver：统一三种连接方式 ----

enum AnyDriver {
    Tcp(TcpDriver),
    Usb(UsbDriver),
    Serial(SerialPortDriver),
}

impl Driver for AnyDriver {
    fn name(&self) -> String {
        match self {
            AnyDriver::Tcp(d) => d.name(),
            AnyDriver::Usb(d) => d.name(),
            AnyDriver::Serial(d) => d.name(),
        }
    }
    fn write(&self, data: &[u8]) -> std::result::Result<(), EscposError> {
        match self {
            AnyDriver::Tcp(d) => d.write(data),
            AnyDriver::Usb(d) => d.write(data),
            AnyDriver::Serial(d) => d.write(data),
        }
    }
    fn read(&self, buf: &mut [u8]) -> std::result::Result<usize, EscposError> {
        match self {
            AnyDriver::Tcp(d) => d.read(buf),
            AnyDriver::Usb(d) => d.read(buf),
            AnyDriver::Serial(d) => d.read(buf),
        }
    }
    fn flush(&self) -> std::result::Result<(), EscposError> {
        match self {
            AnyDriver::Tcp(d) => d.flush(),
            AnyDriver::Usb(d) => d.flush(),
            AnyDriver::Serial(d) => d.flush(),
        }
    }
}

// ---- 错误类型 ----

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
    #[error("原始 hex 指令解析失败: {0}")]
    InvalidRawHex(String),
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

// ---- 模板结构 ----

#[derive(Debug, Deserialize)]
struct Template {
    #[serde(default = "default_width")]
    width: usize,
    #[serde(default = "default_encoding")]
    encoding: String,
    #[serde(default, alias = "fontFamily")]
    font_family: Option<String>,
    #[serde(default, alias = "fontSize")]
    font_size: Option<f32>,
    #[serde(default)]
    elements: Vec<Element>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum Element {
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
struct Column {
    value: String,
    #[serde(default = "default_column_width")]
    width: usize,
    #[serde(default)]
    align: Align,
}

#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Align {
    #[default]
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
enum TextSize {
    #[default]
    Normal,
    Double,
}

#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
enum BarcodeKind {
    #[default]
    Ean13,
    Ean8,
    Code39,
    Codabar,
    Itf,
    Upca,
    Upce,
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

// ---- 公开入口 ----

pub fn print_receipt(
    connection: &PrinterConnection,
    template_json: &str,
    data_json: &str,
) -> String {
    into_response(print_receipt_inner(connection, template_json, data_json))
}

pub fn render_receipt(template_json: &str, data_json: &str) -> String {
    into_response(render_receipt_inner(template_json, data_json))
}

pub fn list_usb_printers() -> String {
    into_response(list_usb_printers_inner())
}

// ---- 内部 ----

fn print_receipt_inner(
    connection: &PrinterConnection,
    template_json: &str,
    data_json: &str,
) -> Result<Value, PrinterError> {
    let template = parse_template(template_json)?;
    let data: Value =
        serde_json::from_str(data_json).map_err(|e| PrinterError::InvalidData(e.to_string()))?;
    let (driver, bytes) = CountingDriver::new(open_driver(connection)?);
    let mut printer = build_printer(driver, &template, &data)?;
    printer.print_cut()?;
    let bytes = *bytes.lock().unwrap();
    Ok(json!({ "printed": true, "bytes": bytes }))
}

fn render_receipt_inner(template_json: &str, data_json: &str) -> Result<Value, PrinterError> {
    let template = parse_template(template_json)?;
    let data: Value =
        serde_json::from_str(data_json).map_err(|e| PrinterError::InvalidData(e.to_string()))?;
    let (driver, buf) = VecDriver::new();
    let mut printer = build_printer(driver, &template, &data)?;
    printer.print_cut()?;
    let bytes = buf.lock().unwrap().clone();
    Ok(json!({ "bytes": hex::encode(&bytes), "length": bytes.len() }))
}

fn parse_template(template_json: &str) -> Result<Template, PrinterError> {
    let mut value: Value = serde_json::from_str(template_json)
        .map_err(|e| PrinterError::InvalidTemplate(e.to_string()))?;
    normalize_template_paper_size(&mut value)?;
    serde_json::from_value(value).map_err(|e| PrinterError::InvalidTemplate(e.to_string()))
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

    template.insert("width".to_string(), json!(width));
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

fn open_driver(connection: &PrinterConnection) -> Result<AnyDriver, PrinterError> {
    match connection {
        PrinterConnection::Network {
            host,
            port,
            timeout_ms,
        } => {
            let driver =
                TcpDriver::open(host, *port, *timeout_ms).map_err(|e| PrinterError::Connect(e))?;
            Ok(AnyDriver::Tcp(driver))
        }
        PrinterConnection::Usb {
            vendor_id,
            product_id,
        } => {
            let driver = UsbDriver::open(*vendor_id, *product_id, None, None)
                .map_err(|e| PrinterError::Connect(e.to_string()))?;
            Ok(AnyDriver::Usb(driver))
        }
        PrinterConnection::Serial { port, baud_rate } => {
            let timeout = Some(Duration::from_secs(5));
            let driver = SerialPortDriver::open(port, *baud_rate, timeout)
                .map_err(|e| PrinterError::Connect(e.to_string()))?;
            Ok(AnyDriver::Serial(driver))
        }
    }
}

fn list_usb_printers_inner() -> Result<Value, PrinterError> {
    let context = rusb::Context::new().map_err(|e| PrinterError::Connect(e.to_string()))?;
    let devices = context
        .devices()
        .map_err(|e| PrinterError::Connect(e.to_string()))?;
    let mut result = Vec::new();

    for device in devices.iter() {
        let descriptor = match device.device_descriptor() {
            Ok(descriptor) => descriptor,
            Err(_) => continue,
        };
        let handle = device.open().ok();
        let product = handle
            .as_ref()
            .and_then(|handle| handle.read_product_string_ascii(&descriptor).ok());
        let manufacturer = handle
            .as_ref()
            .and_then(|handle| handle.read_manufacturer_string_ascii(&descriptor).ok());
        let serial = handle
            .as_ref()
            .and_then(|handle| handle.read_serial_number_string_ascii(&descriptor).ok());
        let class_code = descriptor.class_code();
        let interface_classes = usb_interface_classes(&device);
        let is_printer = class_code == 0x07 || interface_classes.contains(&0x07);

        result.push(json!({
            "vendorId": descriptor.vendor_id(),
            "productId": descriptor.product_id(),
            "vendorIdHex": format!("0x{:04X}", descriptor.vendor_id()),
            "productIdHex": format!("0x{:04X}", descriptor.product_id()),
            "manufacturer": manufacturer,
            "product": product,
            "serial": serial,
            "classCode": class_code,
            "interfaceClasses": interface_classes,
            "isPrinter": is_printer,
        }));
    }

    result.sort_by_key(|value| {
        (
            !value["isPrinter"].as_bool().unwrap_or(false),
            value["manufacturer"].as_str().unwrap_or("").to_owned(),
            value["product"].as_str().unwrap_or("").to_owned(),
        )
    });

    Ok(json!({ "printers": result }))
}

fn usb_interface_classes<T: UsbContext>(device: &rusb::Device<T>) -> Vec<u8> {
    let descriptor = match device.device_descriptor() {
        Ok(descriptor) => descriptor,
        Err(_) => return Vec::new(),
    };
    let mut classes = Vec::new();
    for config_index in 0..descriptor.num_configurations() {
        let config = match device.config_descriptor(config_index) {
            Ok(config) => config,
            Err(_) => continue,
        };
        for interface in config.interfaces() {
            for descriptor in interface.descriptors() {
                let class_code = descriptor.class_code();
                if !classes.contains(&class_code) {
                    classes.push(class_code);
                }
            }
        }
    }
    classes
}

// ---- TcpDriver ----

struct TcpDriver {
    name: String,
    stream: Mutex<TcpStream>,
}

impl TcpDriver {
    fn open(host: &str, port: u16, timeout_ms: u64) -> Result<Self, String> {
        let timeout = Duration::from_millis(timeout_ms);
        let mut addrs = (host, port).to_socket_addrs().map_err(|e| e.to_string())?;
        let addr = addrs
            .next()
            .ok_or_else(|| "没有解析出可用的 IP 地址".to_string())?;
        let stream = TcpStream::connect_timeout(&addr, timeout).map_err(|e| e.to_string())?;
        stream
            .set_write_timeout(Some(timeout))
            .map_err(|e| e.to_string())?;
        Ok(Self {
            name: format!("tcp://{host}:{port}"),
            stream: Mutex::new(stream),
        })
    }
}

impl Driver for TcpDriver {
    fn name(&self) -> String {
        self.name.clone()
    }
    fn write(&self, data: &[u8]) -> std::result::Result<(), EscposError> {
        let mut stream = self.stream.lock().unwrap();
        stream
            .write_all(data)
            .map_err(|e| EscposError::Io(e.to_string()))?;
        stream.flush().map_err(|e| EscposError::Io(e.to_string()))
    }
    fn read(&self, buf: &mut [u8]) -> std::result::Result<usize, EscposError> {
        self.stream
            .lock()
            .unwrap()
            .read(buf)
            .map_err(|e| EscposError::Io(e.to_string()))
    }
    fn flush(&self) -> std::result::Result<(), EscposError> {
        self.stream
            .lock()
            .unwrap()
            .flush()
            .map_err(|e| EscposError::Io(e.to_string()))
    }
}

// ---- 模板 → Printer builder 转换 ----

fn build_printer<D: Driver>(
    driver: D,
    template: &Template,
    data: &Value,
) -> Result<Printer<D>, PrinterError> {
    let handlebars = Handlebars::new();
    let mut printer = Printer::new(driver, Protocol::default(), None);
    printer.init()?;
    printer.custom(&[0x1c, 0x26])?;

    if template.encoding.eq_ignore_ascii_case("image")
        || template.encoding.eq_ignore_ascii_case("bitmap")
    {
        render_template_as_image(&mut printer, template, data, &handlebars)?;
    } else {
        for element in &template.elements {
            render_element(&mut printer, element, data, &handlebars, template.width)?;
        }
    }

    Ok(printer)
}

fn render_template_as_image<D: Driver>(
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

struct TempImageFile {
    path: String,
}

impl TempImageFile {
    fn new(path: String) -> Self {
        Self { path }
    }

    fn path(&self) -> &str {
        &self.path
    }
}

impl Drop for TempImageFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn render_element<D: Driver>(
    printer: &mut Printer<D>,
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
            printer.bold(*bold)?;
            if matches!(size, TextSize::Double) {
                printer.size(2, 2)?;
            }
            printer.justify(justify_mode(*align))?;
            print_text_line(printer, &text)?;
            if matches!(size, TextSize::Double) {
                printer.size(1, 1)?;
            }
        }
        Element::Row { left, right, bold } => {
            let l = render_value(handlebars, left, data)?;
            let r = render_value(handlebars, right, data)?;
            printer.bold(*bold)?;
            for line in format_row(&l, &r, line_width) {
                print_text_line(printer, &line)?;
            }
            printer.bold(false)?;
        }
        Element::Columns { columns } => {
            let mut items = Vec::new();
            for col in columns {
                let value = render_value(handlebars, &col.value, data)?;
                items.push(fit_text(&value, col.width, col.align));
            }
            print_text_line(printer, &items.join(""))?;
        }
        Element::Divider { ch } => {
            let token = ch.chars().next().unwrap_or('-');
            print_text_line(printer, &repeat_to_width(token, line_width))?;
        }
        Element::Feed { lines } => {
            for _ in 0..*lines {
                printer.feed()?;
            }
        }
        Element::Cut => {}
        Element::Repeat { path, elements } => {
            if let Some(Value::Array(items)) = value_ref(data, path) {
                for item in items {
                    for child in elements {
                        render_element(printer, child, item, handlebars, line_width)?;
                    }
                }
            }
        }
        Element::Raw { hex } => {
            let bytes = hex_decode(hex)?;
            printer.custom(&bytes)?;
        }
        Element::QrCode { value, size, align } => {
            let data = render_value(handlebars, value, data)?;
            printer.justify(justify_mode(*align))?;
            printer.qrcode_option(
                &data,
                QRCodeOption::new(QRCodeModel::Model2, *size, QRCodeCorrectionLevel::M),
            )?;
            printer.feed()?;
            printer.justify(JustifyMode::LEFT)?;
        }
        Element::Barcode {
            value,
            system,
            align,
        } => {
            let data = render_value(handlebars, value, data)?;
            printer.justify(justify_mode(*align))?;
            print_barcode(printer, *system, &data)?;
            printer.feed()?;
            printer.justify(JustifyMode::LEFT)?;
        }
        Element::Image {
            path,
            base64,
            max_width,
            max_height,
            align,
        } => {
            printer.justify(justify_mode(*align))?;
            print_image_node(
                printer,
                path,
                base64,
                *max_width,
                *max_height,
                handlebars,
                data,
            )?;
            printer.feed()?;
            printer.justify(JustifyMode::LEFT)?;
        }
    }
    Ok(())
}

fn print_barcode<D: Driver>(
    printer: &mut Printer<D>,
    system: BarcodeKind,
    data: &str,
) -> Result<(), PrinterError> {
    let option = BarcodeOption::new(
        BarcodeWidth::M,
        BarcodeHeight::S,
        BarcodeFont::A,
        BarcodePosition::Below,
    );
    match system {
        BarcodeKind::Ean13 => printer.ean13_option(data, option)?,
        BarcodeKind::Ean8 => printer.ean8_option(data, option)?,
        BarcodeKind::Code39 => printer.code39_option(data, option)?,
        BarcodeKind::Codabar => printer.codabar_option(data, option)?,
        BarcodeKind::Itf => printer.itf_option(data, option)?,
        BarcodeKind::Upca => printer.upca_option(data, option)?,
        BarcodeKind::Upce => printer.upce_option(data, option)?,
    };
    Ok(())
}

fn print_text_line<D: Driver>(printer: &mut Printer<D>, text: &str) -> Result<(), PrinterError> {
    let encoded = encode_printer_text(text)?;
    printer.custom(&encoded)?;
    printer.feed()?;
    Ok(())
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
                    items.push(fit_text(&value, col.width, col.align));
                }
                lines.push(items.join(""));
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

fn print_image_node<D: Driver>(
    printer: &mut Printer<D>,
    path: &Option<String>,
    base64: &Option<String>,
    max_width: u32,
    max_height: Option<u32>,
    handlebars: &Handlebars,
    data: &Value,
) -> Result<(), PrinterError> {
    if let Some(path_template) = path {
        let rendered_path = render_value(handlebars, path_template, data)?;
        if !rendered_path.is_empty() {
            printer.bit_image_option(
                &rendered_path,
                image_bit_option(&rendered_path, max_width, max_height)?,
            )?;
            return Ok(());
        }
    }

    if let Some(base64_template) = base64 {
        let rendered_base64 = render_value(handlebars, base64_template, data)?;
        let bytes = decode_image_base64(&rendered_base64)?;
        printer.bit_image_from_bytes_option(
            &bytes,
            image_bytes_bit_option(&bytes, max_width, max_height)?,
        )?;
        return Ok(());
    }

    Err(PrinterError::InvalidImageData(
        "image 节点需要 path 或 base64".into(),
    ))
}

fn render_lines_to_image(lines: &[String], template: &Template) -> Result<String, PrinterError> {
    let text = lines.join("\n");
    let width = receipt_pixel_width(template.width);
    let font_size = template
        .font_size
        .unwrap_or(if width <= 384 { 24.0 } else { 26.0 })
        .clamp(12.0, 72.0);
    let line_height = (font_size * 1.35_f32).ceil();
    let padding = 12u32;
    let height = ((lines.len().max(1) as f32 * line_height).ceil() as u32) + padding * 2;
    let mut image = GrayImage::from_pixel(width, height, Luma([255]));
    let mut font_system = FontSystem::new();
    let mut swash_cache = SwashCache::new();
    let metrics = Metrics::new(font_size, line_height);
    let mut buffer = Buffer::new(&mut font_system, metrics);

    buffer.set_size(
        &mut font_system,
        Some((width - padding * 2) as f32),
        Some(height as f32),
    );
    let mut attrs = Attrs::new();
    if let Some(font_family) = template.font_family.as_deref().map(str::trim) {
        if !font_family.is_empty() {
            attrs = attrs.family(Family::Name(font_family));
        }
    }
    buffer.set_text(&mut font_system, &text, &attrs, Shaping::Advanced, None);
    buffer.shape_until_scroll(&mut font_system, false);
    buffer.draw(
        &mut font_system,
        &mut swash_cache,
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

fn image_bit_option(
    path: &str,
    max_width: u32,
    max_height: Option<u32>,
) -> Result<BitImageOption, PrinterError> {
    let (width, height) =
        image::image_dimensions(path).map_err(|e| PrinterError::ImageRender(e.to_string()))?;
    image_bit_option_for_dimensions(width, height, max_width, max_height)
}

fn image_bytes_bit_option(
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
        ((height as u64 * target_width as u64 + width as u64 - 1) / width as u64) as u32
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

fn decode_image_base64(value: &str) -> Result<Vec<u8>, PrinterError> {
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

fn unique_temp_suffix() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0)
}

fn encode_printer_text(text: &str) -> Result<Vec<u8>, PrinterError> {
    let normalized = text.replace('¥', "￥");
    let (encoded, _, had_errors) = GBK.encode(&normalized);
    if had_errors {
        return Err(PrinterError::Encode("GBK 不支持部分字符".into()));
    }
    Ok(encoded.into_owned())
}

fn format_row(left: &str, right: &str, line_width: usize) -> Vec<String> {
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

fn fit_text(value: &str, width: usize, align: Align) -> String {
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

fn repeat_to_width(ch: char, width: usize) -> String {
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

fn display_width(value: &str) -> usize {
    UnicodeWidthStr::width(value)
}

fn render_value(handlebars: &Handlebars, tmpl: &str, data: &Value) -> Result<String, PrinterError> {
    handlebars
        .render_template(tmpl, data)
        .map_err(|e| PrinterError::Render(e.to_string()))
}

fn value_ref<'a>(data: &'a Value, path: &str) -> Option<&'a Value> {
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

fn justify_mode(align: Align) -> JustifyMode {
    match align {
        Align::Left => JustifyMode::LEFT,
        Align::Center => JustifyMode::CENTER,
        Align::Right => JustifyMode::RIGHT,
    }
}

fn hex_decode(hex_str: &str) -> Result<Vec<u8>, PrinterError> {
    let normalized = hex_str
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>();
    hex::decode(normalized).map_err(|e| PrinterError::InvalidRawHex(e.to_string()))
}

fn into_response(result: Result<Value, PrinterError>) -> String {
    match result {
        Ok(value) => json!({ "ok": true, "result": value }).to_string(),
        Err(err) => json!({ "ok": false, "error": err.to_string() }).to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_order_template() {
        let template = json!({
            "width": 32,
            "elements": [
                {"type": "text", "value": "{{store.name}}", "align": "center", "bold": true},
                {"type": "repeat", "path": "items", "elements": [
                    {"type": "row", "left": "{{name}}", "right": "{{amount}}"}
                ]},
                {"type": "row", "left": "合计", "right": "{{order.total}}"},
                {"type": "feed", "lines": 2},
                {"type": "cut"}
            ]
        })
        .to_string();
        let data = json!({
            "store": {"name": "测试餐厅"},
            "order": {"total": "¥88.00"},
            "items": [{"name": "牛肉饭", "amount": "¥58.00"}, {"name": "柠檬茶", "amount": "¥30.00"}]
        }).to_string();

        let result = render_receipt_inner(&template, &data).unwrap();
        let hex = result["bytes"].as_str().unwrap();
        assert!(!hex.is_empty());
        assert_eq!(
            result["length"].as_i64().unwrap_or(0) as usize,
            hex::decode(hex).unwrap().len()
        );
    }

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
    fn truncates_long_column_without_overflowing_width() {
        let value = fit_text("超长商品名称", 8, Align::Left);

        assert_eq!(display_width(&value), 8);
        assert_eq!(value, "超长商品");
    }

    #[test]
    fn encodes_cjk_text_as_gbk_for_escpos_printers() {
        let bytes = encode_printer_text("牛肉饭 ¥58.00").unwrap();

        assert_eq!(
            bytes,
            vec![
                0xc5, 0xa3, 0xc8, 0xe2, 0xb7, 0xb9, b' ', 0xa3, 0xa4, b'5', b'8', b'.', b'0', b'0'
            ]
        );
    }

    #[test]
    fn rejects_invalid_raw_hex() {
        let template = json!({
            "elements": [
                {"type": "raw", "hex": "zz"}
            ]
        })
        .to_string();
        let data = json!({}).to_string();

        let result = render_receipt_inner(&template, &data);

        assert!(matches!(result, Err(PrinterError::InvalidRawHex(_))));
    }

    #[test]
    fn renders_advanced_template_with_codes_and_image() {
        let image_path = create_test_logo();
        let template = advanced_template(&image_path);
        let data = advanced_data();

        let result = render_receipt_inner(&template, &data).unwrap();
        let hex = result["bytes"].as_str().unwrap();

        assert!(!hex.is_empty());
        assert!(result["length"].as_u64().unwrap_or(0) > 256);
    }

    #[test]
    fn renders_image_from_base64_stream() {
        let image_path = create_test_logo();
        let image_bytes = std::fs::read(&image_path).unwrap();
        let image_base64 = base64::engine::general_purpose::STANDARD.encode(image_bytes);
        let template = json!({
            "width": 32,
            "elements": [
                {"type": "image", "base64": "{{receipt.imageBase64}}", "max_width": 384, "align": "center"},
                {"type": "feed", "lines": 1},
                {"type": "cut"}
            ]
        })
        .to_string();
        let data = json!({
            "receipt": {"imageBase64": image_base64}
        })
        .to_string();

        let result = render_receipt_inner(&template, &data).unwrap();

        assert!(result["length"].as_u64().unwrap_or(0) > 256);
    }

    #[test]
    fn renders_uyghur_order_template_as_image() {
        let template = json!({
            "width": 32,
            "encoding": "image",
            "elements": [
                {"type": "text", "value": "{{store.name}}", "align": "center", "bold": true},
                {"type": "row", "left": "زاكاز", "right": "{{order.no}}"},
                {"type": "divider"},
                {"type": "repeat", "path": "items", "elements": [
                    {"type": "columns", "columns": [
                        {"value": "{{name}}", "width": 20},
                        {"value": "{{qty}}", "width": 4, "align": "right"},
                        {"value": "{{amount}}", "width": 8, "align": "right"}
                    ]},
                    {"type": "text", "value": "{{remark}}"}
                ]},
                {"type": "row", "left": "جەمئىي", "right": "{{order.total}}"},
                {"type": "feed", "lines": 2},
                {"type": "cut"}
            ]
        })
        .to_string();
        let data = json!({
            "store": {"name": "ئۈرۈمچى ئاشخانىسى"},
            "order": {"no": "U001", "total": "88.00"},
            "items": [
                {"name": "لەغمەن", "qty": "1", "amount": "38.00", "remark": "ئاچچىق بولمىسۇن"},
                {"name": "چاي", "qty": "2", "amount": "50.00", "remark": ""}
            ]
        })
        .to_string();

        let result = render_receipt_inner(&template, &data).unwrap();
        let hex = result["bytes"].as_str().unwrap();

        assert!(!hex.is_empty());
        assert!(result["length"].as_u64().unwrap_or(0) > 1024);
    }

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

    #[test]
    #[ignore = "requires a real USB ESC/POS printer"]
    fn usb_prints_smoke_receipt_from_env() {
        let vendor_id = parse_u16_env("KSERVICE_PRINTER_USB_VENDOR_ID");
        let product_id = parse_u16_env("KSERVICE_PRINTER_USB_PRODUCT_ID");
        let connection = PrinterConnection::Usb {
            vendor_id,
            product_id,
        };
        let template = json!({
            "width": 32,
            "elements": [
                {"type": "text", "value": "KService USB Test", "align": "center", "bold": true},
                {"type": "divider"},
                {"type": "row", "left": "打印机", "right": "Xprinter"},
                {"type": "columns", "columns": [
                    {"value": "商品", "width": 16},
                    {"value": "数量", "width": 6, "align": "right"},
                    {"value": "金额", "width": 10, "align": "right"}
                ]},
                {"type": "columns", "columns": [
                    {"value": "牛肉饭", "width": 16},
                    {"value": "1", "width": 6, "align": "right"},
                    {"value": "¥58.00", "width": 10, "align": "right"}
                ]},
                {"type": "row", "left": "合计", "right": "¥58.00", "bold": true},
                {"type": "feed", "lines": 3},
                {"type": "cut"}
            ]
        })
        .to_string();
        let data = json!({}).to_string();

        let result = print_receipt_inner(&connection, &template, &data).unwrap();

        assert_eq!(result["printed"].as_bool(), Some(true));
        assert!(result["bytes"].as_u64().unwrap_or(0) > 0);
    }

    #[test]
    #[ignore = "requires a real USB ESC/POS printer"]
    fn usb_prints_advanced_receipt_from_env() {
        let vendor_id = parse_u16_env("KSERVICE_PRINTER_USB_VENDOR_ID");
        let product_id = parse_u16_env("KSERVICE_PRINTER_USB_PRODUCT_ID");
        let connection = PrinterConnection::Usb {
            vendor_id,
            product_id,
        };
        let image_path = create_test_logo();
        let template = advanced_template(&image_path);
        let data = advanced_data();

        let result = print_receipt_inner(&connection, &template, &data).unwrap();

        assert_eq!(result["printed"].as_bool(), Some(true));
        assert!(result["bytes"].as_u64().unwrap_or(0) > 512);
    }

    #[test]
    #[ignore = "requires a real USB ESC/POS printer and KSERVICE_PRINTER_IMAGE_PATH"]
    fn usb_prints_image_from_env() {
        let vendor_id = parse_u16_env("KSERVICE_PRINTER_USB_VENDOR_ID");
        let product_id = parse_u16_env("KSERVICE_PRINTER_USB_PRODUCT_ID");
        let image_path = std::env::var("KSERVICE_PRINTER_IMAGE_PATH")
            .expect("KSERVICE_PRINTER_IMAGE_PATH is required");
        let image_max_width = std::env::var("KSERVICE_PRINTER_IMAGE_MAX_WIDTH")
            .ok()
            .and_then(|value| value.parse::<u32>().ok())
            .unwrap_or(576);
        let connection = PrinterConnection::Usb {
            vendor_id,
            product_id,
        };
        let template = json!({
            "width": 32,
            "elements": [
                {"type": "text", "value": "Image Print Test", "align": "center", "bold": true},
                {"type": "image", "path": image_path, "max_width": image_max_width, "align": "center"},
                {"type": "feed", "lines": 3},
                {"type": "cut"}
            ]
        })
        .to_string();
        let data = json!({}).to_string();

        let result = print_receipt_inner(&connection, &template, &data).unwrap();

        assert_eq!(result["printed"].as_bool(), Some(true));
        assert!(result["bytes"].as_u64().unwrap_or(0) > 512);
    }

    fn advanced_template(image_path: &str) -> String {
        json!({
            "width": 32,
            "elements": [
                {"type": "text", "value": "高级打印测试", "align": "center", "bold": true, "size": "double"},
                {"type": "image", "path": image_path, "max_width": 128, "align": "center"},
                {"type": "divider"},
                {"type": "row", "left": "单号", "right": "{{order.no}}"},
                {"type": "row", "left": "客户", "right": "{{#if customer.vip}}会员{{else}}散客{{/if}}"},
                {"type": "columns", "columns": [
                    {"value": "商品", "width": 16},
                    {"value": "数", "width": 4, "align": "right"},
                    {"value": "金额", "width": 12, "align": "right"}
                ]},
                {"type": "repeat", "path": "items", "elements": [
                    {"type": "columns", "columns": [
                        {"value": "{{name}}", "width": 16},
                        {"value": "{{qty}}", "width": 4, "align": "right"},
                        {"value": "{{amount}}", "width": 12, "align": "right"}
                    ]},
                    {"type": "text", "value": "{{#if remark}}备注: {{remark}}{{/if}}"}
                ]},
                {"type": "divider"},
                {"type": "row", "left": "合计", "right": "{{order.total}}", "bold": true},
                {"type": "text", "value": "扫码查看订单", "align": "center"},
                {"type": "qrcode", "value": "{{order.qr}}", "size": 5, "align": "center"},
                {"type": "text", "value": "条码", "align": "center"},
                {"type": "barcode", "system": "ean13", "value": "{{order.barcode}}", "align": "center"},
                {"type": "raw", "hex": "1b 21 00"},
                {"type": "feed", "lines": 3},
                {"type": "cut"}
            ]
        })
        .to_string()
    }

    fn advanced_data() -> String {
        json!({
            "order": {
                "no": "A20260609002",
                "total": "¥108.00",
                "qr": "https://kservice.local/order/A20260609002",
                "barcode": "6901234567892"
            },
            "customer": {"vip": true},
            "items": [
                {"name": "招牌牛肉饭", "qty": "1", "amount": "¥58.00", "remark": "少辣"},
                {"name": "柠檬茶", "qty": "2", "amount": "¥40.00", "remark": ""},
                {"name": "加蛋", "qty": "1", "amount": "¥10.00", "remark": ""}
            ]
        })
        .to_string()
    }

    fn create_test_logo() -> String {
        let path = std::env::temp_dir().join(format!(
            "kservice-printer-test-logo-{}.png",
            std::process::id()
        ));
        let mut image = image::GrayImage::from_pixel(128, 48, image::Luma([255]));
        for x in 0..128 {
            image.put_pixel(x, 0, image::Luma([0]));
            image.put_pixel(x, 47, image::Luma([0]));
        }
        for y in 0..48 {
            image.put_pixel(0, y, image::Luma([0]));
            image.put_pixel(127, y, image::Luma([0]));
        }
        for x in 12..116 {
            if x % 8 < 4 {
                for y in 10..16 {
                    image.put_pixel(x, y, image::Luma([0]));
                }
            }
        }
        for y in 24..38 {
            for x in 18..110 {
                if (x + y) % 13 == 0 || (x > 42 && x < 86 && y > 28 && y < 34) {
                    image.put_pixel(x, y, image::Luma([0]));
                }
            }
        }
        image.save(&path).unwrap();
        path.to_string_lossy().into_owned()
    }

    fn parse_u16_env(name: &str) -> u16 {
        let value = std::env::var(name).unwrap_or_else(|_| panic!("{name} is required"));
        let trimmed = value.trim();
        if let Some(hex) = trimmed
            .strip_prefix("0x")
            .or_else(|| trimmed.strip_prefix("0X"))
        {
            u16::from_str_radix(hex, 16).unwrap()
        } else {
            trimmed.parse().unwrap()
        }
    }
}
