use serde::{Deserialize, Serialize};

/// Sent by the client immediately after the WebSocket opens.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hello {
    pub protocol_version: u32,
    pub player_name: String,
}

/// Messages from client to server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ClientMessage {
    Hello(Hello),
    Ping { nonce: u32 },
}

/// Messages from server to client.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ServerMessage {
    /// Handshake accepted; the player's server-assigned id.
    Welcome {
        player_id: u32,
    },
    /// Handshake rejected (version mismatch, full server, bad name).
    Reject {
        reason: String,
    },
    Pong {
        nonce: u32,
    },
}

pub fn encode<T: Serialize>(msg: &T) -> Vec<u8> {
    postcard::to_allocvec(msg).expect("postcard encode cannot fail for our types")
}

pub fn decode<'a, T: Deserialize<'a>>(bytes: &'a [u8]) -> Option<T> {
    postcard::from_bytes(bytes).ok()
}
