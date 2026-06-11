import 'dart:convert';
import 'dart:math' as math;

import 'package:flutter/foundation.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:kservice_printer/kservice_printer.dart';
import 'package:kservice_printer/src/rust/frb_generated.dart';

final _fakeRustApi = _FakeRustApi();

void main() {
  setUpAll(() {
    TestWidgetsFlutterBinding.ensureInitialized();
    debugDefaultTargetPlatformOverride = TargetPlatform.macOS;
    RustLib.initMock(api: _fakeRustApi);
  });

  tearDownAll(() {
    debugDefaultTargetPlatformOverride = null;
  });

  setUp(() {
    _fakeRustApi.reset();
  });

  test('builds default order receipt job', () {
    final job = PrintJob(
      connection: const PrinterConnection.network(
        host: '127.0.0.1',
        port: 9100,
        timeoutMs: 3000,
      ),
      template: defaultOrderReceiptTemplate(),
      data: {
        'store': {'name': '测试餐厅'},
        'order': {
          'no': 'A001',
          'table': 'A08',
          'time': '12:30',
          'total': '¥88.00',
        },
        'items': [
          {'name': '牛肉饭', 'qty': '1', 'amount': '¥58.00', 'remark': ''},
        ],
      },
    );

    expect(job.template.elements, isNotEmpty);
    expect(job.type, PrintJobType.receipt);
  });

  test('builds kitchen ticket job type and template', () {
    final job = PrintJob(
      type: PrintJobType.kitchen,
      connection: const PrinterConnection.network(
        host: '127.0.0.1',
        port: 9100,
        timeoutMs: 3000,
      ),
      template: defaultKitchenTicketTemplate(),
      data: {
        'order': {
          'no': 'K001',
          'table': 'A08',
          'time': '12:30',
          'mealType': '堂食',
          'remark': '加急',
        },
        'items': [
          {'name': '牛肉饭', 'qty': '2', 'spec': '少辣', 'remark': '不要香菜'},
        ],
      },
    );

    expect(job.type.code, 'kitchen');
    expect(job.type.displayName, '后厨打印');
    expect(job.template.width, 48);
    expect(job.template.elements, isNotEmpty);
  });

  test('builds label template from print job type', () {
    final template = defaultTemplateForPrintJobType(PrintJobType.label);

    expect(template.width, 32);
    expect(template.elements.first['value'], '{{item.name}}');
  });

  test('builds templates for 58mm and 80mm receipt paper', () {
    final template58 = defaultTemplateForPrintJobType(
      PrintJobType.receipt,
      paperSize: ReceiptPaperSize.mm58,
    );
    final template80 = defaultTemplateForPrintJobType(
      PrintJobType.receipt,
      paperSize: ReceiptPaperSize.mm80,
    );

    expect(template58.width, 32);
    expect(template80.width, 48);

    final header58 = template58.elements[6];
    final header80 = template80.elements[6];
    final columns58 = header58['columns']! as List<Map<String, Object?>>;
    final columns80 = header80['columns']! as List<Map<String, Object?>>;

    expect(columns58.map((column) => column['width']), [16, 6, 10]);
    expect(columns80.map((column) => column['width']), [24, 8, 16]);
  });

  test('builds image order template for complex scripts', () {
    final template = defaultOrderReceiptImageTemplate(
      width: 32,
      fontFamily: 'Noto Sans Arabic',
      fontSize: 26,
    );
    final optionTemplate = defaultTemplateForPrintJobType(
      PrintJobType.receipt,
      paperSize: ReceiptPaperSize.mm58,
      mode: ReceiptPrintMode.image,
      fontFamily: 'Noto Sans Arabic',
    );

    expect(template.width, 32);
    expect(template.encoding, 'image');
    expect(template.fontFamily, 'Noto Sans Arabic');
    expect(template.fontSize, 26);
    expect(template.toJson()['fontFamily'], 'Noto Sans Arabic');
    expect(template.toJson()['fontSize'], 26);
    expect(template.elements, isNotEmpty);
    expect(optionTemplate.width, 32);
    expect(optionTemplate.encoding, 'image');
    expect(optionTemplate.fontFamily, 'Noto Sans Arabic');
  });

  test('default text templates use gbk encoding', () {
    expect(const ReceiptTemplate(elements: []).encoding, 'gbk');
    expect(defaultOrderReceiptTemplate().encoding, 'gbk');
  });

  test('exposes built-in template options for selection UI', () {
    expect(
      builtInReceiptTemplateOptions.map((option) => option.displayName),
      containsAll([
        '订单小票 · 58mm 小票 · 文本打印',
        '订单小票 · 80mm 小票 · 文本打印',
        '订单小票 · 58mm 小票 · 图片打印',
        '订单小票 · 80mm 小票 · 图片打印',
        '后厨打印 · 58mm 小票 · 文本打印',
        '后厨打印 · 80mm 小票 · 文本打印',
      ]),
    );

    final option = builtInReceiptTemplateOptions.first;
    expect(option.code, 'receipt_mm58_text');
    expect(option.buildTemplate().width, 32);
  });

  test('builds stable print queue keys by connection', () {
    expect(
      const PrinterConnection.network(
        host: '127.0.0.1',
        port: 9100,
        timeoutMs: 1000,
      ).queueKey,
      'network:127.0.0.1:9100',
    );
    expect(
      const PrinterConnection.usb(vendorId: 0x0483, productId: 0x070B).queueKey,
      'usb:1155:1803',
    );
    expect(
      const PrinterConnection.serial(
        port: '/dev/ttyUSB0',
        baudRate: 115200,
      ).queueKey,
      'serial:/dev/ttyUSB0:115200',
    );
  });

  test('parses network printer discovery result', () {
    final result = NetworkPrinterDiscoveryResult.fromJson({
      'timeoutMs': 3000,
      'durationMs': 3010,
      'timedOut': true,
      'serviceTypes': ['_pdl-datastream._tcp.local.'],
      'printers': [
        {
          'serviceName': 'Kitchen Printer',
          'serviceType': '_pdl-datastream._tcp.local.',
          'fullname': 'Kitchen Printer._pdl-datastream._tcp.local.',
          'hostname': 'kitchen-printer.local',
          'host': '192.168.1.50',
          'port': 9100,
          'addresses': ['192.168.1.50'],
          'txt': {'note': 'raw'},
          'supportsRawTcp': true,
        },
      ],
    });
    final printer = result.printers.single;

    expect(result.timedOut, isTrue);
    expect(result.serviceTypes, ['_pdl-datastream._tcp.local.']);
    expect(printer.displayName, 'Kitchen Printer · 192.168.1.50:9100');
    expect(printer.supportsRawTcp, isTrue);
    expect(printer.txt['note'], 'raw');
    expect(printer.connection().queueKey, 'network:192.168.1.50:9100');
  });

  test('network printer discovery calls are serialized', () async {
    await Future.wait([
      discoverNetworkPrinters(
        timeout: const Duration(milliseconds: 300),
        serviceTypes: ['_printer._tcp.local.'],
      ),
      discoverNetworkPrinters(
        timeout: const Duration(milliseconds: 300),
        serviceTypes: ['_ipp._tcp.local.'],
      ),
    ]);

    expect(_fakeRustApi.discoveryCalls, 2);
    expect(_fakeRustApi.maxActiveDiscoveries, 1);
    expect(activeNetworkDiscoveryCount, 0);
  });

  test('print queue serializes jobs for the same printer', () async {
    final connection = const PrinterConnection.network(
      host: '127.0.0.1',
      port: 9100,
      timeoutMs: 1000,
    );
    final template = const ReceiptTemplate(elements: []);

    await Future.wait([
      printReceipt(
        PrintJob(
          connection: connection,
          template: template,
          data: {'id': 'first'},
        ),
      ),
      printReceipt(
        PrintJob(
          connection: connection,
          template: template,
          data: {'id': 'second'},
        ),
      ),
    ]);

    expect(_fakeRustApi.startedJobIds, ['first', 'second']);
    expect(_fakeRustApi.maxActiveByKey[connection.queueKey], 1);
  });

  test('print queue allows different printers to run concurrently', () async {
    final template = const ReceiptTemplate(elements: []);
    final firstConnection = const PrinterConnection.network(
      host: '127.0.0.1',
      port: 9100,
      timeoutMs: 1000,
    );
    final secondConnection = const PrinterConnection.network(
      host: '127.0.0.2',
      port: 9100,
      timeoutMs: 1000,
    );

    await Future.wait([
      printReceipt(
        PrintJob(
          connection: firstConnection,
          template: template,
          data: {'id': 'first'},
        ),
      ),
      printReceipt(
        PrintJob(
          connection: secondConnection,
          template: template,
          data: {'id': 'second'},
        ),
      ),
    ]);

    expect(_fakeRustApi.maxActiveTotal, 2);
  });
}

class _FakeRustApi extends RustLibApi {
  final startedJobIds = <String>[];
  final maxActiveByKey = <String, int>{};
  final _activeByKey = <String, int>{};
  int _activeTotal = 0;
  int maxActiveTotal = 0;
  int _activeDiscoveries = 0;
  int maxActiveDiscoveries = 0;
  int discoveryCalls = 0;

  void reset() {
    startedJobIds.clear();
    maxActiveByKey.clear();
    _activeByKey.clear();
    _activeTotal = 0;
    maxActiveTotal = 0;
    _activeDiscoveries = 0;
    maxActiveDiscoveries = 0;
    discoveryCalls = 0;
  }

  @override
  Future<String> crateApiPrinterDiscoverNetworkPrinters({
    required int timeoutMs,
    required List<String> serviceTypes,
  }) async {
    discoveryCalls += 1;
    _activeDiscoveries += 1;
    maxActiveDiscoveries = math.max(maxActiveDiscoveries, _activeDiscoveries);

    await Future<void>.delayed(const Duration(milliseconds: 20));

    _activeDiscoveries -= 1;
    return jsonEncode({
      'ok': true,
      'result': {
        'timeoutMs': timeoutMs,
        'durationMs': 0,
        'timedOut': false,
        'serviceTypes': serviceTypes,
        'printers': [],
      },
    });
  }

  @override
  Future<void> crateApiPrinterInitApp() async {}

  @override
  Future<String> crateApiPrinterListUsbPrinters() async {
    return jsonEncode({
      'ok': true,
      'result': {'printers': []},
    });
  }

  @override
  Future<String> crateApiPrinterPrintReceipt({
    required PrinterConnection connection,
    required String templateJson,
    required String dataJson,
  }) async {
    final key = connection.queueKey;
    final data = jsonDecode(dataJson) as Map<String, dynamic>;
    startedJobIds.add(data['id']?.toString() ?? '');
    _activeByKey[key] = (_activeByKey[key] ?? 0) + 1;
    maxActiveByKey[key] = math.max(
      maxActiveByKey[key] ?? 0,
      _activeByKey[key] ?? 0,
    );
    _activeTotal += 1;
    maxActiveTotal = math.max(maxActiveTotal, _activeTotal);

    await Future<void>.delayed(const Duration(milliseconds: 20));

    _activeByKey[key] = (_activeByKey[key] ?? 1) - 1;
    _activeTotal -= 1;
    return jsonEncode({
      'ok': true,
      'result': {'printed': true, 'bytes': 1},
    });
  }

  @override
  Future<String> crateApiPrinterRenderReceipt({
    required String templateJson,
    required String dataJson,
  }) async {
    return jsonEncode({
      'ok': true,
      'result': {'bytes': '', 'length': 0},
    });
  }
}
