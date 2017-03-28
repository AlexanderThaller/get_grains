use serde_json::Value;

#[derive(Debug, Clone, Default)]
pub struct Host {
    pub hostname: String,
    pub data: Option<Value>,
    pub status: HostStatus,
}

#[derive(Debug, Clone, PartialEq)]
pub enum HostStatus {
    Uninitialized,
    Success,
    NoReturnCode,
    ReturnCodeNotNumber,
    RetValueIsNone,
    RetCodeWasNotNull,
    RetValueNotObject,
    RetValueObjectIsEmpty,
    DidNotRespond,
}

impl Default for HostStatus {
    fn default() -> HostStatus {
        HostStatus::Uninitialized
    }
}
