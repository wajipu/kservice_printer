// GENERATED CODE - DO NOT MODIFY BY HAND
// coverage:ignore-file
// ignore_for_file: type=lint
// ignore_for_file: unused_element, deprecated_member_use, deprecated_member_use_from_same_package, use_function_type_syntax_for_parameters, unnecessary_const, avoid_init_to_null, invalid_override_different_default_values_named, prefer_expression_function_bodies, annotate_overrides, invalid_annotation_target, unnecessary_question_mark

part of 'printer.dart';

// **************************************************************************
// FreezedGenerator
// **************************************************************************

// dart format off
T _$identity<T>(T value) => value;
/// @nodoc
mixin _$PrinterConnection {





@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is PrinterConnection);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'PrinterConnection()';
}


}

/// @nodoc
class $PrinterConnectionCopyWith<$Res>  {
$PrinterConnectionCopyWith(PrinterConnection _, $Res Function(PrinterConnection) __);
}


/// Adds pattern-matching-related methods to [PrinterConnection].
extension PrinterConnectionPatterns on PrinterConnection {
/// A variant of `map` that fallback to returning `orElse`.
///
/// It is equivalent to doing:
/// ```dart
/// switch (sealedClass) {
///   case final Subclass value:
///     return ...;
///   case _:
///     return orElse();
/// }
/// ```

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( PrinterConnection_Network value)?  network,TResult Function( PrinterConnection_Usb value)?  usb,TResult Function( PrinterConnection_Serial value)?  serial,required TResult orElse(),}){
final _that = this;
switch (_that) {
case PrinterConnection_Network() when network != null:
return network(_that);case PrinterConnection_Usb() when usb != null:
return usb(_that);case PrinterConnection_Serial() when serial != null:
return serial(_that);case _:
  return orElse();

}
}
/// A `switch`-like method, using callbacks.
///
/// Callbacks receives the raw object, upcasted.
/// It is equivalent to doing:
/// ```dart
/// switch (sealedClass) {
///   case final Subclass value:
///     return ...;
///   case final Subclass2 value:
///     return ...;
/// }
/// ```

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( PrinterConnection_Network value)  network,required TResult Function( PrinterConnection_Usb value)  usb,required TResult Function( PrinterConnection_Serial value)  serial,}){
final _that = this;
switch (_that) {
case PrinterConnection_Network():
return network(_that);case PrinterConnection_Usb():
return usb(_that);case PrinterConnection_Serial():
return serial(_that);}
}
/// A variant of `map` that fallback to returning `null`.
///
/// It is equivalent to doing:
/// ```dart
/// switch (sealedClass) {
///   case final Subclass value:
///     return ...;
///   case _:
///     return null;
/// }
/// ```

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( PrinterConnection_Network value)?  network,TResult? Function( PrinterConnection_Usb value)?  usb,TResult? Function( PrinterConnection_Serial value)?  serial,}){
final _that = this;
switch (_that) {
case PrinterConnection_Network() when network != null:
return network(_that);case PrinterConnection_Usb() when usb != null:
return usb(_that);case PrinterConnection_Serial() when serial != null:
return serial(_that);case _:
  return null;

}
}
/// A variant of `when` that fallback to an `orElse` callback.
///
/// It is equivalent to doing:
/// ```dart
/// switch (sealedClass) {
///   case Subclass(:final field):
///     return ...;
///   case _:
///     return orElse();
/// }
/// ```

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function( String host,  int port,  int timeoutMs)?  network,TResult Function( int vendorId,  int productId,  String? deviceName)?  usb,TResult Function( String port,  int baudRate)?  serial,required TResult orElse(),}) {final _that = this;
switch (_that) {
case PrinterConnection_Network() when network != null:
return network(_that.host,_that.port,_that.timeoutMs);case PrinterConnection_Usb() when usb != null:
return usb(_that.vendorId,_that.productId,_that.deviceName);case PrinterConnection_Serial() when serial != null:
return serial(_that.port,_that.baudRate);case _:
  return orElse();

}
}
/// A `switch`-like method, using callbacks.
///
/// As opposed to `map`, this offers destructuring.
/// It is equivalent to doing:
/// ```dart
/// switch (sealedClass) {
///   case Subclass(:final field):
///     return ...;
///   case Subclass2(:final field2):
///     return ...;
/// }
/// ```

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function( String host,  int port,  int timeoutMs)  network,required TResult Function( int vendorId,  int productId,  String? deviceName)  usb,required TResult Function( String port,  int baudRate)  serial,}) {final _that = this;
switch (_that) {
case PrinterConnection_Network():
return network(_that.host,_that.port,_that.timeoutMs);case PrinterConnection_Usb():
return usb(_that.vendorId,_that.productId,_that.deviceName);case PrinterConnection_Serial():
return serial(_that.port,_that.baudRate);}
}
/// A variant of `when` that fallback to returning `null`
///
/// It is equivalent to doing:
/// ```dart
/// switch (sealedClass) {
///   case Subclass(:final field):
///     return ...;
///   case _:
///     return null;
/// }
/// ```

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function( String host,  int port,  int timeoutMs)?  network,TResult? Function( int vendorId,  int productId,  String? deviceName)?  usb,TResult? Function( String port,  int baudRate)?  serial,}) {final _that = this;
switch (_that) {
case PrinterConnection_Network() when network != null:
return network(_that.host,_that.port,_that.timeoutMs);case PrinterConnection_Usb() when usb != null:
return usb(_that.vendorId,_that.productId,_that.deviceName);case PrinterConnection_Serial() when serial != null:
return serial(_that.port,_that.baudRate);case _:
  return null;

}
}

}

/// @nodoc


class PrinterConnection_Network extends PrinterConnection {
  const PrinterConnection_Network({required this.host, required this.port, required this.timeoutMs}): super._();
  

 final  String host;
 final  int port;
 final  int timeoutMs;

/// Create a copy of PrinterConnection
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$PrinterConnection_NetworkCopyWith<PrinterConnection_Network> get copyWith => _$PrinterConnection_NetworkCopyWithImpl<PrinterConnection_Network>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is PrinterConnection_Network&&(identical(other.host, host) || other.host == host)&&(identical(other.port, port) || other.port == port)&&(identical(other.timeoutMs, timeoutMs) || other.timeoutMs == timeoutMs));
}


@override
int get hashCode => Object.hash(runtimeType,host,port,timeoutMs);

@override
String toString() {
  return 'PrinterConnection.network(host: $host, port: $port, timeoutMs: $timeoutMs)';
}


}

/// @nodoc
abstract mixin class $PrinterConnection_NetworkCopyWith<$Res> implements $PrinterConnectionCopyWith<$Res> {
  factory $PrinterConnection_NetworkCopyWith(PrinterConnection_Network value, $Res Function(PrinterConnection_Network) _then) = _$PrinterConnection_NetworkCopyWithImpl;
@useResult
$Res call({
 String host, int port, int timeoutMs
});




}
/// @nodoc
class _$PrinterConnection_NetworkCopyWithImpl<$Res>
    implements $PrinterConnection_NetworkCopyWith<$Res> {
  _$PrinterConnection_NetworkCopyWithImpl(this._self, this._then);

  final PrinterConnection_Network _self;
  final $Res Function(PrinterConnection_Network) _then;

/// Create a copy of PrinterConnection
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? host = null,Object? port = null,Object? timeoutMs = null,}) {
  return _then(PrinterConnection_Network(
host: null == host ? _self.host : host // ignore: cast_nullable_to_non_nullable
as String,port: null == port ? _self.port : port // ignore: cast_nullable_to_non_nullable
as int,timeoutMs: null == timeoutMs ? _self.timeoutMs : timeoutMs // ignore: cast_nullable_to_non_nullable
as int,
  ));
}


}

/// @nodoc


class PrinterConnection_Usb extends PrinterConnection {
  const PrinterConnection_Usb({required this.vendorId, required this.productId, this.deviceName}): super._();
  

 final  int vendorId;
 final  int productId;
 final  String? deviceName;

/// Create a copy of PrinterConnection
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$PrinterConnection_UsbCopyWith<PrinterConnection_Usb> get copyWith => _$PrinterConnection_UsbCopyWithImpl<PrinterConnection_Usb>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is PrinterConnection_Usb&&(identical(other.vendorId, vendorId) || other.vendorId == vendorId)&&(identical(other.productId, productId) || other.productId == productId)&&(identical(other.deviceName, deviceName) || other.deviceName == deviceName));
}


@override
int get hashCode => Object.hash(runtimeType,vendorId,productId,deviceName);

@override
String toString() {
  return 'PrinterConnection.usb(vendorId: $vendorId, productId: $productId, deviceName: $deviceName)';
}


}

/// @nodoc
abstract mixin class $PrinterConnection_UsbCopyWith<$Res> implements $PrinterConnectionCopyWith<$Res> {
  factory $PrinterConnection_UsbCopyWith(PrinterConnection_Usb value, $Res Function(PrinterConnection_Usb) _then) = _$PrinterConnection_UsbCopyWithImpl;
@useResult
$Res call({
 int vendorId, int productId, String? deviceName
});




}
/// @nodoc
class _$PrinterConnection_UsbCopyWithImpl<$Res>
    implements $PrinterConnection_UsbCopyWith<$Res> {
  _$PrinterConnection_UsbCopyWithImpl(this._self, this._then);

  final PrinterConnection_Usb _self;
  final $Res Function(PrinterConnection_Usb) _then;

/// Create a copy of PrinterConnection
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? vendorId = null,Object? productId = null,Object? deviceName = freezed,}) {
  return _then(PrinterConnection_Usb(
vendorId: null == vendorId ? _self.vendorId : vendorId // ignore: cast_nullable_to_non_nullable
as int,productId: null == productId ? _self.productId : productId // ignore: cast_nullable_to_non_nullable
as int,deviceName: freezed == deviceName ? _self.deviceName : deviceName // ignore: cast_nullable_to_non_nullable
as String?,
  ));
}


}

/// @nodoc


class PrinterConnection_Serial extends PrinterConnection {
  const PrinterConnection_Serial({required this.port, required this.baudRate}): super._();
  

 final  String port;
 final  int baudRate;

/// Create a copy of PrinterConnection
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$PrinterConnection_SerialCopyWith<PrinterConnection_Serial> get copyWith => _$PrinterConnection_SerialCopyWithImpl<PrinterConnection_Serial>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is PrinterConnection_Serial&&(identical(other.port, port) || other.port == port)&&(identical(other.baudRate, baudRate) || other.baudRate == baudRate));
}


@override
int get hashCode => Object.hash(runtimeType,port,baudRate);

@override
String toString() {
  return 'PrinterConnection.serial(port: $port, baudRate: $baudRate)';
}


}

/// @nodoc
abstract mixin class $PrinterConnection_SerialCopyWith<$Res> implements $PrinterConnectionCopyWith<$Res> {
  factory $PrinterConnection_SerialCopyWith(PrinterConnection_Serial value, $Res Function(PrinterConnection_Serial) _then) = _$PrinterConnection_SerialCopyWithImpl;
@useResult
$Res call({
 String port, int baudRate
});




}
/// @nodoc
class _$PrinterConnection_SerialCopyWithImpl<$Res>
    implements $PrinterConnection_SerialCopyWith<$Res> {
  _$PrinterConnection_SerialCopyWithImpl(this._self, this._then);

  final PrinterConnection_Serial _self;
  final $Res Function(PrinterConnection_Serial) _then;

/// Create a copy of PrinterConnection
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? port = null,Object? baudRate = null,}) {
  return _then(PrinterConnection_Serial(
port: null == port ? _self.port : port // ignore: cast_nullable_to_non_nullable
as String,baudRate: null == baudRate ? _self.baudRate : baudRate // ignore: cast_nullable_to_non_nullable
as int,
  ));
}


}

// dart format on
