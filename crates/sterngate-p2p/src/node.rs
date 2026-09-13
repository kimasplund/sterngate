use anyhow::{Context, Result};
use iroh::{
    endpoint::{presets, Connection, Endpoint, Incoming},
    EndpointAddr, SecretKey,
};
use tracing::info;

pub struct P2pNode {
    endpoint: Endpoint,
}

impl P2pNode {
    pub async fn new() -> Result<Self> {
        let secret_key = SecretKey::generate();
        let endpoint = Endpoint::builder(presets::N0)
            .secret_key(secret_key)
            .alpns(vec![b"sterngate-quic-rpc".to_vec()])
            .bind()
            .await
            .context("Failed to bind Iroh Endpoint")?;

        Ok(Self { endpoint })
    }

    pub fn node_id(&self) -> String {
        self.endpoint.id().to_string()
    }

    pub fn generate_ticket(&self) -> Result<String> {
        let addr = self.endpoint.addr();
        let json = serde_json::to_string(&addr)?;
        let hex_str: String = json
            .as_bytes()
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect();
        Ok(format!("sterngate-ticket:{}", hex_str))
    }

    pub fn parse_ticket(ticket: &str) -> Result<EndpointAddr> {
        let raw_hex = ticket
            .strip_prefix("sterngate-ticket:")
            .unwrap_or(ticket)
            .trim();
        if !raw_hex.len().is_multiple_of(2) {
            anyhow::bail!("Invalid ticket hex length");
        }
        let bytes: Result<Vec<u8>, _> = (0..raw_hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&raw_hex[i..i + 2], 16))
            .collect();
        let bytes = bytes.context("Failed to decode hex ticket")?;
        let json_str = String::from_utf8(bytes).context("Invalid UTF-8 in ticket")?;
        let addr: EndpointAddr =
            serde_json::from_str(&json_str).context("Failed to parse EndpointAddr JSON")?;
        Ok(addr)
    }

    pub async fn dial(&self, target: EndpointAddr) -> Result<Connection> {
        info!("Dialing remote peer: {}", target.id);
        let conn = self.endpoint.connect(target, b"sterngate-quic-rpc").await?;
        Ok(conn)
    }

    pub async fn accept(&self) -> Option<Incoming> {
        self.endpoint.accept().await
    }
}
