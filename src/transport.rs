use std::{pin::Pin, task::Poll};

use anyhow::Result;
use futures::{Future, Stream};
use serde::{de::DeserializeOwned, Serialize};
use tokio::sync::mpsc;

use crate::{ErrorRes, Notification, Request, Response, ResultRes};

pub trait Transport {
    fn request_raw(&self, req: Request) -> Pin<Box<dyn Future<Output = Result<Response>> + '_>>;

    fn request<P, R, E>(
        &self,
        req: Request<P>,
    ) -> Result<Pin<Box<dyn Future<Output = Result<Response<R, E>>> + '_>>>
    where
        P: Serialize,
        R: DeserializeOwned,
        E: DeserializeOwned,
    {
        let raw_req = Request {
            jsonrpc: req.jsonrpc,
            method: req.method,
            id: req.id,
            params: serde_json::to_value(req.params)?,
        };

        Ok(Box::pin(async move {
            let raw_res: Response = self.request_raw(raw_req).await?;

            let res: Response<R, E> = match raw_res.into_result() {
                Ok(res) => Response(Ok(ResultRes {
                    jsonrpc: res.jsonrpc,
                    id: res.id,
                    result: serde_json::from_value(res.result)?,
                })),

                Err(res) => {
                    let data = if let Some(data) = res.data {
                        Some(serde_json::from_value(data)?)
                    } else {
                        None
                    };

                    Response(Err(ErrorRes {
                        jsonrpc: res.jsonrpc,
                        id: res.id,
                        code: res.code,
                        message: res.message,
                        data,
                    }))
                }
            };

            Ok(res)
        }))
    }
}

/// Stream of JSON-RPC notifications.
pub struct NotificationStream<P> {
    rx: mpsc::UnboundedReceiver<Notification>,
    _p: std::marker::PhantomData<P>,
}

impl<P> NotificationStream<P> {
    pub fn new(rx: mpsc::UnboundedReceiver<Notification>) -> Self {
        Self {
            rx,
            _p: std::marker::PhantomData::default(),
        }
    }
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

pub trait NotificationTransport {
    fn notification_stream<P: DeserializeOwned>(&self) -> NotificationStream<P>;
}
