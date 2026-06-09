# kservice_printer

[![CI](https://github.com/your-org/kservice_printer/actions/workflows/ci.yml/badge.svg)](https://github.com/your-org/kservice_printer/actions/workflows/ci.yml)

Flutter + Rust 跨平台打印插件，面向餐饮 SaaS/POS 订单小票打印。

## 能力

- Rust `escpos` crate 渲染 ESC/POS 指令
- `flutter_rust_bridge` 直调 Rust（无 C Bridge 中间层）
- 三种连接方式：**Network**（TCP/IP）、**USB**、**Serial**（串口）
- JSON 模板 + Handlebars 动态数据（`{{store.name}}` 语法）
- 支持文本/左右行/列/分隔线/循环明细/走纸/切纸/原始 hex
- `renderReceipt` 调试模式返回十六进制字节，不下发打印机

## 架构

```text
Flutter UI → Dart API → flutter_rust_bridge 生成层 → Rust 引擎
                                                           ↓
                                              NetworkDriver / NativeUsbDriver / SerialPortDriver
                                                           ↓
                                                     real_printer / VecDriver(调试)
```

- Rust 层：模板解析、Handlebars 渲染、Printer builder 生成 ESC/POS
- Dart 层：`PrinterConnection` 枚举选择连接方式，FRB 自动序列化
- 不依赖 C Bridge、dart:ffi 手写绑定、CocoaPods
- macOS 走 SPM（Swift Package Manager），Android/Linux/Windows 走 cargokit

## 支持平台

| 平台 | Network | USB | Serial | 构建方式 |
|------|---------|-----|--------|---------|
| **Android** | ✅ | ✅ (USB Host) | ⚠️ (需 OTG 转串口) | Gradle + cargokit |
| **macOS** | ✅ | ✅ (IOKit) | ✅ | SPM + cargokit |
| **Linux** | ✅ | ✅ (libusb) | ✅ | CMake + cargokit |
| **Windows** | ✅ | ✅ (WinUSB) | ✅ | CMake + cargokit |

## 使用

### 连接方式

```dart
// 网络打印机
PrinterConnection.network(host: '192.168.1.100', port: 9100, timeoutMs: 3000);

// USB 打印机
PrinterConnection.usb(vendorId: 0x0525, productId: 0xa700);

// 串口打印机
PrinterConnection.serial(port: '/dev/ttyUSB0', baudRate: 115200);
```

### 打印示例

```dart
final job = PrintJob(
  connection: const PrinterConnection.network(host: '192.168.1.100', port: 9100, timeoutMs: 3000),
  template: defaultOrderReceiptTemplate(),
  data: {
    'store': {'name': 'KService 餐厅'},
    'order': {'no': 'A001', 'table': 'A08', 'time': '12:30', 'total': '¥128.00'},
    'items': [
      {'name': '招牌牛肉饭', 'qty': '2', 'amount': '¥58.00', 'remark': '少辣'},
    ],
  },
);

final result = await printReceipt(job);
```

### 调试（不连接打印机）

```dart
final result = await renderReceipt(job);
print('ESC/POS bytes: ${result.hex}');
```

## 模板元素

```json
{"type":"text","value":"{{store.name}}","align":"center","bold":true,"size":"double"}
{"type":"row","left":"合计","right":"{{order.total}}","bold":true}
{"type":"columns","columns":[{"value":"{{name}}","width":24},{"value":"{{qty}}","width":8,"align":"right"}]}
{"type":"repeat","path":"items","elements":[...]}
{"type":"divider"}
{"type":"feed","lines":3}
{"type":"cut"}
{"type":"raw","hex":"1b4501"}
```

## 验证

```bash
cargo test --manifest-path rust_printer_core/Cargo.toml
flutter analyze
flutter test
```
