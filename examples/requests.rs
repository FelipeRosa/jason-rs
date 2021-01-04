use jason::{https::Client, transport::Transport, ProtocolVersion, RequestId};

#[tokio::main]
async fn main() {
    let client = Client::new("https://change/me").expect("failed creating client");

    let res: jason::Response<String> = client
        .request(jason::Request {
            jsonrpc: ProtocolVersion::TwoPointO,
            id: RequestId::Number(1),
            method: "some_method".to_owned(),
            params: Some(vec!["param"].into()),
        })
        .expect("failed serializing request")
        .await
        .expect("request failed");

    println!("{:?}", res);
}
