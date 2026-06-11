Pod::Spec.new do |s|
  s.name             = 'kservice_printer'
  s.version          = '0.0.1'
  s.summary          = 'Flutter FFI plugin for kservice printer.'
  s.description      = <<-DESC
Flutter FFI plugin for kservice printer.
                       DESC
  s.homepage         = 'https://example.com'
  s.license          = { :file => '../LICENSE' }
  s.author           = { 'kservice_printer' => 'dev@example.com' }
  s.source           = { :path => '.' }
  s.source_files     = 'Classes/**/*'

  s.dependency 'FlutterMacOS'
  s.platform = :osx, '10.15'
  s.frameworks = 'CoreFoundation', 'IOKit', 'Security'

  s.script_phase = {
    :name => 'Build kservice_printer_core',
    :script => 'sh "$PODS_TARGET_SRCROOT/../cargokit/build_pod.sh" ../rust_printer_core',
    :execution_position => :before_compile,
    :input_files => ['${BUILT_PRODUCTS_DIR}/cargokit_phony'],
    :output_files => [
      '${BUILT_PRODUCTS_DIR}/cargokit_phony_out',
      '${PODS_CONFIGURATION_BUILD_DIR}/${PRODUCT_NAME}/libkservice_printer_core.a',
    ],
  }

  s.pod_target_xcconfig = {
    'DEFINES_MODULE' => 'YES',
    'OTHER_LDFLAGS' => '$(inherited) -force_load $(PODS_CONFIGURATION_BUILD_DIR)/$(PRODUCT_NAME)/libkservice_printer_core.a',
  }
  s.swift_version = '5.0'
end
