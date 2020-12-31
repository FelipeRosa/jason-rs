use std::{collections::HashMap, task::Poll};

use futures::{SinkExt, Stream, StreamExt};
use serde::de::DeserializeOwned;
use tokio::{
    net::TcpStream,
    sync::{mpsc, oneshot},
};
use tokio_tungstenite::{tungstenite, WebSocketStream};

use crate::{Notification, Request, RequestId, Response, Transport};

/// Stream of JSON-RPC notifications.
pub struct NotificationStream<P> {
    rx: mpsc::UnboundedReceiver<Notification>,
    _p: std::marker::PhantomData<P>,
}

impl<P> Unpin for NotificationStream<P> {}

impl<P: DeserializeOwned> Stream for NotificationStream<P> {
    type Item = Notification<P>;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        match self.rx.poll_recv(cx) {
            Poll::Ready(Some(n)) => match serde_json::from_value(n.params) {
                Ok(params) => Poll::Ready(Some(Notification {
                    jsonrpc: n.jsonrpc,
                    method: n.method,
                    params,
                })),

                Err(_) => Poll::Pending,
            },

            Poll::Ready(None) => Poll::Ready(None),

            Poll::Pending => Poll::Pending,
        }
    }
}

/// JSON-RPC websocket client.
#[derive(Debug, Clone)]
pub struct Client {
    client_req_tx: mpsc::UnboundedSender<(Request, oneshot::Sender<Result<Response, String>>)>,
    client_notify_req_tx: mpsc::UnboundedSender<mpsc::UnboundedSender<Notification>>,
}

impl Client {
    pub async fn new(url: &str) -> Result<Self, String> {
        let (ws_stream, _) = tokio_tungstenite::connect_async(url)
            .await
            .map_err(|err| err.to_string())?;

        let (client_req_tx, client_req_rx) = mpsc::unbounded_channel();
        let (client_notify_req_tx, client_notify_req_rx) = mpsc::unbounded_channel();

        tokio::spawn(client_task(ws_stream, client_req_rx, client_notify_req_rx));

        Ok(Self {
            client_req_tx,
            client_notify_req_tx,
        })
    }

    pub fn notification_stream<P>(&self) -> NotificationStream<P>
    where
        P: DeserializeOwned,
    {
        let (tx, rx) = mpsc::unbounded_channel();
        self.client_notify_req_tx.send(tx).unwrap();

        NotificationStream {
            rx,
            _p: std::marker::PhantomData::default(),
        }
    }
}

impl Transport for Client {
    fn request_raw(
        &self,
        req: Request,
    ) -> std::pin::Pin<Box<dyn futures::Future<Output = Result<Response, String>> + '_>> {
        Box::pin(async move {
            let (client_tx, client_rx) = oneshot::channel();

            let req = Request {
                jsonrpc: req.jsonrpc,
                id: req.id,
                method: req.method,
                params: serde_json::to_value(req.params).map_err(|err| err.to_string())?,
            };

            self.client_req_tx
                .send((req, client_tx))
                .map_err(|err| err.to_string())?;

            client_rx.await.map_err(|err| err.to_string())?
        })
    }
}

async fn client_task(
    ws_stream: WebSocketStream<TcpStream>,
    client_req_rx: mpsc::UnboundedReceiver<(Request, oneshot::Sender<Result<Response, String>>)>,
    client_notify_req_rx: mpsc::UnboundedReceiver<mpsc::UnboundedSender<Notification>>,
) {
    log::debug!("spawned websocket client task");

    let mut pending_requests: HashMap<RequestId, oneshot::Sender<Result<Response, String>>> =
        HashMap::new();

    let mut notification_txs: Vec<mpsc::UnboundedSender<Notification>> = vec![];

    let (mut ws_sink, mut ws_stream) = ws_stream.split();

    let mut client_req_rx = Box::pin(mpsc_receiver_stream(client_req_rx)).fuse();
    let mut client_notify_req_rx = Box::pin(mpsc_receiver_stream(client_notify_req_rx)).fuse();

    while !client_req_rx.is_done() {
        tokio::select! {
            c = client_req_rx.next() => if let Some((req, tx)) = c {
                let req_str = serde_json::to_string(&req).unwrap();

                if pending_requests.insert(req.id.clone(), tx).is_some() {
                    log::warn!("replaced existing pending request with the same ID");
                }

                let result = ws_sink
                    .send(tungstenite::Message::text(req_str))
                    .await;

                if let Err(err) = result {
                    pending_requests.remove(&req.id).unwrap().send(Err(err.to_string())).unwrap();
                }
            },

            n = client_notify_req_rx.next() => if let Some(tx) = n {
                notification_txs.push(tx);
            },

            a = ws_stream.next() => if let Some(a) = a {
                match a {
                    Ok(msg) => {
                        let data = msg.into_data();

                        let value: serde_json::Value = serde_json::from_slice(&data).unwrap();

                        let is_response = if let Some(o) = value.as_object() {
                            o.contains_key("id")
                        } else {
                            false
                        };

                        if is_response {
                            let res: Response = serde_json::from_value(value)
                                    .expect("failed to deserialize jsonrpc response");

                            if let Some(tx) = pending_requests.remove(res.id()) {
                                tx.send(Ok(res)).unwrap();
                            }
                        } else {
                            let notf: Notification = serde_json::from_value(value)
                                .expect("failed to deserialize jsonrpc notification");

                            let mut closed_tx_indexes = vec![];

                            for (i, tx) in notification_txs.iter().enumerate() {
                                if tx.is_closed() {
                                    closed_tx_indexes.push(i);
                                } else if let Err(_) = tx.send(notf.clone()) {
                                    log::error!("websocket notification receive error");
                                }
                            }

                            for ix in closed_tx_indexes {
                                notification_txs.remove(ix);
                            }
                        }
                    }

                    Err(err) => log::error!("websocket stream error: {:?}", err)
                }
            },
        }
    }
}

fn mpsc_receiver_stream<T: Unpin>(mut c: mpsc::UnboundedReceiver<T>) -> impl Stream<Item = T> {
    async_stream::stream! {
        while let Some(item) = c.recv().await {
            yield item;
        }
    }
}

#[cfg(test)]
mod test {
    use crate::{ErrorRes, ProtocolVersion, ResultRes};

    use super::*;

    use serde::{Deserialize, Serialize};
    use tokio_tungstenite::tungstenite;

    #[derive(Debug, Serialize, Deserialize)]
    struct AddParams {
        a: i32,
        b: i32,
    }

    lazy_static::lazy_static! {
        static ref TEST_MUTEX: tokio::sync::Mutex<bool> = tokio::sync::Mutex::new(false);
    }

    async fn start_jsonrpc_test_server() -> tokio::task::JoinHandle<()> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:3001")
            .await
            .expect("failed to start tcp listener");

        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                tokio::spawn(async move {
                    let mut ws_stream = tokio_tungstenite::accept_async(stream)
                        .await
                        .expect("failed to accept websocket connection");

                    let mut ticker = tokio::time::interval(std::time::Duration::from_millis(10));

                    loop {
                        tokio::select! {
                            _ = ticker.tick() => {
                                ws_stream
                                .send(tokio_tungstenite::tungstenite::Message::text(
                                    serde_json::to_string(&Notification {
                                        jsonrpc: ProtocolVersion::TwoPointO,
                                        method: "test_notification".to_string(),
                                        params: serde_json::json!(16i32),
                                    })
                                    .unwrap(),
                                ))
                                .await
                                .expect("failed to send test notification");
                            },

                            msg = ws_stream.next() => if let Some(Ok(msg)) = msg {
                                let msg_body = msg.into_text().expect("expected text messages");

                                let rpc_req: Request = serde_json::from_str(&msg_body)
                                    .expect("failed to parse JSONRPC message");

                                let rpc_res = match rpc_req.method.as_str() {
                                    "add" => {
                                        let params: AddParams = serde_json::from_value(rpc_req.params)
                                            .expect("failed to parse add method params");

                                        Response::<_, ()>(Ok(ResultRes {
                                            jsonrpc: ProtocolVersion::TwoPointO,
                                            id: rpc_req.id,
                                            result: params.a + params.b,
                                        }))
                                    }

                                    _ => Response::<i32, ()>(Err(ErrorRes {
                                        jsonrpc: ProtocolVersion::TwoPointO,
                                        id: rpc_req.id,
                                        code: -32601,
                                        message: "Method not found".to_string(),
                                        data: None,
                                    })),
                                };

                                ws_stream
                                    .send(tungstenite::Message::text(
                                        serde_json::to_string(&rpc_res)
                                            .expect("failed serializing jsonrpc response"),
                                    ))
                                    .await
                                    .expect("failed sending jsonrpc response");
                            }
                        }
                    }
                });
            }
        })
    }

    #[tokio::test]
    async fn it_works() {
        let guard = TEST_MUTEX.lock().await;
        if !*guard {
            start_jsonrpc_test_server().await;
        }

        let ws = Client::new("ws://127.0.0.1:3001")
            .await
            .expect("failed connecting to jsonrpc test server");

        let not: Notification<i32> = ws
            .notification_stream()
            .next()
            .await
            .expect("failed receiving test notification");

        println!("{:?}", not);

        for _ in 1..=10 {
            let res: Response<i32> = ws
                .request(Request {
                    jsonrpc: ProtocolVersion::TwoPointO,
                    id: RequestId::String("1".to_string()),
                    method: "add".to_string(),
                    params: AddParams { a: 1, b: 2 },
                })
                .expect("failed serializing test server request")
                .await
                .expect("test request failed");

            println!("{:?}", res);
        }
    }
}
