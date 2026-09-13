use serde::{Deserialize, Serialize};
use sterngate_core::{CanFrame, Dtc, FlashPackageManifest, ParameterValue};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum P2pMessage {
    Ping,
    Pong,
    CanTx(CanFrame),
    CanRx(CanFrame),
    ReadParameterRequest {
        id: String,
    },
    ReadParameterResponse {
        parameter: Option<ParameterValue>,
    },
    ReadDtcRequest {
        module: String,
    },
    ReadDtcResponse {
        dtcs: Vec<Dtc>,
    },
    ClearDtcRequest {
        module: String,
    },
    ClearDtcResponse {
        success: bool,
    },
    StageFlashRequest {
        manifest: FlashPackageManifest,
        rom_data: Vec<u8>,
    },
    StageFlashResponse {
        verified: bool,
        report: String,
    },
    TriggerFlashRequest,
    FlashProgressUpdate {
        percentage: u8,
        state: String,
        log: String,
    },
}
