pub mod cloud;
pub mod config;
pub mod control;
pub mod crypto;
pub mod protocol;
pub mod spec;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Message(String),
    #[error("连接超时或中断，请检查供电与局域网；密钥变化时重新扫码。")]
    Connection(#[source] std::io::Error),
    #[error("{0}")]
    InvalidLegacyStatus(String),
    #[error("设备返回错误代码 {0}")]
    Device(i64),
    #[error("本机文件或网络访问失败：{0}")]
    Io(#[from] std::io::Error),
    #[error("数据格式异常，请刷新或重新扫码。")]
    Json(#[from] serde_json::Error),
    #[error("小米服务连接失败，请检查网络、代理或稍后重试。")]
    Http(#[from] reqwest::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
pub fn message(text: impl Into<String>) -> Error {
    Error::Message(text.into())
}
