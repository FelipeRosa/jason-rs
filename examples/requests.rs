use jason::{https::Client, transport::Transport, ProtocolVersion, RequestId};
use serde_json::json;

#[tokio::main]
async fn main() {
    let client = Client::new("https://change/me").expect("failed creating client");

    let res: jason::Response = client
        .request(jason::Request {
            jsonrpc: ProtocolVersion::TwoPointO,
            id: RequestId::Number(1),
            method: "some_method".to_owned(),
            params: Some(vec![json!("param")].into()),
        })
        .await
        .expect("request failed");

    println!("{:?}", res);
}
