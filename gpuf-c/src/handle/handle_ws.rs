use crate::handle::{WorkerHandle,WSWorker};
use crate::util::cmd::Args;

use anyhow::Result;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio_tungstenite::connect_async;
use futures_util::StreamExt;

//use futures_util::SinkExt;
//use tokio_tungstenite::{WebSocketStream,tungstenite::protocol::Message};

impl WSWorker {
    pub async fn new(args: Args) -> Result<Self> {
        let url = "ws://example.com/ws";
        let (ws_stream, _) = connect_async(url).await?;
        let (write, read) = ws_stream.split();
        Ok(Self {
            reader: Arc::new(Mutex::new(read)),
            writer: Arc::new(Mutex::new(write)),
            args,
        })
    }   
}


impl WorkerHandle for WSWorker {
    async fn login(&self) -> Result<()> {
        todo!()
    }

    async fn handler(&self) -> Result<()> {
        todo!()
    }

    async fn model_task(&self, _get_last_models: &str) -> Result<()> {
        todo!()
    }

    async fn heartbeat_task(&self) -> Result<()> {
        todo!()
    }
}