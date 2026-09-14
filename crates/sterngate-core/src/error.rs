use thiserror::Error;

#[derive(Error, Debug, Clone, PartialEq)]
pub enum SterngateError {
    #[error("HAL Interface error: {0}")]
    HalError(String),

    #[error("Device not connected or unreachable: {0}")]
    DeviceNotFound(String),

    #[error("ISO-TP protocol error: {0}")]
    IsoTpError(String),

    #[error("Protocol error: {0}")]
    ProtocolError(String),

    #[error("ISO-TP timeout waiting for response")]
    IsoTpTimeout,

    #[error(
        "UDS Negative Response Code (NRC: 0x{nrc:02X}) for Service 0x{service:02X}: {description}"
    )]
    UdsNegativeResponse {
        service: u8,
        nrc: u8,
        description: String,
    },

    #[error("Security Access denied: {0}")]
    SecurityAccessDenied(String),

    #[error("Profile error: {0}")]
    ProfileError(String),

    #[error("Parameter parsing error for '{name}': {reason}")]
    ParameterParseError { name: String, reason: String },

    #[error("Flashing pre-flight failure: {0}")]
    PreFlightCheckFailed(String),

    #[error("Flashing API locked: system is busy performing safe flash routine")]
    FlashingApiLocked,

    #[error("Flashing sequence aborted: {0}")]
    FlashAborted(String),

    #[error("Battery voltage too low ({current:.2}V, minimum {required:.2}V required)")]
    VoltageTooLow { current: f64, required: f64 },

    #[error("Checksum mismatch: expected {expected}, calculated {calculated}")]
    ChecksumMismatch {
        expected: String,
        calculated: String,
    },

    #[error("P2P tunnel error: {0}")]
    P2pError(String),

    #[error("Serialization error: {0}")]
    SerializationError(String),

    #[error("Generic internal error: {0}")]
    Internal(String),
}

pub type Result<T> = std::result::Result<T, SterngateError>;
