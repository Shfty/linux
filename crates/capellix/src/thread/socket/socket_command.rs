use std::{fmt::Display, str::FromStr, sync::atomic::Ordering};

use anyhow::{anyhow, Error, Result};
use log::debug;
use tokio::{
    io::{AsyncWrite, AsyncWriteExt},
    sync::{mpsc, watch},
};

use crate::{
    hid::validate_fan_speed,
    thread::{
        capellix::{Colors, SharedState},
        pump_target::Fan,
    },
};

use super::LED_COUNT_TOTAL;

pub const SOCKET_COMMAND_GET_COOLANT_TEMP: u8 = 0;
pub const SOCKET_COMMAND_GET_PUMP_SPEED: u8 = 1;
pub const SOCKET_COMMAND_SET_PUMP_SPEED: u8 = 2;
pub const SOCKET_COMMAND_SET_COLORS: u8 = 3;

#[derive(Debug, Clone)]
pub enum SocketCommand {
    GetCoolantTemp,
    GetPumpSpeed,
    SetFanTarget(Fan, u16),
    SetColors(Colors),
}

impl Display for SocketCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SocketCommand::GetCoolantTemp => f.write_fmt(format_args!("GetCoolantTemp")),
            SocketCommand::GetPumpSpeed => f.write_fmt(format_args!("GetPumpSpeed")),
            SocketCommand::SetFanTarget(fan, speed) => {
                f.write_fmt(format_args!("SetPumpTarget({fan:?}, {speed:})"))
            }
            SocketCommand::SetColors(_) => f.write_fmt(format_args!("SetColors(...)")),
        }
    }
}

impl FromStr for SocketCommand {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (_, output) = socket_command_str(s).map_err(|_| anyhow!("Invalid socket command"))?;
        Ok(output)
    }
}

impl From<SocketCommand> for Vec<u8> {
    fn from(value: SocketCommand) -> Self {
        match value {
            SocketCommand::GetCoolantTemp => vec![SOCKET_COMMAND_GET_COOLANT_TEMP],
            SocketCommand::GetPumpSpeed => vec![SOCKET_COMMAND_GET_PUMP_SPEED],
            SocketCommand::SetFanTarget(fan, speed) => [
                &[SOCKET_COMMAND_SET_PUMP_SPEED][..],
                &[u8::from(fan)],
                &speed.to_le_bytes()[..],
            ]
            .concat(),
            SocketCommand::SetColors(colors) => [
                &[SOCKET_COMMAND_SET_COLORS][..],
                &colors.into_iter().flatten().collect::<Vec<_>>()[..],
            ]
            .concat(),
        }
    }
}

impl SocketCommand {
    pub async fn run(
        self,
        state: &SharedState,
        set_pump_speed_tx: &mpsc::Sender<(Fan, u16)>,
        set_colors_tx: &watch::Sender<Colors>,
        mut sink: impl Unpin + AsyncWrite,
    ) -> Result<()> {
        match self {
            SocketCommand::GetCoolantTemp => {
                let temp = state.coolant_temp.load(Ordering::Relaxed);
                let temp = temp.to_le_bytes();

                sink.write(&[0, temp[0], temp[1]]).await?;
            }
            SocketCommand::GetPumpSpeed => {
                let speed = state.pump_speed.load(Ordering::Relaxed);
                let speed = speed.to_le_bytes();

                sink.write(&[1, speed[0], speed[1]]).await?;
            }
            SocketCommand::SetFanTarget(fan, speed) => {
                debug!("SocketThread setting pump target");
                let speed = validate_fan_speed(speed);
                set_pump_speed_tx.send((fan, speed)).await?;
                sink.write(&[1]).await?;
            }
            SocketCommand::SetColors(in_colors) => {
                debug!("SocketThread setting colors");
                set_colors_tx.send(in_colors)?;
                sink.write(&[1]).await?;
            }
        }

        Ok(())
    }
}

pub fn socket_command_str(input: &str) -> nom::IResult<&str, SocketCommand> {
    nom::branch::alt((
        socket_command_set_colors_str,
        socket_command_set_pump_speed_str,
        socket_command_get_coolant_temp_str,
        socket_command_get_pump_speed_str,
    ))(input)
}

pub fn socket_command_get_coolant_temp_str(input: &str) -> nom::IResult<&str, SocketCommand> {
    let (input, _) = nom::bytes::complete::tag("get-coolant-temp")(input)?;
    Ok((input, SocketCommand::GetCoolantTemp))
}

pub fn socket_command_get_pump_speed_str(input: &str) -> nom::IResult<&str, SocketCommand> {
    let (input, _) = nom::bytes::complete::tag("get-pump-speed")(input)?;
    Ok((input, SocketCommand::GetPumpSpeed))
}

pub fn socket_command_set_pump_speed_str(input: &str) -> nom::IResult<&str, SocketCommand> {
    let (input, _) = nom::bytes::complete::tag("set-fan-target")(input)?;
    let (input, fan) = nom::combinator::map_res(
        nom::sequence::preceded(
            nom::character::complete::space1,
            nom::combinator::recognize(nom::multi::many1(nom::character::complete::alphanumeric1)),
        ),
        str::parse,
    )(input)?;

    let (input, speed) = nom::combinator::map_res(
        nom::sequence::preceded(
            nom::character::complete::space1,
            nom::combinator::recognize(nom::multi::many1(nom::character::complete::one_of(
                "0123456789",
            ))),
        ),
        str::parse,
    )(input)?;

    Ok((input, SocketCommand::SetFanTarget(fan, speed)))
}

pub fn socket_command_set_colors_str(input: &str) -> nom::IResult<&str, SocketCommand> {
    let (input, _) = nom::bytes::complete::tag("set-colors")(input)?;
    let (input, colors) = nom::multi::many0(nom::multi::count(
        nom::combinator::map_res(
            nom::sequence::preceded(
                nom::character::complete::space1,
                nom::combinator::recognize(nom::multi::many1(nom::character::complete::one_of(
                    "0123456789",
                ))),
            ),
            |comp: &str| comp.parse::<u8>(),
        ),
        3,
    ))(input)?;

    let buf: Vec<_> = colors
        .into_iter()
        .map(|chunk| [chunk[0], chunk[1], chunk[2]])
        .collect();

    let mut colors = [[0; 3]; LED_COUNT_TOTAL];
    colors.copy_from_slice(&buf);

    Ok((input, SocketCommand::SetColors(Box::new(colors))))
}

pub fn socket_command_bytes(input: &[u8]) -> nom::IResult<&[u8], SocketCommand> {
    nom::branch::alt((
        socket_command_set_colors_bytes,
        socket_command_set_fan_speed_bytes,
        socket_command_get_coolant_temp_bytes,
        socket_command_get_pump_speed_bytes,
    ))(input)
}

pub fn socket_command_get_coolant_temp_bytes(input: &[u8]) -> nom::IResult<&[u8], SocketCommand> {
    let (input, _) = nom::bytes::complete::tag([SOCKET_COMMAND_GET_COOLANT_TEMP])(input)?;
    Ok((input, SocketCommand::GetCoolantTemp))
}

pub fn socket_command_get_pump_speed_bytes(input: &[u8]) -> nom::IResult<&[u8], SocketCommand> {
    let (input, _) = nom::bytes::complete::tag([SOCKET_COMMAND_GET_PUMP_SPEED])(input)?;
    Ok((input, SocketCommand::GetPumpSpeed))
}

pub fn socket_command_set_fan_speed_bytes(input: &[u8]) -> nom::IResult<&[u8], SocketCommand> {
    let (input, _) = nom::bytes::complete::tag([SOCKET_COMMAND_SET_PUMP_SPEED])(input)?;
    let (input, fan) = nom::number::complete::u8(input)?;
    let fan = Fan::try_from(fan).map_err(|_| {
        nom::Err::Error(nom::error::Error {
            input,
            code: nom::error::ErrorKind::AlphaNumeric,
        })
    })?;

    let (input, speed) = nom::number::complete::le_u16(input)?;
    Ok((input, SocketCommand::SetFanTarget(fan, speed)))
}

pub fn socket_command_set_colors_bytes(input: &[u8]) -> nom::IResult<&[u8], SocketCommand> {
    let (input, _) = nom::bytes::complete::tag([SOCKET_COMMAND_SET_COLORS])(input)?;
    let (input, buf) = nom::multi::count(
        nom::combinator::map(nom::multi::count(nom::number::complete::u8, 3), |color| {
            [color[0], color[1], color[2]]
        }),
        LED_COUNT_TOTAL,
    )(input)?;

    let mut colors = [[0; 3]; LED_COUNT_TOTAL];
    colors.copy_from_slice(&buf);

    Ok((input, SocketCommand::SetColors(Box::new(colors))))
}
