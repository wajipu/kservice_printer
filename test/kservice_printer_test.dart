import 'package:flutter_test/flutter_test.dart';
import 'package:kservice_printer/kservice_printer.dart';

void main() {
  test('builds default order receipt job', () {
    final job = PrintJob(
      connection: const PrinterConnection.network(host: '127.0.0.1', port: 9100, timeoutMs: 3000),
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
  });
}
