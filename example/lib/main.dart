import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:kservice_printer/kservice_printer.dart';

void main() {
  runApp(const PrinterExampleApp());
}

enum _ConnectionMode { network, usb, serial }

class PrinterExampleApp extends StatelessWidget {
  const PrinterExampleApp({super.key});

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      debugShowCheckedModeBanner: false,
      theme: ThemeData(
        colorScheme: ColorScheme.fromSeed(seedColor: const Color(0xff4f46a5)),
        inputDecorationTheme: const InputDecorationTheme(
          border: OutlineInputBorder(),
          isDense: true,
        ),
        useMaterial3: true,
      ),
      home: const PrinterDebugPage(),
    );
  }
}

class PrinterDebugPage extends StatefulWidget {
  const PrinterDebugPage({super.key});

  @override
  State<PrinterDebugPage> createState() => _PrinterDebugPageState();
}

class _PrinterDebugPageState extends State<PrinterDebugPage> {
  final _networkHostController = TextEditingController(text: '192.168.1.100');
  final _networkPortController = TextEditingController(text: '9100');
  final _networkTimeoutController = TextEditingController(text: '3000');
  final _usbVendorController = TextEditingController(text: '0x0483');
  final _usbProductController = TextEditingController(text: '0x070B');
  final _serialPortController = TextEditingController(text: '/dev/ttyUSB0');
  final _serialBaudController = TextEditingController(text: '115200');

  _ConnectionMode _connectionMode = _ConnectionMode.usb;
  ReceiptTemplateOption _selectedTemplate = builtInReceiptTemplateOptions
      .firstWhere(
        (option) =>
            option.type == PrintJobType.receipt &&
            option.paperSize == ReceiptPaperSize.mm58 &&
            option.mode == ReceiptPrintMode.text,
        orElse: () => builtInReceiptTemplateOptions.first,
      );

  bool _busy = false;
  bool _scanningUsb = false;
  bool _scanningNetwork = false;
  String _status = '等待操作';
  String _details = '请选择连接方式和模板，然后渲染或打印测试订单。';
  List<UsbPrinterInfo> _usbPrinters = const [];
  UsbPrinterInfo? _selectedUsbPrinter;
  List<NetworkPrinterInfo> _networkPrinters = const [];
  NetworkPrinterInfo? _selectedNetworkPrinter;

  @override
  void dispose() {
    _networkHostController.dispose();
    _networkPortController.dispose();
    _networkTimeoutController.dispose();
    _usbVendorController.dispose();
    _usbProductController.dispose();
    _serialPortController.dispose();
    _serialBaudController.dispose();
    super.dispose();
  }

  Map<String, Object?> get _sampleData => {
    'store': {'name': 'KService 餐厅'},
    'order': {
      'no': 'A202606090001',
      'table': 'A08',
      'time': '2026-06-09 12:30',
      'mealType': '堂食',
      'total': '¥128.00',
      'remark': '请尽快出餐',
    },
    'items': [
      {
        'name': '招牌牛肉饭',
        'qty': '2',
        'amount': '¥58.00',
        'remark': '少辣',
        'spec': '大份',
      },
      {
        'name': '柠檬茶',
        'qty': '2',
        'amount': '¥20.00',
        'remark': '',
        'spec': '少冰',
      },
      {
        'name': '小食拼盘',
        'qty': '1',
        'amount': '¥50.00',
        'remark': '不要洋葱',
        'spec': '',
      },
    ],
    'item': {
      'name': '招牌牛肉饭',
      'spec': '大份 / 少辣',
      'sku': 'BEEF-001',
      'qty': '1',
      'price': '¥29.00',
    },
    'label': {'remark': '冷藏保存'},
  };

  PrintJob get _job => PrintJob(
    type: _selectedTemplate.type,
    connection: _connection,
    template: _selectedTemplate.buildTemplate(),
    data: _sampleData,
  );

  PrinterConnection get _connection {
    return switch (_connectionMode) {
      _ConnectionMode.network => PrinterConnection.network(
        host: _networkHostController.text.trim(),
        port: _parseInt(_networkPortController.text, fallback: 9100),
        timeoutMs: _parseInt(_networkTimeoutController.text, fallback: 3000),
      ),
      _ConnectionMode.usb => PrinterConnection.usb(
        vendorId: _parseInt(_usbVendorController.text, fallback: 0x0483),
        productId: _parseInt(_usbProductController.text, fallback: 0x070B),
      ),
      _ConnectionMode.serial => PrinterConnection.serial(
        port: _serialPortController.text.trim(),
        baudRate: _parseInt(_serialBaudController.text, fallback: 115200),
      ),
    };
  }

  Future<void> _render() async {
    await _runAction(
      running: '正在渲染打印指令...',
      action: () async {
        final result = await renderReceipt(_job);
        return result.ok
            ? _ActionResult(
                '渲染成功',
                '${result.length} bytes\n${_shortHex(result.hex)}',
              )
            : _ActionResult('渲染失败', result.error ?? '未知错误');
      },
    );
  }

  Future<void> _print({required bool queued}) async {
    await _runAction(
      running: queued ? '已加入打印队列...' : '正在直接打印...',
      action: () async {
        final result = await printReceipt(_job, queued: queued);
        return result.ok
            ? _ActionResult(
                queued ? '队列打印成功' : '直接打印成功',
                '${result.bytes} bytes\n队列数：$activePrintQueueCount',
              )
            : _ActionResult('打印失败', result.error ?? '未知错误');
      },
    );
  }

  Future<void> _scanUsbPrinters() async {
    setState(() {
      _scanningUsb = true;
      _status = '正在扫描 USB 打印机...';
      _details = '只显示系统识别为 USB printer class 的设备。';
    });

    try {
      final printers = await listUsbPrinters();
      final selected = printers.isEmpty ? null : printers.first;
      if (!mounted) return;
      setState(() {
        _usbPrinters = printers;
        _selectedUsbPrinter = selected;
        if (selected != null) {
          _applyUsbPrinter(selected);
        }
        _status = printers.isEmpty
            ? '没有发现 USB 打印机'
            : '发现 ${printers.length} 台 USB 打印机';
        _details = printers.isEmpty
            ? '请确认打印机已连接并上电。如果设备是厂商私有 USB class，可用 listUsbPrinters(includeNonPrinters: true) 排查。'
            : printers.map(_usbPrinterDisplayName).join('\n');
      });
    } catch (error, stackTrace) {
      if (!mounted) return;
      setState(() {
        _status = 'USB 扫描失败';
        _details = '$error\n$stackTrace';
      });
    } finally {
      if (mounted) {
        setState(() => _scanningUsb = false);
      }
    }
  }

  void _applyUsbPrinter(UsbPrinterInfo printer) {
    _usbVendorController.text = printer.vendorIdHex;
    _usbProductController.text = printer.productIdHex;
  }

  String _usbPrinterDisplayName(UsbPrinterInfo printer) {
    final labels = <String>[
      printer.displayName,
      if (printer.isPrinter) 'Printer',
      if (printer.hasPermission != null) printer.hasPermission! ? '已授权' : '未授权',
    ];
    return labels.join(' · ');
  }

  Future<void> _requestUsbPermission() async {
    final printer = _selectedUsbPrinter;
    if (printer == null) {
      return;
    }

    setState(() {
      _busy = true;
      _status = '正在请求 USB 授权...';
      _details = _usbPrinterDisplayName(printer);
    });

    try {
      final granted = await requestUsbPrinterPermission(printer);
      final printers = await listUsbPrinters();
      final selected = _selectUsbPrinter(printers, printer);
      if (!mounted) return;
      setState(() {
        _usbPrinters = printers;
        _selectedUsbPrinter = selected;
        if (selected != null) {
          _applyUsbPrinter(selected);
        }
        _status = granted ? 'USB 授权成功' : 'USB 授权被拒绝';
        _details = printers.isEmpty
            ? '请确认打印机已连接并上电。'
            : printers.map(_usbPrinterDisplayName).join('\n');
      });
    } catch (error, stackTrace) {
      if (!mounted) return;
      setState(() {
        _status = 'USB 授权失败';
        _details = '$error\n$stackTrace';
      });
    } finally {
      if (mounted) {
        setState(() => _busy = false);
      }
    }
  }

  UsbPrinterInfo? _selectUsbPrinter(
    List<UsbPrinterInfo> printers,
    UsbPrinterInfo preferred,
  ) {
    for (final printer in printers) {
      if (preferred.platformDeviceId != null &&
          printer.platformDeviceId == preferred.platformDeviceId) {
        return printer;
      }
    }
    for (final printer in printers) {
      if (printer.vendorId == preferred.vendorId &&
          printer.productId == preferred.productId) {
        return printer;
      }
    }
    return printers.isEmpty ? null : printers.first;
  }

  Future<void> _scanNetworkPrinters() async {
    final timeoutMs = _parseInt(_networkTimeoutController.text, fallback: 3000);
    setState(() {
      _scanningNetwork = true;
      _status = '正在扫描网络打印机...';
      _details = '通过 mDNS/DNS-SD 扫描 WiFi/局域网打印服务，超时 ${timeoutMs}ms。';
    });

    try {
      final result = await discoverNetworkPrinters(
        timeout: Duration(milliseconds: timeoutMs),
      );
      final printers = result.printers;
      NetworkPrinterInfo? rawTcpPrinter;
      for (final printer in printers) {
        if (printer.supportsRawTcp) {
          rawTcpPrinter = printer;
          break;
        }
      }
      if (!mounted) return;
      setState(() {
        _networkPrinters = printers;
        _selectedNetworkPrinter =
            rawTcpPrinter ?? (printers.isEmpty ? null : printers.first);
        if (rawTcpPrinter != null) {
          _applyNetworkPrinter(rawTcpPrinter);
        }
        _status = printers.isEmpty
            ? '没有发现网络打印机'
            : '发现 ${printers.length} 台网络打印机';
        _details = printers.isEmpty
            ? '请确认设备和本机在同一 WiFi/局域网，且路由器未禁用 mDNS。'
            : [
                '耗时：${result.durationMs}ms',
                if (rawTcpPrinter == null)
                  '未发现可直接 ESC/POS raw TCP 连接的 9100/_pdl-datastream 服务',
                ...printers.map(
                  (printer) =>
                      '${printer.displayName} · ${printer.serviceType}'
                      '${printer.supportsRawTcp ? ' · Raw TCP' : ''}',
                ),
              ].join('\n');
      });
    } catch (error, stackTrace) {
      if (!mounted) return;
      setState(() {
        _status = '网络扫描失败';
        _details = '$error\n$stackTrace';
      });
    } finally {
      if (mounted) {
        setState(() => _scanningNetwork = false);
      }
    }
  }

  void _applyNetworkPrinter(NetworkPrinterInfo printer) {
    if (printer.host.isNotEmpty) {
      _networkHostController.text = printer.host;
    }
    if (printer.port > 0) {
      _networkPortController.text = printer.port.toString();
    }
  }

  Future<void> _runAction({
    required String running,
    required Future<_ActionResult> Function() action,
  }) async {
    setState(() {
      _busy = true;
      _status = running;
      _details =
          '连接：${_connection.queueKey}\n模板：${_selectedTemplate.displayName}';
    });

    try {
      final result = await action();
      if (!mounted) return;
      setState(() {
        _status = result.title;
        _details = result.details;
      });
    } catch (error, stackTrace) {
      if (!mounted) return;
      setState(() {
        _status = '执行异常';
        _details = '$error\n$stackTrace';
      });
    } finally {
      if (mounted) {
        setState(() => _busy = false);
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        title: const Text('KService Printer'),
        actions: [
          Padding(
            padding: const EdgeInsets.only(right: 16),
            child: Center(
              child: Text(
                'Queue $activePrintQueueCount',
                style: Theme.of(context).textTheme.labelLarge,
              ),
            ),
          ),
        ],
      ),
      body: SafeArea(
        child: SingleChildScrollView(
          padding: const EdgeInsets.all(16),
          child: Center(
            child: ConstrainedBox(
              constraints: const BoxConstraints(maxWidth: 980),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  _Header(status: _status, busy: _busy),
                  const SizedBox(height: 16),
                  LayoutBuilder(
                    builder: (context, constraints) {
                      final wide = constraints.maxWidth >= 820;
                      final children = [
                        _Panel(
                          title: '连接',
                          icon: Icons.settings_ethernet,
                          child: _connectionSection(),
                        ),
                        _Panel(
                          title: '模板',
                          icon: Icons.receipt_long,
                          child: _templateSection(),
                        ),
                      ];
                      if (!wide) {
                        return Column(
                          children: [
                            children[0],
                            const SizedBox(height: 16),
                            children[1],
                          ],
                        );
                      }
                      return Row(
                        crossAxisAlignment: CrossAxisAlignment.start,
                        children: [
                          Expanded(child: children[0]),
                          const SizedBox(width: 16),
                          Expanded(child: children[1]),
                        ],
                      );
                    },
                  ),
                  const SizedBox(height: 16),
                  _Panel(
                    title: '操作',
                    icon: Icons.print,
                    child: _actionsSection(),
                  ),
                  const SizedBox(height: 16),
                  LayoutBuilder(
                    builder: (context, constraints) {
                      final wide = constraints.maxWidth >= 820;
                      final result = _Panel(
                        title: '结果',
                        icon: Icons.fact_check,
                        child: _resultSection(),
                      );
                      final order = _Panel(
                        title: '测试订单',
                        icon: Icons.article,
                        child: _orderSection(),
                      );
                      if (!wide) {
                        return Column(
                          children: [result, const SizedBox(height: 16), order],
                        );
                      }
                      return Row(
                        crossAxisAlignment: CrossAxisAlignment.start,
                        children: [
                          Expanded(child: result),
                          const SizedBox(width: 16),
                          Expanded(child: order),
                        ],
                      );
                    },
                  ),
                ],
              ),
            ),
          ),
        ),
      ),
    );
  }

  Widget _connectionSection() {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        SegmentedButton<_ConnectionMode>(
          segments: const [
            ButtonSegment(
              value: _ConnectionMode.usb,
              icon: Icon(Icons.usb),
              label: Text('USB'),
            ),
            ButtonSegment(
              value: _ConnectionMode.network,
              icon: Icon(Icons.router),
              label: Text('网络'),
            ),
            ButtonSegment(
              value: _ConnectionMode.serial,
              icon: Icon(Icons.cable),
              label: Text('串口'),
            ),
          ],
          selected: {_connectionMode},
          onSelectionChanged: _busy
              ? null
              : (value) => setState(() => _connectionMode = value.first),
        ),
        const SizedBox(height: 12),
        ...switch (_connectionMode) {
          _ConnectionMode.usb => [
            Row(
              children: [
                Expanded(
                  child: FilledButton.icon(
                    onPressed: _busy || _scanningUsb ? null : _scanUsbPrinters,
                    icon: _scanningUsb
                        ? const SizedBox(
                            width: 18,
                            height: 18,
                            child: CircularProgressIndicator(strokeWidth: 2),
                          )
                        : const Icon(Icons.search),
                    label: const Text('扫描 USB 打印机'),
                  ),
                ),
              ],
            ),
            if (_usbPrinters.isNotEmpty) ...[
              const SizedBox(height: 12),
              DropdownButtonFormField<UsbPrinterInfo>(
                initialValue: _selectedUsbPrinter,
                decoration: const InputDecoration(
                  labelText: 'USB 打印机',
                  prefixIcon: Icon(Icons.print),
                ),
                items: [
                  for (final printer in _usbPrinters)
                    DropdownMenuItem(
                      value: printer,
                      child: Text(
                        _usbPrinterDisplayName(printer),
                        overflow: TextOverflow.ellipsis,
                      ),
                    ),
                ],
                onChanged: _busy
                    ? null
                    : (printer) {
                        if (printer == null) return;
                        setState(() {
                          _selectedUsbPrinter = printer;
                          _applyUsbPrinter(printer);
                        });
                      },
              ),
            ],
            if (_selectedUsbPrinter?.hasPermission == false) ...[
              const SizedBox(height: 12),
              FilledButton.icon(
                onPressed: _busy ? null : _requestUsbPermission,
                icon: const Icon(Icons.usb),
                label: const Text('请求 USB 授权'),
              ),
            ],
            const SizedBox(height: 12),
            Row(
              children: [
                Expanded(
                  child: _TextField(
                    controller: _usbVendorController,
                    label: 'Vendor ID',
                    prefixIcon: Icons.badge,
                  ),
                ),
                const SizedBox(width: 12),
                Expanded(
                  child: _TextField(
                    controller: _usbProductController,
                    label: 'Product ID',
                    prefixIcon: Icons.confirmation_number,
                  ),
                ),
              ],
            ),
          ],
          _ConnectionMode.network => [
            Row(
              children: [
                Expanded(
                  child: FilledButton.icon(
                    onPressed: _busy || _scanningNetwork
                        ? null
                        : _scanNetworkPrinters,
                    icon: _scanningNetwork
                        ? const SizedBox(
                            width: 18,
                            height: 18,
                            child: CircularProgressIndicator(strokeWidth: 2),
                          )
                        : const Icon(Icons.travel_explore),
                    label: const Text('扫描网络打印机'),
                  ),
                ),
              ],
            ),
            if (_networkPrinters.isNotEmpty) ...[
              const SizedBox(height: 12),
              DropdownButtonFormField<NetworkPrinterInfo>(
                initialValue: _selectedNetworkPrinter,
                decoration: const InputDecoration(
                  labelText: '网络打印机',
                  prefixIcon: Icon(Icons.wifi_tethering),
                ),
                items: [
                  for (final printer in _networkPrinters)
                    DropdownMenuItem(
                      value: printer,
                      child: Text(
                        printer.supportsRawTcp
                            ? '${printer.displayName} · Raw TCP'
                            : printer.displayName,
                        overflow: TextOverflow.ellipsis,
                      ),
                    ),
                ],
                onChanged: _busy
                    ? null
                    : (printer) {
                        if (printer == null) return;
                        setState(() {
                          _selectedNetworkPrinter = printer;
                          _applyNetworkPrinter(printer);
                        });
                      },
              ),
            ],
            const SizedBox(height: 12),
            _TextField(
              controller: _networkHostController,
              label: 'IP / Host',
              prefixIcon: Icons.dns,
            ),
            const SizedBox(height: 12),
            Row(
              children: [
                Expanded(
                  child: _TextField(
                    controller: _networkPortController,
                    label: 'Port',
                    prefixIcon: Icons.numbers,
                    keyboardType: TextInputType.number,
                  ),
                ),
                const SizedBox(width: 12),
                Expanded(
                  child: _TextField(
                    controller: _networkTimeoutController,
                    label: 'Timeout ms',
                    prefixIcon: Icons.timer,
                    keyboardType: TextInputType.number,
                  ),
                ),
              ],
            ),
          ],
          _ConnectionMode.serial => [
            _TextField(
              controller: _serialPortController,
              label: 'Port',
              prefixIcon: Icons.cable,
            ),
            const SizedBox(height: 12),
            _TextField(
              controller: _serialBaudController,
              label: 'Baud rate',
              prefixIcon: Icons.speed,
              keyboardType: TextInputType.number,
            ),
          ],
        },
        const SizedBox(height: 12),
        _InfoLine(label: '队列 key', value: _connection.queueKey),
      ],
    );
  }

  Widget _templateSection() {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        DropdownButtonFormField<ReceiptTemplateOption>(
          initialValue: _selectedTemplate,
          decoration: const InputDecoration(
            labelText: '打印模板',
            prefixIcon: Icon(Icons.view_list),
          ),
          items: [
            for (final option in builtInReceiptTemplateOptions)
              DropdownMenuItem(value: option, child: Text(option.displayName)),
          ],
          onChanged: _busy
              ? null
              : (option) {
                  if (option == null) return;
                  setState(() => _selectedTemplate = option);
                },
        ),
        const SizedBox(height: 12),
        Wrap(
          spacing: 8,
          runSpacing: 8,
          children: [
            _Chip(label: _selectedTemplate.type.displayName),
            _Chip(label: _selectedTemplate.paperSize.displayName),
            _Chip(label: _selectedTemplate.mode.displayName),
          ],
        ),
        const SizedBox(height: 12),
        _InfoLine(
          label: '模板编码',
          value: _selectedTemplate.buildTemplate().encoding,
        ),
        _InfoLine(
          label: '字符宽度',
          value: '${_selectedTemplate.buildTemplate().width} 列',
        ),
      ],
    );
  }

  Widget _actionsSection() {
    return Wrap(
      spacing: 12,
      runSpacing: 12,
      children: [
        FilledButton.icon(
          onPressed: _busy ? null : _render,
          icon: const Icon(Icons.memory),
          label: const Text('仅渲染指令'),
        ),
        FilledButton.icon(
          onPressed: _busy ? null : () => _print(queued: true),
          icon: const Icon(Icons.queue),
          label: const Text('加入队列打印'),
        ),
        OutlinedButton.icon(
          onPressed: _busy ? null : () => _print(queued: false),
          icon: const Icon(Icons.flash_on),
          label: const Text('直接打印'),
        ),
      ],
    );
  }

  Widget _resultSection() {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Row(
          children: [
            Icon(
              _busy ? Icons.hourglass_top : Icons.info,
              color: Theme.of(context).colorScheme.primary,
            ),
            const SizedBox(width: 8),
            Expanded(
              child: Text(
                _status,
                style: Theme.of(
                  context,
                ).textTheme.titleMedium?.copyWith(fontWeight: FontWeight.w700),
              ),
            ),
          ],
        ),
        const SizedBox(height: 12),
        SelectableText(
          _details,
          style: const TextStyle(fontFamily: 'monospace', height: 1.35),
        ),
      ],
    );
  }

  Widget _orderSection() {
    final order = _sampleData['order']! as Map<String, Object?>;
    final items = _sampleData['items']! as List<Map<String, Object?>>;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        _InfoLine(label: '订单号', value: order['no'].toString()),
        _InfoLine(label: '桌号', value: order['table'].toString()),
        _InfoLine(label: '合计', value: order['total'].toString()),
        const SizedBox(height: 8),
        for (final item in items)
          Padding(
            padding: const EdgeInsets.only(bottom: 6),
            child: Row(
              children: [
                Expanded(child: Text(item['name'].toString())),
                Text('x${item['qty']}'),
                const SizedBox(width: 12),
                Text(item['amount'].toString()),
              ],
            ),
          ),
        const Divider(height: 20),
        ExpansionTile(
          tilePadding: EdgeInsets.zero,
          title: const Text('查看 JSON 数据'),
          children: [
            Align(
              alignment: Alignment.centerLeft,
              child: SelectableText(
                const JsonEncoder.withIndent('  ').convert(_sampleData),
                style: const TextStyle(fontFamily: 'monospace', fontSize: 12),
              ),
            ),
          ],
        ),
      ],
    );
  }

  int _parseInt(String value, {required int fallback}) {
    final trimmed = value.trim();
    if (trimmed.startsWith('0x') || trimmed.startsWith('0X')) {
      return int.tryParse(trimmed.substring(2), radix: 16) ?? fallback;
    }
    return int.tryParse(trimmed) ?? fallback;
  }

  String _shortHex(String hex) {
    if (hex.length <= 160) {
      return hex;
    }
    return '${hex.substring(0, 160)}...';
  }
}

class _ActionResult {
  const _ActionResult(this.title, this.details);

  final String title;
  final String details;
}

class _Header extends StatelessWidget {
  const _Header({required this.status, required this.busy});

  final String status;
  final bool busy;

  @override
  Widget build(BuildContext context) {
    final colorScheme = Theme.of(context).colorScheme;
    return DecoratedBox(
      decoration: BoxDecoration(
        color: colorScheme.primaryContainer,
        borderRadius: BorderRadius.circular(8),
      ),
      child: Padding(
        padding: const EdgeInsets.all(16),
        child: Row(
          children: [
            Icon(Icons.print, size: 34, color: colorScheme.onPrimaryContainer),
            const SizedBox(width: 12),
            Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(
                    '打印调试台',
                    style: Theme.of(context).textTheme.headlineSmall?.copyWith(
                      color: colorScheme.onPrimaryContainer,
                      fontWeight: FontWeight.w800,
                    ),
                  ),
                  const SizedBox(height: 4),
                  Text(
                    status,
                    style: TextStyle(color: colorScheme.onPrimaryContainer),
                  ),
                ],
              ),
            ),
            if (busy)
              SizedBox(
                width: 24,
                height: 24,
                child: CircularProgressIndicator(
                  strokeWidth: 3,
                  color: colorScheme.onPrimaryContainer,
                ),
              ),
          ],
        ),
      ),
    );
  }
}

class _Panel extends StatelessWidget {
  const _Panel({required this.title, required this.icon, required this.child});

  final String title;
  final IconData icon;
  final Widget child;

  @override
  Widget build(BuildContext context) {
    final colorScheme = Theme.of(context).colorScheme;
    return Material(
      color: colorScheme.surface,
      shape: RoundedRectangleBorder(
        side: BorderSide(color: colorScheme.outlineVariant),
        borderRadius: BorderRadius.circular(8),
      ),
      clipBehavior: Clip.antiAlias,
      child: Padding(
        padding: const EdgeInsets.all(16),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Row(
              children: [
                Icon(icon, color: colorScheme.primary),
                const SizedBox(width: 8),
                Text(
                  title,
                  style: Theme.of(context).textTheme.titleMedium?.copyWith(
                    fontWeight: FontWeight.w800,
                  ),
                ),
              ],
            ),
            const SizedBox(height: 14),
            child,
          ],
        ),
      ),
    );
  }
}

class _TextField extends StatelessWidget {
  const _TextField({
    required this.controller,
    required this.label,
    required this.prefixIcon,
    this.keyboardType,
  });

  final TextEditingController controller;
  final String label;
  final IconData prefixIcon;
  final TextInputType? keyboardType;

  @override
  Widget build(BuildContext context) {
    return TextField(
      controller: controller,
      keyboardType: keyboardType,
      decoration: InputDecoration(
        labelText: label,
        prefixIcon: Icon(prefixIcon),
      ),
    );
  }
}

class _InfoLine extends StatelessWidget {
  const _InfoLine({required this.label, required this.value});

  final String label;
  final String value;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.only(bottom: 6),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          SizedBox(
            width: 82,
            child: Text(
              label,
              style: TextStyle(
                color: Theme.of(context).colorScheme.onSurfaceVariant,
              ),
            ),
          ),
          Expanded(
            child: SelectableText(
              value,
              style: const TextStyle(fontWeight: FontWeight.w600),
            ),
          ),
        ],
      ),
    );
  }
}

class _Chip extends StatelessWidget {
  const _Chip({required this.label});

  final String label;

  @override
  Widget build(BuildContext context) {
    return Chip(
      label: Text(label),
      visualDensity: VisualDensity.compact,
      side: BorderSide(color: Theme.of(context).colorScheme.outlineVariant),
    );
  }
}
