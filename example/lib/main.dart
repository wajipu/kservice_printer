import 'package:flutter/material.dart';
import 'package:kservice_printer/kservice_printer.dart';

void main() {
  runApp(const MyApp());
}

/// 插件示例应用。
///
/// 提供两个动作：
/// - 仅渲染：验证模板是否能正确生成 ESC/POS 字节，不连接打印机。
/// - 打印测试订单：连接网络打印机并发送小票指令。
class MyApp extends StatefulWidget {
  const MyApp({super.key});

  @override
  State<MyApp> createState() => _MyAppState();
}

class _MyAppState extends State<MyApp> {
  String _status = '未打印';

  /// 示例订单打印任务。
  ///
  /// 使用时请将 host 改成真实打印机 IP。
  /// 也支持 USB: `PrinterConnection.usb(vendorId: 0x0525, productId: 0xa700)`
  /// 或串口: `PrinterConnection.serial(port: '/dev/ttyUSB0', baudRate: 115200)`
  PrintJob get _job => PrintJob(
    connection: const PrinterConnection.network(host: '192.168.1.100', port: 9100, timeoutMs: 3000),
    template: defaultOrderReceiptTemplate(),
    data: {
      'store': {'name': 'KService 餐厅'},
      'order': {
        'no': 'A202606090001',
        'table': 'A08',
        'time': '2026-06-09 12:30',
        'total': '¥128.00',
      },
      'items': [
        {'name': '招牌牛肉饭', 'qty': '2', 'amount': '¥58.00', 'remark': '少辣'},
        {'name': '柠檬茶', 'qty': '2', 'amount': '¥20.00', 'remark': ''},
        {'name': '小食拼盘', 'qty': '1', 'amount': '¥50.00', 'remark': ''},
      ],
    },
  );

  /// 仅渲染模板，用于调试小票格式。
  Future<void> _render() async {
    setState(() => _status = '渲染中...');
    final result = await renderReceipt(_job);
    setState(
      () => _status = result.ok
          ? '渲染成功：${result.length} bytes'
          : '渲染失败：${result.error}',
    );
  }

  /// 发送真实打印任务。
  Future<void> _print() async {
    setState(() => _status = '打印中...');
    final result = await printReceipt(_job);
    setState(
      () => _status = result.ok
          ? '打印成功：${result.bytes} bytes'
          : '打印失败：${result.error}',
    );
  }

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      home: Scaffold(
        appBar: AppBar(title: const Text('KService Printer')),
        body: Padding(
          padding: const EdgeInsets.all(16),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Text(_status),
              const SizedBox(height: 12),
              FilledButton(
                onPressed: _render,
                child: const Text('仅渲染 ESC/POS'),
              ),
              const SizedBox(height: 12),
              FilledButton(onPressed: _print, child: const Text('打印测试订单')),
            ],
          ),
        ),
      ),
    );
  }
}
