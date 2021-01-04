use anyhow::Result;
use futures::StreamExt;
use serde::de::DeserializeOwned;
use std::collections::HashMap;
use tokio::{
    io::AsyncWriteExt,
    net::UnixStream,
    sync::{mpsc, oneshot},
};

use crate::{
    transport::{helpers, NotificationStream, NotificationTransport, Transport},
    Notification, Request, RequestId, Response,
};

/// IPC client.
#[derive(Debug, Clone)]
pub struct Client {
    client_req_tx: mpsc::UnboundedSender<(Request, oneshot::Sender<Result<Response>>)>,
    client_notify_req_tx: mpsc::UnboundedSender<mpsc::UnboundedSender<Notification>>,
}

impl Client {
    /// Creates a new IPC client connected to the socket at the given path.
    pub async fn new<P: AsRef<std::path::Path>>(path: P) -> Result<Self> {
        let uds_stream = UnixStream::connect(path).await?;

        Ok(Self::from_stream(uds_stream))
    }

    /// Creates a new IPC client using the given UnixStream.
    pub fn from_stream(st: UnixStream) -> Self {
        let (client_req_tx, client_req_rx) = mpsc::unbounded_channel();
        let (client_notify_req_tx, client_notify_req_rx) = mpsc::unbounded_channel();

        tokio::spawn(client_task(st, client_req_rx, client_notify_req_rx));

        Client {
            client_req_tx,
            client_notify_req_tx,
        }
    }
}

impl Transport for Client {
    fn request_raw(
        &self,
        req: crate::Request,
    ) -> std::pin::Pin<Box<dyn futures::Future<Output = Result<Response>> + Send + '_>> {
        Box::pin(async move {
            let (client_tx, client_rx) = oneshot::channel();

            self.client_req_tx.send((req, client_tx))?;

            client_rx.await?
        })
    }
}

impl NotificationTransport for Client {
    fn notification_stream<P: DeserializeOwned>(&self) -> Result<NotificationStream<P>> {
        let (tx, rx) = mpsc::unbounded_channel();
        self.client_notify_req_tx.send(tx)?;

        Ok(NotificationStream::new(rx))
    }
}

async fn client_task(
    uds_stream: UnixStream,
    client_req_rx: mpsc::UnboundedReceiver<(Request, oneshot::Sender<Result<Response>>)>,
    client_notify_req_rx: mpsc::UnboundedReceiver<mpsc::UnboundedSender<Notification>>,
) {
    log::debug!("spawned IPC client task");

    let mut pending_requests: HashMap<RequestId, oneshot::Sender<Result<Response>>> =
        HashMap::new();

    let mut notification_txs: Vec<mpsc::UnboundedSender<Notification>> = vec![];

    let (uds_reader, mut uds_writer) = uds_stream.into_split();

    let mut uds_reader = helpers::unix_read_stream(uds_reader).fuse();
    let mut client_req_rx = helpers::mpsc_receiver_stream(client_req_rx).fuse();
    let mut client_notify_req_rx = helpers::mpsc_receiver_stream(client_notify_req_rx).fuse();

    let mut buffer = vec![];

    loop {
        tokio::select! {
            c = client_req_rx.next() => if let Some((req, tx)) = c {
                let req_se = serde_json::to_string(&req);

                let req_str = match req_se {
                    Ok(req_str) => req_str,
                    Err(err) => {
                        log::error!("failed serializing JSON-RPC request");

                        // Should we unwrap?
                        let _ = tx.send(Err(anyhow::anyhow!(err)));

                        continue;
                    }
                };

                if pending_requests.insert(req.id.clone(), tx).is_some() {
                    log::warn!("replaced existing pending request with the same ID");
                }

                let result = uds_writer
                    .write_all(req_str.as_bytes())
                    .await;

                if let Err(err) = result {
                    // Should always match but let's be safe.
                    if let Some(tx) = pending_requests.remove(&req.id) {
                        let _ = tx.send(Err(anyhow::anyhow!(err)));
                    }
                }
            },

            n = client_notify_req_rx.next() => if let Some(tx) = n {
                notification_txs.push(tx);
            },

            bs = uds_reader.next() => if let Some(Ok(bs)) = bs {
                buffer.extend_from_slice(&bs);

                let consumed_len = {
                    let mut de: serde_json::StreamDeserializer<_, serde_json::Value> =
                        serde_json::Deserializer::from_slice(&buffer).into_iter();

                    while let Some(Ok(value)) = de.next() {
                        let is_response = if let Some(o) = value.as_object() {
                            o.contains_key("id")
                        } else {
                            false
                        };

                        if is_response {
                            if let Ok(res) = serde_json::from_value::<Response>(value) {
                                if let Some(tx) = pending_requests.remove(res.id()) {
                                    // Should we unwrap?
                                    let _ = tx.send(Ok(res));
                                }
                            }
                        } else {
                            if let Ok(notf) = serde_json::from_value::<Notification>(value) {
                                let mut closed_tx_indexes = vec![];

                                for (i, tx) in notification_txs.iter().enumerate() {
                                    if tx.is_closed() {
                                        closed_tx_indexes.push(i);
                                    } else {
                                        // Should we unwrap?
                                        let _ = tx.send(notf.clone());
                                    }
                                }

                                for ix in closed_tx_indexes {
                                    notification_txs.remove(ix);
                                }
                            }
                        }
                    }

                    de.byte_offset()
                };

                buffer.copy_within(consumed_len.., 0);
                buffer.truncate(buffer.len() - consumed_len);
            }
        };
    }
}

#[cfg(test)]
mod test {
    use crate::{transport::NotificationTransport, ProtocolVersion, RequestParams, ResultRes};

    use super::*;

    use tokio::net::UnixStream;

    fn start_jsonrpc_test_server(st: UnixStream) {
        tokio::spawn(async move {
            let (reader, mut writer) = st.into_split();
            let mut reader = helpers::unix_read_stream(reader);

            let mut ticker = tokio::time::interval(std::time::Duration::from_millis(10));

            loop {
                tokio::select! {
                    _ = ticker.tick() => {
                        writer.write_all(serde_json::to_string(&Notification {
                            jsonrpc: ProtocolVersion::TwoPointO,
                            method: "test_notification".to_string(),
                            params: Some(vec![serde_json::json!(15i32)].into()),
                        })
                        .unwrap()
                        .as_bytes())
                        .await
                        .expect("failed sending test notification");
                    },

                    bs = reader.next() => if let Some(Ok(bytes)) = bs {
                        let rpc_req: Request<()> =
                            serde_json::from_slice(&bytes).expect("failed deserializing test request");

                        let rpc_res: Response<i32> = Response(Ok(ResultRes {
                            jsonrpc: ProtocolVersion::TwoPointO,
                            id: rpc_req.id,
                            result: 16i32,
                        }));

                        writer
                            .write_all(
                                serde_json::to_string(&rpc_res)
                                    .expect("failed serializing test reponse")
                                    .as_bytes(),
                            )
                            .await
                            .expect("failed sending test response");
                    }
                }
            }
        });
    }

    #[tokio::test]
    async fn it_works() {
        let (st1, st2) = UnixStream::pair().expect("failed creating test unix streams");

        start_jsonrpc_test_server(st2);

        let c = Client::from_stream(st1);

        let not: Notification<i32> = c
            .notification_stream()
            .expect("failed creating notification stream")
            .next()
            .await
            .expect("failed receiving test notification");

        println!("{:?}", not);

        for _ in 1..=10 {
            let res: Response<i32> = c
                .request(Request::<()> {
                    jsonrpc: ProtocolVersion::TwoPointO,
                    id: RequestId::String("1".to_string()),
                    method: "some_method".to_string(),
                    params: None,
                })
                .expect("failed serializing test server request")
                .await
                .expect("test request failed");

            println!("{:?}", res);
        }
    }
}
