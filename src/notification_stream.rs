use std::task::Poll;

use futures::Stream;
use serde::de::DeserializeOwned;
use tokio::sync::mpsc;

use crate::Notification;

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
