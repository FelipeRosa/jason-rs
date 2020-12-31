use std::str::FromStr;

use hyper::{client::HttpConnector, Uri};

use crate::{Request, Response, Transport};

/// JSON-RPC HTTP client.
pub struct Client {
    uri: Uri,
    http_client: hyper::Client<HttpConnector>,
}

impl Client {
    pub fn new(addr: &str) -> Result<Self, String> {
        Ok(Self {
            uri: Uri::from_str(addr).map_err(|err| err.to_string())?,
            http_client: hyper::Client::new(),
        })
    }
}

impl Transport for Client {
    fn request_raw(
        &self,
        req: Request,
    ) -> std::pin::Pin<Box<dyn futures::Future<Output = Result<Response, String>> + '_>> {
        Box::pin(async move {
            let req_uri = self.uri.clone();
            let req_body = serde_json::to_string(&req).map_err(|err| err.to_string())?;

            let http_req = hyper::Request::builder()
                .header(hyper::header::USER_AGENT, "jason.rs/0.1.0")
                .header(hyper::header::CONTENT_TYPE, "application/json")
                .method(hyper::Method::POST)
                .uri(req_uri)
                .body(hyper::Body::from(req_body))
                .map_err(|err| err.to_string())?;

            let res_body = self
                .http_client
                .request(http_req)
                .await
                .map_err(|err| err.to_string())?
                .into_body();

            let res_data = hyper::body::to_bytes(res_body)
                .await
                .map_err(|err| err.to_string())?;

            let parsed_res: Response =
                serde_json::from_slice(&res_data).map_err(|err| err.to_string())?;

            Ok(parsed_res)
        })
    }
}

#[cfg(test)]
mod test {
    use crate::{ProtocolVersion, Request, RequestId};

    use super::*;

    use std::convert::Infallible;

    async fn test_server_handle(
        _req: hyper::Request<hyper::Body>,
    ) -> Result<hyper::Response<hyper::Body>, Infallible> {
        Ok::<_, Infallible>(hyper::Response::new(hyper::Body::from(
            serde_json::json!({
                "jsonrpc": "2.0",
                "id": "1",
                "result": 7,
            })
            .to_string(),
        )))
    }

    async fn start_jsonrpc_test_server() {
        let server = hyper::Server::bind(&std::net::SocketAddr::from(([127, 0, 0, 1], 3000)));

        let make_service = hyper::service::make_service_fn(|_conn| async move {
            Ok::<_, Infallible>(hyper::service::service_fn(test_server_handle))
        });

        tokio::spawn(server.serve(make_service));
    }

    #[tokio::test]
    async fn it_works() {
        start_jsonrpc_test_server().await;

        let c = Client::new("http://127.0.0.1:3000").expect("failed to create client");

        let res: Response<i32> = c
            .request(Request {
                jsonrpc: ProtocolVersion::TwoPointO,
                id: RequestId::String("1".to_string()),
                method: "some_method".to_string(),
                params: (),
            })
            .expect("failed serializing request")
            .await
            .expect("test request failed");

        println!("{:?}", res);
    }
}