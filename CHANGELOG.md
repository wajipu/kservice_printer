## 0.0.8

* Add Rust-worker PNG Base64 rendering for short-lived image print pipelines.
* Add explicit app font-file registration shared by receipt and TSPL/ZPL image renderers.
* Reuse the configured font system instead of relying only on Android system fonts.

## 0.0.7

* Add native Android USB bulk printing after `UsbManager` permission is granted.
* Add Android USB cash drawer, ESC/POS status, identity, and serial queries.
* Preserve Android USB device paths to distinguish printers with identical VID/PID values.
* Add stable `errorCode` fields while retaining existing user-readable error messages.
* Add pub.dev repository, issue tracker, and topic metadata.

## 0.0.6

* Add queued ESC/POS printer status queries with raw status bytes.
* Add ESC/POS printer identity and serial number retrieval helpers.
* Add Dart concurrent printer stress testing utilities and example app controls.
* Set TCP read timeouts for bidirectional printer commands to avoid hanging status reads.

## 0.0.5

* Add ZPL and ZPL image label printing for Zebra-compatible label printers.
* Add ESC/POS cash drawer pulse control with queued Dart APIs.
* Add built-in ZPL label templates and example app controls for cash drawer testing.

## 0.0.1

* Add ESC/POS receipt rendering and printing across network, USB, and serial connections.
* Add built-in 58mm/80mm receipt templates for POS, kitchen, delivery, refund, pre-checkout, and custom jobs.
* Add TSPL label template support for TSC-compatible label printers.
* Add Android, Linux, macOS, and Windows example builds for GitHub releases.
