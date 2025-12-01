use std::collections::{HashMap, HashSet};
use std::io;
use std::mem::MaybeUninit;
use std::net::{Ipv4Addr, SocketAddr};
use std::time::{Duration, Instant};

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
    let timeout = timeout.max(Duration::from_millis(10));
    let socket = Socket::new(Domain::IPV4, Type::RAW, Some(Protocol::ICMPV4))
        .context("creating ICMP socket (requires root/CAP_NET_RAW)")?;
    socket
        .set_nonblocking(true)
        .context("setting ICMP socket nonblocking")?;
    socket
        .set_write_timeout(Some(timeout))
        .context("setting write timeout")?;

    // Track probes so we only report replies we originated.
    let mut inflight: HashMap<u16, Ipv4Addr> = HashMap::new();
    let mut seen: HashSet<Ipv4Addr> = HashSet::new();
    let mut hosts = Vec::new();
    let mut seq: u16 = 1;
    let ident: u16 = 0xBEEF;

    for ip in network.hosts() {
        // Skip network/broadcast are excluded by hosts()
        let packet = build_icmp_echo(ident, seq);
        let addr = SocketAddr::new(ip.into(), 0);
        let sock_addr = socket2::SockAddr::from(addr);
        if let Err(err) = socket.send_to(&packet, &sock_addr) {
            // Permission errors after socket creation are fatal; other per-host errors are skipped.
            if err.kind() == io::ErrorKind::PermissionDenied {
                return Err(err).context("sending ICMP probe (permission denied)");
            }
            continue;
        }
        inflight.insert(seq, ip);
        seq = seq.wrapping_add(1);
    }

    if inflight.is_empty() {
        return Ok(LanDiscoveryResult {
            network,
            hosts: Vec::new(),
        });
    }

    let mut buf = [MaybeUninit::<u8>::uninit(); 1500];
    let deadline = Instant::now() + timeout;

    while Instant::now() < deadline {
        match socket.recv_from(&mut buf) {
            Ok((n, from)) => {
                if n < 28 {
                    continue;
                }
                // Safety: recv_from initialized the first `n` bytes.
                let bytes = unsafe { std::slice::from_raw_parts(buf.as_ptr() as *const u8, n) };
                let icmp = &bytes[20..];
                // Only accept echo replies that match our identifier.
                if icmp[0] != 0 || icmp[1] != 0 {
                    continue;
                }
                let reply_ident = u16::from_be_bytes([icmp[4], icmp[5]]);
                if reply_ident != ident {
                    continue;
                }
                let reply_seq = u16::from_be_bytes([icmp[6], icmp[7]]);
                if let Some(sock) = from.as_socket() {
                    if let SocketAddr::V4(from_v4) = sock {
                        if let Some(expected_ip) = inflight.get(&reply_seq) {
                            if from_v4.ip() == expected_ip && seen.insert(*expected_ip) {
                                hosts.push(*expected_ip);
                            }
                        }
                    }
                }
            }
            Err(err) if err.kind() == io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
            Err(err) => return Err(err).context("receiving ICMP replies"),
        }
    }

    Ok(LanDiscoveryResult { network, hosts })
}

/// Perform a TCP SYN-like check using connect (no external binaries).
/// This uses TCP connect with a timeout; it is slower than raw SYN but is dependency-free.
pub fn quick_port_scan(
    target: Ipv4Addr,
    ports: &[u16],
    timeout: Duration,
) -> Result<PortScanResult> {
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
