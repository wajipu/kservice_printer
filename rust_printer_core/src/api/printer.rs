use crate::engine;

/// 打印机连接方式。
pub enum PrinterConnection {
    /// 网络打印机（TCP/IP 直连）。
    /// host: IP 或主机名
    /// port: 端口号，常见 9100
    /// timeout_ms: 连接超时毫秒数
    Network {
        host: String,
        port: u16,
        timeout_ms: u64,
    },
    /// USB 打印机。
    /// vendor_id: USB 厂商 ID（十六进制，如 0x0525）
    /// product_id: USB 产品 ID（十六进制，如 0xa700）
    Usb { vendor_id: u16, product_id: u16 },
    /// 串口打印机。
    /// port: 串口路径（如 /dev/ttyUSB0、COM3）
    /// baud_rate: 波特率（常见 9600、115200）
    Serial { port: String, baud_rate: u32 },
}

pub fn print_receipt(
    connection: PrinterConnection,
    template_json: String,
    data_json: String,
) -> String {
    engine::print_receipt(&connection, &template_json, &data_json)
}

pub fn render_receipt(template_json: String, data_json: String) -> String {
    engine::render_receipt(&template_json, &data_json)
}

pub fn list_usb_printers() -> String {
    engine::list_usb_printers()
}

#[flutter_rust_bridge::frb(init)]
pub fn init_app() {
    flutter_rust_bridge::setup_default_user_utils();
}
