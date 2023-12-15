use std::fmt::Display;

use anyhow::{anyhow, Error};

use crate::thread::socket::socket_command::{
    SOCKET_COMMAND_GET_COOLANT_TEMP, SOCKET_COMMAND_GET_PUMP_SPEED, SOCKET_COMMAND_SET_COLORS,
    SOCKET_COMMAND_SET_PUMP_SPEED,
};

#[derive(Debug)]
pub enum SocketResponse {
    GetCoolantTemp(u16),
    GetPumpSpeed(u16),
    SetPumpSpeed(bool),
    SetColors(bool),
}

impl Display for SocketResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SocketResponse::GetCoolantTemp(temp) => temp.fmt(f),
            SocketResponse::GetPumpSpeed(speed) => speed.fmt(f),
            SocketResponse::SetPumpSpeed(success) => success.fmt(f),
            SocketResponse::SetColors(success) => success.fmt(f),
        }
    }
}

impl From<SocketResponse> for Vec<u8> {
    fn from(value: SocketResponse) -> Self {
        match value {
            SocketResponse::GetCoolantTemp(temp) => [
                &[SOCKET_COMMAND_GET_COOLANT_TEMP][..],
                &temp.to_le_bytes()[..],
            ]
            .concat(),
            SocketResponse::GetPumpSpeed(speed) => [
                &[SOCKET_COMMAND_GET_PUMP_SPEED][..],
                &speed.to_le_bytes()[..],
            ]
            .concat(),
            SocketResponse::SetPumpSpeed(success) => {
                vec![
                    SOCKET_COMMAND_SET_PUMP_SPEED,
                    if success { 0x01 } else { 0x00 },
                ]
            }
            SocketResponse::SetColors(success) => {
                vec![SOCKET_COMMAND_SET_COLORS, if success { 0x01 } else { 0x00 }]
            }
        }
    }
}

impl TryFrom<&[u8]> for SocketResponse {
    type Error = Error;

    fn try_from(s: &[u8]) -> Result<Self, Self::Error> {
        let (_, output) =
            socket_response_bytes(s).map_err(|_| anyhow!("Invalid socket response"))?;
        Ok(output)
    }
}

fn socket_response_bytes(input: &[u8]) -> nom::IResult<&[u8], SocketResponse> {
    nom::branch::alt((
        socket_response_get_coolant_temp_bytes,
        socket_response_get_pump_speed_bytes,
        socket_response_set_pump_speed_bytes,
        socket_response_set_colors_bytes,
    ))(input)
}

fn socket_response_get_coolant_temp_bytes(input: &[u8]) -> nom::IResult<&[u8], SocketResponse> {
    let (input, _) = nom::bytes::complete::tag([SOCKET_COMMAND_GET_COOLANT_TEMP])(input)?;
    let (input, temp) = nom::number::complete::le_u16(input)?;
    Ok((input, SocketResponse::GetCoolantTemp(temp)))
}

fn socket_response_get_pump_speed_bytes(input: &[u8]) -> nom::IResult<&[u8], SocketResponse> {
    let (input, _) = nom::bytes::complete::tag([SOCKET_COMMAND_GET_PUMP_SPEED])(input)?;
    let (input, temp) = nom::number::complete::le_u16(input)?;
    Ok((input, SocketResponse::GetPumpSpeed(temp)))
}

fn socket_response_set_pump_speed_bytes(input: &[u8]) -> nom::IResult<&[u8], SocketResponse> {
    let (input, _) = nom::bytes::complete::tag([SOCKET_COMMAND_SET_PUMP_SPEED])(input)?;
    let (input, success) = nom::number::complete::u8(input)?;
    Ok((input, SocketResponse::SetPumpSpeed(success == 1)))
}

fn socket_response_set_colors_bytes(input: &[u8]) -> nom::IResult<&[u8], SocketResponse> {
    let (input, _) = nom::bytes::complete::tag([SOCKET_COMMAND_SET_COLORS])(input)?;
    let (input, success) = nom::number::complete::u8(input)?;
    Ok((input, SocketResponse::SetColors(success == 1)))
}
