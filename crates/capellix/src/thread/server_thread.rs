use std::{net::SocketAddr, sync::Arc};

use anyhow::Result;
use log::{debug, info};
use tokio::{
    net::{TcpListener, TcpStream, UdpSocket},
    sync::{watch, mpsc},
    task::{spawn, JoinHandle},
};
use tokio_stream::{
    wrappers::{TcpListenerStream, WatchStream},
    StreamExt,
};

use crate::{
    then::Then,
    thread::{
        capellix::SharedState,
        print_thread_result,
        socket::{SocketCommandCodec, SocketThread},
    },
};

use super::{capellix::Colors, pump_target::Fan, socket::socket_command::SocketCommand};

#[derive(Debug)]
pub struct ServerThread {
    state: Arc<SharedState>,
    set_pump_speed_tx: mpsc::Sender<(Fan, u16)>,
    set_colors_tx: Arc<watch::Sender<Colors>>,
    exit_rx: watch::Receiver<bool>,
    address: SocketAddr,
    sockets: Vec<JoinHandle<()>>,
}

enum ServerEvent {
    TcpConnection(tokio::io::Result<TcpStream>),
    UdpPacket(Result<(SocketCommand, SocketAddr)>),
    RunningChanged(bool),
}

impl ServerThread {
    pub fn new(
        state: Arc<SharedState>,
        set_pump_speed_tx: mpsc::Sender<(Fan, u16)>,
        set_colors_tx: Arc<watch::Sender<Colors>>,
        exit_rx: watch::Receiver<bool>,
        address: SocketAddr,
    ) -> Self {
        ServerThread {
            state,
            set_pump_speed_tx,
            set_colors_tx,
            exit_rx,
            address,
            sockets: vec![],
        }
    }

    pub async fn run(mut self) -> Result<()> {
        let tcp_listener = TcpListenerStream::new(TcpListener::bind(&self.address).await?);

        let udp_listener = tokio_util::udp::UdpFramed::new(
            UdpSocket::bind(&self.address).await?,
            SocketCommandCodec::default(),
        );

        let exit_rx = self.exit_rx.clone();
        let exit = WatchStream::new(self.exit_rx);

        info!("Server listening on {:?}", self.address);

        let mut events = futures::stream_select!(
            tcp_listener.map(ServerEvent::TcpConnection),
            udp_listener.map(ServerEvent::UdpPacket),
            exit.map(ServerEvent::RunningChanged),
        );

        while let Some(event) = events.next().await {
            match event {
                ServerEvent::TcpConnection(stream) => {
                    let stream = stream?;

                    info!("Accepted TCP connection");
                    let state = self.state.clone();
                    let set_pump_speed_tx = self.set_pump_speed_tx.clone();
                    let set_colors_tx = self.set_colors_tx.clone();
                    let exit_rx = exit_rx.clone();

                    let join_handle = spawn(async move {
                        SocketThread::new(state, set_pump_speed_tx, set_colors_tx, exit_rx, stream)
                            .run()
                            .await
                            .then(print_thread_result("SocketThread"))
                            .ok();
                    });

                    self.sockets.push(join_handle);
                }
                ServerEvent::UdpPacket(packet) => {
                    let (command, _) = packet?;
                    debug!("Received UDP packet");
                    command
                        .run(
                            &self.state,
                            &self.set_pump_speed_tx,
                            &self.set_colors_tx,
                            vec![],
                        )
                        .await?;
                }
                ServerEvent::RunningChanged(running) => {
                    if !running {
                        info!("ServerThread received Exit event");
                        break;
                    }
                }
            }
        }

        for handle in self.sockets.into_iter() {
            handle.await?;
        }

        Ok(())
    }
}
