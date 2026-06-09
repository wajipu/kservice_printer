import 'dart:async';
import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated.dart';

import 'src/rust/api/printer.dart' as rust_printer;
import 'src/rust/api/printer.dart' show PrinterConnection;
import 'src/rust/frb_generated.dart';

export 'src/rust/api/printer.dart' show PrinterConnection;

Future<void>? _rustInitFuture;

Future<void> initKservicePrinter() {
  return _rustInitFuture ??= RustLib.init(
    externalLibrary: defaultTargetPlatform == TargetPlatform.macOS
        ? ExternalLibrary.process(iKnowHowToUseIt: true)
        : null,
  );
}

/// 打印任务类型。
enum PrintJobType {
  /// 收银/顾客订单小票。
  receipt,

  /// 后厨制作单。
  kitchen,

  /// 商品/菜品标签。
  label,

  /// 预结账单。
  preCheckout,

  /// 退款/退菜单。
  refund,

  /// 外卖/配送单。
  delivery,

  /// 自定义模板。
  custom,
}

extension PrintJobTypeInfo on PrintJobType {
  String get code => switch (this) {
    PrintJobType.receipt => 'receipt',
    PrintJobType.kitchen => 'kitchen',
    PrintJobType.label => 'label',
    PrintJobType.preCheckout => 'pre_checkout',
    PrintJobType.refund => 'refund',
    PrintJobType.delivery => 'delivery',
    PrintJobType.custom => 'custom',
  };

  String get displayName => switch (this) {
    PrintJobType.receipt => '订单小票',
    PrintJobType.kitchen => '后厨打印',
    PrintJobType.label => '标签打印',
    PrintJobType.preCheckout => '预结账单',
    PrintJobType.refund => '退款/退菜单',
    PrintJobType.delivery => '外卖/配送单',
    PrintJobType.custom => '自定义打印',
  };
}

/// 小票纸张规格。
///
/// ESC/POS 常用小票机通常按字符列宽控制版式：
/// - 58mm 纸约 32 列
/// - 80mm 纸约 48 列
enum ReceiptPaperSize { mm58, mm80 }

extension ReceiptPaperSizeInfo on ReceiptPaperSize {
  int get width => switch (this) {
    ReceiptPaperSize.mm58 => 32,
    ReceiptPaperSize.mm80 => 48,
  };

  String get displayName => switch (this) {
    ReceiptPaperSize.mm58 => '58mm 小票',
    ReceiptPaperSize.mm80 => '80mm 小票',
  };
}

/// 小票内容输出模式。
enum ReceiptPrintMode {
  /// 直接使用 ESC/POS 文本指令，速度快、字节少。
  text,

  /// 先将整张小票渲染成临时图片再打印，适合打印机字体不支持的语言。
  image,
}

extension ReceiptPrintModeInfo on ReceiptPrintMode {
  String get encoding => switch (this) {
    ReceiptPrintMode.text => 'utf8',
    ReceiptPrintMode.image => 'image',
  };

  String get displayName => switch (this) {
    ReceiptPrintMode.text => '文本打印',
    ReceiptPrintMode.image => '图片打印',
  };
}

/// 可供 UI 展示的内置模板选项。
class ReceiptTemplateOption {
  const ReceiptTemplateOption({
    required this.type,
    required this.paperSize,
    this.mode = ReceiptPrintMode.text,
  });

  final PrintJobType type;
  final ReceiptPaperSize paperSize;
  final ReceiptPrintMode mode;

  String get code => '${type.code}_${paperSize.name}_${mode.name}';

  String get displayName =>
      '${type.displayName} · ${paperSize.displayName} · ${mode.displayName}';

  ReceiptTemplate buildTemplate() =>
      defaultTemplateForPrintJobType(type, paperSize: paperSize, mode: mode);
}

/// 内置模板选择项，适合直接绑定到下拉框/设置页。
const builtInReceiptTemplateOptions = <ReceiptTemplateOption>[
  ReceiptTemplateOption(
    type: PrintJobType.receipt,
    paperSize: ReceiptPaperSize.mm58,
  ),
  ReceiptTemplateOption(
    type: PrintJobType.receipt,
    paperSize: ReceiptPaperSize.mm80,
  ),
  ReceiptTemplateOption(
    type: PrintJobType.receipt,
    paperSize: ReceiptPaperSize.mm58,
    mode: ReceiptPrintMode.image,
  ),
  ReceiptTemplateOption(
    type: PrintJobType.receipt,
    paperSize: ReceiptPaperSize.mm80,
    mode: ReceiptPrintMode.image,
  ),
  ReceiptTemplateOption(
    type: PrintJobType.kitchen,
    paperSize: ReceiptPaperSize.mm58,
  ),
  ReceiptTemplateOption(
    type: PrintJobType.kitchen,
    paperSize: ReceiptPaperSize.mm80,
  ),
  ReceiptTemplateOption(
    type: PrintJobType.preCheckout,
    paperSize: ReceiptPaperSize.mm58,
  ),
  ReceiptTemplateOption(
    type: PrintJobType.preCheckout,
    paperSize: ReceiptPaperSize.mm80,
  ),
  ReceiptTemplateOption(
    type: PrintJobType.refund,
    paperSize: ReceiptPaperSize.mm58,
  ),
  ReceiptTemplateOption(
    type: PrintJobType.refund,
    paperSize: ReceiptPaperSize.mm80,
  ),
  ReceiptTemplateOption(
    type: PrintJobType.delivery,
    paperSize: ReceiptPaperSize.mm58,
  ),
  ReceiptTemplateOption(
    type: PrintJobType.delivery,
    paperSize: ReceiptPaperSize.mm80,
  ),
  ReceiptTemplateOption(
    type: PrintJobType.label,
    paperSize: ReceiptPaperSize.mm58,
  ),
];

/// 打印模板。
class ReceiptTemplate {
  const ReceiptTemplate({
    this.width = 48,
    this.encoding = 'utf8',
    this.fontFamily,
    this.fontSize,
    required this.elements,
  });

  final int width;
  final String encoding;
  final String? fontFamily;
  final double? fontSize;
  final List<Map<String, Object?>> elements;

  Map<String, Object?> toJson() => {
    'width': width,
    'encoding': encoding,
    if (fontFamily != null) 'fontFamily': fontFamily,
    if (fontSize != null) 'fontSize': fontSize,
    'elements': elements,
  };
}

/// 一次完整打印任务。
class PrintJob {
  const PrintJob({
    required this.connection,
    required this.template,
    required this.data,
    this.type = PrintJobType.receipt,
  });

  final PrinterConnection connection;
  final PrintJobType type;
  final ReceiptTemplate template;
  final Map<String, Object?> data;
}

/// 打印执行结果。
class PrintResult {
  const PrintResult({
    required this.ok,
    this.printed = false,
    this.bytes = 0,
    this.error,
  });

  final bool ok;
  final bool printed;
  final int bytes;
  final String? error;

  factory PrintResult.fromJson(Map<String, dynamic> json) {
    final result = json['result'];
    return PrintResult(
      ok: json['ok'] == true,
      printed: result is Map && result['printed'] == true,
      bytes: result is Map && result['bytes'] is num
          ? (result['bytes'] as num).toInt()
          : 0,
      error: json['error']?.toString(),
    );
  }
}

/// 模板渲染结果。
class RenderResult {
  const RenderResult({
    required this.ok,
    this.hex = '',
    this.length = 0,
    this.error,
  });

  final bool ok;
  final String hex;
  final int length;
  final String? error;

  factory RenderResult.fromJson(Map<String, dynamic> json) {
    final result = json['result'];
    return RenderResult(
      ok: json['ok'] == true,
      hex: result is Map ? result['bytes']?.toString() ?? '' : '',
      length: result is Map && result['length'] is num
          ? (result['length'] as num).toInt()
          : 0,
      error: json['error']?.toString(),
    );
  }
}

/// USB 打印机/设备信息。
class UsbPrinterInfo {
  const UsbPrinterInfo({
    required this.vendorId,
    required this.productId,
    required this.vendorIdHex,
    required this.productIdHex,
    this.manufacturer,
    this.product,
    this.serial,
    this.isPrinter = false,
  });

  final int vendorId;
  final int productId;
  final String vendorIdHex;
  final String productIdHex;
  final String? manufacturer;
  final String? product;
  final String? serial;
  final bool isPrinter;

  String get displayName {
    final name = [
      if (manufacturer != null && manufacturer!.isNotEmpty) manufacturer,
      if (product != null && product!.isNotEmpty) product,
    ].join(' ');
    final label = name.isEmpty ? 'USB Device' : name;
    return '$label · $vendorIdHex/$productIdHex';
  }

  PrinterConnection get connection =>
      PrinterConnection.usb(vendorId: vendorId, productId: productId);

  factory UsbPrinterInfo.fromJson(Map<String, dynamic> json) {
    return UsbPrinterInfo(
      vendorId: (json['vendorId'] as num).toInt(),
      productId: (json['productId'] as num).toInt(),
      vendorIdHex: json['vendorIdHex']?.toString() ?? '',
      productIdHex: json['productIdHex']?.toString() ?? '',
      manufacturer: json['manufacturer']?.toString(),
      product: json['product']?.toString(),
      serial: json['serial']?.toString(),
      isPrinter: json['isPrinter'] == true,
    );
  }
}

/// 扫描本机可见 USB 设备，并优先返回 USB printer class 设备。
Future<List<UsbPrinterInfo>> listUsbPrinters() async {
  await initKservicePrinter();
  final response = await rust_printer.listUsbPrinters();
  final json = jsonDecode(response) as Map<String, dynamic>;
  if (json['ok'] != true) {
    throw StateError(json['error']?.toString() ?? 'USB 扫描失败');
  }
  final result = json['result'];
  final printers = result is Map ? result['printers'] : null;
  if (printers is! List) {
    return const [];
  }
  return [
    for (final item in printers)
      if (item is Map<String, dynamic>) UsbPrinterInfo.fromJson(item),
  ];
}

final _printQueue = _PrinterQueue();

extension PrinterConnectionQueueKey on PrinterConnection {
  /// 用于打印队列分组的稳定 key。
  ///
  /// 同一个 key 的任务会串行执行；不同 key 的任务可以并行。
  String get queueKey => when(
    network: (host, port, timeoutMs) => 'network:$host:$port',
    usb: (vendorId, productId) => 'usb:$vendorId:$productId',
    serial: (port, baudRate) => 'serial:$port:$baudRate',
  );
}

/// 当前仍有待执行任务的打印机队列数量。
int get activePrintQueueCount => _printQueue.activeQueueCount;

/// 打印一张小票。
///
/// 默认会按打印机连接自动排队。同一台打印机的任务串行执行，避免并发写入。
Future<PrintResult> printReceipt(PrintJob job, {bool queued = true}) {
  if (!queued) {
    return printReceiptNow(job);
  }
  return _printQueue.enqueue(
    job.connection.queueKey,
    () => printReceiptNow(job),
  );
}

/// 立即打印一张小票，不经过 Dart 队列。
///
/// 只有在调用方已经自行保证同一台打印机串行时才建议使用。
Future<PrintResult> printReceiptNow(PrintJob job) async {
  await initKservicePrinter();
  final response = await rust_printer.printReceipt(
    connection: job.connection,
    templateJson: jsonEncode(job.template.toJson()),
    dataJson: jsonEncode(job.data),
  );
  return PrintResult.fromJson(jsonDecode(response) as Map<String, dynamic>);
}

/// 将“生成打印任务 + 打印”作为一个整体放进队列。
///
/// 图片小票建议使用这个入口，把 Flutter 生成 PNG/base64 的逻辑放在 [buildJob]
/// 里面，避免高并发时同时生成大量图片。
Future<PrintResult> enqueuePrintReceipt({
  required PrinterConnection connection,
  required FutureOr<PrintJob> Function() buildJob,
}) {
  return _printQueue.enqueue(connection.queueKey, () async {
    final job = await buildJob();
    return printReceiptNow(job);
  });
}

/// 只渲染小票，不连接打印机。
Future<RenderResult> renderReceipt(PrintJob job) async {
  await initKservicePrinter();
  final response = await rust_printer.renderReceipt(
    templateJson: jsonEncode(job.template.toJson()),
    dataJson: jsonEncode(job.data),
  );
  return RenderResult.fromJson(jsonDecode(response) as Map<String, dynamic>);
}

/// 按任务类型打印。兼容所有模板类型，底层仍使用 ESC/POS 模板渲染。
Future<PrintResult> printJob(PrintJob job, {bool queued = true}) =>
    printReceipt(job, queued: queued);

/// 按任务类型只渲染，不连接打印机。
Future<RenderResult> renderJob(PrintJob job) => renderReceipt(job);

class _PrinterQueue {
  final _tails = <String, Future<void>>{};

  int get activeQueueCount => _tails.length;

  Future<T> enqueue<T>(String key, FutureOr<T> Function() task) {
    final completer = Completer<T>();
    final previous = _tails[key] ?? Future<void>.value();

    final tail = previous.catchError((_) {}).then((_) async {
      try {
        completer.complete(await task());
      } catch (error, stackTrace) {
        completer.completeError(error, stackTrace);
      }
    });

    late Future<void> queuedTail;
    queuedTail = tail.whenComplete(() {
      if (identical(_tails[key], queuedTail)) {
        _tails.remove(key);
      }
    });
    _tails[key] = queuedTail;

    return completer.future;
  }
}

/// 根据打印类型返回默认模板。
ReceiptTemplate defaultTemplateForPrintJobType(
  PrintJobType type, {
  int? width,
  ReceiptPaperSize? paperSize,
  ReceiptPrintMode mode = ReceiptPrintMode.text,
  String? fontFamily,
  double? fontSize,
}) {
  final templateWidth = width ?? paperSize?.width;
  final template = switch (type) {
    PrintJobType.kitchen => defaultKitchenTicketTemplate(
      width: templateWidth ?? ReceiptPaperSize.mm80.width,
    ),
    PrintJobType.label => defaultLabelTemplate(
      width: templateWidth ?? ReceiptPaperSize.mm58.width,
    ),
    PrintJobType.receipt ||
    PrintJobType.preCheckout ||
    PrintJobType.refund ||
    PrintJobType.delivery ||
    PrintJobType.custom => defaultOrderReceiptTemplate(
      width: templateWidth ?? ReceiptPaperSize.mm80.width,
    ),
  };
  return mode == ReceiptPrintMode.image
      ? template.asImageTemplate(fontFamily: fontFamily, fontSize: fontSize)
      : template;
}

/// SaaS/POS 默认订单小票模板。
ReceiptTemplate defaultOrderReceiptTemplate({int width = 48}) {
  final itemNameWidth = width <= ReceiptPaperSize.mm58.width ? 16 : 24;
  final itemQtyWidth = width <= ReceiptPaperSize.mm58.width ? 6 : 8;
  final itemAmountWidth = width - itemNameWidth - itemQtyWidth;

  return ReceiptTemplate(
    width: width,
    elements: [
      {
        'type': 'text',
        'value': '{{store.name}}',
        'align': 'center',
        'bold': true,
        'size': 'double',
      },
      {
        'type': 'text',
        'value': '订单号：{{order.no}}',
        'align': 'center',
        'bold': true,
      },
      {'type': 'divider'},
      {'type': 'row', 'left': '桌号', 'right': '{{order.table}}'},
      {'type': 'row', 'left': '时间', 'right': '{{order.time}}'},
      {'type': 'divider'},
      {
        'type': 'columns',
        'columns': [
          {'value': '商品', 'width': itemNameWidth},
          {'value': '数量', 'width': itemQtyWidth, 'align': 'right'},
          {'value': '金额', 'width': itemAmountWidth, 'align': 'right'},
        ],
      },
      {
        'type': 'repeat',
        'path': 'items',
        'elements': [
          {
            'type': 'columns',
            'columns': [
              {'value': '{{name}}', 'width': itemNameWidth},
              {'value': '{{qty}}', 'width': itemQtyWidth, 'align': 'right'},
              {
                'value': '{{amount}}',
                'width': itemAmountWidth,
                'align': 'right',
              },
            ],
          },
          {'type': 'text', 'value': '{{remark}}'},
        ],
      },
      {'type': 'divider'},
      {'type': 'row', 'left': '合计', 'right': '{{order.total}}', 'bold': true},
      {'type': 'feed', 'lines': 3},
      {'type': 'cut'},
    ],
  );
}

/// 图片化订单小票模板。
///
/// 适合维吾尔语、阿拉伯语等 ESC/POS 文本编码不支持或需要复杂连写的订单。
ReceiptTemplate defaultOrderReceiptImageTemplate({
  int width = 48,
  String? fontFamily,
  double? fontSize,
}) {
  return defaultOrderReceiptTemplate(
    width: width,
  ).asImageTemplate(fontFamily: fontFamily, fontSize: fontSize);
}

extension ReceiptTemplateMode on ReceiptTemplate {
  /// 将现有模板改成图片输出模式。
  ///
  /// Rust 渲染层会创建临时 PNG、发送 ESC/POS 图片指令，然后自动删除临时图片。
  ReceiptTemplate asImageTemplate({String? fontFamily, double? fontSize}) {
    return ReceiptTemplate(
      width: width,
      encoding: ReceiptPrintMode.image.encoding,
      fontFamily: fontFamily ?? this.fontFamily,
      fontSize: fontSize ?? this.fontSize,
      elements: elements,
    );
  }
}

/// 后厨制作单模板。
ReceiptTemplate defaultKitchenTicketTemplate({int width = 48}) {
  final nameWidth = width > 16 ? width - 16 : width;
  final qtyWidth = width > 16 ? 16 : 0;

  return ReceiptTemplate(
    width: width,
    elements: [
      {
        'type': 'text',
        'value': '后厨单',
        'align': 'center',
        'bold': true,
        'size': 'double',
      },
      {'type': 'row', 'left': '订单号', 'right': '{{order.no}}', 'bold': true},
      {'type': 'row', 'left': '桌号/取餐号', 'right': '{{order.table}}'},
      {'type': 'row', 'left': '下单时间', 'right': '{{order.time}}'},
      {'type': 'row', 'left': '类型', 'right': '{{order.mealType}}'},
      {'type': 'divider', 'ch': '='},
      {
        'type': 'columns',
        'columns': [
          {'value': '品项', 'width': nameWidth},
          if (qtyWidth > 0)
            {'value': '数量', 'width': qtyWidth, 'align': 'right'},
        ],
      },
      {
        'type': 'repeat',
        'path': 'items',
        'elements': [
          {
            'type': 'columns',
            'columns': [
              {'value': '{{name}}', 'width': nameWidth, 'bold': true},
              if (qtyWidth > 0)
                {'value': 'x{{qty}}', 'width': qtyWidth, 'align': 'right'},
            ],
          },
          {'type': 'text', 'value': '{{spec}}'},
          {'type': 'text', 'value': '{{remark}}', 'bold': true},
          {'type': 'divider'},
        ],
      },
      {'type': 'text', 'value': '{{order.remark}}', 'bold': true},
      {'type': 'feed', 'lines': 3},
      {'type': 'cut'},
    ],
  );
}

/// 标签打印模板，适合 32 列标签纸。
ReceiptTemplate defaultLabelTemplate({int width = 32}) {
  return ReceiptTemplate(
    width: width,
    elements: [
      {
        'type': 'text',
        'value': '{{item.name}}',
        'align': 'center',
        'bold': true,
        'size': 'double',
      },
      {'type': 'divider'},
      {'type': 'row', 'left': '规格', 'right': '{{item.spec}}'},
      {'type': 'row', 'left': 'SKU', 'right': '{{item.sku}}'},
      {'type': 'row', 'left': '数量', 'right': '{{item.qty}}'},
      {'type': 'row', 'left': '价格', 'right': '{{item.price}}'},
      {'type': 'text', 'value': '{{label.remark}}', 'align': 'center'},
      {'type': 'feed', 'lines': 2},
      {'type': 'cut'},
    ],
  );
}
