use std::collections::{HashMap, HashSet};
use std::io::{self, Read, Write};
use std::mem::MaybeUninit;
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::time::{Duration, Instant};

#[cfg(target_os = "linux")]
use std::ffi::CString;
#[cfg(target_os = "linux")]
use std::os::unix::io::{AsRawFd, FromRawFd};

use anyhow::{anyhow, Context, Result};
use ipnet::Ipv4Net;
use socket2::{Domain, Protocol, Socket, Type};

const DEFAULT_ARP_PPS: u32 = 50;
const DEFAULT_BANNER_READ: Duration = Duration::from_millis(750);
const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_millis(500);

/// Discovery transport used for a host hit.
#[derive(Debug, Clone, Copy)]
pub enum DiscoveryMethod {
    Icmp,
    Arp,
}

/// Host hit with optional metadata.
#[derive(Debug, Clone)]
pub struct DiscoveredHost {
    pub ip: Ipv4Addr,
    pub ttl: Option<u8>,
    pub method: DiscoveryMethod,
}

/// Result of a LAN discovery sweep.
#[derive(Debug, Clone)]
pub struct LanDiscoveryResult {
    pub network: Ipv4Net,
    pub hosts: Vec<Ipv4Addr>,
    pub details: Vec<DiscoveredHost>,
}

/// Result of a TCP port scan.
#[derive(Debug, Clone)]
pub struct PortScanResult {
    pub target: Ipv4Addr,
    pub open_ports: Vec<u16>,
    pub banners: Vec<PortBanner>,
}

/// Service banner result.
#[derive(Debug, Clone)]
pub struct PortBanner {
    pub port: u16,
    pub probe: String,
    pub banner: String,
}

/// Crude OS guess from an observed TTL value.
#[must_use]
pub fn guess_os_from_ttl(ttl: Option<u8>) -> Option<&'static str> {
    match ttl {
        Some(t) if t >= 240 => Some("network appliance/router"),
        Some(t) if t >= 128 => Some("windows"),
        Some(t) if t >= 64 => Some("linux/unix"),
        Some(t) if t >= 32 => Some("embedded/older stack"),
        _ => None,
    }
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
    let mut details = Vec::new();
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
                let ttl = bytes.get(8).copied();
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
                                details.push(DiscoveredHost {
                                    ip: *expected_ip,
                                    ttl,
                                    method: DiscoveryMethod::Icmp,
                                });
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

    Ok(LanDiscoveryResult { network, hosts, details })
}

/// Perform a TCP SYN-like check using connect (no external binaries).
/// This uses TCP connect with a timeout; it is slower than raw SYN but is dependency-free.
pub fn quick_port_scan(
    target: Ipv4Addr,
    ports: &[u16],
    timeout: Duration,
) -> Result<PortScanResult> {
    let mut open = Vec::new();
    let mut banners = Vec::new();
    for port in ports {
        let addr = SocketAddr::new(target.into(), *port);
        if let Ok(stream) = TcpStream::connect_timeout(&addr, timeout) {
            stream.set_read_timeout(Some(DEFAULT_BANNER_READ)).ok();
            stream.set_write_timeout(Some(DEFAULT_CONNECT_TIMEOUT)).ok();
            open.push(*port);
            if let Some(info) = grab_banner(stream, *port) {
                banners.push(info);
            }
        }
    }

    Ok(PortScanResult {
        target,
        open_ports: open,
        banners,
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

/// Perform an ARP sweep across a CIDR on a specific interface.
/// This complements ICMP by finding hosts even when ICMP is blocked.
#[cfg(target_os = "linux")]
pub fn discover_hosts_arp(
    interface: &str,
    network: Ipv4Net,
    rate_limit_pps: Option<u32>,
    timeout: Duration,
) -> Result<LanDiscoveryResult> {
    let local_mac = read_iface_mac(interface)?;
    let local_ip = read_iface_ipv4(interface)?;
    let ifindex = unsafe { libc::if_nametoindex(CString::new(interface)?.as_ptr()) };
    if ifindex == 0 {
        return Err(anyhow!("failed to resolve ifindex for {}", interface));
    }

    let sock = unsafe {
        let fd = libc::socket(libc::AF_PACKET, libc::SOCK_RAW, (libc::ETH_P_ARP as u16).to_be() as i32);
        if fd < 0 {
            return Err(io::Error::last_os_error()).context("creating ARP raw socket");
        }
        Socket::from_raw_fd(fd)
    };

    // Bind to interface
    let mut sll = libc::sockaddr_ll {
        sll_family: libc::AF_PACKET as u16,
        sll_protocol: (libc::ETH_P_ARP as u16).to_be(),
        sll_ifindex: ifindex as i32,
        sll_hatype: 0,
        sll_pkttype: 0,
        sll_halen: 6,
        sll_addr: [0; 8],
    };
    sll.sll_addr[..6].copy_from_slice(&local_mac);
    let bind_res = unsafe {
        libc::bind(
            sock.as_raw_fd(),
            &sll as *const libc::sockaddr_ll as *const libc::sockaddr,
            std::mem::size_of::<libc::sockaddr_ll>() as u32,
        )
    };
    if bind_res != 0 {
        return Err(io::Error::last_os_error()).context("binding ARP socket to interface");
    }

    let delay = Duration::from_micros(1_000_000 / rate_limit_pps.unwrap_or(DEFAULT_ARP_PPS).max(1) as u64);
    let targets: Vec<Ipv4Addr> = network.hosts().filter(|ip| *ip != local_ip).collect();
    let mut inflight: HashSet<Ipv4Addr> = HashSet::new();

    for ip in &targets {
        let frame = build_arp_request(&local_mac, &local_ip, ip);
        let sent = unsafe {
            libc::send(
                sock.as_raw_fd(),
                frame.as_ptr() as *const libc::c_void,
                frame.len(),
                0,
            )
        };
        if sent < 0 {
            let err = io::Error::last_os_error();
            if err.kind() == io::ErrorKind::PermissionDenied {
                return Err(err).context("sending ARP probe (permission denied)");
            }
        } else {
            inflight.insert(*ip);
        }
        std::thread::sleep(delay);
    }

    let mut details = Vec::new();
    if inflight.is_empty() {
        return Ok(LanDiscoveryResult {
            network,
            hosts: Vec::new(),
            details,
        });
    }

    sock.set_read_timeout(Some(timeout))?;
    let mut buf = [0u8; 1500];
    loop {
        match sock.recv(&mut buf) {
            Ok(n) => {
                if n < 42 {
                    continue;
                }
                if buf[12] != 0x08 || buf[13] != 0x06 {
                    continue;
                }
                // ARP reply opcode 2 at bytes 20-21 of Ethernet payload
                let payload = &buf[14..];
                if payload[6] != 0x00 || payload[7] != 0x02 {
                    continue;
                }
                let sender_ip = Ipv4Addr::new(payload[14], payload[15], payload[16], payload[17]);
                if inflight.contains(&sender_ip) {
                    details.push(DiscoveredHost {
                        ip: sender_ip,
                        ttl: None,
                        method: DiscoveryMethod::Arp,
                    });
                }
            }
            Err(err) if err.kind() == io::ErrorKind::WouldBlock || err.kind() == io::ErrorKind::TimedOut => {
                break;
            }
            Err(err) => return Err(err).context("receiving ARP replies"),
        }
    }

    let mut unique = HashSet::new();
    let hosts: Vec<Ipv4Addr> = details
        .iter()
        .filter_map(|d| {
            if unique.insert(d.ip) {
                Some(d.ip)
            } else {
                None
            }
        })
        .collect();

    Ok(LanDiscoveryResult {
        network,
        hosts,
        details,
    })
}

/// Non-Linux stub for ARP discovery.
#[cfg(not(target_os = "linux"))]
pub fn discover_hosts_arp(
    _interface: &str,
    network: Ipv4Net,
    _rate_limit_pps: Option<u32>,
    _timeout: Duration,
) -> Result<LanDiscoveryResult> {
    Err(anyhow!(
        "ARP discovery is only supported on Linux; target network {} not scanned",
        network
    ))
}

#[cfg(target_os = "linux")]
fn build_arp_request(src_mac: &[u8; 6], src_ip: &Ipv4Addr, target_ip: &Ipv4Addr) -> Vec<u8> {
    let mut frame = vec![0u8; 42];
    // Ethernet header
    frame[0..6].fill(0xFF); // dest broadcast
    frame[6..12].copy_from_slice(src_mac);
    frame[12..14].copy_from_slice(&0x0806u16.to_be_bytes()); // ARP
    // ARP payload
    frame[14..16].copy_from_slice(&0x0001u16.to_be_bytes()); // HTYPE Ethernet
    frame[16..18].copy_from_slice(&0x0800u16.to_be_bytes()); // PTYPE IPv4
    frame[18] = 6; // HLEN
    frame[19] = 4; // PLEN
    frame[20..22].copy_from_slice(&0x0001u16.to_be_bytes()); // OPCODE request
    frame[22..28].copy_from_slice(src_mac); // Sender MAC
    frame[28..32].copy_from_slice(&src_ip.octets()); // Sender IP
    frame[32..38].fill(0); // Target MAC unknown
    frame[38..42].copy_from_slice(&target_ip.octets()); // Target IP
    frame
}

#[cfg(target_os = "linux")]
fn read_iface_mac(interface: &str) -> Result<[u8; 6]> {
    let path = format!("/sys/class/net/{}/address", interface);
    let content = std::fs::read_to_string(&path).with_context(|| format!("reading {}", path))?;
    let parts: Vec<&str> = content.trim().split(':').collect();
    if parts.len() != 6 {
        return Err(anyhow!("invalid MAC address format in {}", path));
    }
    let mut mac = [0u8; 6];
    for (i, part) in parts.iter().enumerate() {
        mac[i] = u8::from_str_radix(part, 16).with_context(|| format!("parsing MAC octet {}", part))?;
    }
    Ok(mac)
}

#[cfg(target_os = "linux")]
fn read_iface_ipv4(interface: &str) -> Result<Ipv4Addr> {
    let output = std::process::Command::new("ip")
        .args(["-4", "addr", "show", "dev", interface])
        .output()
        .with_context(|| format!("reading IPv4 address for {}", interface))?;
    if !output.status.success() {
        return Err(anyhow!("ip -4 addr failed for {}", interface));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        if let Some(rest) = line.trim().strip_prefix("inet ") {
            let mut parts = rest.split_whitespace();
            if let Some(addr) = parts.next() {
                if let Some((ip_str, _)) = addr.split_once('/') {
                    if let Ok(ip) = ip_str.parse() {
                        return Ok(ip);
                    }
                }
            }
        }
    }
    Err(anyhow!("no IPv4 address found on {}", interface))
}

fn grab_banner(mut stream: TcpStream, port: u16) -> Option<PortBanner> {
    let mut probe = String::new();
    let mut banner = String::new();

    match port {
        80 | 8080 | 8000 | 8443 | 443 => {
            probe = "http-head".to_string();
            let _ = stream.write_all(b"HEAD / HTTP/1.0\r\nHost: localhost\r\n\r\n");
        }
        _ => {}
    }

    let mut buf = [0u8; 512];
    if let Ok(n) = stream.read(&mut buf) {
        if n > 0 {
            banner = String::from_utf8_lossy(&buf[..n]).lines().next().unwrap_or("").to_string();
        }
    }

    if banner.is_empty() {
        return None;
    }

    Some(PortBanner {
        port,
        probe,
        banner,
    })
}
