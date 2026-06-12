//! Windows USB printer driver using the `usbprint.sys` device interface.
//!
//! Most Windows USB receipt printers are owned by the system printer class
//! driver, so libusb/rusb cannot claim them. This driver writes raw ESC/POS or
//! TSPL bytes through the device interface exposed by `usbprint.sys`.

use escpos::driver::Driver;
use escpos::errors::PrinterError as EscposError;
use std::ffi::OsString;
use std::io;
use std::mem;
use std::os::windows::ffi::OsStringExt;
use std::ptr;
use std::sync::{Arc, Mutex};
use windows_sys::core::GUID;
use windows_sys::Win32::Devices::DeviceAndDriverInstallation::{
    SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInterfaces, SetupDiGetClassDevsW,
    SetupDiGetDeviceInterfaceDetailW, DIGCF_DEVICEINTERFACE, DIGCF_PRESENT,
    SP_DEVICE_INTERFACE_DATA,
};
use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_INVALID_FUNCTION, ERROR_NOT_SUPPORTED, GENERIC_READ, GENERIC_WRITE, HANDLE,
    INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FlushFileBuffers, ReadFile, WriteFile, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ,
    FILE_SHARE_WRITE, OPEN_EXISTING,
};

#[derive(Debug, Clone)]
pub(crate) struct WindowsUsbPrintInfo {
    pub(crate) device_path: String,
    pub(crate) vendor_id: Option<u16>,
    pub(crate) product_id: Option<u16>,
}

#[derive(Clone)]
pub(crate) struct WindowsUsbPrintDriver {
    device_path: String,
    handle: Arc<Mutex<WinHandle>>,
}

struct WinHandle(HANDLE);

// Windows kernel handles can be moved between threads. Access is serialized by
// the mutex around each file operation.
unsafe impl Send for WinHandle {}
unsafe impl Sync for WinHandle {}

impl Drop for WinHandle {
    fn drop(&mut self) {
        unsafe {
            if self.0 as isize != 0 && self.0 as isize != INVALID_HANDLE_VALUE as isize {
                CloseHandle(self.0);
            }
        }
    }
}

impl WindowsUsbPrintDriver {
    // {28D78FAD-5A12-11D1-AE5B-0000F803A8C2}
    const GUID_DEVINTERFACE_USBPRINT: GUID = GUID {
        data1: 0x28d7_8fad,
        data2: 0x5a12,
        data3: 0x11d1,
        data4: [0xae, 0x5b, 0x00, 0x00, 0xf8, 0x03, 0xa8, 0xc2],
    };

    pub(crate) fn list() -> Result<Vec<WindowsUsbPrintInfo>, EscposError> {
        unsafe {
            let hdev = SetupDiGetClassDevsW(
                &Self::GUID_DEVINTERFACE_USBPRINT,
                ptr::null(),
                ptr::null_mut(),
                DIGCF_PRESENT | DIGCF_DEVICEINTERFACE,
            );
            if hdev as isize == 0 || hdev as isize == INVALID_HANDLE_VALUE as isize {
                return Err(EscposError::Io("SetupDiGetClassDevsW failed".to_string()));
            }

            let mut results = Vec::new();
            let mut index: u32 = 0;
            loop {
                let mut iface_data: SP_DEVICE_INTERFACE_DATA = mem::zeroed();
                iface_data.cbSize = mem::size_of::<SP_DEVICE_INTERFACE_DATA>() as u32;

                let ok = SetupDiEnumDeviceInterfaces(
                    hdev,
                    ptr::null_mut(),
                    &Self::GUID_DEVINTERFACE_USBPRINT,
                    index,
                    &mut iface_data,
                );
                if ok == 0 {
                    break;
                }
                index += 1;

                let mut required_size: u32 = 0;
                SetupDiGetDeviceInterfaceDetailW(
                    hdev,
                    &iface_data,
                    ptr::null_mut(),
                    0,
                    &mut required_size,
                    ptr::null_mut(),
                );
                if required_size == 0 {
                    continue;
                }

                let mut buffer = vec![0u8; required_size as usize];
                let header_size = {
                    #[repr(C)]
                    struct DetailHeader {
                        cb_size: u32,
                        _device_path: [u16; 1],
                    }
                    mem::size_of::<DetailHeader>() as u32
                };
                *(buffer.as_mut_ptr() as *mut u32) = header_size;

                let ok = SetupDiGetDeviceInterfaceDetailW(
                    hdev,
                    &iface_data,
                    buffer.as_mut_ptr() as *mut _,
                    required_size,
                    ptr::null_mut(),
                    ptr::null_mut(),
                );
                if ok == 0 {
                    continue;
                }

                let path_ptr = buffer.as_ptr().add(4) as *const u16;
                let mut len = 0usize;
                while *path_ptr.add(len) != 0 {
                    len += 1;
                }
                let slice = std::slice::from_raw_parts(path_ptr, len);
                let device_path = OsString::from_wide(slice).to_string_lossy().into_owned();
                let (vendor_id, product_id) = parse_vid_pid(&device_path);

                results.push(WindowsUsbPrintInfo {
                    device_path,
                    vendor_id,
                    product_id,
                });
            }

            SetupDiDestroyDeviceInfoList(hdev);
            Ok(results)
        }
    }

    pub(crate) fn open_by_vid_pid(vendor_id: u16, product_id: u16) -> Result<Self, EscposError> {
        let info = Self::list()?
            .into_iter()
            .find(|info| info.vendor_id == Some(vendor_id) && info.product_id == Some(product_id))
            .ok_or_else(|| {
                EscposError::Io(format!(
                    "No Windows USB print device found with VID=0x{vendor_id:04X}, PID=0x{product_id:04X}"
                ))
            })?;
        Self::open(&info.device_path)
    }

    fn open(device_path: &str) -> Result<Self, EscposError> {
        let wide: Vec<u16> = device_path
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();

        unsafe {
            let handle = CreateFileW(
                wide.as_ptr(),
                (GENERIC_READ | GENERIC_WRITE) as u32,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                ptr::null(),
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL,
                ptr::null_mut(),
            );
            if handle as isize == 0 || handle as isize == INVALID_HANDLE_VALUE as isize {
                return Err(EscposError::Io(format!(
                    "CreateFileW failed for {device_path}: {}",
                    io::Error::last_os_error()
                )));
            }

            Ok(Self {
                device_path: device_path.to_string(),
                handle: Arc::new(Mutex::new(WinHandle(handle))),
            })
        }
    }
}

impl Driver for WindowsUsbPrintDriver {
    fn name(&self) -> String {
        format!("Windows USB print ({})", self.device_path)
    }

    fn write(&self, data: &[u8]) -> std::result::Result<(), EscposError> {
        let guard = self.handle.lock()?;
        let mut remaining = data;
        while !remaining.is_empty() {
            let mut written: u32 = 0;
            let ok = unsafe {
                WriteFile(
                    guard.0,
                    remaining.as_ptr(),
                    remaining.len() as u32,
                    &mut written,
                    ptr::null_mut(),
                )
            };
            if ok == 0 {
                return Err(EscposError::Io(format!(
                    "WriteFile failed: {}",
                    io::Error::last_os_error()
                )));
            }
            if written == 0 {
                return Err(EscposError::Io("WriteFile wrote 0 bytes".to_string()));
            }
            remaining = &remaining[written as usize..];
        }
        Ok(())
    }

    fn read(&self, buf: &mut [u8]) -> std::result::Result<usize, EscposError> {
        let guard = self.handle.lock()?;
        let mut read: u32 = 0;
        let ok = unsafe {
            ReadFile(
                guard.0,
                buf.as_mut_ptr(),
                buf.len() as u32,
                &mut read,
                ptr::null_mut(),
            )
        };
        if ok == 0 {
            return Err(EscposError::Io(format!(
                "ReadFile failed: {}",
                io::Error::last_os_error()
            )));
        }
        Ok(read as usize)
    }

    fn flush(&self) -> std::result::Result<(), EscposError> {
        let guard = self.handle.lock()?;
        let ok = unsafe { FlushFileBuffers(guard.0) };
        if ok == 0 {
            let err = io::Error::last_os_error();
            let code = err.raw_os_error().unwrap_or(0) as u32;
            if code == ERROR_INVALID_FUNCTION || code == ERROR_NOT_SUPPORTED {
                return Ok(());
            }
            return Err(EscposError::Io(format!("FlushFileBuffers failed: {err}")));
        }
        Ok(())
    }
}

fn parse_vid_pid(path: &str) -> (Option<u16>, Option<u16>) {
    fn parse_hex_after(haystack: &str, needle: &str) -> Option<u16> {
        let lower = haystack.to_ascii_lowercase();
        let idx = lower.find(needle)?;
        let start = idx + needle.len();
        let hex: String = haystack[start..]
            .chars()
            .take_while(|c| c.is_ascii_hexdigit())
            .take(4)
            .collect();
        if hex.is_empty() {
            None
        } else {
            u16::from_str_radix(&hex, 16).ok()
        }
    }

    (parse_hex_after(path, "vid_"), parse_hex_after(path, "pid_"))
}
