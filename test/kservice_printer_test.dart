import 'package:flutter_test/flutter_test.dart';
import 'package:kservice_printer/kservice_printer.dart';

void main() {
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
}
