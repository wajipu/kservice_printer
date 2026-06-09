use escpos::driver::{Driver, NativeUsbDriver, SerialPortDriver};
use escpos::errors::PrinterError as EscposError;
use escpos::printer::Printer;
use escpos::utils::*;
use handlebars::Handlebars;
use serde::Deserialize;
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use thiserror::Error;

use crate::api::printer::PrinterConnection;

// ---- 自定义 VecDriver：捕获打印字节，不做网络发送 ----

struct VecDriver {
    name: String,
    buf: Arc<Mutex<Vec<u8>>>,
}

impl VecDriver {
    fn new() -> (Self, Arc<Mutex<Vec<u8>>>) {
        let buf = Arc::new(Mutex::new(Vec::new()));
        (Self { name: "vec".into(), buf: buf.clone() }, buf)
    }
}

impl Driver for VecDriver {
    fn name(&self) -> String { self.name.clone() }
    fn write(&self, data: &[u8]) -> std::result::Result<(), EscposError> {
        self.buf.lock().unwrap().extend_from_slice(data);
        Ok(())
    }
    fn read(&self, _buf: &mut [u8]) -> std::result::Result<usize, EscposError> { Ok(0) }
    fn flush(&self) -> std::result::Result<(), EscposError> { Ok(()) }
}

// ---- AnyDriver：统一三种连接方式 ----

enum AnyDriver {
    Tcp(TcpDriver),
    Usb(NativeUsbDriver),
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
    _width: usize,
    #[serde(default)]
    elements: Vec<Element>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum Element {
    #[serde(rename = "text")]
    Text { value: String, #[serde(default)] align: Align, #[serde(default)] bold: bool, #[serde(default)] size: TextSize },
    #[serde(rename = "row")]
    Row { left: String, right: String, #[serde(default)] bold: bool },
    #[serde(rename = "columns")]
    Columns { columns: Vec<Column> },
    #[serde(rename = "divider")]
    Divider { #[serde(default = "default_divider_char")] ch: String },
    #[serde(rename = "feed")]
    Feed { #[serde(default = "default_feed_lines")] lines: u8 },
    #[serde(rename = "cut")]
    Cut,
    #[serde(rename = "repeat")]
    Repeat { path: String, elements: Vec<Element> },
    #[serde(rename = "raw")]
    Raw { hex: String },
}

#[derive(Debug, Deserialize)]
struct Column { value: String, #[serde(default = "default_column_width")] _width: usize, #[serde(default)] _align: Align }

#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Align { #[default] Left, Center, Right }

#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
enum TextSize { #[default] Normal, Double }

fn default_width() -> usize { 48 }
fn default_divider_char() -> String { "-".into() }
fn default_feed_lines() -> u8 { 3 }
fn default_column_width() -> usize { 12 }

// ---- 公开入口 ----

pub fn print_receipt(connection: &PrinterConnection, template_json: &str, data_json: &str) -> String {
    into_response(print_receipt_inner(connection, template_json, data_json))
}

pub fn render_receipt(template_json: &str, data_json: &str) -> String {
    into_response(render_receipt_inner(template_json, data_json))
}

// ---- 内部 ----

fn print_receipt_inner(connection: &PrinterConnection, template_json: &str, data_json: &str) -> Result<Value, PrinterError> {
    let template: Template =
        serde_json::from_str(template_json).map_err(|e| PrinterError::InvalidTemplate(e.to_string()))?;
    let data: Value =
        serde_json::from_str(data_json).map_err(|e| PrinterError::InvalidData(e.to_string()))?;
    let driver = open_driver(connection)?;
    let mut printer = build_printer(driver, &template, &data)?;
    printer.print_cut()?;
    Ok(json!({ "ok": true }))
}

fn render_receipt_inner(template_json: &str, data_json: &str) -> Result<Value, PrinterError> {
    let template: Template =
        serde_json::from_str(template_json).map_err(|e| PrinterError::InvalidTemplate(e.to_string()))?;
    let data: Value =
        serde_json::from_str(data_json).map_err(|e| PrinterError::InvalidData(e.to_string()))?;
    let (driver, buf) = VecDriver::new();
    let mut printer = build_printer(driver, &template, &data)?;
    printer.print_cut()?;
    let bytes = buf.lock().unwrap().clone();
    Ok(json!({ "bytes": hex::encode(&bytes), "length": bytes.len() }))
}

fn open_driver(connection: &PrinterConnection) -> Result<AnyDriver, PrinterError> {
    match connection {
        PrinterConnection::Network { host, port, timeout_ms } => {
            let driver = TcpDriver::open(host, *port, *timeout_ms)
                .map_err(|e| PrinterError::Connect(e))?;
            Ok(AnyDriver::Tcp(driver))
        }
        PrinterConnection::Usb { vendor_id, product_id } => {
            let driver = NativeUsbDriver::open(*vendor_id, *product_id)
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

// ---- TcpDriver ----

struct TcpDriver {
    name: String,
    stream: Mutex<TcpStream>,
}

impl TcpDriver {
    fn open(host: &str, port: u16, timeout_ms: u64) -> Result<Self, String> {
        let timeout = Duration::from_millis(timeout_ms);
        let mut addrs = (host, port)
            .to_socket_addrs()
            .map_err(|e| e.to_string())?;
        let addr = addrs
            .next()
            .ok_or_else(|| "没有解析出可用的 IP 地址".to_string())?;
        let stream = TcpStream::connect_timeout(&addr, timeout).map_err(|e| e.to_string())?;
        stream.set_write_timeout(Some(timeout)).map_err(|e| e.to_string())?;
        Ok(Self { name: format!("tcp://{host}:{port}"), stream: Mutex::new(stream) })
    }
}

impl Driver for TcpDriver {
    fn name(&self) -> String { self.name.clone() }
    fn write(&self, data: &[u8]) -> std::result::Result<(), EscposError> {
        let mut stream = self.stream.lock().unwrap();
        stream.write_all(data).map_err(|e| EscposError::Io(e.to_string()))?;
        stream.flush().map_err(|e| EscposError::Io(e.to_string()))
    }
    fn read(&self, buf: &mut [u8]) -> std::result::Result<usize, EscposError> {
        self.stream.lock().unwrap().read(buf).map_err(|e| EscposError::Io(e.to_string()))
    }
    fn flush(&self) -> std::result::Result<(), EscposError> {
        self.stream.lock().unwrap().flush().map_err(|e| EscposError::Io(e.to_string()))
    }
}

// ---- 模板 → Printer builder 转换 ----

fn build_printer<D: Driver>(driver: D, template: &Template, data: &Value) -> Result<Printer<D>, PrinterError> {
    let handlebars = Handlebars::new();
    let mut printer = Printer::new(driver, Protocol::default(), None);
    printer.init()?;

    for element in &template.elements {
        render_element(&mut printer, element, data, &handlebars)?;
    }

    Ok(printer)
}

fn render_element<D: Driver>(printer: &mut Printer<D>, element: &Element, data: &Value, handlebars: &Handlebars) -> Result<(), PrinterError> {
    match element {
        Element::Text { value, align, bold, size } => {
            let text = render_value(handlebars, value, data)?;
            printer.bold(*bold)?;
            if matches!(size, TextSize::Double) {
                printer.size(2, 2)?;
            }
            printer.justify(justify_mode(*align))?;
            printer.writeln(&text)?;
            if matches!(size, TextSize::Double) {
                printer.size(1, 1)?;
            }
        }
        Element::Row { left, right, bold } => {
            let l = render_value(handlebars, left, data)?;
            let r = render_value(handlebars, right, data)?;
            printer.bold(*bold)?;
            printer.writeln(&format!("{l}  {r}"))?;
            printer.bold(false)?;
        }
        Element::Columns { columns } => {
            let mut items = Vec::new();
            for col in columns {
                items.push(render_value(handlebars, &col.value, data)?);
            }
            printer.writeln(&items.join("  "))?;
        }
        Element::Divider { ch } => {
            let token = ch.chars().next().unwrap_or('-');
            printer.writeln(&token.to_string().repeat(20))?;
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
                        render_element(printer, child, item, handlebars)?;
                    }
                }
            }
        }
        Element::Raw { hex } => {
            let bytes = hex_decode(hex);
            printer.custom(&bytes)?;
        }
    }
    Ok(())
}

fn render_value(handlebars: &Handlebars, tmpl: &str, data: &Value) -> Result<String, PrinterError> {
    handlebars.render_template(tmpl, data).map_err(|e| PrinterError::Render(e.to_string()))
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
    match align { Align::Left => JustifyMode::LEFT, Align::Center => JustifyMode::CENTER, Align::Right => JustifyMode::RIGHT }
}

fn hex_decode(hex_str: &str) -> Vec<u8> {
    hex::decode(hex_str.chars().filter(|c| !c.is_whitespace()).collect::<String>()).unwrap_or_default()
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
        }).to_string();
        let data = json!({
            "store": {"name": "测试餐厅"},
            "order": {"total": "¥88.00"},
            "items": [{"name": "牛肉饭", "amount": "¥58.00"}, {"name": "柠檬茶", "amount": "¥30.00"}]
        }).to_string();

        let result = render_receipt_inner(&template, &data).unwrap();
        let hex = result["bytes"].as_str().unwrap();
        assert!(!hex.is_empty());
        assert_eq!(result["length"].as_i64().unwrap_or(0) as usize, hex::decode(hex).unwrap().len());
    }
}
