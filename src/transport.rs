pub(crate) mod helpers;

use std::{pin::Pin, task::Poll};

use anyhow::Result;
use futures::{Future, Stream};
use serde::{de::DeserializeOwned, Serialize};
use tokio::sync::mpsc;

use crate::{ErrorRes, Notification, Request, Response, ResultRes};

/// Transport is able to send requests.
pub trait Transport: Send + Sync {
    /// Sends a request serializing and deserializing request params
    /// and response result and error data as serde_json::Value.
    fn request_raw(
        &self,
        req: Request,
    ) -> Pin<Box<dyn Future<Output = Result<Response>> + Send + '_>>;

    /// Sends a request serializing and deserializing request params
    /// and response result and error data to the given generic types.
    fn request<P, R, E>(
        &self,
        req: Request<P>,
    ) -> Result<Pin<Box<dyn Future<Output = Result<Response<R, E>>> + Send + '_>>>
    where
        P: Serialize,
        R: DeserializeOwned,
        E: DeserializeOwned,
    {
        let raw_params = match req.params {
            Some(params) => Some(params.try_map(serde_json::to_value)?),
            None => None,
        };

        let raw_req = Request {
            jsonrpc: req.jsonrpc,
            method: req.method,
            id: req.id,
            params: raw_params,
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

/// Stream of notifications.
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
            Poll::Ready(Some(n)) => {
                let params = match n.params {
                    Some(params) => params.try_map(serde_json::from_value).map(Option::Some),

                    None => std::result::Result::Ok(None),
                };

                match params {
                    Ok(params) => Poll::Ready(Some(Notification {
                        jsonrpc: n.jsonrpc,
                        method: n.method,
                        params,
                    })),

                    Err(_) => Poll::Pending,
                }
            }

            Poll::Ready(None) => Poll::Ready(None),

            Poll::Pending => Poll::Pending,
        }
    }
}

/// Notification transport is able to receive notifications.
pub trait NotificationTransport {
    fn notification_stream<P: DeserializeOwned>(&self) -> Result<NotificationStream<P>>;
}
