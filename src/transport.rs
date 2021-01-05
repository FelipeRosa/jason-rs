pub(crate) mod helpers;

use std::{pin::Pin, task::Poll};

use anyhow::Result;
use futures::{Future, Stream};
use tokio::sync::mpsc;

use crate::{Notification, Request, Response};

/// Transport is able to send requests.
pub trait Transport: Send + Sync {
    /// Sends a request serializing and deserializing request params
    /// and response result and error data as serde_json::Value.
    fn request(&self, req: Request) -> Pin<Box<dyn Future<Output = Result<Response>> + Send + '_>>;
}

/// Stream of notifications.
pub struct NotificationStream {
    rx: mpsc::UnboundedReceiver<Notification>,
}

impl NotificationStream {
    pub fn new(rx: mpsc::UnboundedReceiver<Notification>) -> Self {
        Self { rx }
    }
}

impl Unpin for NotificationStream {}

impl Stream for NotificationStream {
    type Item = Notification;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        self.rx.poll_recv(cx)
    }
}

/// Notification transport is able to receive notifications.
pub trait NotificationTransport {
    fn notification_stream(&self) -> Result<NotificationStream>;
}
