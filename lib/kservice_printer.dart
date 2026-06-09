import 'dart:async';
import 'dart:convert';

import 'src/rust/api/printer.dart' as rust_printer;
import 'src/rust/api/printer.dart' show PrinterConnection;
import 'src/rust/frb_generated.dart';

export 'src/rust/api/printer.dart' show PrinterConnection;

Future<void>? _rustInitFuture;

Future<void> initKservicePrinter() {
  return _rustInitFuture ??= RustLib.init();
}

/// 小票模板。
class ReceiptTemplate {
  const ReceiptTemplate({
    this.width = 48,
    this.encoding = 'utf8',
    required this.elements,
  });

  final int width;
  final String encoding;
  final List<Map<String, Object?>> elements;

  Map<String, Object?> toJson() => {
    'width': width,
    'encoding': encoding,
    'elements': elements,
  };
}

/// 一次完整打印任务。
class PrintJob {
  const PrintJob({
    required this.connection,
    required this.template,
    required this.data,
  });

  final PrinterConnection connection;
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

/// 打印一张小票。
Future<PrintResult> printReceipt(PrintJob job) async {
  await initKservicePrinter();
  final response = await rust_printer.printReceipt(
    connection: job.connection,
    templateJson: jsonEncode(job.template.toJson()),
    dataJson: jsonEncode(job.data),
  );
  return PrintResult.fromJson(jsonDecode(response) as Map<String, dynamic>);
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

/// SaaS/POS 默认订单小票模板。
ReceiptTemplate defaultOrderReceiptTemplate({int width = 48}) {
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
          {'value': '商品', 'width': 24},
          {'value': '数量', 'width': 8, 'align': 'right'},
          {'value': '金额', 'width': 16, 'align': 'right'},
        ],
      },
      {
        'type': 'repeat',
        'path': 'items',
        'elements': [
          {
            'type': 'columns',
            'columns': [
              {'value': '{{name}}', 'width': 24},
              {'value': '{{qty}}', 'width': 8, 'align': 'right'},
              {'value': '{{amount}}', 'width': 16, 'align': 'right'},
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
