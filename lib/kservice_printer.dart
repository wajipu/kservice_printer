import 'dart:async';
import 'dart:convert';
import 'dart:math' as math;

import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';
import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated.dart';

import 'src/rust/api/printer.dart' as rust_printer;
import 'src/rust/api/printer.dart' show PrinterConnection;
import 'src/rust/frb_generated.dart';

export 'src/rust/api/printer.dart' show PrinterConnection;

Future<void>? _rustInitFuture;
const MethodChannel _platformChannel = MethodChannel('kservice_printer');

Future<void> initKservicePrinter() {
  if (RustLib.instance.initialized) {
    return Future<void>.value();
  }
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

  /// 使用 TSPL 标签语言，适合 TSC 兼容标签打印机。
  tspl,

  /// 将整张标签渲染成 TSPL 位图，适合复杂文字和精确版式。
  tsplImage,

  /// 将整张标签渲染成 TSPL BAR 栅格，适合不兼容二进制 BITMAP 的设备。
  tsplRaster,

  /// 使用 ZPL 标签语言，适合 Zebra 兼容标签打印机。
  zpl,

  /// 将整张标签渲染成 ZPL ^GFA 位图，适合复杂文字和精确版式。
  zplImage,
}

extension ReceiptPrintModeInfo on ReceiptPrintMode {
  String get encoding => switch (this) {
    ReceiptPrintMode.text => 'gbk',
    ReceiptPrintMode.image => 'image',
    ReceiptPrintMode.tspl => 'tspl',
    ReceiptPrintMode.tsplImage => 'tspl-image',
    ReceiptPrintMode.tsplRaster => 'tspl-raster',
    ReceiptPrintMode.zpl => 'zpl',
    ReceiptPrintMode.zplImage => 'zpl-image',
  };

  String get displayName => switch (this) {
    ReceiptPrintMode.text => '文本打印',
    ReceiptPrintMode.image => '图片打印',
    ReceiptPrintMode.tspl => 'TSPL 标签',
    ReceiptPrintMode.tsplImage => 'TSPL 图片标签',
    ReceiptPrintMode.tsplRaster => 'TSPL 兼容图片标签',
    ReceiptPrintMode.zpl => 'ZPL 标签',
    ReceiptPrintMode.zplImage => 'ZPL 图片标签',
  };

  bool get isLabelLanguage => switch (this) {
    ReceiptPrintMode.tspl ||
    ReceiptPrintMode.tsplImage ||
    ReceiptPrintMode.tsplRaster ||
    ReceiptPrintMode.zpl ||
    ReceiptPrintMode.zplImage => true,
    ReceiptPrintMode.text || ReceiptPrintMode.image => false,
  };
}

/// 可供 UI 展示的内置模板选项。
class ReceiptTemplateOption {
  const ReceiptTemplateOption({
    required this.type,
    required this.paperSize,
    this.mode = ReceiptPrintMode.text,
    this.labelHeightMm,
    this.labelGapMm,
    this.labelDensity,
    this.labelSpeed,
    this.labelHomeBeforePrint,
  });

  final PrintJobType type;
  final ReceiptPaperSize paperSize;
  final ReceiptPrintMode mode;

  final double? labelHeightMm;
  final double? labelGapMm;
  final int? labelDensity;
  final int? labelSpeed;
  final bool? labelHomeBeforePrint;

  String get code => '${type.code}_${paperSize.name}_${mode.name}';

  String get paperDisplayName =>
      type == PrintJobType.label && mode.isLabelLanguage
      ? switch (paperSize) {
          ReceiptPaperSize.mm58 => '58mm 标签',
          ReceiptPaperSize.mm80 => '80mm 标签',
        }
      : paperSize.displayName;

  String get displayName =>
      '${type.displayName} · $paperDisplayName · ${mode.displayName}';

  ReceiptTemplate buildTemplate() => defaultTemplateForPrintJobType(
    type,
    paperSize: paperSize,
    mode: mode,
    labelHeightMm: labelHeightMm,
    labelGapMm: labelGapMm,
    labelDensity: labelDensity,
    labelSpeed: labelSpeed,
    labelHomeBeforePrint: labelHomeBeforePrint,
  );
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
    mode: ReceiptPrintMode.tspl,
  ),
  ReceiptTemplateOption(
    type: PrintJobType.label,
    paperSize: ReceiptPaperSize.mm58,
    mode: ReceiptPrintMode.tsplImage,
  ),
  ReceiptTemplateOption(
    type: PrintJobType.label,
    paperSize: ReceiptPaperSize.mm58,
    mode: ReceiptPrintMode.tsplRaster,
  ),
  ReceiptTemplateOption(
    type: PrintJobType.label,
    paperSize: ReceiptPaperSize.mm58,
    mode: ReceiptPrintMode.zpl,
  ),
  ReceiptTemplateOption(
    type: PrintJobType.label,
    paperSize: ReceiptPaperSize.mm58,
    mode: ReceiptPrintMode.zplImage,
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
    this.encoding = 'gbk',
    this.fontFamily,
    this.fontSize,
    this.labelWidthMm,
    this.labelHeightMm,
    this.labelGapMm,
    this.labelDensity,
    this.labelSpeed,
    this.labelHomeBeforePrint,
    this.labelReferenceX,
    this.labelReferenceY,
    this.labelShiftDots,
    required this.elements,
  });

  final int width;
  final String encoding;
  final String? fontFamily;
  final double? fontSize;
  final double? labelWidthMm;
  final double? labelHeightMm;
  final double? labelGapMm;
  final int? labelDensity;
  final int? labelSpeed;
  final bool? labelHomeBeforePrint;
  final int? labelReferenceX;
  final int? labelReferenceY;
  final int? labelShiftDots;
  final List<Map<String, Object?>> elements;

  ReceiptTemplate copyWith({
    int? width,
    String? encoding,
    String? fontFamily,
    double? fontSize,
    double? labelWidthMm,
    double? labelHeightMm,
    double? labelGapMm,
    int? labelDensity,
    int? labelSpeed,
    bool? labelHomeBeforePrint,
    int? labelReferenceX,
    int? labelReferenceY,
    int? labelShiftDots,
    List<Map<String, Object?>>? elements,
  }) => ReceiptTemplate(
    width: width ?? this.width,
    encoding: encoding ?? this.encoding,
    fontFamily: fontFamily ?? this.fontFamily,
    fontSize: fontSize ?? this.fontSize,
    labelWidthMm: labelWidthMm ?? this.labelWidthMm,
    labelHeightMm: labelHeightMm ?? this.labelHeightMm,
    labelGapMm: labelGapMm ?? this.labelGapMm,
    labelDensity: labelDensity ?? this.labelDensity,
    labelSpeed: labelSpeed ?? this.labelSpeed,
    labelHomeBeforePrint: labelHomeBeforePrint ?? this.labelHomeBeforePrint,
    labelReferenceX: labelReferenceX ?? this.labelReferenceX,
    labelReferenceY: labelReferenceY ?? this.labelReferenceY,
    labelShiftDots: labelShiftDots ?? this.labelShiftDots,
    elements: elements ?? this.elements,
  );

  factory ReceiptTemplate.fromJson(Map<String, dynamic> json) {
    return ReceiptTemplate(
      width: json['width'] as int? ?? 48,
      encoding: json['encoding'] as String? ?? 'gbk',
      fontFamily: json['fontFamily'] as String?,
      fontSize: (json['fontSize'] as num?)?.toDouble(),
      labelWidthMm: (json['labelWidthMm'] as num?)?.toDouble(),
      labelHeightMm: (json['labelHeightMm'] as num?)?.toDouble(),
      labelGapMm: (json['labelGapMm'] as num?)?.toDouble(),
      labelDensity: json['labelDensity'] as int?,
      labelSpeed: json['labelSpeed'] as int?,
      labelHomeBeforePrint: json['labelHomeBeforePrint'] as bool?,
      labelReferenceX: json['labelReferenceX'] as int?,
      labelReferenceY: json['labelReferenceY'] as int?,
      labelShiftDots: json['labelShiftDots'] as int?,
      elements: (json['elements'] as List<dynamic>)
          .cast<Map<String, Object?>>(),
    );
  }

  Map<String, Object?> toJson() => {
    'width': width,
    'encoding': encoding,
    if (fontFamily != null) 'fontFamily': fontFamily,
    if (fontSize != null) 'fontSize': fontSize,
    if (labelWidthMm != null) 'labelWidthMm': labelWidthMm,
    if (labelHeightMm != null) 'labelHeightMm': labelHeightMm,
    if (labelGapMm != null) 'labelGapMm': labelGapMm,
    if (labelDensity != null) 'labelDensity': labelDensity,
    if (labelSpeed != null) 'labelSpeed': labelSpeed,
    if (labelHomeBeforePrint != null)
      'labelHomeBeforePrint': labelHomeBeforePrint,
    if (labelReferenceX != null) 'labelReferenceX': labelReferenceX,
    if (labelReferenceY != null) 'labelReferenceY': labelReferenceY,
    if (labelShiftDots != null) 'labelShiftDots': labelShiftDots,
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

/// 租户/门店下的一台打印机业务绑定。
///
/// 一个绑定可以覆盖多个票据类型；如果 [template] 为空，分发时会按票据类型使用默认模板。
/// 同一台物理打印机可以配置多条绑定，用于不同票据类型或不同区域标签。
class PrinterBinding {
  const PrinterBinding({
    required this.id,
    required this.connection,
    this.name,
    this.tenantId,
    this.storeId,
    this.types = const <PrintJobType>[],
    this.tags = const <String>[],
    this.template,
    this.enabled = true,
  });

  /// 绑定 ID，建议使用业务系统里的打印机绑定主键。
  final String id;

  /// UI 展示名称，例如“收银台”“后厨热菜”“吧台”。
  final String? name;

  /// 绑定所属租户；为空表示全局绑定。
  final String? tenantId;

  /// 绑定所属门店；为空表示租户下全门店可用。
  final String? storeId;

  /// 打印机连接信息。
  final PrinterConnection connection;

  /// 该绑定可处理的票据类型；为空表示不限制类型。
  final List<PrintJobType> types;

  /// 业务标签，例如 `cashier`、`kitchen`、`bar`、`drink`。
  final List<String> tags;

  /// 该绑定使用的模板；为空时按 [PrintJobType] 使用默认模板。
  final ReceiptTemplate? template;

  /// false 时分发会跳过该绑定。
  final bool enabled;

  bool matches({
    required PrintJobType type,
    Iterable<String> tags = const <String>[],
    String? tenantId,
    String? storeId,
  }) {
    if (!enabled) {
      return false;
    }
    if (this.tenantId != null && this.tenantId != tenantId) {
      return false;
    }
    if (this.storeId != null && this.storeId != storeId) {
      return false;
    }
    if (types.isNotEmpty && !types.contains(type)) {
      return false;
    }

    final requestedTags = tags.toSet();
    if (requestedTags.isEmpty) {
      return true;
    }
    if (this.tags.isEmpty) {
      return false;
    }
    return this.tags.any(requestedTags.contains);
  }

  PrintJob buildJob({
    required PrintJobType type,
    required Map<String, Object?> data,
    ReceiptTemplate? fallbackTemplate,
  }) {
    return PrintJob(
      connection: connection,
      type: type,
      template:
          template ?? fallbackTemplate ?? defaultTemplateForPrintJobType(type),
      data: data,
    );
  }
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

/// ESC/POS 打印机实时状态。
class PrinterStatus {
  const PrinterStatus({
    required this.ok,
    required this.supported,
    required this.online,
    required this.drawerKickOutHigh,
    required this.coverOpen,
    required this.paperFeedPressed,
    required this.paperNearEnd,
    required this.paperEnd,
    required this.mechanicalError,
    required this.cutterError,
    required this.recoverableError,
    required this.unrecoverableError,
    required this.error,
    required this.raw,
    required this.rawHex,
    required this.timeoutMs,
    this.message,
  });

  final bool ok;
  final bool supported;
  final bool online;
  final bool drawerKickOutHigh;
  final bool coverOpen;
  final bool paperFeedPressed;
  final bool paperNearEnd;
  final bool paperEnd;
  final bool mechanicalError;
  final bool cutterError;
  final bool recoverableError;
  final bool unrecoverableError;
  final bool error;
  final Map<int, int> raw;
  final Map<String, String> rawHex;
  final int timeoutMs;
  final String? message;

  bool get ready => ok;

  factory PrinterStatus.fromJson(Map<String, dynamic> json) {
    if (json['ok'] != true) {
      return PrinterStatus.unavailable(message: json['error']?.toString());
    }
    final result = json['result'];
    if (result is! Map<String, dynamic>) {
      return const PrinterStatus.unavailable(message: '打印机状态查询返回格式无效');
    }
    return PrinterStatus(
      ok: result['ok'] == true,
      supported: result['supported'] == true,
      online: result['online'] == true,
      drawerKickOutHigh: result['drawerKickOutHigh'] == true,
      coverOpen: result['coverOpen'] == true,
      paperFeedPressed: result['paperFeedPressed'] == true,
      paperNearEnd: result['paperNearEnd'] == true,
      paperEnd: result['paperEnd'] == true,
      mechanicalError: result['mechanicalError'] == true,
      cutterError: result['cutterError'] == true,
      recoverableError: result['recoverableError'] == true,
      unrecoverableError: result['unrecoverableError'] == true,
      error: result['error'] == true,
      raw: _intMap(result['raw']),
      rawHex: _stringMap(result['rawHex']),
      timeoutMs: result['timeoutMs'] is num
          ? (result['timeoutMs'] as num).toInt()
          : 0,
    );
  }

  const factory PrinterStatus.unavailable({String? message}) =
      _UnavailablePrinterStatus;
}

class _UnavailablePrinterStatus extends PrinterStatus {
  const _UnavailablePrinterStatus({super.message})
    : super(
        ok: false,
        supported: false,
        online: false,
        drawerKickOutHigh: false,
        coverOpen: false,
        paperFeedPressed: false,
        paperNearEnd: false,
        paperEnd: false,
        mechanicalError: false,
        cutterError: false,
        recoverableError: false,
        unrecoverableError: false,
        error: true,
        raw: const {},
        rawHex: const {},
        timeoutMs: 0,
      );
}

/// 打印机设备身份信息。
class PrinterIdentity {
  const PrinterIdentity({
    required this.ok,
    required this.supported,
    this.maker,
    this.model,
    this.serial,
    this.firmware,
    this.raw = const {},
    this.timeoutMs = 0,
    this.error,
  });

  final bool ok;
  final bool supported;
  final String? maker;
  final String? model;
  final String? serial;
  final String? firmware;
  final Map<String, String> raw;
  final int timeoutMs;
  final String? error;

  String get displayName {
    final parts = [
      if (maker != null && maker!.isNotEmpty) maker,
      if (model != null && model!.isNotEmpty) model,
      if (serial != null && serial!.isNotEmpty) serial,
    ];
    return parts.isEmpty ? 'Unknown printer' : parts.join(' · ');
  }

  factory PrinterIdentity.fromJson(Map<String, dynamic> json) {
    if (json['ok'] != true) {
      return PrinterIdentity(
        ok: false,
        supported: false,
        error: json['error']?.toString(),
      );
    }
    final result = json['result'];
    if (result is! Map<String, dynamic>) {
      return const PrinterIdentity(
        ok: false,
        supported: false,
        error: '打印机身份查询返回格式无效',
      );
    }
    return PrinterIdentity(
      ok: true,
      supported: result['supported'] == true,
      maker: _nullableString(result['maker']),
      model: _nullableString(result['model']),
      serial: _nullableString(result['serial']),
      firmware: _nullableString(result['firmware']),
      raw: _stringMap(result['raw']),
      timeoutMs: result['timeoutMs'] is num
          ? (result['timeoutMs'] as num).toInt()
          : 0,
    );
  }
}

/// 单次压测打印结果。
class PrinterStressTestJobResult {
  const PrinterStressTestJobResult({
    required this.index,
    required this.ok,
    required this.bytes,
    required this.durationMs,
    this.error,
  });

  final int index;
  final bool ok;
  final int bytes;
  final int durationMs;
  final String? error;
}

/// 并发压测汇总结果。
class PrinterStressTestResult {
  const PrinterStressTestResult({
    required this.total,
    required this.success,
    required this.failure,
    required this.concurrency,
    required this.queued,
    required this.durationMs,
    required this.maxInFlight,
    required this.totalBytes,
    required this.jobs,
  });

  final int total;
  final int success;
  final int failure;
  final int concurrency;
  final bool queued;
  final int durationMs;
  final int maxInFlight;
  final int totalBytes;
  final List<PrinterStressTestJobResult> jobs;

  bool get ok => failure == 0;
}

/// ESC/POS 钱箱脉冲引脚。
enum CashDrawerPin { pin2, pin5 }

extension CashDrawerPinInfo on CashDrawerPin {
  int get code => switch (this) {
    CashDrawerPin.pin2 => 0,
    CashDrawerPin.pin5 => 1,
  };

  String get displayName => switch (this) {
    CashDrawerPin.pin2 => 'Pin 2',
    CashDrawerPin.pin5 => 'Pin 5',
  };
}

/// 多打印机分发后的单台打印机结果。
class PrintDispatchResult {
  const PrintDispatchResult({
    required this.binding,
    required this.job,
    required this.result,
  });

  final PrinterBinding binding;
  final PrintJob job;
  final PrintResult result;

  String get targetId => binding.id;

  bool get ok => result.ok;
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
    this.hasPermission,
    this.platformDeviceId,
  });

  final int vendorId;
  final int productId;
  final String vendorIdHex;
  final String productIdHex;
  final String? manufacturer;
  final String? product;
  final String? serial;
  final bool isPrinter;
  final bool? hasPermission;
  final String? platformDeviceId;

  String get displayName {
    final name = [
      if (manufacturer != null && manufacturer!.isNotEmpty) manufacturer,
      if (product != null && product!.isNotEmpty) product,
    ].join(' ');
    final label = name.isEmpty
        ? (platformDeviceId != null && platformDeviceId!.isNotEmpty
              ? platformDeviceId!
              : 'USB Device')
        : name;
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
      hasPermission: json['hasPermission'] is bool
          ? json['hasPermission'] as bool
          : null,
      platformDeviceId: json['deviceName']?.toString(),
    );
  }
}

/// 扫描本机可见 USB 打印机。
///
/// 默认只返回底层识别为 USB printer class 的设备，避免把 Hub、手机、
/// 声卡、鼠标键盘接收器等普通 USB 设备展示成打印机。排查硬件识别问题时，
/// 可以设置 [includeNonPrinters] 返回完整 USB 设备清单。
Future<List<UsbPrinterInfo>> listUsbPrinters({
  bool includeNonPrinters = false,
}) async {
  if (defaultTargetPlatform == TargetPlatform.android) {
    final response = await _platformChannel.invokeMethod<String>(
      'listUsbPrinters',
    );
    return _decodeUsbPrinterListResponse(
      response,
      includeNonPrinters: includeNonPrinters,
    );
  }

  await initKservicePrinter();
  final response = await rust_printer.listUsbPrinters();
  return _decodeUsbPrinterListResponse(
    response,
    includeNonPrinters: includeNonPrinters,
  );
}

List<UsbPrinterInfo> _decodeUsbPrinterListResponse(
  String? response, {
  required bool includeNonPrinters,
}) {
  if (response == null || response.isEmpty) {
    throw StateError('USB 扫描未返回结果');
  }
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
      if (item is Map<String, dynamic>)
        if (includeNonPrinters || item['isPrinter'] == true)
          UsbPrinterInfo.fromJson(item),
  ];
}

/// Android 上请求指定 USB 设备授权；其它平台没有对应运行时授权，直接返回 true。
Future<bool> requestUsbPrinterPermission(UsbPrinterInfo printer) async {
  if (defaultTargetPlatform != TargetPlatform.android) {
    return printer.hasPermission ?? true;
  }

  final response = await _platformChannel
      .invokeMethod<String>('requestUsbPrinterPermission', {
        'vendorId': printer.vendorId,
        'productId': printer.productId,
        'deviceName': printer.platformDeviceId,
      });
  if (response == null || response.isEmpty) {
    throw StateError('USB 授权未返回结果');
  }
  final json = jsonDecode(response) as Map<String, dynamic>;
  if (json['ok'] != true) {
    throw StateError(json['error']?.toString() ?? 'USB 授权失败');
  }
  final result = json['result'];
  return result is Map && result['granted'] == true;
}

/// mDNS/DNS-SD 发现到的网络打印设备。
class NetworkPrinterInfo {
  const NetworkPrinterInfo({
    required this.serviceName,
    required this.serviceType,
    required this.fullname,
    required this.hostname,
    required this.host,
    required this.port,
    this.addresses = const [],
    this.txt = const {},
    this.supportsRawTcp = false,
  });

  /// 服务实例名称，适合 UI 展示。
  final String serviceName;

  /// mDNS 服务类型，例如 `_pdl-datastream._tcp.local.`。
  final String serviceType;

  /// mDNS 完整服务名。
  final String fullname;

  /// 服务主机名。
  final String hostname;

  /// 推荐连接地址，优先 IPv4，其次其它解析地址，最后 hostname。
  final String host;

  /// 服务端口。
  final int port;

  /// mDNS 返回的所有地址。
  final List<String> addresses;

  /// TXT record 属性。
  final Map<String, String> txt;

  /// 是否看起来支持 ESC/POS raw TCP 直连。
  ///
  /// `_pdl-datastream._tcp` 或 9100 端口会被标记为 true。
  final bool supportsRawTcp;

  String get displayName {
    final label = serviceName.isNotEmpty
        ? serviceName
        : (hostname.isNotEmpty ? hostname : host);
    return '$label · $host:$port';
  }

  PrinterConnection connection({
    Duration timeout = const Duration(seconds: 3),
  }) {
    return PrinterConnection.network(
      host: host,
      port: port,
      timeoutMs: timeout.inMilliseconds,
    );
  }

  factory NetworkPrinterInfo.fromJson(Map<String, dynamic> json) {
    return NetworkPrinterInfo(
      serviceName: json['serviceName']?.toString() ?? '',
      serviceType: json['serviceType']?.toString() ?? '',
      fullname: json['fullname']?.toString() ?? '',
      hostname: json['hostname']?.toString() ?? '',
      host: json['host']?.toString() ?? '',
      port: json['port'] is num ? (json['port'] as num).toInt() : 0,
      addresses: _stringList(json['addresses']),
      txt: _stringMap(json['txt']),
      supportsRawTcp: json['supportsRawTcp'] == true,
    );
  }
}

/// 网络打印机发现结果。
class NetworkPrinterDiscoveryResult {
  const NetworkPrinterDiscoveryResult({
    required this.printers,
    required this.serviceTypes,
    required this.timeoutMs,
    required this.durationMs,
    required this.timedOut,
  });

  final List<NetworkPrinterInfo> printers;
  final List<String> serviceTypes;
  final int timeoutMs;
  final int durationMs;

  /// true 表示扫描跑满了指定超时；false 通常只会出现在平台快速失败等情况。
  final bool timedOut;

  factory NetworkPrinterDiscoveryResult.fromJson(Map<String, dynamic> json) {
    final printers = json['printers'];
    return NetworkPrinterDiscoveryResult(
      printers: _dedupeNetworkPrinters([
        if (printers is List)
          for (final item in printers)
            if (item is Map<String, dynamic>) NetworkPrinterInfo.fromJson(item),
      ]),
      serviceTypes: _stringList(json['serviceTypes']),
      timeoutMs: json['timeoutMs'] is num
          ? (json['timeoutMs'] as num).toInt()
          : 0,
      durationMs: json['durationMs'] is num
          ? (json['durationMs'] as num).toInt()
          : 0,
      timedOut: json['timedOut'] == true,
    );
  }
}

List<NetworkPrinterInfo> _dedupeNetworkPrinters(
  List<NetworkPrinterInfo> printers,
) {
  final byDevice = <String, NetworkPrinterInfo>{};
  for (final printer in printers) {
    final key = _networkPrinterDeviceKey(printer);
    final existing = byDevice[key];
    if (existing == null ||
        _networkPrinterPreference(printer) >
            _networkPrinterPreference(existing)) {
      byDevice[key] = printer;
    }
  }
  return byDevice.values.toList();
}

String _networkPrinterDeviceKey(NetworkPrinterInfo printer) {
  final host = printer.host.trim().toLowerCase();
  if (host.isNotEmpty) {
    return 'host:$host';
  }
  final hostname = printer.hostname.trim().toLowerCase();
  if (hostname.isNotEmpty) {
    return 'hostname:$hostname';
  }
  final serviceName = printer.serviceName.trim().toLowerCase();
  if (serviceName.isNotEmpty) {
    return 'service:$serviceName';
  }
  return 'fullname:${printer.fullname.trim().toLowerCase()}';
}

int _networkPrinterPreference(NetworkPrinterInfo printer) {
  final serviceType = printer.serviceType.toLowerCase();
  var score = 0;
  if (printer.supportsRawTcp) {
    score += 1000;
  }
  if (printer.port == 9100) {
    score += 200;
  }
  if (serviceType.contains('_pdl-datastream._tcp')) {
    score += 100;
  } else if (serviceType.contains('_printer._tcp')) {
    score += 30;
  } else if (serviceType.contains('_ipps._tcp')) {
    score += 20;
  } else if (serviceType.contains('_ipp._tcp')) {
    score += 10;
  }
  return score;
}

/// 通过 mDNS/DNS-SD 扫描局域网/WiFi 中可见的网络打印服务。
///
/// 默认服务类型包含 `_pdl-datastream._tcp.local.`、`_printer._tcp.local.`、
/// `_ipp._tcp.local.` 和 `_ipps._tcp.local.`。Android 走原生 `NsdManager`，
/// 其它桌面平台走 Rust/FRB 后台任务，调用 Dart Future 不会阻塞 Flutter UI
/// isolate。
Future<NetworkPrinterDiscoveryResult> discoverNetworkPrinters({
  Duration timeout = const Duration(seconds: 3),
  List<String> serviceTypes = const [],
}) async {
  return _networkDiscoveryQueue.enqueue(
    () => _discoverNetworkPrintersNow(
      timeout: timeout,
      serviceTypes: serviceTypes,
    ),
  );
}

Future<NetworkPrinterDiscoveryResult> _discoverNetworkPrintersNow({
  required Duration timeout,
  required List<String> serviceTypes,
}) async {
  if (defaultTargetPlatform == TargetPlatform.android) {
    final response = await _platformChannel.invokeMethod<String>(
      'discoverNetworkPrinters',
      {'timeoutMs': timeout.inMilliseconds, 'serviceTypes': serviceTypes},
    );
    return _decodeNetworkPrinterDiscoveryResponse(response);
  }

  await initKservicePrinter();
  final response = await rust_printer.discoverNetworkPrinters(
    timeoutMs: timeout.inMilliseconds,
    serviceTypes: serviceTypes,
  );
  return _decodeNetworkPrinterDiscoveryResponse(response);
}

/// 当前排队或执行中的网络发现任务数量。
int get activeNetworkDiscoveryCount => _networkDiscoveryQueue.activeTaskCount;

NetworkPrinterDiscoveryResult _decodeNetworkPrinterDiscoveryResponse(
  String? response,
) {
  if (response == null || response.isEmpty) {
    throw StateError('网络打印机扫描未返回结果');
  }
  final json = jsonDecode(response) as Map<String, dynamic>;
  if (json['ok'] != true) {
    throw StateError(json['error']?.toString() ?? '网络打印机扫描失败');
  }
  final result = json['result'];
  if (result is! Map<String, dynamic>) {
    return const NetworkPrinterDiscoveryResult(
      printers: [],
      serviceTypes: [],
      timeoutMs: 0,
      durationMs: 0,
      timedOut: false,
    );
  }
  return NetworkPrinterDiscoveryResult.fromJson(result);
}

/// 只返回扫描到的网络打印设备列表。
Future<List<NetworkPrinterInfo>> listNetworkPrinters({
  Duration timeout = const Duration(seconds: 3),
  List<String> serviceTypes = const [],
}) async {
  return (await discoverNetworkPrinters(
    timeout: timeout,
    serviceTypes: serviceTypes,
  )).printers;
}

List<String> _stringList(Object? value) {
  if (value is! List) {
    return const [];
  }
  return [for (final item in value) item.toString()];
}

String? _nullableString(Object? value) {
  final text = value?.toString();
  if (text == null || text.isEmpty) {
    return null;
  }
  return text;
}

Map<int, int> _intMap(Object? value) {
  if (value is! Map) {
    return const {};
  }
  return {
    for (final entry in value.entries)
      if (int.tryParse(entry.key.toString()) != null && entry.value is num)
        int.parse(entry.key.toString()): (entry.value as num).toInt(),
  };
}

Map<String, String> _stringMap(Object? value) {
  if (value is! Map) {
    return const {};
  }
  return {
    for (final entry in value.entries)
      entry.key.toString(): entry.value?.toString() ?? '',
  };
}

final _networkDiscoveryQueue = _SerialTaskQueue();
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

/// 打开连接在打印机上的钱箱。
///
/// 大多数 ESC/POS 设备使用 [CashDrawerPin.pin2]；少数设备接在 pin5。
/// 默认会按打印机连接自动排队，避免和正在进行的打印任务并发写入。
Future<PrintResult> openCashDrawer(
  PrinterConnection connection, {
  CashDrawerPin pin = CashDrawerPin.pin2,
  Duration on = const Duration(milliseconds: 200),
  Duration off = const Duration(milliseconds: 200),
  bool queued = true,
}) {
  if (!queued) {
    return openCashDrawerNow(connection, pin: pin, on: on, off: off);
  }
  return _printQueue.enqueue(
    connection.queueKey,
    () => openCashDrawerNow(connection, pin: pin, on: on, off: off),
  );
}

/// 立即发送开钱箱脉冲，不经过 Dart 队列。
Future<PrintResult> openCashDrawerNow(
  PrinterConnection connection, {
  CashDrawerPin pin = CashDrawerPin.pin2,
  Duration on = const Duration(milliseconds: 200),
  Duration off = const Duration(milliseconds: 200),
}) async {
  await initKservicePrinter();
  final response = await rust_printer.openCashDrawer(
    connection: connection,
    pin: pin.code,
    onMs: _cashDrawerDurationMs(on),
    offMs: _cashDrawerDurationMs(off),
  );
  return PrintResult.fromJson(jsonDecode(response) as Map<String, dynamic>);
}

int _cashDrawerDurationMs(Duration duration) =>
    duration.inMilliseconds.clamp(0, 510).toInt();

/// 查询 ESC/POS 打印机实时状态。
///
/// 默认进入同一台打印机的串行队列，避免状态指令和打印数据交叉。
Future<PrinterStatus> queryPrinterStatus(
  PrinterConnection connection, {
  Duration timeout = const Duration(seconds: 2),
  bool queued = true,
}) {
  if (!queued) {
    return queryPrinterStatusNow(connection, timeout: timeout);
  }
  return _printQueue.enqueue(
    connection.queueKey,
    () => queryPrinterStatusNow(connection, timeout: timeout),
  );
}

/// 立即查询打印机状态，不经过 Dart 队列。
Future<PrinterStatus> queryPrinterStatusNow(
  PrinterConnection connection, {
  Duration timeout = const Duration(seconds: 2),
}) async {
  await initKservicePrinter();
  final response = await rust_printer.queryPrinterStatus(
    connection: connection,
    timeoutMs: _queryTimeoutMs(timeout),
  );
  return PrinterStatus.fromJson(jsonDecode(response) as Map<String, dynamic>);
}

/// 获取打印机身份信息，包含常见 ESC/POS 设备的厂商、型号、序列号和固件版本。
Future<PrinterIdentity> getPrinterIdentity(
  PrinterConnection connection, {
  Duration timeout = const Duration(seconds: 2),
  bool queued = true,
}) {
  if (!queued) {
    return getPrinterIdentityNow(connection, timeout: timeout);
  }
  return _printQueue.enqueue(
    connection.queueKey,
    () => getPrinterIdentityNow(connection, timeout: timeout),
  );
}

/// 立即获取打印机身份信息，不经过 Dart 队列。
Future<PrinterIdentity> getPrinterIdentityNow(
  PrinterConnection connection, {
  Duration timeout = const Duration(seconds: 2),
}) async {
  await initKservicePrinter();
  final response = await rust_printer.getPrinterIdentity(
    connection: connection,
    timeoutMs: _queryTimeoutMs(timeout),
  );
  return PrinterIdentity.fromJson(jsonDecode(response) as Map<String, dynamic>);
}

/// 只取打印机序列号。
Future<String?> getPrinterSerialNumber(
  PrinterConnection connection, {
  Duration timeout = const Duration(seconds: 2),
  bool queued = true,
}) async {
  final identity = await getPrinterIdentity(
    connection,
    timeout: timeout,
    queued: queued,
  );
  return identity.serial;
}

int _queryTimeoutMs(Duration timeout) =>
    timeout.inMilliseconds.clamp(100, 30000).toInt();

/// 对同一打印任务做并发压测。
///
/// [queued] 为 true 时，同一台打印机最终仍会串行写入；压测会模拟多个业务请求
/// 同时进来，适合验证队列稳定性。设置为 false 会直接并发写设备，通常只用于排查。
Future<PrinterStressTestResult> runPrinterStressTest({
  required PrintJob job,
  int count = 20,
  int concurrency = 4,
  bool queued = true,
  Duration delay = Duration.zero,
}) async {
  final total = count.clamp(1, 10000).toInt();
  final workerCount = concurrency.clamp(1, total).toInt();
  final stopwatch = Stopwatch()..start();
  final results = <PrinterStressTestJobResult>[];
  var nextIndex = 0;
  var inFlight = 0;
  var maxInFlight = 0;

  Future<void> worker() async {
    while (true) {
      final index = nextIndex;
      nextIndex += 1;
      if (index >= total) {
        return;
      }

      inFlight += 1;
      maxInFlight = math.max(maxInFlight, inFlight);
      final jobStopwatch = Stopwatch()..start();
      try {
        final result = await printReceipt(job, queued: queued);
        jobStopwatch.stop();
        results.add(
          PrinterStressTestJobResult(
            index: index,
            ok: result.ok,
            bytes: result.bytes,
            durationMs: jobStopwatch.elapsedMilliseconds,
            error: result.error,
          ),
        );
      } catch (error) {
        jobStopwatch.stop();
        results.add(
          PrinterStressTestJobResult(
            index: index,
            ok: false,
            bytes: 0,
            durationMs: jobStopwatch.elapsedMilliseconds,
            error: error.toString(),
          ),
        );
      } finally {
        inFlight -= 1;
      }

      if (delay.inMicroseconds > 0 && nextIndex < total) {
        await Future<void>.delayed(delay);
      }
    }
  }

  await Future.wait([for (var i = 0; i < workerCount; i++) worker()]);
  stopwatch.stop();
  results.sort((a, b) => a.index.compareTo(b.index));

  final success = results.where((result) => result.ok).length;
  final totalBytes = results.fold<int>(0, (sum, result) => sum + result.bytes);
  return PrinterStressTestResult(
    total: total,
    success: success,
    failure: total - success,
    concurrency: workerCount,
    queued: queued,
    durationMs: stopwatch.elapsedMilliseconds,
    maxInFlight: maxInFlight,
    totalBytes: totalBytes,
    jobs: List.unmodifiable(results),
  );
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

/// 按任务类型打印。兼容 ESC/POS 小票模板和 TSPL 标签模板。
Future<PrintResult> printJob(PrintJob job, {bool queued = true}) =>
    printReceipt(job, queued: queued);

/// 根据租户、门店、票据类型和标签筛选可用打印机绑定。
List<PrinterBinding> resolvePrinterBindings({
  required Iterable<PrinterBinding> bindings,
  required PrintJobType type,
  Iterable<String> tags = const <String>[],
  String? tenantId,
  String? storeId,
}) {
  return [
    for (final binding in bindings)
      if (binding.matches(
        type: type,
        tags: tags,
        tenantId: tenantId,
        storeId: storeId,
      ))
        binding,
  ];
}

/// 将同一份业务数据按打印机绑定分发到多台打印机。
///
/// 返回值按匹配到的绑定顺序排列。单台打印机失败会变成对应的 [PrintDispatchResult]，
/// 不会阻止其它打印机继续打印。
Future<List<PrintDispatchResult>> dispatchPrintJobs({
  required Iterable<PrinterBinding> bindings,
  required PrintJobType type,
  required Map<String, Object?> data,
  Iterable<String> tags = const <String>[],
  String? tenantId,
  String? storeId,
  ReceiptTemplate? fallbackTemplate,
  bool queued = true,
  bool requireTargets = true,
}) async {
  final targets = resolvePrinterBindings(
    bindings: bindings,
    type: type,
    tags: tags,
    tenantId: tenantId,
    storeId: storeId,
  );
  if (targets.isEmpty) {
    if (requireTargets) {
      throw StateError('没有匹配的打印机绑定');
    }
    return const [];
  }

  return Future.wait([
    for (final binding in targets)
      _dispatchPrintJob(
        binding: binding,
        type: type,
        data: data,
        fallbackTemplate: fallbackTemplate,
        queued: queued,
      ),
  ]);
}

Future<PrintDispatchResult> _dispatchPrintJob({
  required PrinterBinding binding,
  required PrintJobType type,
  required Map<String, Object?> data,
  required ReceiptTemplate? fallbackTemplate,
  required bool queued,
}) async {
  final job = binding.buildJob(
    type: type,
    data: data,
    fallbackTemplate: fallbackTemplate,
  );
  try {
    return PrintDispatchResult(
      binding: binding,
      job: job,
      result: await printJob(job, queued: queued),
    );
  } catch (error) {
    return PrintDispatchResult(
      binding: binding,
      job: job,
      result: PrintResult(ok: false, error: error.toString()),
    );
  }
}

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

class _SerialTaskQueue {
  Future<void> _tail = Future<void>.value();
  int _activeTaskCount = 0;

  int get activeTaskCount => _activeTaskCount;

  Future<T> enqueue<T>(FutureOr<T> Function() task) {
    final completer = Completer<T>();
    _activeTaskCount += 1;

    final next = _tail.catchError((_) {}).then((_) async {
      try {
        completer.complete(await task());
      } catch (error, stackTrace) {
        completer.completeError(error, stackTrace);
      }
    });

    _tail = next.whenComplete(() {
      _activeTaskCount -= 1;
    });

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
  double? labelHeightMm,
  double? labelGapMm,
  int? labelDensity,
  int? labelSpeed,
  bool? labelHomeBeforePrint,
}) {
  final templateWidth = width ?? paperSize?.width;
  if (type == PrintJobType.label && mode == ReceiptPrintMode.zplImage) {
    return defaultZplLabelImageTemplate(
      width: templateWidth ?? ReceiptPaperSize.mm58.width,
      widthMm: _labelWidthMmForPaper(paperSize),
      heightMm: labelHeightMm ?? 40,
      gapMm: labelGapMm ?? 2,
      density: labelDensity ?? 8,
      speed: labelSpeed ?? 4,
      homeBeforePrint: labelHomeBeforePrint ?? true,
      fontFamily: fontFamily,
      fontSize: fontSize,
    );
  }

  if (type == PrintJobType.label && mode == ReceiptPrintMode.zpl) {
    return defaultZplLabelTemplate(
      width: templateWidth ?? ReceiptPaperSize.mm58.width,
      widthMm: _labelWidthMmForPaper(paperSize),
      heightMm: labelHeightMm ?? 40,
      gapMm: labelGapMm ?? 2,
      density: labelDensity ?? 8,
      speed: labelSpeed ?? 4,
      homeBeforePrint: labelHomeBeforePrint ?? true,
    );
  }

  if (type == PrintJobType.label &&
      (mode == ReceiptPrintMode.tsplImage ||
          mode == ReceiptPrintMode.tsplRaster)) {
    final template = defaultTsplLabelImageTemplate(
      width: templateWidth ?? ReceiptPaperSize.mm58.width,
      widthMm: _labelWidthMmForPaper(paperSize),
      heightMm: labelHeightMm ?? 40,
      gapMm: labelGapMm ?? 2,
      density: labelDensity ?? 8,
      speed: labelSpeed ?? 4,
      homeBeforePrint: labelHomeBeforePrint ?? true,
      fontFamily: fontFamily,
      fontSize: fontSize,
    );
    return mode == ReceiptPrintMode.tsplRaster
        ? template.copyWith(encoding: ReceiptPrintMode.tsplRaster.encoding)
        : template;
  }

  if (type == PrintJobType.label && mode == ReceiptPrintMode.tspl) {
    return defaultTsplLabelTemplate(
      width: templateWidth ?? ReceiptPaperSize.mm58.width,
      widthMm: _labelWidthMmForPaper(paperSize),
      heightMm: labelHeightMm ?? 40,
      gapMm: labelGapMm ?? 2,
      density: labelDensity ?? 8,
      speed: labelSpeed ?? 4,
      homeBeforePrint: labelHomeBeforePrint ?? true,
    );
  }

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

double _labelWidthMmForPaper(ReceiptPaperSize? paperSize) {
  return switch (paperSize) {
    ReceiptPaperSize.mm80 => 80,
    ReceiptPaperSize.mm58 || null => 58,
  };
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
          _receiptNoteElement(label: '备注', value: 'remark', width: width),
        ],
      },
      {'type': 'divider'},
      {'type': 'row', 'left': '合计', 'right': '{{order.total}}', 'bold': true},
      {'type': 'feed', 'lines': 3},
      {'type': 'cut'},
    ],
  );
}

Map<String, Object?> _receiptNoteElement({
  required String label,
  required String value,
  required int width,
  bool bold = false,
  int indent = 2,
}) {
  final prefix = '${' ' * indent}$label：';
  final prefixWidth = _receiptTextWidth(prefix);
  if (width <= prefixWidth) {
    return {
      'type': 'columns',
      'columns': [
        {
          'value': '{{#if $value}}$prefix{{$value}}{{/if}}',
          'width': width,
          if (bold) 'bold': true,
        },
      ],
    };
  }
  return {
    'type': 'columns',
    'columns': [
      {
        'value': '{{#if $value}}$prefix{{/if}}',
        'width': prefixWidth,
        if (bold) 'bold': true,
      },
      {
        'value': '{{#if $value}}{{$value}}{{/if}}',
        'width': width - prefixWidth,
        if (bold) 'bold': true,
      },
    ],
  };
}

int _receiptTextWidth(String value) {
  var width = 0;
  for (final rune in value.runes) {
    width += rune <= 0x7f ? 1 : 2;
  }
  return width;
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
      labelWidthMm: labelWidthMm,
      labelHeightMm: labelHeightMm,
      labelGapMm: labelGapMm,
      labelDensity: labelDensity,
      labelSpeed: labelSpeed,
      labelHomeBeforePrint: labelHomeBeforePrint,
      labelReferenceX: labelReferenceX,
      labelReferenceY: labelReferenceY,
      labelShiftDots: labelShiftDots,
      elements: elements,
    );
  }

  /// 将现有模板改成 TSPL 标签语言输出。
  ReceiptTemplate asTsplLabelTemplate({
    double widthMm = 58,
    double heightMm = 40,
    double gapMm = 2,
    int density = 8,
    int speed = 4,
    bool homeBeforePrint = true,
    int? referenceX,
    int? referenceY,
    int? shiftDots,
  }) {
    return ReceiptTemplate(
      width: width,
      encoding: ReceiptPrintMode.tspl.encoding,
      fontFamily: fontFamily,
      fontSize: fontSize,
      labelWidthMm: widthMm,
      labelHeightMm: heightMm,
      labelGapMm: gapMm,
      labelDensity: density,
      labelSpeed: speed,
      labelHomeBeforePrint: homeBeforePrint,
      labelReferenceX: referenceX,
      labelReferenceY: referenceY,
      labelShiftDots: shiftDots,
      elements: elements,
    );
  }

  /// 将现有模板改成 ZPL 标签语言输出。
  ReceiptTemplate asZplLabelTemplate({
    double widthMm = 58,
    double heightMm = 40,
    double gapMm = 2,
    int density = 8,
    int speed = 4,
    bool homeBeforePrint = true,
    int? referenceX,
    int? referenceY,
    int? shiftDots,
  }) {
    return ReceiptTemplate(
      width: width,
      encoding: ReceiptPrintMode.zpl.encoding,
      fontFamily: fontFamily,
      fontSize: fontSize,
      labelWidthMm: widthMm,
      labelHeightMm: heightMm,
      labelGapMm: gapMm,
      labelDensity: density,
      labelSpeed: speed,
      labelHomeBeforePrint: homeBeforePrint,
      labelReferenceX: referenceX,
      labelReferenceY: referenceY,
      labelShiftDots: shiftDots,
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
          _receiptNoteElement(label: '规格', value: 'spec', width: width),
          _receiptNoteElement(
            label: '备注',
            value: 'remark',
            width: width,
            bold: true,
          ),
          {'type': 'divider'},
        ],
      },
      _receiptNoteElement(
        label: '整单备注',
        value: 'order.remark',
        width: width,
        bold: true,
        indent: 0,
      ),
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

/// TSPL 标签打印模板，适合 TSC 兼容标签机。
///
/// 这类设备与传统 ESC/POS 小票机不同，通常不会响应小票初始化、走纸和切刀指令。
ReceiptTemplate defaultTsplLabelTemplate({
  int width = 32,
  double widthMm = 58,
  double heightMm = 40,
  double gapMm = 2,
  int density = 8,
  int speed = 4,
  bool homeBeforePrint = true,
  int? referenceX,
  int? referenceY,
  int? shiftDots,
}) {
  const dotsPerMm = 8;
  const qrSize = 2;
  final labelWidthDots = (widthMm * dotsPerMm).round();
  final labelHeightDots = (heightMm * dotsPerMm).round();
  final qrX = math.max(24, labelWidthDots - 160);
  final qrY = math.max(24, labelHeightDots - 128);

  return ReceiptTemplate(
    width: width,
    encoding: ReceiptPrintMode.tspl.encoding,
    labelWidthMm: widthMm,
    labelHeightMm: heightMm,
    labelGapMm: gapMm,
    labelDensity: density,
    labelSpeed: speed,
    labelHomeBeforePrint: homeBeforePrint,
    labelReferenceX: referenceX,
    labelReferenceY: referenceY,
    labelShiftDots: shiftDots,
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
      {
        'type': 'qrcode',
        'value': '{{item.sku}}',
        'size': qrSize,
        'x': qrX,
        'y': qrY,
      },
    ],
  );
}

/// TSPL 图片标签模板。
///
/// 适合维吾尔语、阿拉伯语等需要字体 fallback、连写和方向处理的标签。
ReceiptTemplate defaultTsplLabelImageTemplate({
  int width = 32,
  double widthMm = 58,
  double heightMm = 40,
  double gapMm = 2,
  int density = 8,
  int speed = 4,
  String? fontFamily,
  double? fontSize,
  bool homeBeforePrint = true,
  int? referenceX,
  int? referenceY,
  int? shiftDots,
}) {
  final template = defaultTsplLabelTemplate(
    width: width,
    widthMm: widthMm,
    heightMm: heightMm,
    gapMm: gapMm,
    density: density,
    speed: speed,
    homeBeforePrint: homeBeforePrint,
    referenceX: referenceX,
    referenceY: referenceY,
    shiftDots: shiftDots,
  );
  return ReceiptTemplate(
    width: template.width,
    encoding: ReceiptPrintMode.tsplImage.encoding,
    fontFamily: fontFamily,
    fontSize: fontSize,
    labelWidthMm: template.labelWidthMm,
    labelHeightMm: template.labelHeightMm,
    labelGapMm: template.labelGapMm,
    labelDensity: template.labelDensity,
    labelSpeed: template.labelSpeed,
    labelHomeBeforePrint: template.labelHomeBeforePrint,
    labelReferenceX: template.labelReferenceX,
    labelReferenceY: template.labelReferenceY,
    labelShiftDots: template.labelShiftDots,
    elements: template.elements,
  );
}

/// ZPL 标签打印模板，适合 Zebra 兼容标签机。
ReceiptTemplate defaultZplLabelTemplate({
  int width = 32,
  double widthMm = 58,
  double heightMm = 40,
  double gapMm = 2,
  int density = 8,
  int speed = 4,
  bool homeBeforePrint = true,
  int? referenceX,
  int? referenceY,
  int? shiftDots,
}) {
  return defaultTsplLabelTemplate(
    width: width,
    widthMm: widthMm,
    heightMm: heightMm,
    gapMm: gapMm,
    density: density,
    speed: speed,
    homeBeforePrint: homeBeforePrint,
    referenceX: referenceX,
    referenceY: referenceY,
    shiftDots: shiftDots,
  ).copyWith(encoding: ReceiptPrintMode.zpl.encoding);
}

/// ZPL 图片标签模板。
ReceiptTemplate defaultZplLabelImageTemplate({
  int width = 32,
  double widthMm = 58,
  double heightMm = 40,
  double gapMm = 2,
  int density = 8,
  int speed = 4,
  String? fontFamily,
  double? fontSize,
  bool homeBeforePrint = true,
  int? referenceX,
  int? referenceY,
  int? shiftDots,
}) {
  return defaultTsplLabelImageTemplate(
    width: width,
    widthMm: widthMm,
    heightMm: heightMm,
    gapMm: gapMm,
    density: density,
    speed: speed,
    fontFamily: fontFamily,
    fontSize: fontSize,
    homeBeforePrint: homeBeforePrint,
    referenceX: referenceX,
    referenceY: referenceY,
    shiftDots: shiftDots,
  ).copyWith(encoding: ReceiptPrintMode.zplImage.encoding);
}
