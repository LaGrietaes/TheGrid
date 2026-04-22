pub mod tailscale;
pub mod rdp;
pub mod agent;
pub mod wol;
pub mod win_sys;
pub mod termux_agent;

pub use tailscale::TailscaleClient;
pub use rdp::{RdpLauncher, RdpResolution};
pub use agent::{AgentServer, AgentClient};
pub use wol::WolSentry;
pub use win_sys::{is_rdp_enabled, enable_rdp};
pub use termux_agent::{TermuxAgent, ConnectionMethod, setup_termux_agent};
