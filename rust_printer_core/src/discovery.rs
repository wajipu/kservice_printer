//! 打印机发现：USB 通过 libusb 枚举，网络设备通过 mDNS 扫描。
//!
//! USB 发现使用 `rusb` 枚举设备，并根据打印机设备类（class code `0x07`）
//! 做保守标记。网络发现扫描常见 mDNS 服务类型（`_pdl-datastream`、
//! `_printer`、`_ipp`、`_ipps`），并支持超时控制。Android/iOS 上暂不在
//! Rust 层做网络发现，因为 mDNS 需要平台原生 API 和运行时权限配合。

use std::collections::HashMap;
use std::time::{Duration, Instant};

#[cfg(not(target_os = "windows"))]
use rusb::UsbContext;
use serde_json::{json, Value};

use crate::error::PrinterError;
#[cfg(target_os = "windows")]
use crate::windows_usbprint::WindowsUsbPrintDriver;

const DEFAULT_DISCOVERY_TIMEOUT_MS: u64 = 3_000;
const MIN_DISCOVERY_TIMEOUT_MS: u64 = 250;
const MAX_DISCOVERY_TIMEOUT_MS: u64 = 30_000;
const MDNS_RECV_SLICE_MS: u64 = 50;
const DEFAULT_NETWORK_PRINTER_SERVICE_TYPES: &[&str] = &[
    "_pdl-datastream._tcp.local.",
    "_printer._tcp.local.",
    "_ipp._tcp.local.",
    "_ipps._tcp.local.",
];

#[derive(Debug, Clone)]
struct NetworkPrinterCandidate {
    service_name: String,
    service_type: String,
    fullname: String,
    hostname: String,
    host: String,
    port: u16,
    addresses: Vec<String>,
    txt: HashMap<String, String>,
    supports_raw_tcp: bool,
}

impl NetworkPrinterCandidate {
    fn to_json(&self) -> Value {
        json!({
            "serviceName": self.service_name,
            "serviceType": self.service_type,
            "fullname": self.fullname,
            "hostname": self.hostname,
            "host": self.host,
            "port": self.port,
            "addresses": self.addresses,
            "txt": self.txt,
            "supportsRawTcp": self.supports_raw_tcp,
        })
    }
}

pub(crate) fn list_usb_printers_inner() -> Result<Value, PrinterError> {
    #[cfg(target_os = "windows")]
    {
        return list_windows_usb_printers_inner();
    }

    #[cfg(not(target_os = "windows"))]
    {
        let context = rusb::Context::new().map_err(|e| PrinterError::Connect(e.to_string()))?;
        let devices = context
            .devices()
            .map_err(|e| PrinterError::Connect(e.to_string()))?;
        let mut result = Vec::new();

        for device in devices.iter() {
            let descriptor = match device.device_descriptor() {
                Ok(descriptor) => descriptor,
                Err(_) => continue,
            };
            let handle = device.open().ok();
            let product = handle
                .as_ref()
                .and_then(|handle| handle.read_product_string_ascii(&descriptor).ok());
            let manufacturer = handle
                .as_ref()
                .and_then(|handle| handle.read_manufacturer_string_ascii(&descriptor).ok());
            let serial = handle
                .as_ref()
                .and_then(|handle| handle.read_serial_number_string_ascii(&descriptor).ok());
            let class_code = descriptor.class_code();
            let interface_classes = usb_interface_classes(&device);
            let is_printer = class_code == 0x07 || interface_classes.contains(&0x07);

            result.push(json!({
                "vendorId": descriptor.vendor_id(),
                "productId": descriptor.product_id(),
                "vendorIdHex": format!("0x{:04X}", descriptor.vendor_id()),
                "productIdHex": format!("0x{:04X}", descriptor.product_id()),
                "manufacturer": manufacturer,
                "product": product,
                "serial": serial,
                "classCode": class_code,
                "interfaceClasses": interface_classes,
                "isPrinter": is_printer,
            }));
        }

        result.sort_by_key(|value| {
            (
                !value["isPrinter"].as_bool().unwrap_or(false),
                value["manufacturer"].as_str().unwrap_or("").to_owned(),
                value["product"].as_str().unwrap_or("").to_owned(),
            )
        });

        Ok(json!({ "printers": result }))
    }
}

#[cfg(target_os = "windows")]
fn list_windows_usb_printers_inner() -> Result<Value, PrinterError> {
    let mut result = Vec::new();

    for printer in
        WindowsUsbPrintDriver::list().map_err(|e| PrinterError::Connect(e.to_string()))?
    {
        let (vendor_id, product_id) = match (printer.vendor_id, printer.product_id) {
            (Some(vendor_id), Some(product_id)) => (vendor_id, product_id),
            _ => continue,
        };

        result.push(json!({
            "vendorId": vendor_id,
            "productId": product_id,
            "vendorIdHex": format!("0x{:04X}", vendor_id),
            "productIdHex": format!("0x{:04X}", product_id),
            "manufacturer": null,
            "product": "Windows USB Printer",
            "serial": null,
            "classCode": 0x07,
            "interfaceClasses": [0x07],
            "isPrinter": true,
            "hasPermission": true,
            "deviceName": printer.device_path,
        }));
    }

    result.sort_by_key(|value| {
        (
            value["product"].as_str().unwrap_or("").to_owned(),
            value["vendorId"].as_u64().unwrap_or(0),
            value["productId"].as_u64().unwrap_or(0),
        )
    });

    Ok(json!({ "printers": result }))
}

#[cfg(not(target_os = "windows"))]
fn usb_interface_classes<T: rusb::UsbContext>(device: &rusb::Device<T>) -> Vec<u8> {
    let descriptor = match device.device_descriptor() {
        Ok(descriptor) => descriptor,
        Err(_) => return Vec::new(),
    };
    let mut classes = Vec::new();
    for config_index in 0..descriptor.num_configurations() {
        let config = match device.config_descriptor(config_index) {
            Ok(config) => config,
            Err(_) => continue,
        };
        for interface in config.interfaces() {
            for descriptor in interface.descriptors() {
                let class_code = descriptor.class_code();
                if !classes.contains(&class_code) {
                    classes.push(class_code);
                }
            }
        }
    }
    classes
}

// On mobile platforms mDNS socket access requires platform-native APIs and
// runtime permissions unavailable through this crate's dependency set.
#[cfg(any(target_os = "android", target_os = "ios"))]
pub(crate) fn discover_network_printers_inner(
    _timeout_ms: u64,
    _service_types: Vec<String>,
) -> Result<Value, PrinterError> {
    Err(PrinterError::Discovery(
        "当前平台需要通过原生网络服务发现 API 和运行时权限适配 mDNS".into(),
    ))
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub(crate) fn discover_network_printers_inner(
    timeout_ms: u64,
    service_types: Vec<String>,
) -> Result<Value, PrinterError> {
    use mdns_sd::{ServiceDaemon, ServiceEvent};

    let timeout_ms = normalize_discovery_timeout_ms(timeout_ms);
    let service_types = normalize_discovery_service_types(service_types)?;
    let started_at = Instant::now();
    let deadline = started_at + Duration::from_millis(timeout_ms);
    let mdns = ServiceDaemon::new().map_err(|e| PrinterError::Discovery(e.to_string()))?;
    let mut receivers = Vec::new();

    for service_type in &service_types {
        let receiver = mdns
            .browse(service_type)
            .map_err(|e| PrinterError::Discovery(e.to_string()))?;
        receivers.push((service_type.clone(), receiver));
    }

    let mut candidates = HashMap::<String, NetworkPrinterCandidate>::new();

    'scan: while Instant::now() < deadline {
        for (service_type, receiver) in &receivers {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break 'scan;
            }
            let wait = remaining.min(Duration::from_millis(MDNS_RECV_SLICE_MS));
            match receiver.recv_timeout(wait) {
                Ok(ServiceEvent::ServiceResolved(info)) => {
                    if let Some(candidate) = network_candidate_from_mdns(service_type, &info) {
                        let key = network_candidate_key(&candidate);
                        candidates.insert(key, candidate);
                    }
                }
                Ok(_) => {}
                Err(_) => {}
            }
        }
    }

    for service_type in &service_types {
        let _ = mdns.stop_browse(service_type);
    }
    if let Ok(status) = mdns.shutdown() {
        let _ = status.recv_timeout(Duration::from_millis(200));
    }

    let elapsed_ms = started_at.elapsed().as_millis() as u64;
    let mut printers = candidates.into_values().collect::<Vec<_>>();
    printers.sort_by(|a, b| {
        a.service_name
            .to_lowercase()
            .cmp(&b.service_name.to_lowercase())
            .then_with(|| a.host.cmp(&b.host))
            .then_with(|| a.port.cmp(&b.port))
    });

    Ok(json!({
        "timeoutMs": timeout_ms,
        "durationMs": elapsed_ms,
        "timedOut": elapsed_ms >= timeout_ms,
        "serviceTypes": service_types,
        "printers": printers.into_iter().map(|printer| printer.to_json()).collect::<Vec<_>>(),
    }))
}

fn normalize_discovery_timeout_ms(timeout_ms: u64) -> u64 {
    let timeout_ms = if timeout_ms == 0 {
        DEFAULT_DISCOVERY_TIMEOUT_MS
    } else {
        timeout_ms
    };
    timeout_ms.clamp(MIN_DISCOVERY_TIMEOUT_MS, MAX_DISCOVERY_TIMEOUT_MS)
}

fn normalize_discovery_service_types(
    service_types: Vec<String>,
) -> Result<Vec<String>, PrinterError> {
    let source = if service_types.is_empty() {
        DEFAULT_NETWORK_PRINTER_SERVICE_TYPES
            .iter()
            .map(|value| value.to_string())
            .collect()
    } else {
        service_types
    };
    let mut normalized = Vec::new();
    for service_type in source {
        let service_type = normalize_mdns_service_type(&service_type)?;
        if !normalized.contains(&service_type) {
            normalized.push(service_type);
        }
    }
    Ok(normalized)
}

fn normalize_mdns_service_type(service_type: &str) -> Result<String, PrinterError> {
    let mut value = service_type
        .trim()
        .to_ascii_lowercase()
        .trim_end_matches('.')
        .to_string();
    if value.is_empty() {
        return Err(PrinterError::Discovery("mDNS 服务类型不能为空".into()));
    }
    if value.ends_with(".local") {
        value.truncate(value.len() - ".local".len());
    }
    if !value.starts_with('_') {
        value.insert(0, '_');
    }
    if !value.contains("._tcp") && !value.contains("._udp") {
        if value.contains('.') {
            return Err(PrinterError::Discovery(format!(
                "mDNS 服务类型缺少 _tcp 或 _udp 协议段: {service_type}"
            )));
        }
        value.push_str("._tcp");
    }
    value.push_str(".local.");
    if !(value.ends_with("._tcp.local.") || value.ends_with("._udp.local.")) {
        return Err(PrinterError::Discovery(format!(
            "mDNS 服务类型必须以 ._tcp.local. 或 ._udp.local. 结尾: {service_type}"
        )));
    }
    Ok(value)
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn network_candidate_from_mdns(
    fallback_service_type: &str,
    info: &mdns_sd::ResolvedService,
) -> Option<NetworkPrinterCandidate> {
    let mut addresses = info
        .get_addresses()
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    addresses.sort();
    let host = info
        .get_addresses_v4()
        .into_iter()
        .map(|addr| addr.to_string())
        .min()
        .or_else(|| addresses.first().cloned())
        .unwrap_or_else(|| info.get_hostname().trim_end_matches('.').to_string());
    if host.is_empty() {
        return None;
    }
    let service_type = if info.ty_domain.is_empty() {
        fallback_service_type.to_string()
    } else {
        info.ty_domain.clone()
    };
    let txt = info
        .get_properties()
        .iter()
        .map(|property| (property.key().to_string(), property.val_str().to_string()))
        .collect::<HashMap<_, _>>();
    Some(NetworkPrinterCandidate {
        service_name: mdns_instance_name(info.get_fullname(), &service_type),
        service_type: service_type.clone(),
        fullname: info.get_fullname().to_string(),
        hostname: info.get_hostname().trim_end_matches('.').to_string(),
        host,
        port: info.get_port(),
        addresses,
        txt,
        supports_raw_tcp: supports_raw_tcp_service(&service_type, info.get_port()),
    })
}

fn network_candidate_key(candidate: &NetworkPrinterCandidate) -> String {
    if candidate.host.is_empty() {
        format!("{}:{}", candidate.fullname, candidate.port)
    } else {
        format!("{}:{}", candidate.host, candidate.port)
    }
}

fn mdns_instance_name(fullname: &str, service_type: &str) -> String {
    fullname
        .strip_suffix(service_type)
        .unwrap_or(fullname)
        .trim_end_matches('.')
        .replace("\\.", ".")
        .replace("\\\\", "\\")
}

fn supports_raw_tcp_service(service_type: &str, port: u16) -> bool {
    let service_type = service_type.to_ascii_lowercase();
    service_type.contains("_pdl-datastream._tcp") || port == 9100
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_mdns_service_types() {
        assert_eq!(
            normalize_mdns_service_type("ipp").unwrap(),
            "_ipp._tcp.local."
        );
        assert_eq!(
            normalize_mdns_service_type("_pdl-datastream._tcp").unwrap(),
            "_pdl-datastream._tcp.local."
        );
        assert_eq!(
            normalize_mdns_service_type("_printer._tcp.local").unwrap(),
            "_printer._tcp.local."
        );
    }

    #[test]
    fn rejects_invalid_mdns_service_types() {
        let result = normalize_mdns_service_type("_bad._http.local.");

        assert!(matches!(result, Err(PrinterError::Discovery(_))));
    }

    #[test]
    fn detects_raw_tcp_printer_services_conservatively() {
        assert!(supports_raw_tcp_service(
            "_pdl-datastream._tcp.local.",
            9100
        ));
        assert!(supports_raw_tcp_service("_printer._tcp.local.", 9100));
        assert!(!supports_raw_tcp_service("_printer._tcp.local.", 515));
        assert!(!supports_raw_tcp_service("_ipp._tcp.local.", 631));
    }
}
