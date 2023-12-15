pub mod command;
pub mod mode;
pub mod request;
pub mod state;

use anyhow::{anyhow, Result};
use hidapi::{HidApi, HidDevice};
use log::{debug, info, warn};

const REPORT_LENGTH: usize = 96;

const HEADER_WRITE: &[u8] = &[0x08];

pub const LED_COUNT_PUMP: usize = 29;
pub const LED_COUNT_FAN: usize = 34;
pub const LED_COUNT_TOTAL: usize = LED_COUNT_PUMP + LED_COUNT_FAN * 6;

pub const VID: u16 = 0x1b1c;
pub const PID: u16 = 0x0c1c;
pub const INTERFACE_NUMBER: i32 = 0;

/// Fixed-length report buffer, plus one byte for the report ID
type Report = [u8; 1 + REPORT_LENGTH];

pub struct Hid {
    pub api: HidApi,
    pub device: HidDevice,
    pub buffer: Report,
}

impl Hid {
    pub fn new() -> Result<Self> {
        let api = HidApi::new()?;

        let device_info = api
            .device_list()
            .find(|device_info| {
                device_info.vendor_id() == VID
                    && device_info.product_id() == PID
                    && device_info.interface_number() == INTERFACE_NUMBER
            })
            .ok_or_else(|| anyhow!("Failed to find device"))?;

        info!(
            "Found {} at {}",
            device_info
                .product_string()
                .ok_or_else(|| anyhow!("Failed to fetch product string"))?,
            device_info.path().to_string_lossy()
        );

        let device = device_info
            .open_device(&api)
            .map_err(|_| anyhow!("Failed to open device"))?;
        device.set_blocking_mode(true)?;

        let buffer = [0x00; 1 + REPORT_LENGTH];

        Ok(Hid {
            api,
            device,
            buffer,
        })
    }

    /// Discard any pending read packets to ensure the write-read cycle syncs up
    pub fn flush_read(&mut self, timeout: i32) -> Result<()> {
        info!("Flushing HID read buffer");
        self.device.read_timeout(&mut self.buffer, timeout)?;
        debug!("Flushed {:02x?}", &self.buffer);
        Ok(())
    }

    /// Write the given header and body into the report buffer
    fn buffer(&mut self, header: &[u8], body: &[u8]) {
        let (_report_id_slice, next_slice) = self.buffer.split_at_mut(1);
        let (header_slice, next_slice) = next_slice.split_at_mut(header.len());
        let (body_slice, _) = next_slice.split_at_mut(body.len());

        header_slice.copy_from_slice(header);
        body_slice.copy_from_slice(body);
    }

    /// Read from the HID device into the report buffer
    pub fn read(&mut self) -> Result<()> {
        // Receive response
        self.device.read(&mut self.buffer)?;
        debug!("Received {:02x?}", &self.buffer);
        Ok(())
    }

    /// Populate the report buffer and send to the HID device
    pub fn write(&mut self, command: &[u8]) -> Result<()> {
        self.buffer(HEADER_WRITE, command);
        self.device.write(&self.buffer)?;
        debug!("Sent {:02x?}", &self.buffer);
        Ok(())
    }

    /// Write command to the HID device, then read back input
    pub fn command(&mut self, command: &[u8]) -> Result<()> {
        // Zero buffer
        self.buffer.fill(0);

        self.write(command)?;

        // Zero buffer
        self.buffer.fill(0);

        self.read()?;

        // Error check
        if self.buffer[1] != command[0] {
            return Err(anyhow!(
                "Response {:02x} does not match command {:02x}",
                self.buffer[1],
                command[0]
            ));
        }

        Ok(())
    }

    pub fn request<C: AsRef<[u8]>, I: IntoIterator<Item = C>>(&mut self, request: I) -> Result<()> {
        for command in request.into_iter() {
            self.command(command.as_ref())?;
        }
        Ok(())
    }
}

/// Parse the given number of windows into u16s in both big and little endian,
/// and print it to the log
///
/// Useful for examining HID output
pub fn print_u16_windows(buf: &[u8], count: usize) {
    for (i, window) in buf.windows(2).enumerate().take(count) {
        info!(
            "Window {i:}, LE: {}, BE: {}",
            u16::from_le_bytes([window[0], window[1]]),
            u16::from_be_bytes([window[0], window[1]])
        )
    }
}

pub fn validate_fan_speed(speed: u16) -> u16 {
    if speed > 100 {
        warn!("Fan speed > 100 is invalid, clamping");
        100
    } else {
        speed
    }
}
