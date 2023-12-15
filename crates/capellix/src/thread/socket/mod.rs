pub mod socket_command;
pub mod socket_response;

use std::sync::Arc;

use anyhow::Result;
use futures::StreamExt;
use log::{debug, info};
use tokio::sync::watch;
use tokio::{net::TcpStream, sync::mpsc};
use tokio_util::codec::FramedRead;

use crate::{
    hid::LED_COUNT_TOTAL,
    thread::capellix::SharedState,
    thread::socket::socket_command::{socket_command_bytes, SocketCommand},
};

use super::{capellix::Colors, pump_target::Fan};

pub struct SocketThread {
    state: Arc<SharedState>,
    set_fan_speed_tx: mpsc::Sender<(Fan, u16)>,
    set_colors_tx: Arc<watch::Sender<Colors>>,
    exit_rx: watch::Receiver<bool>,
    stream: TcpStream,
}

enum SocketEvent {
    Read(Result<Box<SocketCommand>>),
    RunningChanged(bool),
}

#[derive(Debug, Default)]
pub struct SocketCommandCodec {
    buf: Vec<u8>,
}

impl<'a> tokio_util::codec::Decoder for SocketCommandCodec {
    type Item = SocketCommand;

    type Error = anyhow::Error;

    fn decode(&mut self, src: &mut bytes::BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        debug!("Read {} bytes", src.len());

        // Move received bytes into data buffer
        self.buf.extend(src.split_to(src.len()));

        let mut next_commands = self
            .buf
            .windows(4)
            .enumerate()
            .filter_map(|(i, window)| {
                if window == [b'C', b'P', b'L', b'X'] {
                    Some(i)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();

        if next_commands.is_empty() {
            return Ok(None);
        }

        let next_command = next_commands.remove(0);
        debug!("Next command at {next_command:}");

        let end = if !next_commands.is_empty() {
            next_commands[0]
        } else {
            self.buf.len()
        };

        let next_command_bytes = &self.buf[(next_command + 4)..end];
        if let Ok((input, command)) = socket_command_bytes(next_command_bytes) {
            let len = self.buf.len() - input.len();
            debug!("Splitting off {len:} bytes");
            self.buf = self.buf.split_off(len);
            Ok(Some(command))
        } else {
            Ok(None)
        }
    }

    fn decode_eof(&mut self, buf: &mut bytes::BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        match self.decode(buf)? {
            Some(frame) => Ok(Some(frame)),
            None => {
                if buf.is_empty() {
                    debug!("decode_eof, clearing buffer");
                    self.buf.clear();
                    Ok(None)
                } else {
                    Err(
                        std::io::Error::new(std::io::ErrorKind::Other, "bytes remaining on stream")
                            .into(),
                    )
                }
            }
        }
    }
}

impl tokio_util::codec::Encoder<SocketCommand> for SocketCommandCodec {
    type Error = anyhow::Error;

    fn encode(
        &mut self,
        item: SocketCommand,
        dst: &mut bytes::BytesMut,
    ) -> Result<(), Self::Error> {
        let bytes: Vec<u8> = item.into();
        dst.extend(bytes.into_iter());
        Ok(())
    }
}

impl SocketThread {
    pub fn new(
        state: Arc<SharedState>,
        set_pump_speed_tx: mpsc::Sender<(Fan, u16)>,
        set_colors_tx: Arc<watch::Sender<Colors>>,
        exit_rx: watch::Receiver<bool>,
        stream: TcpStream,
    ) -> Self {
        SocketThread {
            state,
            set_fan_speed_tx: set_pump_speed_tx,
            set_colors_tx,
            exit_rx,
            stream,
        }
    }

    pub async fn run(mut self) -> Result<()> {
        self.stream.set_nodelay(true)?;
        let (stream, mut sink) = self.stream.split();

        let stream = FramedRead::new(stream, SocketCommandCodec::default());
        let exit = tokio_stream::wrappers::WatchStream::new(self.exit_rx);

        let mut events = futures::stream_select!(
            stream.map(|command| SocketEvent::Read(command.map(Box::new))),
            exit.map(SocketEvent::RunningChanged)
        );

        while let Some(event) = events.next().await {
            match event {
                SocketEvent::Read(command) => {
                    let command = command?;

                    debug!("Received socket command: {command:}");

                    command
                        .run(
                            &self.state,
                            &self.set_fan_speed_tx,
                            &self.set_colors_tx,
                            &mut sink,
                        )
                        .await?;
                }
                SocketEvent::RunningChanged(running) => {
                    if !running {
                        info!("SocketThread got exit event");
                        break;
                    }
                }
            }
        }

        Ok(())
    }
}
