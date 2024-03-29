use serde::{
    Deserialize,
    Serialize,
};
use serde_json::Value;

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct Host {
    pub hostname: String,
    pub data: Option<Value>,
    pub status: HostStatus,
}

impl Host {
    pub fn is_success(&self) -> bool {
        self.status.is_success()
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub enum HostStatus {
    Uninitialized,
    Success,
    NoReturnCode,
    ReturnCodeNotNumber,
    RetValueIsNone,
    RetCodeWasNotNull,
    RetValueNotObject,
    RetValueObjectIsEmpty,
    DeletedMinion,
    DidNotRespond,
}

impl Default for HostStatus {
    fn default() -> HostStatus {
        HostStatus::Uninitialized
    }
}

impl HostStatus {
    pub fn is_success(&self) -> bool {
        self == &HostStatus::Success
    }
}
