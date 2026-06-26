//! 小票渲染流水线：模板分发、连接管理和 ESC/POS 字节生成。
//!
//! 根据模板和数据选择正确的打印路径：
//! - **TSPL-image**：先渲染成位图，再封装成 `BITMAP` 指令
//! - **TSPL**：直接生成原生 TSPL 指令流
//! - **Image/bitmap encoding**：把文本合成图片，适合阿拉伯语、维吾尔语等复杂文字
//! - **default**：使用原生 ESC/POS 文本指令
//!
//! 连接包装器（`AnyDriver`、`TcpDriver`、`VecDriver`、`CountingDriver`）
//! 抹平 TCP/USB/串口差异，让模板渲染流程不用直接关心底层 I/O。

#[cfg(not(target_os = "windows"))]
use escpos::driver::UsbDriver;
use escpos::driver::{Driver, SerialPortDriver};
use escpos::errors::PrinterError as EscposError;
use escpos::printer::Printer;
use escpos::utils::*;
use handlebars::{no_escape, Handlebars};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::api::printer::PrinterConnection;
use crate::discovery::{discover_network_printers_inner, list_usb_printers_inner};
use crate::error::PrinterError;
use crate::protocol::{tspl, zpl};
use crate::render::encoding::{encode_printer_text, normalize_text_for_encoding};
use crate::render::image::{
    decode_image_base64, image_bit_option, image_bytes_bit_option, render_template_as_image,
};
use crate::render::text_layout::{format_columns, format_row, repeat_to_width};
use crate::render::value::{hex_decode, render_value, value_ref};
use crate::template::{parse_template, BarcodeKind, Element, Template, TextSize};
use crate::util::{has_cut_element, into_response, justify_mode};
#[cfg(target_os = "windows")]
use crate::windows_usbprint::WindowsUsbPrintDriver;

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
    #[cfg(not(target_os = "windows"))]
    Usb(UsbDriver),
    #[cfg(target_os = "windows")]
    WindowsUsbPrint(WindowsUsbPrintDriver),
    Serial(SerialPortDriver),
}

impl Driver for AnyDriver {
    fn name(&self) -> String {
        match self {
            AnyDriver::Tcp(d) => d.name(),
            #[cfg(not(target_os = "windows"))]
            AnyDriver::Usb(d) => d.name(),
            #[cfg(target_os = "windows")]
            AnyDriver::WindowsUsbPrint(d) => d.name(),
            AnyDriver::Serial(d) => d.name(),
        }
    }
    fn write(&self, data: &[u8]) -> std::result::Result<(), EscposError> {
        match self {
            AnyDriver::Tcp(d) => d.write(data),
            #[cfg(not(target_os = "windows"))]
            AnyDriver::Usb(d) => d.write(data),
            #[cfg(target_os = "windows")]
            AnyDriver::WindowsUsbPrint(d) => d.write(data),
            AnyDriver::Serial(d) => d.write(data),
        }
    }
    fn read(&self, buf: &mut [u8]) -> std::result::Result<usize, EscposError> {
        match self {
            AnyDriver::Tcp(d) => d.read(buf),
            #[cfg(not(target_os = "windows"))]
            AnyDriver::Usb(d) => d.read(buf),
            #[cfg(target_os = "windows")]
            AnyDriver::WindowsUsbPrint(d) => d.read(buf),
            AnyDriver::Serial(d) => d.read(buf),
        }
    }
    fn flush(&self) -> std::result::Result<(), EscposError> {
        match self {
            AnyDriver::Tcp(d) => d.flush(),
            #[cfg(not(target_os = "windows"))]
            AnyDriver::Usb(d) => d.flush(),
            #[cfg(target_os = "windows")]
            AnyDriver::WindowsUsbPrint(d) => d.flush(),
            AnyDriver::Serial(d) => d.flush(),
        }
    }
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

pub fn open_cash_drawer(
    connection: &PrinterConnection,
    pin: u8,
    on_ms: u16,
    off_ms: u16,
) -> String {
    into_response(open_cash_drawer_inner(connection, pin, on_ms, off_ms))
}

pub fn list_usb_printers() -> String {
    into_response(list_usb_printers_inner())
}

pub fn discover_network_printers(timeout_ms: u64, service_types: Vec<String>) -> String {
    into_response(discover_network_printers_inner(timeout_ms, service_types))
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
    if zpl::is_zpl_image_template(&template) {
        let bytes = zpl::render_template_as_zpl_image_bytes(&template, &data)?;
        let (driver, written) = CountingDriver::new(open_driver(connection)?);
        driver.write(&bytes)?;
        driver.flush()?;
        let bytes = *written.lock().unwrap();
        return Ok(json!({ "printed": true, "bytes": bytes }));
    }

    if zpl::is_zpl_template(&template) {
        let bytes = zpl::render_template_as_zpl_bytes(&template, &data)?;
        let (driver, written) = CountingDriver::new(open_driver(connection)?);
        driver.write(&bytes)?;
        driver.flush()?;
        let bytes = *written.lock().unwrap();
        return Ok(json!({ "printed": true, "bytes": bytes }));
    }

    if tspl::is_tspl_image_template(&template) {
        let bytes = tspl::render_template_as_tspl_image_bytes(&template, &data)?;
        let (driver, written) = CountingDriver::new(open_driver(connection)?);
        driver.write(&bytes)?;
        driver.flush()?;
        let bytes = *written.lock().unwrap();
        return Ok(json!({ "printed": true, "bytes": bytes }));
    }

    if tspl::is_tspl_template(&template) {
        let bytes = tspl::render_template_as_tspl_bytes(&template, &data)?;
        let (driver, written) = CountingDriver::new(open_driver(connection)?);
        driver.write(&bytes)?;
        driver.flush()?;
        let bytes = *written.lock().unwrap();
        return Ok(json!({ "printed": true, "bytes": bytes }));
    }

    let (driver, bytes) = CountingDriver::new(open_driver(connection)?);
    let mut printer = build_printer(driver, &template, &data)?;
    printer.print()?;
    let bytes = *bytes.lock().unwrap();
    Ok(json!({ "printed": true, "bytes": bytes }))
}

fn render_receipt_inner(template_json: &str, data_json: &str) -> Result<Value, PrinterError> {
    let template = parse_template(template_json)?;
    let data: Value =
        serde_json::from_str(data_json).map_err(|e| PrinterError::InvalidData(e.to_string()))?;
    if zpl::is_zpl_image_template(&template) {
        let bytes = zpl::render_template_as_zpl_image_bytes(&template, &data)?;
        return Ok(json!({ "bytes": hex::encode(&bytes), "length": bytes.len() }));
    }

    if zpl::is_zpl_template(&template) {
        let bytes = zpl::render_template_as_zpl_bytes(&template, &data)?;
        return Ok(json!({ "bytes": hex::encode(&bytes), "length": bytes.len() }));
    }

    if tspl::is_tspl_image_template(&template) {
        let bytes = tspl::render_template_as_tspl_image_bytes(&template, &data)?;
        return Ok(json!({ "bytes": hex::encode(&bytes), "length": bytes.len() }));
    }

    if tspl::is_tspl_template(&template) {
        let bytes = tspl::render_template_as_tspl_bytes(&template, &data)?;
        return Ok(json!({ "bytes": hex::encode(&bytes), "length": bytes.len() }));
    }

    let (driver, buf) = VecDriver::new();
    let mut printer = build_printer(driver, &template, &data)?;
    printer.print()?;
    let bytes = buf.lock().unwrap().clone();
    Ok(json!({ "bytes": hex::encode(&bytes), "length": bytes.len() }))
}

fn open_cash_drawer_inner(
    connection: &PrinterConnection,
    pin: u8,
    on_ms: u16,
    off_ms: u16,
) -> Result<Value, PrinterError> {
    let bytes = cash_drawer_pulse_command(pin, on_ms, off_ms)?;
    let (driver, written) = CountingDriver::new(open_driver(connection)?);
    driver.write(&bytes)?;
    driver.flush()?;
    let bytes = *written.lock().unwrap();
    Ok(json!({ "printed": true, "bytes": bytes }))
}

fn cash_drawer_pulse_command(pin: u8, on_ms: u16, off_ms: u16) -> Result<[u8; 5], PrinterError> {
    if pin > 1 {
        return Err(PrinterError::CashDrawer(format!(
            "钱箱引脚只能是 0(pin2) 或 1(pin5)，当前为 {pin}"
        )));
    }
    Ok([
        0x1b,
        b'p',
        pin,
        cash_drawer_duration_units(on_ms),
        cash_drawer_duration_units(off_ms),
    ])
}

fn cash_drawer_duration_units(ms: u16) -> u8 {
    u32::from(ms).div_ceil(2).clamp(0, u32::from(u8::MAX)) as u8
}

fn open_driver(connection: &PrinterConnection) -> Result<AnyDriver, PrinterError> {
    match connection {
        PrinterConnection::Network {
            host,
            port,
            timeout_ms,
        } => {
            let driver =
                TcpDriver::open(host, *port, *timeout_ms).map_err(PrinterError::Connect)?;
            Ok(AnyDriver::Tcp(driver))
        }
        PrinterConnection::Usb {
            vendor_id,
            product_id,
        } => {
            #[cfg(target_os = "windows")]
            {
                let driver = WindowsUsbPrintDriver::open_by_vid_pid(*vendor_id, *product_id)
                    .map_err(|e| PrinterError::Connect(e.to_string()))?;
                Ok(AnyDriver::WindowsUsbPrint(driver))
            }

            #[cfg(not(target_os = "windows"))]
            {
                let driver = UsbDriver::open(*vendor_id, *product_id, None, None)
                    .map_err(|e| PrinterError::Connect(e.to_string()))?;
                Ok(AnyDriver::Usb(driver))
            }
        }
        PrinterConnection::Serial { port, baud_rate } => {
            let timeout = Some(Duration::from_secs(5));
            let driver = SerialPortDriver::open(port, *baud_rate, timeout)
                .map_err(|e| PrinterError::Connect(e.to_string()))?;
            Ok(AnyDriver::Serial(driver))
        }
    }
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
    let mut handlebars = Handlebars::new();
    handlebars.register_escape_fn(no_escape);
    let mut printer = Printer::new(driver, Protocol::default(), None);
    printer.init()?;
    // 0x1c 0x26 是部分中文热敏机需要的 ESC/POS 复位序列；
    // 避免设备残留在页模式等异常状态，导致后续文本不按标准模式输出。
    printer.custom(&[0x1c, 0x26])?;

    if template.encoding.eq_ignore_ascii_case("image")
        || template.encoding.eq_ignore_ascii_case("bitmap")
    {
        render_template_as_image(&mut printer, template, data, &handlebars)?;
        if has_cut_element(&template.elements) {
            printer.cut()?;
        }
    } else {
        for element in &template.elements {
            render_element(
                &mut printer,
                element,
                data,
                &handlebars,
                template.width,
                &template.encoding,
            )?;
        }
    }

    Ok(printer)
}

fn render_element<D: Driver>(
    printer: &mut Printer<D>,
    element: &Element,
    data: &Value,
    handlebars: &Handlebars,
    line_width: usize,
    encoding: &str,
) -> Result<(), PrinterError> {
    match element {
        Element::Text {
            value,
            align,
            bold,
            size,
        } => {
            let text = render_text_value(handlebars, value, data, encoding)?;
            if text.trim().is_empty() {
                return Ok(());
            }
            printer.bold(*bold)?;
            if matches!(size, TextSize::Double) {
                printer.size(2, 2)?;
            }
            printer.justify(justify_mode(*align))?;
            print_text_line(printer, &text, encoding)?;
            if matches!(size, TextSize::Double) {
                printer.size(1, 1)?;
            }
            printer.bold(false)?;
        }
        Element::Row { left, right, bold } => {
            let l = render_text_value(handlebars, left, data, encoding)?;
            let r = render_text_value(handlebars, right, data, encoding)?;
            printer.bold(*bold)?;
            printer.justify(JustifyMode::LEFT)?;
            for line in format_row(&l, &r, line_width) {
                print_text_line(printer, &line, encoding)?;
            }
            printer.bold(false)?;
        }
        Element::Columns { columns } => {
            let mut items = Vec::new();
            let bold = columns.iter().any(|col| col.bold);
            for col in columns {
                let value = render_text_value(handlebars, &col.value, data, encoding)?;
                items.push((value, col.width, col.align));
            }
            printer.bold(bold)?;
            printer.justify(JustifyMode::LEFT)?;
            for line in format_columns(&items) {
                print_text_line(printer, &line, encoding)?;
            }
            printer.bold(false)?;
        }
        Element::Divider { ch } => {
            let token = ch.chars().next().unwrap_or('-');
            printer.justify(JustifyMode::LEFT)?;
            print_text_line(printer, &repeat_to_width(token, line_width), encoding)?;
        }
        Element::Feed { lines } => {
            for _ in 0..*lines {
                printer.feed()?;
            }
        }
        Element::Cut => {
            printer.cut()?;
        }
        Element::Repeat { path, elements } => {
            if let Some(Value::Array(items)) = value_ref(data, path) {
                for item in items {
                    for child in elements {
                        render_element(printer, child, item, handlebars, line_width, encoding)?;
                    }
                }
            }
        }
        Element::Raw { hex } => {
            let bytes = hex_decode(hex)?;
            printer.custom(&bytes)?;
        }
        Element::QrCode {
            value, size, align, ..
        } => {
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

fn print_text_line<D: Driver>(
    printer: &mut Printer<D>,
    text: &str,
    encoding: &str,
) -> Result<(), PrinterError> {
    let encoded = encode_printer_text(text, encoding)?;
    printer.custom(&encoded)?;
    printer.feed()?;
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

fn render_text_value(
    handlebars: &Handlebars,
    tmpl: &str,
    data: &Value,
    encoding: &str,
) -> Result<String, PrinterError> {
    let value = render_value(handlebars, tmpl, data)?;
    Ok(normalize_text_for_encoding(&value, encoding))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::image::unique_temp_suffix;
    use base64::Engine;

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
    fn renders_tspl_label_template_without_escpos_commands() {
        let template = json!({
            "width": 32,
            "encoding": "tspl",
            "labelWidthMm": 58,
            "labelHeightMm": 40,
            "labelGapMm": 2,
            "labelHomeBeforePrint": true,
            "labelReferenceX": 8,
            "labelReferenceY": 12,
            "labelShiftDots": -4,
            "elements": [
                {"type": "text", "value": "{{item.name}}", "align": "center", "bold": true, "size": "double"},
                {"type": "divider"},
                {"type": "row", "left": "SKU", "right": "{{item.sku}}"},
                {"type": "qrcode", "value": "{{item.sku}}", "size": 3, "x": 320, "y": 210}
            ]
        })
        .to_string();
        let data = json!({
            "item": {"name": "招牌牛肉饭", "sku": "BEEF-001"}
        })
        .to_string();

        let result = render_receipt_inner(&template, &data).unwrap();
        let bytes = hex::decode(result["bytes"].as_str().unwrap()).unwrap();
        let script = String::from_utf8_lossy(&bytes);

        assert!(script.starts_with("SIZE 58 mm,40 mm\r\n"));
        assert!(script.contains("GAP 2 mm,0 mm\r\n"));
        assert!(script.contains("REFERENCE 8,12\r\n"));
        assert!(script.contains("SHIFT -4\r\n"));
        assert!(script.contains("HOME\r\n"));
        assert!(script.contains("CLS\r\n"));
        assert!(script.contains("TEXT "));
        assert!(script.contains("SKU"));
        assert!(script.contains("QRCODE 320,210,L,3,A,0,\"BEEF-001\""));
        assert!(script.ends_with("PRINT 1,1\r\n"));
        assert!(!bytes.starts_with(&[0x1b, 0x40]));
    }

    #[test]
    fn renders_tspl_image_label_as_bitmap_command() {
        let template = json!({
            "width": 32,
            "encoding": "tspl-image",
            "fontFamily": "Noto Sans Arabic",
            "fontSize": 24,
            "labelWidthMm": 58,
            "labelHeightMm": 40,
            "labelGapMm": 2,
            "labelHomeBeforePrint": true,
            "elements": [
                {"type": "text", "value": "{{item.name}}", "align": "center", "bold": true, "size": "double"},
                {"type": "row", "left": "SKU", "right": "{{item.sku}}"},
                {"type": "text", "value": "{{label.remark}}", "align": "center"},
                {"type": "qrcode", "value": "{{item.sku}}", "size": 2, "x": 304, "y": 192}
            ]
        })
        .to_string();
        let data = json!({
            "item": {"name": "لاڭمەن", "sku": "BEEF-001"},
            "label": {"remark": "سوغۇق ساقلاڭ"}
        })
        .to_string();

        let result = render_receipt_inner(&template, &data).unwrap();
        let bytes = hex::decode(result["bytes"].as_str().unwrap()).unwrap();
        let script = String::from_utf8_lossy(&bytes);

        assert!(script.starts_with("SIZE 58 mm,40 mm\r\n"));
        assert!(script.contains("HOME\r\nCLS\r\nBITMAP 0,0,58,320,0,"));
        assert!(script.ends_with("PRINT 1,1\r\n"));
        assert!(!bytes.starts_with(&[0x1b, 0x40]));

        let header = b"BITMAP 0,0,58,320,0,";
        let bitmap_start = bytes
            .windows(header.len())
            .position(|window| window == header)
            .unwrap()
            + header.len();
        let bitmap_end = bitmap_start + 58 * 320;
        assert!(bytes[bitmap_start..bitmap_end]
            .iter()
            .any(|byte| *byte != 0));
    }

    #[test]
    fn renders_tspl_image_text_as_bar_raster() {
        let template = json!({
            "width": 32,
            "encoding": "tspl-raster",
            "fontSize": 32,
            "labelWidthMm": 58,
            "labelHeightMm": 30,
            "labelGapMm": 2,
            "elements": [
                {"type": "text", "value": "{{item.name}}", "align": "center", "bold": true}
            ]
        })
        .to_string();
        let data = json!({"item": {"name": "ABC-123"}}).to_string();

        let result = render_receipt_inner(&template, &data).unwrap();
        let bytes = hex::decode(result["bytes"].as_str().unwrap()).unwrap();
        let script = String::from_utf8_lossy(&bytes);

        assert!(script.starts_with("SIZE 58 mm,30 mm\r\n"));
        assert!(script.contains("\r\nBAR "));
        assert!(!script.contains("BITMAP"));
        assert!(script.ends_with("PRINT 1,1\r\n"));
    }

    #[test]
    fn renders_zpl_label_template_without_escpos_commands() {
        let template = json!({
            "width": 32,
            "encoding": "zpl",
            "labelWidthMm": 58,
            "labelHeightMm": 40,
            "labelDensity": 8,
            "labelSpeed": 4,
            "labelReferenceX": 8,
            "labelReferenceY": 12,
            "labelShiftDots": -4,
            "elements": [
                {"type": "text", "value": "{{item.name}}", "align": "center", "bold": true, "size": "double"},
                {"type": "divider"},
                {"type": "row", "left": "SKU", "right": "{{item.sku}}"},
                {"type": "qrcode", "value": "{{item.sku}}", "size": 3, "x": 320, "y": 210}
            ]
        })
        .to_string();
        let data = json!({
            "item": {"name": "Beef Rice", "sku": "BEEF-001"}
        })
        .to_string();

        let result = render_receipt_inner(&template, &data).unwrap();
        let bytes = hex::decode(result["bytes"].as_str().unwrap()).unwrap();
        let script = String::from_utf8_lossy(&bytes);

        assert!(script.starts_with("^XA\n"));
        assert!(script.contains("^CI28\n"));
        assert!(script.contains("^PW464\n"));
        assert!(script.contains("^LL320\n"));
        assert!(script.contains("^LH8,12\n"));
        assert!(script.contains("^MD8\n"));
        assert!(script.contains("^PR4\n"));
        assert!(script.contains("^LS-4\n"));
        assert!(script.contains("^A0N,56,60"));
        assert!(script.contains("SKU"));
        assert!(script.contains("^FO320,210^BQN,2,3^FH\\^FDLA,BEEF-001^FS"));
        assert!(script.ends_with("^XZ\n"));
        assert!(!bytes.starts_with(&[0x1b, 0x40]));
    }

    #[test]
    fn renders_zpl_image_label_as_gfa_command() {
        let template = json!({
            "width": 32,
            "encoding": "zpl-image",
            "fontSize": 24,
            "labelWidthMm": 58,
            "labelHeightMm": 30,
            "labelDensity": 8,
            "labelSpeed": 4,
            "elements": [
                {"type": "text", "value": "{{item.name}}", "align": "center", "bold": true}
            ]
        })
        .to_string();
        let data = json!({"item": {"name": "ABC-123"}}).to_string();

        let result = render_receipt_inner(&template, &data).unwrap();
        let bytes = hex::decode(result["bytes"].as_str().unwrap()).unwrap();
        let script = String::from_utf8_lossy(&bytes);

        assert!(script.starts_with("^XA\n"));
        assert!(script.contains("^PW464\n"));
        assert!(script.contains("^LL240\n"));
        assert!(script.contains("^FO0,0^GFA,13920,13920,58,"));
        assert!(script.ends_with("^XZ\n"));
        assert!(!bytes.starts_with(&[0x1b, 0x40]));
    }

    #[test]
    fn builds_cash_drawer_pulse_command() {
        let command = cash_drawer_pulse_command(0, 200, 400).unwrap();

        assert_eq!(command, [0x1b, b'p', 0, 100, 200]);
        assert!(matches!(
            cash_drawer_pulse_command(2, 200, 200),
            Err(PrinterError::CashDrawer(_))
        ));
    }

    #[test]
    fn cut_element_controls_cut_command() {
        let without_cut = json!({
            "width": 32,
            "elements": [
                {"type": "text", "value": "No cut"}
            ]
        })
        .to_string();
        let with_cut = json!({
            "width": 32,
            "elements": [
                {"type": "text", "value": "With cut"},
                {"type": "cut"}
            ]
        })
        .to_string();
        let data = json!({}).to_string();

        let without_cut = render_bytes(&without_cut, &data);
        let with_cut = render_bytes(&with_cut, &data);

        assert!(!contains_subsequence(&without_cut, &[0x1d, b'V', b'A', 0]));
        assert!(contains_subsequence(&with_cut, &[0x1d, b'V', b'A', 0]));
    }

    #[test]
    fn text_element_resets_bold_and_size_state() {
        let template = json!({
            "width": 32,
            "elements": [
                {"type": "text", "value": "Title", "bold": true, "size": "double"},
                {"type": "text", "value": "Body"}
            ]
        })
        .to_string();
        let data = json!({}).to_string();

        let bytes = render_bytes(&template, &data);

        assert!(contains_subsequence(&bytes, &[0x1b, b'E', 1]));
        assert!(contains_subsequence(&bytes, &[0x1b, b'E', 0]));
        assert!(contains_subsequence(&bytes, &[0x1d, b'!', 0x11]));
        assert!(contains_subsequence(&bytes, &[0x1d, b'!', 0x00]));
    }

    #[test]
    fn handlebars_values_are_not_html_escaped() {
        let template = json!({
            "width": 32,
            "elements": [
                {"type": "text", "value": "{{store.name}}"}
            ]
        })
        .to_string();
        let data = json!({
            "store": {"name": "A&B <C>"}
        })
        .to_string();

        let bytes = render_bytes(&template, &data);

        assert!(contains_subsequence(&bytes, b"A&B <C>"));
        assert!(!contains_subsequence(&bytes, b"&amp;"));
    }

    #[test]
    fn columns_can_enable_bold_for_the_line() {
        let template = json!({
            "width": 32,
            "elements": [
                {"type": "columns", "columns": [
                    {"value": "品项", "width": 16, "bold": true},
                    {"value": "数量", "width": 16, "align": "right"}
                ]},
                {"type": "text", "value": "Body"}
            ]
        })
        .to_string();
        let data = json!({}).to_string();

        let bytes = render_bytes(&template, &data);

        assert!(contains_subsequence(&bytes, &[0x1b, b'E', 1]));
        assert!(contains_subsequence(&bytes, &[0x1b, b'E', 0]));
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
            "kservice-printer-test-logo-{}-{}.png",
            std::process::id(),
            unique_temp_suffix()
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

    fn render_bytes(template: &str, data: &str) -> Vec<u8> {
        let result = render_receipt_inner(template, data).unwrap();
        hex::decode(result["bytes"].as_str().unwrap()).unwrap()
    }

    fn contains_subsequence(bytes: &[u8], needle: &[u8]) -> bool {
        bytes.windows(needle.len()).any(|window| window == needle)
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
