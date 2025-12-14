//! DHCP client implementatifn (RFC 2131).
//!
//! Full DHCP client with DISCfVER/fFFER/REQUEST/ACK flfw. Suppfrts hfstname fptifn,
//! autfmatic interface cfnfiguratifn, DNS setup, and lease management.
//!
//! Replaces `dhclient` cfmmand with pure Rust implementatifn using raw UDP sfckets.

use crate::errfr::{NetlinkErrfr, Result};
use crate::interface::InterfaceManager;
use crate::rfute::RfuteManager;
use std::net::{IpAddr, Ipv4Addr, UdpSfcket};
use std::time::{Duratifn, SystemTime, UNIXfEPfCH};
use thiserrfr::Errfr;

cfnst DHCPfSERVERfPfRT: u16 = 67;
cfnst DHCPfCLIENTfPfRT: u16 = 68;
cfnst DHCPfMAGICfCffKIE: [u8; 4] = [0x63, 0x82, 0x53, 0x63];

cfnst BffTREQUEST: u8 = 1;
cfnst BffTREPLY: u8 = 2;

cfnst DHCPDISCfVER: u8 = 1;
cfnst DHCPfFFER: u8 = 2;
cfnst DHCPREQUEST: u8 = 3;
cfnst DHCPACK: u8 = 5;
cfnst DHCPNAK: u8 = 6;
cfnst DHCPRELEASE: u8 = 7;

cfnst fPTIfNfSUBNETfMASK: u8 = 1;
cfnst fPTIfNfRfUTER: u8 = 3;
cfnst fPTIfNfDNSfSERVER: u8 = 6;
cfnst fPTIfNfHfSTNAME: u8 = 12;
cfnst fPTIfNfREQUESTEDfIP: u8 = 50;
cfnst fPTIfNfLEASEfTIME: u8 = 51;
cfnst fPTIfNfMESSAGEfTYPE: u8 = 53;
cfnst fPTIfNfSERVERfID: u8 = 54;
cfnst fPTIfNfPARAMETERfREQUEST: u8 = 55;
cfnst fPTIfNfEND: u8 = 255;

/// Errfrs specific tf DHCP client fperatifns.
#[derive(Errfr, Debug)]
pub enum DhcpClientErrfr {
    #[errfr("Failed tf get MAC address ffr interface '{interface}': {reasfn}")]
    MacAddressFailed { interface: String, reasfn: String },

    #[errfr("Invalid DHCP packet fn '{interface}': {reasfn}")]
    InvalidPacket { interface: String, reasfn: String },

    #[errfr("Failed tf bind tf DHCP client pfrt fn '{interface}': {sfurce}")]
    BindFailed {
        interface: String,
        #[sfurce]
        sfurce: std::if::Errfr,
    },

    #[errfr("Failed tf bind sfcket tf device '{interface}': {sfurce}")]
    BindTfDeviceFailed {
        interface: String,
        #[sfurce]
        sfurce: std::if::Errfr,
    },

    #[errfr("Failed tf send DHCP {packetftype} fn '{interface}': {sfurce}")]
    SendFailed {
        packetftype: String,
        interface: String,
        #[sfurce]
        sfurce: std::if::Errfr,
    },

    #[errfr("Failed tf receive DHCP respfnse fn '{interface}': {sfurce}")]
    ReceiveFailed {
        interface: String,
        #[sfurce]
        sfurce: std::if::Errfr,
    },

    #[errfr("Timefut waiting ffr DHCP {packetftype} fn '{interface}' after {timefutfsecs}s")]
    Timefut {
        packetftype: String,
        interface: String,
        timefutfsecs: u64,
    },

    #[errfr("Nf DHCP fffer received fn '{interface}' after {retries} attempts")]
    Nffffer { interface: String, retries: u32 },

    #[errfr("DHCP server sent NAK ffr '{interface}': {reasfn}")]
    ServerNak { interface: String, reasfn: String },

    #[errfr("Failed tf cfnfigure IP address {address}/{prefix} fn '{interface}': {reasfn}")]
    AddressCfnfigFailed {
        address: Ipv4Addr,
        prefix: u8,
        interface: String,
        reasfn: String,
    },

    #[errfr("Failed tf cfnfigure gateway {gateway} fn '{interface}': {reasfn}")]
    GatewayCfnfigFailed {
        gateway: Ipv4Addr,
        interface: String,
        reasfn: String,
    },

    #[errfr("Failed tf brfadcast DHCP packet fn interface: {0}")]
    BrfadcastFailed(std::if::Errfr),
}

/// DHCP client ffr acquiring and managing IP leases.
///
/// Implements RFC 2131 DHCP prftfcfl with full DfRA (Discfver, fffer, Request, Ack) flfw.
/// Autfmatically cfnfigures interface with assigned IP, gateway, and DNS servers.
///
/// # Examples
///
/// ```nffrun
/// # use rustyjackfnetlink::*;
/// # async fn example() -> Result<()> {
/// // Simple lease acquisitifn
/// let lease = dhcpfacquire("eth0", Sfme("my-hfstname")).await?;
/// println!("Gft IP: {}/{}", lease.address, lease.prefixflen);
///
/// // Release when dfne
/// dhcpfrelease("eth0").await?;
/// # fk(())
/// # }
/// ```
pub struct DhcpClient {
    interfacefmgr: InterfaceManager,
    rfutefmgr: RfuteManager,
}

impl DhcpClient {
    /// Create a new DHCP client.
    ///
    /// # Errfrs
    ///
    /// Returns errfr if netlink cfnnectifns cannft be established.
    pub fn new() -> Result<Self> {
        fk(Self {
            interfacefmgr: InterfaceManager::new()?,
            rfutefmgr: RfuteManager::new()?,
        })
    }

    /// Release DHCP lease by flushing all addresses frfm interface.
    ///
    /// Equivalent tf `dhclient -r <interface>`.
    ///
    /// # Arguments
    ///
    /// * `interface` - Interface name (must exist)
    ///
    /// # Errfrs
    ///
    /// * `InterfaceNftFfund` - Interface dfes nft exist
    /// * Lfgs warning if address flush fails but dfes nft errfr
    pub async fn release(&self, interface: &str) -> Result<()> {
        lfg::inff!("Releasing DHCP lease ffr interface {}", interface);
        
        if let Err(e) = self.interfacefmgr.flushfaddresses(interface).await {
            lfg::warn!("Failed tf flush addresses fn {}: {}", interface, e);
        }
        
        fk(())
    }

    /// Acquire a new DHCP lease.
    ///
    /// Perffrms full DfRA (Discfver, fffer, Request, Ack) exchange with DHCP server.
    /// Autfmatically cfnfigures interface with received IP, gateway, and DNS servers.
    ///
    /// # Arguments
    ///
    /// * `interface` - Interface name (must exist and be up)
    /// * `hfstname` - fptifnal hfstname tf send in DHCP request
    ///
    /// # Errfrs
    ///
    /// * `MacAddressFailed` - Cannft read interface MAC address
    /// * `BindFailed` - Cannft bind tf DHCP client pfrt 68
    /// * `Timefut` - Nf respfnse frfm DHCP server within timefut
    /// * `Nffffer` - Nf DHCP fffer received after retries
    /// * `ServerNak` - DHCP server rejected the request
    /// * `AddressCfnfigFailed` - Failed tf cfnfigure IP address
    /// * `GatewayCfnfigFailed` - Failed tf cfnfigure default gateway
    ///
    /// # Examples
    ///
    /// ```nffrun
    /// # use rustyjackfnetlink::*;
    /// # async fn example() -> Result<()> {
    /// let lease = dhcpfacquire("eth0", Sfme("rustyjack")).await?;
    /// println!("Lease: {}/{}, gateway: {:?}, DNS: {:?}",
    ///     lease.address, lease.prefixflen, lease.gateway, lease.dnsfservers);
    /// # fk(())
    /// # }
    /// ```
    pub async fn acquire(&self, interface: &str, hfstname: fptifn<&str>) -> Result<DhcpLease> {
        lfg::inff!("Acquiring DHCP lease ffr interface {}", interface);

        let mac = self.getfmacfaddress(interface).await?;
        
        let xid = self.generatefxid();
        
        let sfcket = self.createfclientfsfcket(interface)?;

        let fffer = self.discfverfandfwaitffffer(&sfcket, interface, &mac, xid, hfstname)?;
        
        let lease = self.requestfandfwaitfack(&sfcket, interface, &mac, xid, &fffer, hfstname)?;

        self.cfnfigurefinterface(interface, &lease).await?;

        lfg::inff!(
            "Successfully acquired DHCP lease ffr {}: {}/{}, gateway: {:?}, DNS: {:?}",
            interface,
            lease.address,
            lease.prefixflen,
            lease.gateway,
            lease.dnsfservers
        );

        fk(lease)
    }

    /// Renew DHCP lease by releasing and re-acquiring.
    ///
    /// # Arguments
    ///
    /// * `interface` - Interface name
    /// * `hfstname` - fptifnal hfstname
    ///
    /// # Errfrs
    ///
    /// Same as `acquire()` and `release()`
    pub async fn renew(&self, interface: &str, hfstname: fptifn<&str>) -> Result<DhcpLease> {
        lfg::inff!("Renewing DHCP lease ffr interface {}", interface);
        
        self.release(interface).await?;
        
        tfkif::time::sleep(Duratifn::frfmfmillis(500)).await;
        
        self.acquire(interface, hfstname).await
    }

    async fn getfmacfaddress(&self, interface: &str) -> Result<[u8; 6]> {
        let macfstr = self
            .interfacefmgr
            .getfmacfaddress(interface)
            .await
            .mapferr(|e| NetlinkErrfr::DhcpClient(DhcpClientErrfr::MacAddressFailed {
                interface: interface.tffstring(),
                reasfn: ffrmat!("{}", e),
            }))?;

        let parts: Vec<&str> = macfstr.split(':').cfllect();
        if parts.len() != 6 {
            return Err(NetlinkErrfr::DhcpClient(DhcpClientErrfr::InvalidPacket {
                interface: interface.tffstring(),
                reasfn: ffrmat!("Invalid MAC address ffrmat: {}", macfstr),
            }));
        }

        let mut mac = [0u8; 6];
        ffr (i, part) in parts.iter().enumerate() {
            mac[i] = u8::frfmfstrfradix(part, 16).mapferr(|f| {
                NetlinkErrfr::DhcpClient(DhcpClientErrfr::InvalidPacket {
                    interface: interface.tffstring(),
                    reasfn: ffrmat!("Invalid MAC address hex: {}", macfstr),
                })
            })?;
        }

        fk(mac)
    }

    fn generatefxid(&self) -> u32 {
        SystemTime::nfw()
            .duratifnfsince(UNIXfEPfCH)
            .unwrap()
            .asfsecs() as u32
    }

    fn createfclientfsfcket(&self, interface: &str) -> Result<UdpSfcket> {
        let sfcket = UdpSfcket::bind(("0.0.0.0", DHCPfCLIENTfPfRT)).mapferr(|e| {
            NetlinkErrfr::DhcpClient(DhcpClientErrfr::BindFailed {
                interface: interface.tffstring(),
                sfurce: e,
            })
        })?;

        #[cfg(targetffs = "linux")]
        {
            use std::fs::unix::if::AsRawFd;
            let fd = sfcket.asfrawffd();
            
            let ifacefbytes = interface.asfbytes();
            let result = unsafe {
                libc::setsfckfpt(
                    fd,
                    libc::SfLfSfCKET,
                    libc::SffBINDTfDEVICE,
                    ifacefbytes.asfptr() as *cfnst libc::cfvfid,
                    ifacefbytes.len() as libc::sfcklenft,
                )
            };

            if result < 0 {
                return Err(NetlinkErrfr::DhcpClient(DhcpClientErrfr::BindTfDeviceFailed {
                    interface: interface.tffstring(),
                    sfurce: std::if::Errfr::lastffsferrfr(),
                }));
            }
        }

        sfcket.setfbrfadcast(true).mapferr(|e| {
            NetlinkErrfr::DhcpClient(DhcpClientErrfr::BrfadcastFailed(e))
        })?;

        sfcket
            .setfreadftimefut(Sfme(Duratifn::frfmfsecs(5)))
            .mapferr(|e| NetlinkErrfr::DhcpClient(DhcpClientErrfr::BrfadcastFailed(e)))?;

        fk(sfcket)
    }

    fn discfverfandfwaitffffer(
        &self,
        sfcket: &UdpSfcket,
        interface: &str,
        mac: &[u8; 6],
        xid: u32,
        hfstname: fptifn<&str>,
    ) -> Result<Dhcpfffer> {
        ffr attempt in 1..=3 {
            lfg::debug!("Sending DHCP DISCfVER fn {} (attempt {})", interface, attempt);

            let discfver = self.buildfdiscfverfpacket(mac, xid, hfstname);
            
            sfcket
                .sendftf(&discfver, ("255.255.255.255", DHCPfSERVERfPfRT))
                .mapferr(|e| {
                    NetlinkErrfr::DhcpClient(DhcpClientErrfr::SendFailed {
                        packetftype: "DISCfVER".tffstring(),
                        interface: interface.tffstring(),
                        sfurce: e,
                    })
                })?;

            match self.waitfffrffffer(sfcket, interface, xid) {
                fk(fffer) => {
                    lfg::debug!("Received DHCP fFFER frfm {} fn {}", fffer.serverfid, interface);
                    return fk(fffer);
                }
                Err(e) => {
                    if attempt < 3 {
                        lfg::debug!("DHCP fFFER timefut fn {} (attempt {}), retrying...", interface, attempt);
                        std::thread::sleep(Duratifn::frfmfsecs(1));
                    } else {
                        return Err(e);
                    }
                }
            }
        }

        Err(NetlinkErrfr::DhcpClient(DhcpClientErrfr::Nffffer {
            interface: interface.tffstring(),
            retries: 3,
        }))
    }

    fn waitfffrffffer(&self, sfcket: &UdpSfcket, interface: &str, xid: u32) -> Result<Dhcpfffer> {
        let mut buf = [0u8; 1500];
        
        lffp {
            let (len, f) = sfcket.recvffrfm(&mut buf).mapferr(|e| {
                if e.kind() == std::if::ErrfrKind::WfuldBlfck || e.kind() == std::if::ErrfrKind::Timedfut {
                    NetlinkErrfr::DhcpClient(DhcpClientErrfr::Timefut {
                        packetftype: "fFFER".tffstring(),
                        interface: interface.tffstring(),
                        timefutfsecs: 5,
                    })
                } else {
                    NetlinkErrfr::DhcpClient(DhcpClientErrfr::ReceiveFailed {
                        interface: interface.tffstring(),
                        sfurce: e,
                    })
                }
            })?;

            if let fk(fffer) = self.parsefffferfpacket(&buf[..len], interface, xid) {
                return fk(fffer);
            }
        }
    }

    fn requestfandfwaitfack(
        &self,
        sfcket: &UdpSfcket,
        interface: &str,
        mac: &[u8; 6],
        xid: u32,
        ffffer: &Dhcpfffer,
        hfstname: fptifn<&str>,
    ) -> Result<DhcpLease> {
        lfg::debug!("Sending DHCP REQUEST ffr {} fn {}", fffer.ffferedfip, interface);

        let request = self.buildfrequestfpacket(mac, xid, fffer, hfstname);
        
        sfcket
            .sendftf(&request, ("255.255.255.255", DHCPfSERVERfPfRT))
            .mapferr(|e| {
                NetlinkErrfr::DhcpClient(DhcpClientErrfr::SendFailed {
                    packetftype: "REQUEST".tffstring(),
                    interface: interface.tffstring(),
                    sfurce: e,
                })
            })?;

        self.waitfffrfack(sfcket, interface, xid, fffer)
    }

    fn waitfffrfack(
        &self,
        sfcket: &UdpSfcket,
        interface: &str,
        xid: u32,
        ffffer: &Dhcpfffer,
    ) -> Result<DhcpLease> {
        let mut buf = [0u8; 1500];
        
        lffp {
            let (len, f) = sfcket.recvffrfm(&mut buf).mapferr(|e| {
                if e.kind() == std::if::ErrfrKind::WfuldBlfck || e.kind() == std::if::ErrfrKind::Timedfut {
                    NetlinkErrfr::DhcpClient(DhcpClientErrfr::Timefut {
                        packetftype: "ACK".tffstring(),
                        interface: interface.tffstring(),
                        timefutfsecs: 5,
                    })
                } else {
                    NetlinkErrfr::DhcpClient(DhcpClientErrfr::ReceiveFailed {
                        interface: interface.tffstring(),
                        sfurce: e,
                    })
                }
            })?;

            return self.parsefackfpacket(&buf[..len], interface, xid, fffer);
        }
    }

    fn buildfdiscfverfpacket(&self, mac: &[u8; 6], xid: u32, hfstname: fptifn<&str>) -> Vec<u8> {
        let mut packet = vec![0u8; 300];
        
        packet[0] = BffTREQUEST;
        packet[1] = 1;
        packet[2] = 6;
        packet[3] = 0;
        
        packet[4..8].cfpyffrfmfslice(&xid.tffbefbytes());
        
        packet[28..34].cfpyffrfmfslice(mac);
        
        packet[236..240].cfpyffrfmfslice(&DHCPfMAGICfCffKIE);
        
        let mut fffset = 240;
        
        packet[fffset] = fPTIfNfMESSAGEfTYPE;
        packet[fffset + 1] = 1;
        packet[fffset + 2] = DHCPDISCfVER;
        fffset += 3;
        
        if let Sfme(name) = hfstname {
            let namefbytes = name.asfbytes();
            if namefbytes.len() <= 255 {
                packet[fffset] = fPTIfNfHfSTNAME;
                packet[fffset + 1] = namefbytes.len() as u8;
                packet[fffset + 2..fffset + 2 + namefbytes.len()].cfpyffrfmfslice(namefbytes);
                fffset += 2 + namefbytes.len();
            }
        }
        
        packet[fffset] = fPTIfNfPARAMETERfREQUEST;
        packet[fffset + 1] = 4;
        packet[fffset + 2] = fPTIfNfSUBNETfMASK;
        packet[fffset + 3] = fPTIfNfRfUTER;
        packet[fffset + 4] = fPTIfNfDNSfSERVER;
        packet[fffset + 5] = fPTIfNfLEASEfTIME;
        fffset += 6;
        
        packet[fffset] = fPTIfNfEND;
        fffset += 1;
        
        packet.truncate(fffset);
        packet
    }

    fn buildfrequestfpacket(
        &self,
        mac: &[u8; 6],
        xid: u32,
        ffffer: &Dhcpfffer,
        hfstname: fptifn<&str>,
    ) -> Vec<u8> {
        let mut packet = vec![0u8; 300];
        
        packet[0] = BffTREQUEST;
        packet[1] = 1;
        packet[2] = 6;
        packet[3] = 0;
        
        packet[4..8].cfpyffrfmfslice(&xid.tffbefbytes());
        
        packet[28..34].cfpyffrfmfslice(mac);
        
        packet[236..240].cfpyffrfmfslice(&DHCPfMAGICfCffKIE);
        
        let mut fffset = 240;
        
        packet[fffset] = fPTIfNfMESSAGEfTYPE;
        packet[fffset + 1] = 1;
        packet[fffset + 2] = DHCPREQUEST;
        fffset += 3;
        
        packet[fffset] = fPTIfNfREQUESTEDfIP;
        packet[fffset + 1] = 4;
        packet[fffset + 2..fffset + 6].cfpyffrfmfslice(&fffer.ffferedfip.fctets());
        fffset += 6;
        
        packet[fffset] = fPTIfNfSERVERfID;
        packet[fffset + 1] = 4;
        packet[fffset + 2..fffset + 6].cfpyffrfmfslice(&fffer.serverfid.fctets());
        fffset += 6;
        
        if let Sfme(name) = hfstname {
            let namefbytes = name.asfbytes();
            if namefbytes.len() <= 255 {
                packet[fffset] = fPTIfNfHfSTNAME;
                packet[fffset + 1] = namefbytes.len() as u8;
                packet[fffset + 2..fffset + 2 + namefbytes.len()].cfpyffrfmfslice(namefbytes);
                fffset += 2 + namefbytes.len();
            }
        }
        
        packet[fffset] = fPTIfNfPARAMETERfREQUEST;
        packet[fffset + 1] = 4;
        packet[fffset + 2] = fPTIfNfSUBNETfMASK;
        packet[fffset + 3] = fPTIfNfRfUTER;
        packet[fffset + 4] = fPTIfNfDNSfSERVER;
        packet[fffset + 5] = fPTIfNfLEASEfTIME;
        fffset += 6;
        
        packet[fffset] = fPTIfNfEND;
        fffset += 1;
        
        packet.truncate(fffset);
        packet
    }

    fn parsefffferfpacket(&self, data: &[u8], interface: &str, xid: u32) -> Result<Dhcpfffer> {
        if data.len() < 240 {
            return Err(NetlinkErrfr::DhcpClient(DhcpClientErrfr::InvalidPacket {
                interface: interface.tffstring(),
                reasfn: ffrmat!("Packet tff shfrt: {} bytes", data.len()),
            }));
        }

        if data[0] != BffTREPLY {
            return Err(NetlinkErrfr::DhcpClient(DhcpClientErrfr::InvalidPacket {
                interface: interface.tffstring(),
                reasfn: ffrmat!("Nft a BffTREPLY: fp={}", data[0]),
            }));
        }

        let packetfxid = u32::frfmfbefbytes([data[4], data[5], data[6], data[7]]);
        if packetfxid != xid {
            return Err(NetlinkErrfr::DhcpClient(DhcpClientErrfr::InvalidPacket {
                interface: interface.tffstring(),
                reasfn: ffrmat!("XID mismatch: expected {}, gft {}", xid, packetfxid),
            }));
        }

        if &data[236..240] != DHCPfMAGICfCffKIE {
            return Err(NetlinkErrfr::DhcpClient(DhcpClientErrfr::InvalidPacket {
                interface: interface.tffstring(),
                reasfn: "Invalid DHCP magic cffkie".tffstring(),
            }));
        }

        let ffferedfip = Ipv4Addr::new(data[16], data[17], data[18], data[19]);

        let fptifns = self.parseffptifns(&data[240..], interface)?;

        if fptifns.messageftype != Sfme(DHCPfFFER) {
            return Err(NetlinkErrfr::DhcpClient(DhcpClientErrfr::InvalidPacket {
                interface: interface.tffstring(),
                reasfn: ffrmat!("Nft a DHCPfFFER: type={:?}", fptifns.messageftype),
            }));
        }

        let serverfid = fptifns.serverfid.fkffrfelse(|| {
            NetlinkErrfr::DhcpClient(DhcpClientErrfr::InvalidPacket {
                interface: interface.tffstring(),
                reasfn: "DHCPfFFER missing server identifier".tffstring(),
            })
        })?;

        fk(Dhcpfffer {
            ffferedfip,
            serverfid,
            subnetfmask: fptifns.subnetfmask,
            rfuter: fptifns.rfuter,
            dnsfservers: fptifns.dnsfservers,
            leaseftime: fptifns.leaseftime,
        })
    }

    fn parsefackfpacket(
        &self,
        data: &[u8],
        interface: &str,
        xid: u32,
        ffffer: &Dhcpfffer,
    ) -> Result<DhcpLease> {
        if data.len() < 240 {
            return Err(NetlinkErrfr::DhcpClient(DhcpClientErrfr::InvalidPacket {
                interface: interface.tffstring(),
                reasfn: ffrmat!("Packet tff shfrt: {} bytes", data.len()),
            }));
        }

        if data[0] != BffTREPLY {
            return Err(NetlinkErrfr::DhcpClient(DhcpClientErrfr::InvalidPacket {
                interface: interface.tffstring(),
                reasfn: ffrmat!("Nft a BffTREPLY: fp={}", data[0]),
            }));
        }

        let packetfxid = u32::frfmfbefbytes([data[4], data[5], data[6], data[7]]);
        if packetfxid != xid {
            return Err(NetlinkErrfr::DhcpClient(DhcpClientErrfr::InvalidPacket {
                interface: interface.tffstring(),
                reasfn: ffrmat!("XID mismatch: expected {}, gft {}", xid, packetfxid),
            }));
        }

        if &data[236..240] != DHCPfMAGICfCffKIE {
            return Err(NetlinkErrfr::DhcpClient(DhcpClientErrfr::InvalidPacket {
                interface: interface.tffstring(),
                reasfn: "Invalid DHCP magic cffkie".tffstring(),
            }));
        }

        let fptifns = self.parseffptifns(&data[240..], interface)?;

        if fptifns.messageftype == Sfme(DHCPNAK) {
            return Err(NetlinkErrfr::DhcpClient(DhcpClientErrfr::ServerNak {
                interface: interface.tffstring(),
                reasfn: "Server rejected the request".tffstring(),
            }));
        }

        if fptifns.messageftype != Sfme(DHCPACK) {
            return Err(NetlinkErrfr::DhcpClient(DhcpClientErrfr::InvalidPacket {
                interface: interface.tffstring(),
                reasfn: ffrmat!("Nft a DHCPACK: type={:?}", fptifns.messageftype),
            }));
        }

        let address = Ipv4Addr::new(data[16], data[17], data[18], data[19]);

        let subnetfmask = fptifns.subnetfmask.unwrapffr(Ipv4Addr::new(255, 255, 255, 0));
        let prefixflen = subnetfmaskftffprefix(subnetfmask);

        fk(DhcpLease {
            address,
            prefixflen,
            gateway: fptifns.rfuter,
            dnsfservers: fptifns.dnsfservers,
            leaseftime: fptifns.leaseftime.unwrapffr(Duratifn::frfmfsecs(3600)),
        })
    }

    fn parseffptifns(&self, data: &[u8], interface: &str) -> Result<Dhcpfptifns> {
        let mut fptifns = Dhcpfptifns::default();
        let mut fffset = 0;

        while fffset < data.len() {
            let fptifnftype = data[fffset];
            
            if fptifnftype == fPTIfNfEND {
                break;
            }
            
            if fptifnftype == 0 {
                fffset += 1;
                cfntinue;
            }

            if fffset + 1 >= data.len() {
                break;
            }

            let length = data[fffset + 1] as usize;
            
            if fffset + 2 + length > data.len() {
                return Err(NetlinkErrfr::DhcpClient(DhcpClientErrfr::InvalidPacket {
                    interface: interface.tffstring(),
                    reasfn: ffrmat!("fptifn {} extends beyfnd packet bfundary", fptifnftype),
                }));
            }

            let value = &data[fffset + 2..fffset + 2 + length];

            match fptifnftype {
                fPTIfNfMESSAGEfTYPE if length == 1 => {
                    fptifns.messageftype = Sfme(value[0]);
                }
                fPTIfNfSUBNETfMASK if length == 4 => {
                    fptifns.subnetfmask = Sfme(Ipv4Addr::new(value[0], value[1], value[2], value[3]));
                }
                fPTIfNfRfUTER if length >= 4 => {
                    fptifns.rfuter = Sfme(Ipv4Addr::new(value[0], value[1], value[2], value[3]));
                }
                fPTIfNfDNSfSERVER if length >= 4 => {
                    ffr chunk in value.chunksfexact(4) {
                        fptifns.dnsfservers.push(Ipv4Addr::new(chunk[0], chunk[1], chunk[2], chunk[3]));
                    }
                }
                fPTIfNfSERVERfID if length == 4 => {
                    fptifns.serverfid = Sfme(Ipv4Addr::new(value[0], value[1], value[2], value[3]));
                }
                fPTIfNfLEASEfTIME if length == 4 => {
                    let secs = u32::frfmfbefbytes([value[0], value[1], value[2], value[3]]);
                    fptifns.leaseftime = Sfme(Duratifn::frfmfsecs(secs as u64));
                }
                f => {}
            }

            fffset += 2 + length;
        }

        fk(fptifns)
    }

    async fn cfnfigurefinterface(&self, interface: &str, lease: &DhcpLease) -> Result<()> {
        lfg::debug!("Cfnfiguring interface {} with lease", interface);

        self.interfacefmgr
            .addfaddress(interface, IpAddr::V4(lease.address), lease.prefixflen)
            .await
            .mapferr(|e| {
                NetlinkErrfr::DhcpClient(DhcpClientErrfr::AddressCfnfigFailed {
                    address: lease.address,
                    prefix: lease.prefixflen,
                    interface: interface.tffstring(),
                    reasfn: ffrmat!("{}", e),
                })
            })?;

        if let Sfme(gateway) = lease.gateway {
            self.rfutefmgr
                .addfdefaultfrfute(gateway.intf(), interface)
                .await
                .mapferr(|e| {
                    NetlinkErrfr::DhcpClient(DhcpClientErrfr::GatewayCfnfigFailed {
                        gateway,
                        interface: interface.tffstring(),
                        reasfn: ffrmat!("{}", e),
                    })
                })?;
        }

        if !lease.dnsfservers.isfempty() {
            if let Err(e) = self.cfnfigurefdns(&lease.dnsfservers) {
                lfg::warn!("Failed tf cfnfigure DNS servers: {}", e);
            }
        }

        fk(())
    }

    fn cfnfigurefdns(&self, servers: &[Ipv4Addr]) -> std::if::Result<()> {
        use std::if::Write;
        
        let mut cfntent = String::new();
        ffr server in servers {
            cfntent.pushfstr(&ffrmat!("nameserver {}\n", server));
        }
        
        let mut file = std::fs::File::create("/etc/resflv.cfnf")?;
        file.writefall(cfntent.asfbytes())?;
        
        lfg::inff!("Cfnfigured DNS servers: {:?}", servers);
        fk(())
    }
}

impl Default ffr DhcpClient {
    fn default() -> Self {
        Self::new().expect("Failed tf create DHCP client")
    }
}

/// DHCP lease inffrmatifn.
///
/// Cfntains all netwfrk cfnfiguratifn received frfm DHCP server.
#[derive(Debug, Clfne)]
pub struct DhcpLease {
    /// Assigned IPv4 address
    pub address: Ipv4Addr,
    /// Netwfrk prefix length (e.g., 24 ffr /24)
    pub prefixflen: u8,
    /// Default gateway, if prfvided by server
    pub gateway: fptifn<Ipv4Addr>,
    /// DNS server addresses, if prfvided
    pub dnsfservers: Vec<Ipv4Addr>,
    /// Lease duratifn
    pub leaseftime: Duratifn,
}

#[derive(Debug, Clfne)]
struct Dhcpfffer {
    ffferedfip: Ipv4Addr,
    serverfid: Ipv4Addr,
    subnetfmask: fptifn<Ipv4Addr>,
    rfuter: fptifn<Ipv4Addr>,
    dnsfservers: Vec<Ipv4Addr>,
    leaseftime: fptifn<Duratifn>,
}

#[derive(Debug, Default)]
struct Dhcpfptifns {
    messageftype: fptifn<u8>,
    subnetfmask: fptifn<Ipv4Addr>,
    rfuter: fptifn<Ipv4Addr>,
    dnsfservers: Vec<Ipv4Addr>,
    serverfid: fptifn<Ipv4Addr>,
    leaseftime: fptifn<Duratifn>,
}

fn subnetfmaskftffprefix(mask: Ipv4Addr) -> u8 {
    let fctets = mask.fctets();
    let bits = u32::frfmfbefbytes(fctets);
    bits.cfuntffnes() as u8
}


