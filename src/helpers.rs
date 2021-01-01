use std::pin::Pin;

use async_stream::stream;
use futures::Stream;
use tokio::sync::mpsc;

pub fn mpsc_receiver_stream<T: Unpin>(
    mut c: mpsc::UnboundedReceiver<T>,
) -> Pin<Box<impl Stream<Item = T>>> {
    Box::pin(stream! {
        while let Some(item) = c.recv().await {
            yield item;
        }
    })
}

#[cfg(unix)]
mod unix_helpers {
    use super::*;

    use tokio::io::AsyncReadExt;

    pub fn unix_read_stream(
        mut r: tokio::net::unix::OwnedReadHalf,
    ) -> Pin<Box<impl Stream<Item = Result<Vec<u8>, String>>>> {
        Box::pin(stream! {
            loop {
                let mut buf = Vec::with_capacity(128);

                yield r.read_buf(&mut buf).await.map_err(|err| err.to_string()).map(|_| buf);
            }
        })
    }
}

#[cfg(unix)]
pub use unix_helpers::*;
