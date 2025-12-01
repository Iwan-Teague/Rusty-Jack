use std::net::{Ipv4Addr, SocketAddr};
use std::time::Duration;

use anyhow::{Context, Result};
use ipnet::Ipv4Net;
use socket2::{Domain, Protocol, Socket, Type};

/// Result of a LAN discovery sweep.
#[derive(Debug, Clone)]
pub struct LanDiscoveryResult {
    pub network: Ipv4Net,
    pub hosts: Vec<Ipv4Addr>,
}

/// Result of a TCP port scan.
#[derive(Debug, Clone)]
pub struct PortScanResult {
    pub target: Ipv4Addr,
    pub open_ports: Vec<u16>,
}

/// Perform a simple ICMP echo sweep across the given CIDR.
/// Requires root (RAW socket).
pub fn discover_hosts(network: Ipv4Net, timeout: Duration) -> Result<LanDiscoveryResult> {
    let socket = Socket::new(Domain::IPV4, Type::RAW, Some(Protocol::ICMPV4))
        .context("creating ICMP socket")?;
    socket
        .set_read_timeout(Some(timeout))
        .context("setting read timeout")?;
    socket
        .set_write_timeout(Some(timeout))
        .context("setting write timeout")?;

    let mut hosts = Vec::new();
    let mut seq: u16 = 1;
    let ident: u16 = 0xBEEF;

    for ip in network.hosts() {
        // Skip network/broadcast are excluded by hosts()
        let packet = build_icmp_echo(ident, seq);
        seq = seq.wrapping_add(1);

        let addr = SocketAddr::new(ip.into(), 0);
        let sock_addr = socket2::SockAddr::from(addr);
        let _ = socket.send_to(&packet, &sock_addr);

        let mut buf = [std::mem::MaybeUninit::<u8>::uninit(); 1500];
        if let Ok((n, from)) = socket.recv_from(&mut buf) {
            if n >= 20 {
                if let Some(sock) = from.as_socket() {
                    if let std::net::SocketAddr::V4(from_v4) = sock {
                        if from_v4.ip() == &ip {
                            hosts.push(ip);
                        }
                    }
                }
            }
        }
    }

    Ok(LanDiscoveryResult { network, hosts })
}

/// Perform a TCP SYN-like check using connect (no external binaries).
/// This uses TCP connect with a timeout; it is slower than raw SYN but is dependency-free.
pub fn quick_port_scan(target: Ipv4Addr, ports: &[u16], timeout: Duration) -> Result<PortScanResult> {
    use std::net::TcpStream;

    let mut open = Vec::new();
    for port in ports {
        let addr = SocketAddr::new(target.into(), *port);
        if TcpStream::connect_timeout(&addr, timeout).is_ok() {
            open.push(*port);
        }
    }

    Ok(PortScanResult {
        target,
        open_ports: open,
    })
}

fn checksum(data: &[u8]) -> u16 {
    let mut sum = 0u32;
    let mut chunks = data.chunks_exact(2);
    for chunk in &mut chunks {
        let v = u16::from_be_bytes([chunk[0], chunk[1]]) as u32;
        sum = sum.wrapping_add(v);
    }
    if let Some(&b) = chunks.remainder().get(0) {
        sum = sum.wrapping_add((b as u32) << 8);
    }
    while (sum >> 16) != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    !(sum as u16)
}

fn build_icmp_echo(ident: u16, seq: u16) -> [u8; 8] {
    let mut packet = [0u8; 8];
    packet[0] = 8; // type: echo request
    packet[1] = 0; // code
    packet[4..6].copy_from_slice(&ident.to_be_bytes());
    packet[6..8].copy_from_slice(&seq.to_be_bytes());
    let csum = checksum(&packet);
    packet[2..4].copy_from_slice(&csum.to_be_bytes());
    packet
}

/// Convenience to parse CIDR and run discovery.
pub fn discover_cidr(cidr: &str, timeout: Duration) -> Result<LanDiscoveryResult> {
    let net: Ipv4Net = cidr.parse().context("parsing CIDR")?;
    discover_hosts(net, timeout)
}
