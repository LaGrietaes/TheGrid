use anyhow::{bail, Context, Result};
use std::net::{UdpSocket, SocketAddr};

/// The standard WoL UDP port. Port 9 is the "discard" service — safe to use.
const WOL_PORT: u16 = 9;

pub struct WolSentry;

impl WolSentry {
    /// Send a Wake-on-LAN magic packet to `mac_addr` via `broadcast_addr`.
    pub fn send(mac_addr: &str, broadcast_addr: &str) -> Result<()> {
        let mac_bytes = parse_mac(mac_addr)
            .with_context(|| format!("Invalid MAC address: {}", mac_addr))?;

        let packet = build_magic_packet(&mac_bytes);

        let socket = UdpSocket::bind("0.0.0.0:0")
            .context("Binding UDP socket for WoL")?;

        socket.set_broadcast(true)
            .context("Enabling broadcast on WoL socket")?;

        let target: SocketAddr = format!("{}:{}", broadcast_addr, WOL_PORT)
            .parse()
            .with_context(|| format!("Invalid broadcast addr: {}", broadcast_addr))?;

        socket.send_to(&packet, target)
            .context("Sending WoL magic packet")?;

        log::info!(
            "[WoL] Magic packet sent to {} via {}:{}",
            mac_addr, broadcast_addr, WOL_PORT
        );
        Ok(())
    }

    /// Attempt WoL via multiple broadcast addresses simultaneously.
    pub fn send_multi(mac_addr: &str, broadcast_addrs: &[&str]) -> Result<()> {
        let mut last_err = None;
        let mut any_ok = false;

        for &addr in broadcast_addrs {
            match Self::send(mac_addr, addr) {
                Ok(_)  => { any_ok = true; }
                Err(e) => { last_err = Some(e); }
            }
        }

        if any_ok {
            Ok(())
        } else {
            Err(last_err.unwrap_or_else(|| anyhow::anyhow!("No broadcast addresses provided")))
        }
    }
}

fn parse_mac(mac: &str) -> Result<[u8; 6]> {
    let parts: Vec<&str> = mac.split(':').collect();
    if parts.len() != 6 {
        bail!("MAC address must have exactly 6 octets, got {}", parts.len());
    }
    let mut bytes = [0u8; 6];
    for (i, part) in parts.iter().enumerate() {
        bytes[i] = u8::from_str_radix(part, 16)
            .with_context(|| format!("Invalid hex octet '{}' in MAC", part))?;
    }
    Ok(bytes)
}

fn build_magic_packet(mac: &[u8; 6]) -> Vec<u8> {
    let mut packet = Vec::with_capacity(102);
    packet.extend_from_slice(&[0xFF; 6]);
    for _ in 0..16 {
        packet.extend_from_slice(mac);
    }
    debug_assert_eq!(packet.len(), 102);
    packet
}
