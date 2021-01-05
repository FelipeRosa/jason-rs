use anyhow::Result;
use hyper::client::HttpConnector;
use hyper_tls::HttpsConnector;

use crate::{http::RawClient, transport::Transport, Request, Response};

/// HTTPS client.
pub struct Client {
    raw: RawClient<HttpsConnector<HttpConnector>>,
}

impl Client {
    /// Creates a new HTTPS client connected to the server at the given URL.
    pub fn new(addr: &str) -> Result<Self> {
        Ok(Client {
            raw: RawClient::new(addr, HttpsConnector::new())?,
        })
    }
}

impl Transport for Client {
    fn request(
        &self,
        req: Request,
    ) -> std::pin::Pin<Box<dyn futures::Future<Output = Result<Response>> + Send + '_>> {
        self.raw.request(req)
    }
}
