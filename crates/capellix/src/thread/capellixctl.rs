use std::net::SocketAddr;

use anyhow::{anyhow, Result};
use clap::Parser;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::{
    thread::socket::socket_command::SocketCommand, thread::socket::socket_response::SocketResponse,
};

/// Control program for the capellix daemon
#[derive(Parser)]
pub struct CapellixCtl {
    /// Socket address
    #[clap(short, long, default_value = "127.0.0.1:27359")]
    address: SocketAddr,

    /// Command to execute
    command: Vec<String>,
}

impl CapellixCtl {
    pub fn run(self) -> Result<()> {
        let runtime = tokio::runtime::Runtime::new()?;
        let _guard = runtime.enter();
        runtime.block_on(self.run_async())
    }

    pub async fn run_async(self) -> Result<()> {
        let command: SocketCommand = self
            .command
            .join(" ")
            .parse()
            .map_err(|_| anyhow!("Failed to parse command"))?;

        let mut socket = tokio::net::TcpStream::connect(&self.address).await?;

        socket
            .write(&[vec![b'C', b'P', b'L', b'X'], Vec::from(command)].concat())
            .await?;

        let mut buf = [0; 16];
        socket.read(&mut buf).await?;

        match SocketResponse::try_from(&buf[..])? {
            SocketResponse::GetCoolantTemp(temp) => println!("{temp:}"),
            SocketResponse::GetPumpSpeed(speed) => println!("{speed:}"),
            SocketResponse::SetPumpSpeed(success) => {
                if !success {
                    std::process::exit(1)
                }
            }
            SocketResponse::SetColors(success) => {
                if !success {
                    std::process::exit(1)
                }
            }
        }

        Ok(())
    }
}
