# kservice_printer

Flutter + Rust 跨平台打印插件，面向 SaaS/POS 订单小票、后厨单、标签打印。

## 能力

- Rust `escpos` crate 渲染 ESC/POS 小票指令，内置 TSPL/ZPL 标签指令渲染
- `flutter_rust_bridge` 直调 Rust（无 C Bridge 中间层）
- 三种连接方式：**Network**（TCP/IP）、**USB**、**Serial**（串口）
- WiFi/局域网 mDNS/DNS-SD 自动发现网络打印服务（Android 走原生 NsdManager）
- JSON 模板 + Handlebars 动态数据（`{{store.name}}` 语法）
- 支持文本/左右行/列/分隔线/循环明细/走纸/切纸/原始 hex/二维码/条码/图片；TSPL/ZPL 标签模式支持文本、分隔线、二维码和条码
- 内置打印类型：订单小票、后厨打印、标签打印、预结账单、退款/退菜单、外卖/配送单、自定义打印
- 内置 58mm/80mm 小票模板和 58mm TSPL/ZPL 标签模板选择，适合 POS 设置页或打印前选择
- 支持 ESC/POS 状态查询、设备身份/序列号读取和 Dart 侧并发压测
- `renderReceipt` 调试模式返回十六进制字节，不下发打印机

## 架构

```text
Flutter UI → Dart API → flutter_rust_bridge 生成层 → Rust 引擎
                                                            ↓
                                               TcpDriver / USB driver / SerialPortDriver
                                                            ↓
                                                      real_printer / VecDriver(调试)
```

### Rust 模块结构

```text
rust_printer_core/src/
├── lib.rs              # crate 根文件，注册所有模块
├── engine.rs           # 核心编排：print_receipt、render_receipt、build_printer、render_element
├── error.rs            # PrinterError 错误类型定义
├── template.rs         # 模板解析(parse_template)、类型定义(Template/Element/Align 等)
├── discovery.rs        # 网络发现(mDNS/USB list)
├── util.rs             # 通用工具(into_response/justify_mode/has_cut_element)
├── api/
│   └── printer.rs      # PrinterConnection 枚举(FRB 公开类型)
├── protocol/
│   ├── tspl.rs         # TSPL 标签指令渲染
│   └── zpl.rs          # ZPL 标签指令渲染
└── render/
    ├── mod.rs
    ├── encoding.rs      # 文本编码(GBK/UTF-8 互转)
    ├── image.rs         # 图片渲染(TempImageFile/render_lines_to_image/BitImageOption)
    ├── layout.rs        # 文字布局(format_row/format_columns/fit_text/display_width)
    └── value.rs         # Handlebars 渲染(render_value/value_ref/hex_decode)
```

- Rust 层：模板解析、Handlebars 渲染，按模板编码生成 ESC/POS 或 TSPL 指令
- Dart 层：`PrinterConnection` 枚举选择连接方式，FRB 自动序列化；Android 网络发现和授权 USB I/O 走 MethodChannel 调原生 API
- 不依赖 C Bridge 或 dart:ffi 手写绑定
- macOS 走 CocoaPods script phase + cargokit，Android/Linux/Windows 走 cargokit

## 支持平台

| 平台 | Network | USB | Serial | 构建方式 |
|------|---------|-----|--------|---------|
| **Android** | ✅ (含原生 mDNS) | ✅* (`UsbManager` bulk I/O) | ⚠️ (需 OTG 转串口) | Gradle + cargokit |
| **macOS** | ✅ | ✅ (IOKit) | ✅ | CocoaPods + cargokit |
| **Linux** | ✅ | ✅ (libusb) | ✅ | CMake + cargokit |
| **Windows** | ✅ | ✅ (`usbprint.sys`) | ✅ | CMake + cargokit |

Android USB 扫描、授权和数据传输均通过原生 `UsbManager` 执行。授权后插件会选择带 bulk OUT 的打印接口，分块写入 Rust 渲染出的 ESC/POS、TSPL 或 ZPL 指令；带 bulk IN 的双向设备还可以查询状态和序列号。插件 Manifest 会自动合并 `android.hardware.usb.host`。

`*` Android USB 原生通道和四 ABI APK 构建已通过自动化验证；当前仓库尚未建立真实打印机型号矩阵，正式接入前仍需对目标机型验证出纸、切刀、钱箱、状态回读和序列号。只有 bulk OUT 的设备可以打印，身份信息会回退到 Android USB 描述符，但实时状态查询需要 bulk IN。

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

### 自动发现网络打印机

```dart
final result = await discoverNetworkPrinters(
  timeout: const Duration(seconds: 3),
);

for (final printer in result.printers) {
  print('${printer.displayName} ${printer.serviceType}');
}

final rawTcpPrinter = result.printers.firstWhere(
  (printer) => printer.supportsRawTcp,
);

final connection = rawTcpPrinter.connection(
  timeout: const Duration(seconds: 3),
);
```

默认会通过 mDNS/DNS-SD 扫描 `_pdl-datastream._tcp.local.`、`_printer._tcp.local.`、`_ipp._tcp.local.`、`_ipps._tcp.local.`。扫描有超时控制，Android 使用原生 `NsdManager` 异步回调和 `MulticastLock`，macOS/Linux/Windows 通过 FRB 后台任务执行，不会阻塞 Flutter UI isolate。`_ipp/_ipps` 和非 9100 端口的 `_printer._tcp` 设备通常不是 ESC/POS raw TCP 打印机，调用方应优先选择 `supportsRawTcp == true` 的设备用于 `printReceipt`。

也可以指定服务类型：

```dart
final result = await discoverNetworkPrinters(
  timeout: const Duration(seconds: 5),
  serviceTypes: ['_pdl-datastream._tcp.local.'],
);
```

Android 插件 Manifest 会合并 `INTERNET`、`ACCESS_NETWORK_STATE`、`ACCESS_WIFI_STATE`、`CHANGE_WIFI_MULTICAST_STATE`、`NEARBY_WIFI_DEVICES` 和 `android.hardware.usb.host`。如果业务 App targetSdk 为 33+ 且系统要求 Nearby Wi-Fi 权限，请在调用扫描前完成运行时授权，例如用 `permission_handler` 请求 Nearby Wi-Fi Devices；插件会在权限缺失时返回明确错误。iOS 当前不考虑支持。

### 自动发现 USB 打印机

```dart
final printers = await listUsbPrinters();

for (final printer in printers) {
  print('${printer.displayName} permission=${printer.hasPermission}');
}

final allUsbDevices = await listUsbPrinters(includeNonPrinters: true);
```

`listUsbPrinters()` 默认只返回 `isPrinter == true` 的设备，避免把 Hub、手机、声卡、鼠标键盘接收器等普通 USB 设备展示成打印机。现场排查某些厂商私有 USB class 打印机时，可以临时使用 `includeNonPrinters: true` 查看完整 USB 设备清单。

Android 打印前需要系统授权，并应使用扫描结果生成连接；这样会保留 `deviceName`，两台相同 VID/PID 的设备也不会被误选：

```dart
final printer = (await listUsbPrinters()).first;
if (printer.hasPermission != true) {
  final granted = await requestUsbPrinterPermission(printer);
  if (!granted) throw StateError('用户未授权 USB 打印机');
}

final job = PrintJob(
  connection: printer.connection,
  template: defaultOrderReceiptTemplate(),
  data: orderData,
);
await printReceipt(job);
```

### 平台权限说明

- macOS：如果宿主 App 开启 App Sandbox，USB 扫描/打印需要 `com.apple.security.device.usb`，网络打印和 mDNS 发现需要 `com.apple.security.network.client`；mDNS 接收响应时建议同时保留 `com.apple.security.network.server`。示例 App 已配置。
- Android：网络发现的 manifest 权限已由插件合并；Android 13+ 的 `NEARBY_WIFI_DEVICES` 是运行时权限，需要业务 App 在扫描前请求。`listUsbPrinters()` 走原生 `UsbManager` 扫描，默认过滤非打印机设备；返回的 `UsbPrinterInfo.hasPermission` 表示系统是否已授权该 USB 设备；`requestUsbPrinterPermission(printer)` 可请求授权。打印、钱箱和双向查询均复用授权后的原生 USB bulk 通道。
- Linux：没有 App manifest 权限；网络发现/网络打印通常不需要应用级权限，但防火墙可能影响 mDNS UDP 5353。普通用户访问 USB 设备通常需要 udev 规则或加入对应设备组，否则 libusb 可能只能用 `sudo` 访问。
- Windows：普通 Flutter Win32 App 没有类似 macOS 的网络 entitlement；网络发现/网络打印通常不需要 manifest 能力，但防火墙可能影响 mDNS UDP 5353。USB 打印通过 Windows 标准 `usbprint.sys` 设备接口写入原始 ESC/POS/TSPL 字节，适合系统识别为 USB printer class 的小票机/标签机。

网络模式如果报 `Operation not permitted (os error 1)`，通常是宿主应用被系统策略拒绝创建 TCP socket；macOS 优先检查当前运行的 `.app` 是否实际签入了 `com.apple.security.network.client`，Android 优先检查最终合并后的 Manifest 是否仍包含 `android.permission.INTERNET`。

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

`defaultOrderReceiptTemplate()` 只是内置默认模板，不是固定格式。`PrintJob.template` 可以换成任意自定义 `ReceiptTemplate`，`PrintJob.data` 只负责提供模板变量。

例如只打印单号和合计：

```dart
final job = PrintJob(
  connection: const PrinterConnection.network(host: '192.168.1.100', port: 9100, timeoutMs: 3000),
  template: const ReceiptTemplate(
    width: 32,
    elements: [
      {'type': 'text', 'value': '{{store.name}}', 'align': 'center', 'bold': true},
      {'type': 'divider'},
      {'type': 'row', 'left': '单号', 'right': '{{order.no}}'},
      {'type': 'row', 'left': '合计', 'right': '{{order.total}}', 'bold': true},
      {'type': 'feed', 'lines': 3},
      {'type': 'cut'},
    ],
  ),
  data: {
    'store': {'name': 'KService 餐厅'},
    'order': {'no': 'A001', 'total': '¥128.00'},
  },
);

await printReceipt(job);
```

也可以把模板保存成 JSON，由服务端或本地配置下发；只要最终传入 `ReceiptTemplate(width, encoding, elements)` 即可。
文本模式默认使用 `gbk`，适合多数中文 ESC/POS 小票机；如果打印机确认支持 UTF-8，可以把模板的 `encoding` 设置为 `utf8`。

### 打印队列

`printReceipt(job)` 默认会进入 Dart 侧打印队列。同一台打印机按连接信息串行执行，不同打印机可以并行执行：

```dart
await printReceipt(job); // 默认 queued: true
```

如果调用方已经自行保证串行，可以绕过队列：

```dart
await printReceiptNow(job);
// 或
await printReceipt(job, queued: false);
```

图片小票建议把“生成图片 + 打印”整体放进队列，避免高并发时同时生成大量 PNG/base64：

```dart
final connection = const PrinterConnection.usb(vendorId: 0x0483, productId: 0x070B);

await enqueuePrintReceipt(
  connection: connection,
  buildJob: () async {
    final pngBytes = await buildReceiptPng(order); // Flutter Canvas/RepaintBoundary
    final imageBase64 = base64Encode(pngBytes);

    return PrintJob(
      connection: connection,
      template: const ReceiptTemplate(
        width: 32,
        elements: [
          {
            'type': 'image',
            'base64': '{{receipt.imageBase64}}',
            'max_width': 384,
            'align': 'center',
          },
          {'type': 'feed', 'lines': 3},
          {'type': 'cut'},
        ],
      ),
      data: {
        'receipt': {'imageBase64': imageBase64},
      },
    );
  },
);
```

可以用 `activePrintQueueCount` 观察当前还有多少个打印机队列未完成。队列只保证单进程内的 Dart 调用串行；如果有多个 App 实例或多个进程同时写同一台打印机，仍需要在业务层做互斥。

### 状态、序列号和压测

状态查询和身份读取默认也进入同一台打印机的队列，避免 ESC/POS 查询指令和正在打印的票据字节交叉：

```dart
final status = await queryPrinterStatus(
  job.connection,
  timeout: const Duration(seconds: 2),
);

final identity = await getPrinterIdentity(job.connection);
final serial = await getPrinterSerialNumber(job.connection);

final stress = await runPrinterStressTest(
  job: job,
  count: 20,
  concurrency: 4,
);
```

这些查询依赖设备支持双向 ESC/POS raw 读写。常见 9100 网络小票机和部分串口/USB 设备可用；不支持的设备会返回 `supported == false` 或错误信息，不会和普通打印结果混在一起。

### 稳定错误码

失败结果保留原有中文 `error`，并新增固定的 `errorCode`，便于业务层做重试、提示或告警。`PrintResult`、`RenderResult`、`PrinterStatus` 和 `PrinterIdentity` 都会解析该字段。常见值包括 `usb_permission_required`、`usb_device_not_found`、`usb_device_ambiguous`、`usb_write_failed`、`usb_read_unsupported`、`connect_failed`、`query_failed` 和 `invalid_template`。

### 模板选择

```dart
// 58mm 小票，约 32 列
final receipt58 = defaultTemplateForPrintJobType(
  PrintJobType.receipt,
  paperSize: ReceiptPaperSize.mm58,
);

// 80mm 小票，约 48 列
final receipt80 = defaultTemplateForPrintJobType(
  PrintJobType.receipt,
  paperSize: ReceiptPaperSize.mm80,
);

// 打印机字体不支持维吾尔语、阿拉伯语等复杂文字时，选择图片打印
final minorityLanguageReceipt = defaultTemplateForPrintJobType(
  PrintJobType.receipt,
  paperSize: ReceiptPaperSize.mm58,
  mode: ReceiptPrintMode.image,
  fontFamily: 'Noto Sans Arabic',
  fontSize: 26,
);

// 也可以直接把内置选项绑定到下拉框/设置页
for (final option in builtInReceiptTemplateOptions) {
  print('${option.code}: ${option.displayName}');
}

final selected = builtInReceiptTemplateOptions.first;
final job = PrintJob(
  type: selected.type,
  connection: const PrinterConnection.network(host: '192.168.1.100', port: 9100, timeoutMs: 3000),
  template: selected.buildTemplate(),
  data: data,
);
```

模板 JSON 也可以直接声明纸宽：

```json
{
  "paperSize": 58,
  "elements": [
    {"type": "text", "value": "{{store.name}}", "align": "center"},
    {"type": "divider"},
    {"type": "row", "left": "合计", "right": "{{order.total}}"}
  ]
}
```

支持 `paperSize` / `paper_size`，值可以是 `58`、`80`、`"58mm"`、`"80mm"`。如果同时设置 `width`，则以 `width` 为准。

如果打印机不支持某些语言的文本编码，可以在模板 JSON 里打开图片打印：

```json
{
  "paperSize": 58,
  "encoding": "image",
  "fontFamily": "Noto Sans Arabic",
  "fontSize": 26,
  "elements": [
    {"type": "text", "value": "{{store.name}}", "align": "center"},
    {"type": "row", "left": "زاكاز", "right": "{{order.no}}"},
    {"type": "divider"},
    {"type": "row", "left": "جەمئىي", "right": "{{order.total}}"}
  ]
}
```

`encoding: "image"` 会把整张小票生成临时 PNG，再用 ESC/POS 图片指令打印；临时图片会在打印流程结束或出错时自动删除。`fontFamily` 使用系统字体名，不传时走系统默认 fallback；`fontSize` 会限制在 12 到 72 之间，避免异常字号导致图片过大。二维码、条码和 raw 指令不会进入整票图片模式，复杂票据建议单独测试。

### 图片打印模式

图片打印适合打印机字库不支持的内容，例如维吾尔语、阿拉伯语等需要复杂连写的文字。推荐优先在 Flutter 侧生成整张小票图片，再把 PNG bytes 作为 base64 传给 Rust；Rust 只负责读取图片尺寸、按比例计算高度并发送 ESC/POS 图片指令。这样字体、字号、行距和布局都由 Flutter 控制，预览和实际打印更一致。

58mm 打印机建议图片宽度使用 `384px`；80mm 打印机建议使用 `576px`。高度不需要写死，按内容自然撑开即可。

#### Flutter 生成图片流

Flutter 端可以用 `Canvas`、`PictureRecorder` 或 `RepaintBoundary` 生成 PNG。生成后把 `Uint8List` 转成 base64：

```dart
import 'dart:convert';

final imageBase64 = base64Encode(pngBytes);

final job = PrintJob(
  connection: const PrinterConnection.usb(vendorId: 0x0483, productId: 0x070B),
  template: const ReceiptTemplate(
    width: 32,
    elements: [
      {
        'type': 'image',
        'base64': '{{receipt.imageBase64}}',
        'max_width': 384,
        'align': 'center',
      },
      {'type': 'feed', 'lines': 3},
      {'type': 'cut'},
    ],
  ),
  data: {
    'receipt': {'imageBase64': imageBase64},
  },
);

await printReceipt(job);
```

`base64` 也支持 data URL：

```json
{
  "type": "image",
  "base64": "data:image/png;base64,iVBORw0KGgo...",
  "max_width": 384,
  "align": "center"
}
```

#### 打印本地图片文件

如果已经有图片文件，也可以传路径：

```json
{
  "type": "image",
  "path": "{{receipt.imagePath}}",
  "max_width": 384,
  "align": "center"
}
```

```dart
data: {
  'receipt': {'imagePath': '/path/to/receipt.png'},
}
```

#### 高度控制

`image` 节点支持 `max_width` 和 `max_height`：

```json
{
  "type": "image",
  "base64": "{{receipt.imageBase64}}",
  "max_width": 384,
  "max_height": 720,
  "align": "center"
}
```

- 不传 `max_height`：Rust 自动读取图片真实宽高，按 `max_width` 等比例计算高度。
- 传 `max_height`：按 JSON 指定高度限制图片。
- 宽高会自动补齐到 8 的倍数，避免 ESC/POS 图片指令报错。

通常不要手动写死高度。更推荐 Flutter 直接生成目标宽度的图片，例如 58mm 生成 `384px` 宽，字号和行距在 Flutter 中调整，高度由内容决定。

### 打印类型

```dart
// 后厨打印
final kitchenJob = PrintJob(
  type: PrintJobType.kitchen,
  connection: const PrinterConnection.network(host: '192.168.1.100', port: 9100, timeoutMs: 3000),
  template: defaultKitchenTicketTemplate(),
  data: {
    'order': {'no': 'K001', 'table': 'A08', 'time': '12:30', 'mealType': '堂食', 'remark': '加急'},
    'items': [
      {'name': '招牌牛肉饭', 'qty': '2', 'spec': '少辣', 'remark': '不要香菜'},
    ],
  },
);

// TSPL 标签机打印
final labelJob = PrintJob(
  type: PrintJobType.label,
  connection: const PrinterConnection.network(host: '192.168.1.101', port: 9100, timeoutMs: 3000),
  template: defaultTsplLabelTemplate(widthMm: 58, heightMm: 40),
  data: {
    'item': {'name': '招牌牛肉饭', 'spec': '中份', 'sku': 'BEEF-001', 'qty': '1', 'price': '¥29.00'},
    'label': {'remark': '冷藏保存'},
  },
);

await printJob(kitchenJob);
await printJob(labelJob);
```

### 调试（不连接打印机）

```dart
final result = await renderReceipt(job);
print('printer command bytes: ${result.hex}');
```

### TSPL 标签机

标签机和传统账单/小票机通常不是同一种打印语言。小票机常用 ESC/POS；TSC 兼容标签机常用 TSPL。连接方式仍然可以是 USB、Network 或 Serial，但模板需要使用 `encoding: "tspl"`，或者直接用 `defaultTsplLabelTemplate()`。

```dart
final job = PrintJob(
  type: PrintJobType.label,
  connection: const PrinterConnection.usb(vendorId: 0x0483, productId: 0x5743),
  template: defaultTsplLabelTemplate(widthMm: 58, heightMm: 40, gapMm: 2),
  data: {
    'item': {'name': '招牌牛肉饭', 'spec': '大份', 'sku': 'BEEF-001', 'qty': '1', 'price': '¥29.00'},
    'label': {'remark': '冷藏保存'},
  },
);

await printJob(job);
```

`defaultTsplLabelTemplate()` 默认会在 `CLS/PRINT` 前下发 `HOME`，让打印机根据标签间隙传感器把纸定位到下一张标签起点。换纸、首次安装或打印从半张标签开始时，先在打印机面板/厂商工具执行一次 Gap 校准；部分 TSC/Xprinter 兼容机也可直接发送 `GAPDETECT` 原始 TSPL 指令做校准。TSPL 通常拿不到“当前纸张绝对坐标”，只能查询少量状态或主动执行 `HOME`/`FORMFEED`/`GAPDETECT` 让设备重新定位。

维吾尔语、阿拉伯语等复杂文字不要用普通 TSPL `TEXT` 字库，建议使用 TSPL 图片标签模式。插件会把整张标签渲染成位图，再用 TSPL `BITMAP` 指令打印：

```dart
final job = PrintJob(
  type: PrintJobType.label,
  connection: const PrinterConnection.network(host: '192.168.1.101', port: 9100, timeoutMs: 3000),
  template: defaultTsplLabelImageTemplate(
    widthMm: 58,
    heightMm: 40,
    gapMm: 2,
    fontFamily: 'Noto Sans Arabic',
    fontSize: 24,
  ),
  data: {
    'item': {'name': 'لاڭمەن', 'spec': 'چوڭ', 'sku': 'BEEF-001', 'qty': '1', 'price': '¥29.00'},
    'label': {'remark': 'سوغۇق ساقلاڭ'},
  },
);

await printJob(job);
```

如果纸张机械位置已经准，但内容整体偏移，可以用模板参数微调：

```dart
template: defaultTsplLabelTemplate(
  widthMm: 58,
  heightMm: 40,
  gapMm: 2,
  referenceX: 0,
  referenceY: 0,
  shiftDots: 0,
),
```

## 模板元素

```json
{"type":"text","value":"{{store.name}}","align":"center","bold":true,"size":"double"}
{"type":"row","left":"合计","right":"{{order.total}}","bold":true}
{"type":"columns","columns":[{"value":"{{name}}","width":24,"bold":true},{"value":"{{qty}}","width":8,"align":"right"}]}
{"type":"repeat","path":"items","elements":[...]}
{"type":"divider"}
{"type":"feed","lines":3}
{"type":"cut"}
{"type":"raw","hex":"1b4501"}
{"type":"qrcode","value":"{{order.qr}}","size":5,"align":"center"}
{"type":"barcode","system":"ean13","value":"{{order.barcode}}","align":"center"}
{"type":"image","path":"{{receipt.imagePath}}","max_width":384,"align":"center"}
{"type":"image","base64":"{{receipt.imageBase64}}","max_width":384,"align":"center"}
```

## 验证

```bash
cargo test --manifest-path rust_printer_core/Cargo.toml
flutter analyze
flutter test
```
