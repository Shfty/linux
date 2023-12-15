// Controller HID commands
pub const GET_FIRMWARE_INFO: &[u8] = &[0x02, 0x13];
pub const RESET: &[u8] = &[0x05, 0x01];
pub const RESET_DIRECT: &[u8] = &[0x05, 0x01, 0x01];
pub const ACK: &[u8] = &[0x09];
pub const ACK_DIRECT: &[u8] = &[0x09, 0x01];
pub const READ: &[u8] = &[0x08];
pub const READ_DIRECT: &[u8] = &[0x08, 0x01];

pub const fn set_controller_state(state: u8) -> [u8; 4] {
    [0x01, 0x03, 0x00, state]
}

pub fn set_mode(mode: &[u8]) -> Vec<u8> {
    [&[0x0d, 0x00], mode].concat()
}

pub fn set_mode_direct(mode: &[u8]) -> Vec<u8> {
    [&[0x0d, 0x01], mode].concat()
}

pub fn write(bytes: &[u8]) -> Vec<u8> {
    [&[0x06, 0x00], bytes].concat()
}

pub fn write_direct(bytes: &[u8]) -> Vec<u8> {
    [&[0x06, 0x01], bytes].concat()
}
