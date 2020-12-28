#[cfg(feature = "ws")]
pub mod websocket;

use async_trait::async_trait;
use serde::{de::DeserializeOwned, Deserialize, Serialize};

pub use serde_json::Number;

#[async_trait]
pub trait Transport {
    async fn request<P, R, E>(&self, req: Request<P>) -> Result<Response<R, E>, String>
    where
        P: Serialize + Send,
        R: DeserializeOwned,
        E: DeserializeOwned;
}

/// Represents JSON-RPC protocol versions.
#[derive(Debug, Eq, PartialEq, Copy, Clone)]
pub enum ProtocolVersion {
    TwoPointO,
}

impl Serialize for ProtocolVersion {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            ProtocolVersion::TwoPointO => serializer.serialize_str("2.0"),
        }
    }
}

impl<'a> Deserialize<'a> for ProtocolVersion {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'a>,
    {
        let v: String = Deserialize::deserialize(deserializer)?;

        if v == "2.0" {
            Ok(ProtocolVersion::TwoPointO)
        } else {
            Err(serde::de::Error::custom("invalid RPC protocol version"))
        }
    }
}

/// Represents a JSON-RPC request ID.
#[derive(Debug, Eq, PartialEq, Clone)]
pub enum RequestId {
    String(String),
    Number(Number),
}

impl std::hash::Hash for RequestId {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        match self {
            RequestId::String(s) => state.write(s.as_bytes()),
            RequestId::Number(n) => state.write(n.to_string().as_bytes()),
        }
    }
}

impl Serialize for RequestId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            RequestId::String(s) => serializer.serialize_str(s.as_str()),
            RequestId::Number(n) => Serialize::serialize(n, serializer),
        }
    }
}

impl<'a> Deserialize<'a> for RequestId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'a>,
    {
        let val: serde_json::Value = Deserialize::deserialize(deserializer)?;

        match val {
            serde_json::Value::Number(n) => Ok(RequestId::Number(n)),
            serde_json::Value::String(s) => Ok(RequestId::String(s)),

            _ => Err(serde::de::Error::custom(
                "request id must be either string or number",
            )),
        }
    }
}

/// Represents a JSON-RPC request.
#[derive(Debug, Serialize, Deserialize, Eq, PartialEq, Clone)]
pub struct Request<P = serde_json::Value> {
    pub jsonrpc: ProtocolVersion,
    pub id: RequestId,
    pub method: String,
    pub params: P,
}

impl<P> Request<P> {
    pub fn map<Q, F: FnOnce(P) -> Q>(self, f: F) -> Request<Q> {
        Request {
            jsonrpc: self.jsonrpc,
            id: self.id,
            method: self.method,
            params: f(self.params),
        }
    }
}

/// Represents a JSON-RPC notification.
#[derive(Debug, Serialize, Deserialize, Eq, PartialEq, Clone)]
pub struct Notification<P = serde_json::Value> {
    pub jsonrpc: ProtocolVersion,
    pub method: String,
    pub params: P,
}

impl<P> Notification<P> {
    pub fn map<Q, F: FnOnce(P) -> Q>(self, f: F) -> Notification<Q> {
        Notification {
            jsonrpc: self.jsonrpc,
            method: self.method,
            params: f(self.params),
        }
    }
}

/// Represents a JSON-RPC successful response.
#[derive(Debug, Eq, PartialEq, Clone, Serialize)]
pub struct ResultRes<R> {
    pub jsonrpc: ProtocolVersion,
    pub id: RequestId,
    pub result: R,
}

impl<R> ResultRes<R> {
    pub fn map<T, F: FnOnce(R) -> T>(self, f: F) -> ResultRes<T> {
        ResultRes {
            jsonrpc: self.jsonrpc,
            id: self.id,
            result: f(self.result),
        }
    }
}

/// Represents a JSON-RPC failed response.
#[derive(Debug, Eq, PartialEq, Clone, Serialize)]
pub struct ErrorRes<E> {
    pub jsonrpc: ProtocolVersion,
    pub id: RequestId,
    pub code: i64,
    pub message: String,
    pub data: Option<E>,
}

impl<E> ErrorRes<E> {
    pub fn map<T, F: FnOnce(Option<E>) -> Option<T>>(self, f: F) -> ErrorRes<T> {
        ErrorRes {
            jsonrpc: self.jsonrpc,
            id: self.id,
            code: self.code,
            message: self.message,
            data: f(self.data),
        }
    }
}

/// Represents a JSON-RPC response which can be successful or failed.
#[derive(Debug, Eq, PartialEq, Clone)]
pub struct Response<R = serde_json::Value, E = serde_json::Value>(
    std::result::Result<ResultRes<R>, ErrorRes<E>>,
);

impl<R, E> Response<R, E> {
    pub fn id(&self) -> &RequestId {
        match self {
            Response(Ok(res)) => &res.id,
            Response(Err(res)) => &res.id,
        }
    }

    pub fn map<T, F: FnOnce(R) -> T>(self, f: F) -> Response<T, E> {
        match self {
            Response(Ok(res)) => Response(Ok(res.map(f))),
            Response(Err(res)) => Response(Err(res)),
        }
    }

    pub fn as_result(&self) -> &std::result::Result<ResultRes<R>, ErrorRes<E>> {
        &self.0
    }

    pub fn into_result(self) -> std::result::Result<ResultRes<R>, ErrorRes<E>> {
        self.0
    }
}

impl<R, E> Into<std::result::Result<ResultRes<R>, ErrorRes<E>>> for Response<R, E> {
    fn into(self) -> std::result::Result<ResultRes<R>, ErrorRes<E>> {
        self.0
    }
}

impl<R, E> Serialize for Response<R, E>
where
    R: Serialize,
    E: Serialize,
{
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Response(Ok(res)) => res.serialize(serializer),
            Response(Err(res)) => res.serialize(serializer),
        }
    }
}

impl<'a, R, E> Deserialize<'a> for Response<R, E>
where
    R: Deserialize<'a>,
    E: Deserialize<'a>,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'a>,
    {
        let raw: RawResponse<R, E> = Deserialize::deserialize(deserializer)?;

        match (raw.result, raw.error) {
            (Some(r), None) => Ok(Response(Ok(ResultRes {
                jsonrpc: raw.jsonrpc,
                id: raw.id,
                result: r,
            }))),

            (None, Some(e)) => Ok(Response(Err(ErrorRes {
                jsonrpc: raw.jsonrpc,
                id: raw.id,
                code: e.code,
                message: e.message,
                data: e.data,
            }))),

            (None, None) => Err(serde::de::Error::custom(
                "response does not contain neither result nor error",
            )),

            (Some(_), Some(_)) => Err(serde::de::Error::custom(
                "respose must not contain both result and error",
            )),
        }
    }
}

#[derive(Debug, Deserialize)]
struct RawError<E> {
    pub code: i64,
    pub message: String,
    pub data: Option<E>,
}

#[derive(Debug, Deserialize)]
struct RawResponse<R, E> {
    pub jsonrpc: ProtocolVersion,
    pub id: RequestId,
    pub result: Option<R>,
    pub error: Option<RawError<E>>,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn serialize_requests() {
        let req = Request {
            jsonrpc: ProtocolVersion::TwoPointO,
            method: "method1".to_string(),
            params: &json!([]),
            id: RequestId::String("1".to_string()),
        };
        let val = serde_json::to_value(req);

        assert!(val.is_ok());
        let val = val.unwrap();

        assert_eq!(
            val,
            json!({
                "jsonrpc": "2.0",
                "method": "method1",
                "params": [],
                "id": "1",
            })
        );
    }

    #[test]
    fn deserialize_result_res() {
        let res_json = json!({
           "jsonrpc": "2.0",
           "id": "1",
           "result": {
               "some_key1": 1,
               "some_key2": "a",
           }
        });

        #[derive(Debug, Deserialize, Eq, PartialEq)]
        struct S {
            pub some_key1: u64,
            pub some_key2: String,
        }

        let res: Result<Response<S>, _> = serde_json::from_value(res_json);

        assert!(res.is_ok());
        let res = res.unwrap();

        assert_eq!(
            res,
            Response(Ok(ResultRes {
                jsonrpc: ProtocolVersion::TwoPointO,
                id: RequestId::String("1".to_string()),
                result: S {
                    some_key1: 1,
                    some_key2: "a".to_string(),
                }
            }))
        );
    }

    #[test]
    fn deserialize_error_res() {
        let res_json = json!({
           "jsonrpc": "2.0",
           "id": "1",
           "error": {
               "code": -1,
               "message": "err",
               "data": {
                   "some_key1": 1,
                   "some_key2": "a"
               }
           }
        });

        #[derive(Debug, Deserialize, Eq, PartialEq)]
        struct S {
            pub some_key1: u64,
            pub some_key2: String,
        }

        let res: Result<Response<(), S>, _> = serde_json::from_value(res_json);

        assert!(res.is_ok());
        let res = res.unwrap();

        assert_eq!(
            res,
            Response(Err(ErrorRes {
                jsonrpc: ProtocolVersion::TwoPointO,
                id: RequestId::String("1".to_string()),
                code: -1,
                message: "err".to_string(),
                data: Some(S {
                    some_key1: 1,
                    some_key2: "a".to_string(),
                })
            }))
        );
    }
}
