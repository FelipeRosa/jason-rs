#[cfg(feature = "http")]
pub mod http;

#[cfg(feature = "http-tls")]
pub mod https;

#[cfg(all(feature = "ipc", unix))]
pub mod ipc;

#[cfg(feature = "transport")]
pub mod transport;

#[cfg(feature = "ws")]
pub mod websocket;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// Represents protocol versions.
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

/// Represents a request ID.
#[derive(Debug, Clone)]
pub enum RequestId {
    String(String),
    Number(u64),
}

impl Eq for RequestId {}

impl PartialEq for RequestId {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (RequestId::String(s1), RequestId::String(s2)) => s1 == s2,
            (RequestId::Number(n1), RequestId::Number(n2)) => n1 == n2,
            _ => false,
        }
    }
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
            serde_json::Value::Number(n) => {
                Ok(RequestId::Number(n.as_u64().ok_or_else(|| {
                    serde::de::Error::custom("request id must be u64")
                })?))
            }

            serde_json::Value::String(s) => Ok(RequestId::String(s)),

            _ => Err(serde::de::Error::custom(
                "request id must be either string or number",
            )),
        }
    }
}

/// Represents request parameters.
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum RequestParams {
    /// Request parameters by position.
    ByPosition(Vec<Value>),

    /// Request parameters by name.
    ByName(BTreeMap<String, Value>),
}

impl Serialize for RequestParams {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            RequestParams::ByPosition(vs) => vs.serialize(serializer),
            RequestParams::ByName(m) => m.serialize(serializer),
        }
    }
}

impl<'a> Deserialize<'a> for RequestParams {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'a>,
    {
        deserializer.deserialize_any(RequestParamsVisitor {})
    }
}

impl From<Vec<(String, Value)>> for RequestParams {
    fn from(vs: Vec<(String, Value)>) -> Self {
        RequestParams::ByName(vs.into_iter().collect())
    }
}

impl From<Vec<Value>> for RequestParams {
    fn from(vs: Vec<Value>) -> Self {
        RequestParams::ByPosition(vs)
    }
}

struct RequestParamsVisitor;

impl<'a> serde::de::Visitor<'a> for RequestParamsVisitor {
    type Value = RequestParams;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("array or object")
    }

    fn visit_seq<A>(self, seq: A) -> Result<Self::Value, A::Error>
    where
        A: serde::de::SeqAccess<'a>,
    {
        let vs: Vec<Value> =
            Deserialize::deserialize(serde::de::value::SeqAccessDeserializer::new(seq))?;

        Ok(RequestParams::ByPosition(vs))
    }

    fn visit_map<A>(self, map: A) -> Result<Self::Value, A::Error>
    where
        A: serde::de::MapAccess<'a>,
    {
        let m: BTreeMap<String, Value> =
            Deserialize::deserialize(serde::de::value::MapAccessDeserializer::new(map))?;

        Ok(RequestParams::ByName(m))
    }
}

/// Represents a request.
#[derive(Debug, Serialize, Deserialize, Eq, PartialEq, Clone)]
pub struct Request {
    pub jsonrpc: ProtocolVersion,
    pub id: RequestId,
    pub method: String,
    pub params: Option<RequestParams>,
}

/// Represents a notification.
#[derive(Debug, Serialize, Deserialize, Eq, PartialEq, Clone)]
pub struct Notification {
    pub jsonrpc: ProtocolVersion,
    pub method: String,
    pub params: Option<RequestParams>,
}

/// Represents a successful response.
#[derive(Debug, Eq, PartialEq, Clone, Serialize)]
pub struct ResultRes {
    pub jsonrpc: ProtocolVersion,
    pub id: RequestId,
    pub result: Value,
}

/// Represents a failed response.
#[derive(Debug, Eq, PartialEq, Clone, Serialize)]
pub struct ErrorRes {
    pub jsonrpc: ProtocolVersion,
    pub id: RequestId,
    pub code: i64,
    pub message: String,
    pub data: Option<Value>,
}

/// Represents a response which can be successful or failed.
#[derive(Debug, Eq, PartialEq, Clone)]
pub struct Response(std::result::Result<ResultRes, ErrorRes>);

impl Response {
    pub fn id(&self) -> &RequestId {
        match self {
            Response(Ok(res)) => &res.id,
            Response(Err(res)) => &res.id,
        }
    }

    pub fn as_result(&self) -> &std::result::Result<ResultRes, ErrorRes> {
        &self.0
    }

    pub fn into_result(self) -> std::result::Result<ResultRes, ErrorRes> {
        self.0
    }
}

impl Serialize for Response {
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

impl<'a> Deserialize<'a> for Response {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'a>,
    {
        let raw: RawResponse = Deserialize::deserialize(deserializer)?;

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
struct RawError {
    pub code: i64,
    pub message: String,
    pub data: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct RawResponse {
    pub jsonrpc: ProtocolVersion,
    pub id: RequestId,
    pub result: Option<Value>,
    pub error: Option<RawError>,
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
            params: Some(vec![json!(1), json!(2)].into()),
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
                "params": [1, 2],
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

        let res: Result<Response, _> = serde_json::from_value(res_json);

        assert!(res.is_ok());
        let res = res.unwrap();

        assert_eq!(
            res,
            Response(Ok(ResultRes {
                jsonrpc: ProtocolVersion::TwoPointO,
                id: RequestId::String("1".to_string()),
                result: json!({
                    "some_key1": 1,
                    "some_key2": "a",
                })
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

        let res: Result<Response, _> = serde_json::from_value(res_json);

        assert!(res.is_ok());
        let res = res.unwrap();

        assert_eq!(
            res,
            Response(Err(ErrorRes {
                jsonrpc: ProtocolVersion::TwoPointO,
                id: RequestId::String("1".to_string()),
                code: -1,
                message: "err".to_string(),
                data: Some(json!({
                    "some_key1": 1,
                    "some_key2": "a",
                })),
            }))
        );
    }
}
