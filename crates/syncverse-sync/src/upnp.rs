//! Automatic port forwarding for watch-party hosts: UPnP IGD (SOAP over HTTP) with NAT-PMP
//! (RFC 6886) as fallback.
//!
//! * UPnP: SSDP discovery, `AddPortMapping` with a 3600 s lease, renewal at 1800 s, and
//!   `GetSpecificPortMappingEntry` to verify after every add/renew. A leftover mapping of ours
//!   (same description) from a crashed earlier run is removed before mapping; a mapping that
//!   belongs to something else is never touched. Routers that accept only permanent leases
//!   (error 725) get a permanent mapping that is removed when the guard is dropped.
//! * NAT-PMP: UDP port mapping request to the default gateway on port 5351. The gateway is
//!   read from `/proc/net/route` on Linux; on other systems [`NatPmpError::Unsupported`] is
//!   returned and UPnP remains the only method.
//!
//! [`UpnpMappingGuard::map_port`] tries UPnP first, then NAT-PMP. Dropping the guard stops the
//! renewal thread and removes the mapping from the router.
//!
//! Nothing here contacts a third-party server: only the local router is addressed.
//! Real routers are not available to the in-tree tests; they use fake IGD and NAT-PMP servers
//! on loopback, so behaviour on specific router models is unverified.

use std::io::{self, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream, UdpSocket};
use std::sync::atomic::{AtomicU16, AtomicU32, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// Default SSDP multicast discovery address for UPnP (`239.255.255.250:1900`).
pub const SSDP_MULTICAST_ADDR: &str = "239.255.255.250:1900";

/// Description string of mappings created by Syncverse (used to recognise stale ones).
pub const MAPPING_DESCRIPTION: &str = "Syncverse Watch Party";

/// Lease requested from the router, in seconds.
pub const DEFAULT_LEASE_SECS: u32 = 3600;

/// Time after which a mapping is renewed (half the lease).
pub const DEFAULT_RENEW_AFTER: Duration = Duration::from_secs(1800);

/// UDP port of a NAT-PMP gateway (RFC 6886).
pub const NATPMP_PORT: u16 = 5351;

/// Errors of the UPnP IGD client.
#[derive(thiserror::Error, Debug, Clone, PartialEq, Eq)]
pub enum UpnpError {
    /// No IGD answered the SSDP search in time.
    #[error("UPnP IGD discovery timed out (no router answered)")]
    DiscoveryTimeout,
    /// SSDP answer without a `LOCATION` header.
    #[error("SSDP response is missing the LOCATION header")]
    MissingLocation,
    /// Router description is not served over plain HTTP.
    #[error("only http:// UPnP descriptors are supported")]
    UnsupportedScheme,
    /// Router description has no WANIPConnection / WANPPPConnection service.
    #[error("router has no WANIPConnection or WANPPPConnection service")]
    NoCompatibleService,
    /// Network I/O failed.
    #[error("network error while {context} ({kind:?})")]
    Io {
        /// What was being done.
        context: &'static str,
        /// OS error kind.
        kind: io::ErrorKind,
    },
    /// HTTP answer was not understood or had an unexpected status.
    #[error("router sent an invalid response: {0}")]
    InvalidResponse(&'static str),
    /// UPnP error 714: no such mapping.
    #[error("no such port mapping on the router (714)")]
    NoSuchEntry,
    /// UPnP error 718: the external port is already mapped for another host.
    #[error("external port is already mapped on the router (718)")]
    ConflictInMappingEntry,
    /// UPnP error 725: router supports only permanent leases.
    #[error("router supports only permanent leases (725)")]
    OnlyPermanentLeases,
    /// Any other UPnP SOAP fault.
    #[error("router rejected the request: UPnP error {code} {description}")]
    Soap {
        /// UPnP error code.
        code: u32,
        /// Router-provided description, may be empty.
        description: String,
    },
    /// The port is mapped by something that is not Syncverse; it was left alone.
    #[error("port is already forwarded by another program on the router")]
    PortUsedByOther,
    /// After adding, the router does not list the mapping as expected.
    #[error("router did not confirm the new port mapping")]
    VerificationFailed,
    /// Could not find the local address facing the router.
    #[error("could not determine the local address facing the router")]
    NoLocalAddress,
}

/// Errors of the NAT-PMP client.
#[derive(thiserror::Error, Debug, Clone, PartialEq, Eq)]
pub enum NatPmpError {
    /// NAT-PMP gateway lookup is only implemented for Linux.
    #[error("NAT-PMP gateway discovery is not supported on this operating system")]
    Unsupported,
    /// No default IPv4 gateway is configured.
    #[error("no default IPv4 gateway found")]
    NoGateway,
    /// Gateway did not answer.
    #[error("NAT-PMP gateway did not answer")]
    Timeout,
    /// Answer has wrong size, version, opcode or fields.
    #[error("NAT-PMP gateway sent an invalid response")]
    BadResponse,
    /// Gateway answered with a non-zero result code.
    #[error("NAT-PMP gateway refused the request: {0}")]
    ResultCode(NatPmpResultCode),
    /// Network I/O failed.
    #[error("network error ({0:?})")]
    Io(io::ErrorKind),
}

/// NAT-PMP result codes (RFC 6886 section 3.5).
#[derive(thiserror::Error, Debug, Clone, Copy, PartialEq, Eq)]
pub enum NatPmpResultCode {
    /// Result 1.
    #[error("unsupported version")]
    UnsupportedVersion,
    /// Result 2.
    #[error("not authorized or refused")]
    NotAuthorized,
    /// Result 3.
    #[error("network failure")]
    NetworkFailure,
    /// Result 4.
    #[error("out of resources")]
    OutOfResources,
    /// Result 5.
    #[error("unsupported opcode")]
    UnsupportedOpcode,
    /// Any other non-zero value.
    #[error("unknown result code {0}")]
    Other(u16),
}

impl NatPmpResultCode {
    fn from_code(code: u16) -> Self {
        match code {
            1 => Self::UnsupportedVersion,
            2 => Self::NotAuthorized,
            3 => Self::NetworkFailure,
            4 => Self::OutOfResources,
            5 => Self::UnsupportedOpcode,
            other => Self::Other(other),
        }
    }
}

/// Failure of [`UpnpMappingGuard::try_map_port`]: which methods were tried and why they failed.
#[derive(thiserror::Error, Debug, Clone, PartialEq, Eq)]
pub enum PortMapError {
    /// UPnP failed and NAT-PMP was not attempted (disabled by configuration).
    #[error("UPnP: {0}")]
    Upnp(UpnpError),
    /// UPnP failed and NAT-PMP also failed.
    #[error("UPnP: {upnp}; NAT-PMP: {natpmp}")]
    Both {
        /// Why UPnP failed.
        upnp: UpnpError,
        /// Why NAT-PMP failed.
        natpmp: NatPmpError,
    },
    /// Renewal failed (only reported through [`UpnpMappingGuard::last_error`]).
    #[error("UPnP renewal failed: {0}")]
    UpnpRenewal(UpnpError),
    /// NAT-PMP renewal failed (only reported through [`UpnpMappingGuard::last_error`]).
    #[error("NAT-PMP renewal failed: {0}")]
    NatPmpRenewal(NatPmpError),
}

/// Which protocol created a mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortMapProtocol {
    /// UPnP Internet Gateway Device.
    Upnp,
    /// NAT-PMP (RFC 6886).
    NatPmp,
}

impl PortMapProtocol {
    /// Display name.
    pub fn name(self) -> &'static str {
        match self {
            PortMapProtocol::Upnp => "UPnP",
            PortMapProtocol::NatPmp => "NAT-PMP",
        }
    }
}

/// Tunables for port mapping; the defaults are what the application uses.
#[derive(Debug, Clone)]
pub struct PortMapConfig {
    /// Where the SSDP M-SEARCH goes (multicast by default; tests inject a loopback address).
    pub ssdp_addr: String,
    /// How long to wait for SSDP answers.
    pub discovery_timeout: Duration,
    /// Timeout for each HTTP request to the router.
    pub http_timeout: Duration,
    /// Lease requested in seconds (0 means permanent).
    pub lease_secs: u32,
    /// Renew this long after (re)mapping.
    pub renew_after: Duration,
    /// Local IPv4 to register as the internal client; discovered from the route when `None`.
    pub local_ip: Option<String>,
    /// NAT-PMP gateway; the default IPv4 gateway on port 5351 when `None`.
    pub natpmp_gateway: Option<SocketAddr>,
    /// NAT-PMP first retransmission timeout (doubles each try; RFC says 250 ms).
    pub natpmp_initial_timeout: Duration,
    /// NAT-PMP transmissions before giving up.
    pub natpmp_attempts: u32,
    /// Whether to try NAT-PMP when UPnP fails.
    pub try_natpmp: bool,
}

impl Default for PortMapConfig {
    fn default() -> Self {
        Self {
            ssdp_addr: SSDP_MULTICAST_ADDR.to_string(),
            discovery_timeout: Duration::from_millis(1500),
            http_timeout: Duration::from_millis(2000),
            lease_secs: DEFAULT_LEASE_SECS,
            renew_after: DEFAULT_RENEW_AFTER,
            local_ip: None,
            natpmp_gateway: None,
            natpmp_initial_timeout: Duration::from_millis(250),
            natpmp_attempts: 4,
            try_natpmp: true,
        }
    }
}

/// Discovered UPnP Internet Gateway Device control information.
#[derive(Debug, Clone)]
pub struct IgdService {
    /// Router host address (IP and port).
    pub host: String,
    /// Control path URL for WANIPConnection or WANPPPConnection.
    pub control_url: String,
    /// Exact service type schema string.
    pub service_type: String,
}

/// Status of automatic port forwarding for a watch-party session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpnpStatus {
    /// Port was forwarded (by UPnP or NAT-PMP, see [`UpnpMappingGuard::protocol`]).
    Active {
        /// Public WAN IP address of the router, if discoverable.
        external_ip: Option<String>,
        /// Forwarded UDP port.
        port: u16,
    },
    /// Neither UPnP nor NAT-PMP worked.
    Disabled {
        /// Human-readable reason for falling back to direct UDP.
        reason: String,
    },
}

#[derive(Clone)]
enum Backend {
    Igd {
        service: IgdService,
        local_ip: String,
    },
    NatPmp {
        client: NatPmpClient,
    },
}

struct GuardShared {
    stop: Mutex<bool>,
    wake: Condvar,
    renewals: AtomicU32,
    lease_secs: AtomicU32,
    external_port: AtomicU16,
    last_error: Mutex<Option<PortMapError>>,
}

/// Active port mapping. Renews itself in the background and unmaps the port when dropped.
pub struct UpnpMappingGuard {
    backend: Backend,
    internal_port: u16,
    external_ip: Option<String>,
    verified: bool,
    shared: Arc<GuardShared>,
    renew_thread: Option<JoinHandle<()>>,
}

impl UpnpMappingGuard {
    /// Returns the public WAN IP discovered from the router, if available.
    pub fn external_ip(&self) -> Option<&str> {
        self.external_ip.as_deref()
    }

    /// Returns the mapped external port.
    pub fn port(&self) -> u16 {
        self.shared.external_port.load(Ordering::Relaxed)
    }

    /// Which protocol created this mapping.
    pub fn protocol(&self) -> PortMapProtocol {
        match self.backend {
            Backend::Igd { .. } => PortMapProtocol::Upnp,
            Backend::NatPmp { .. } => PortMapProtocol::NatPmp,
        }
    }

    /// Lease the router granted in seconds (0 = permanent, never renewed).
    pub fn lease_secs(&self) -> u32 {
        self.shared.lease_secs.load(Ordering::Relaxed)
    }

    /// True when the router listed the mapping back after it was added (UPnP only; NAT-PMP
    /// answers are the confirmation and count as verified).
    pub fn verified(&self) -> bool {
        self.verified
    }

    /// Number of successful renewals so far.
    pub fn renewals(&self) -> u32 {
        self.shared.renewals.load(Ordering::Relaxed)
    }

    /// Error of the latest renewal attempt, cleared by the next success.
    pub fn last_error(&self) -> Option<PortMapError> {
        self.shared
            .last_error
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// One line for the host dialog, e.g. `UPnP active: 203.0.113.5:7000 (verified)`.
    pub fn status_text(&self) -> String {
        let ip = self.external_ip().unwrap_or("unknown address");
        let mut text = format!(
            "{} active: {}:{}{}",
            self.protocol().name(),
            ip,
            self.port(),
            if self.verified { " (verified)" } else { "" }
        );
        if let Some(err) = self.last_error() {
            text.push_str(&format!("; last renewal failed: {err}"));
        }
        text
    }

    /// Discovers a gateway and maps `port` with the default configuration.
    ///
    /// Uses a 1.5 s discovery budget. Returns [`UpnpStatus::Disabled`] with the reason when
    /// neither UPnP nor NAT-PMP works.
    pub fn map_port(port: u16) -> (UpnpStatus, Option<Self>) {
        Self::map_port_with(port, &PortMapConfig::default())
    }

    /// Like [`UpnpMappingGuard::map_port`] with explicit configuration.
    pub fn map_port_with(port: u16, cfg: &PortMapConfig) -> (UpnpStatus, Option<Self>) {
        match Self::try_map_port(port, cfg) {
            Ok(guard) => (
                UpnpStatus::Active {
                    external_ip: guard.external_ip.clone(),
                    port: guard.port(),
                },
                Some(guard),
            ),
            Err(err) => (
                UpnpStatus::Disabled {
                    reason: err.to_string(),
                },
                None,
            ),
        }
    }

    /// Typed variant: UPnP first, NAT-PMP second.
    pub fn try_map_port(port: u16, cfg: &PortMapConfig) -> Result<Self, PortMapError> {
        let upnp_err = match Self::map_upnp(port, cfg) {
            Ok(g) => return Ok(g),
            Err(e) => e,
        };
        if !cfg.try_natpmp {
            return Err(PortMapError::Upnp(upnp_err));
        }
        match Self::map_natpmp(port, cfg) {
            Ok(g) => Ok(g),
            Err(natpmp) => Err(PortMapError::Both {
                upnp: upnp_err,
                natpmp,
            }),
        }
    }

    fn new_shared(external_port: u16, lease: u32) -> Arc<GuardShared> {
        Arc::new(GuardShared {
            stop: Mutex::new(false),
            wake: Condvar::new(),
            renewals: AtomicU32::new(0),
            lease_secs: AtomicU32::new(lease),
            external_port: AtomicU16::new(external_port),
            last_error: Mutex::new(None),
        })
    }

    fn map_upnp(port: u16, cfg: &PortMapConfig) -> Result<Self, UpnpError> {
        let service = discover_igd(&cfg.ssdp_addr, cfg.discovery_timeout, cfg.http_timeout)?;
        let local_ip = match &cfg.local_ip {
            Some(ip) => ip.clone(),
            None => get_local_ip(&service.host).ok_or(UpnpError::NoLocalAddress)?,
        };
        let timeout = cfg.http_timeout;

        // Stale mapping cleanup: remove our own leftover, never somebody else's.
        match get_specific_mapping(&service, port, timeout) {
            Ok(entry) if entry.description == MAPPING_DESCRIPTION => {
                delete_port_mapping(&service, port, timeout)?;
            }
            Ok(_) => return Err(UpnpError::PortUsedByOther),
            Err(_) => {} // 714 or router without the action: just try to add
        }

        let mut lease = cfg.lease_secs;
        match add_port_mapping(&service, port, port, &local_ip, lease, timeout) {
            Ok(()) => {}
            Err(UpnpError::OnlyPermanentLeases) if lease != 0 => {
                lease = 0;
                add_port_mapping(&service, port, port, &local_ip, 0, timeout)?;
            }
            Err(e) => return Err(e),
        }
        let verified = verify_mapping(&service, port, &local_ip, timeout)?;
        let external_ip = get_external_ip(&service, timeout).ok();

        let shared = Self::new_shared(port, lease);
        let backend = Backend::Igd { service, local_ip };
        let renew_thread = spawn_renewer(backend.clone(), port, Arc::clone(&shared), cfg, lease);
        Ok(Self {
            backend,
            internal_port: port,
            external_ip,
            verified,
            shared,
            renew_thread,
        })
    }

    fn map_natpmp(port: u16, cfg: &PortMapConfig) -> Result<Self, NatPmpError> {
        let gateway = match cfg.natpmp_gateway {
            Some(g) => g,
            None => SocketAddr::from((default_gateway()?, NATPMP_PORT)),
        };
        let client = NatPmpClient::new(gateway, cfg.natpmp_initial_timeout, cfg.natpmp_attempts);
        let external_ip = client.external_address().ok().map(|ip| ip.to_string());
        let granted = client.map_udp(port, port, cfg.lease_secs)?;
        let shared = Self::new_shared(granted.external_port, granted.lifetime_secs);
        let backend = Backend::NatPmp { client };
        let renew_thread = spawn_renewer(
            backend.clone(),
            port,
            Arc::clone(&shared),
            cfg,
            granted.lifetime_secs,
        );
        Ok(Self {
            backend,
            internal_port: port,
            external_ip,
            verified: true,
            shared,
            renew_thread,
        })
    }

    /// Explicitly unmaps the forwarded port from the router.
    pub fn release(self) {
        // Dropping stops the renewer and deletes the mapping.
        drop(self);
    }
}

impl Drop for UpnpMappingGuard {
    fn drop(&mut self) {
        *self.shared.stop.lock().unwrap_or_else(|e| e.into_inner()) = true;
        self.shared.wake.notify_all();
        if let Some(h) = self.renew_thread.take() {
            let _ = h.join();
        }
        let ext = self.port();
        match &self.backend {
            Backend::Igd { service, .. } => {
                let _ = delete_port_mapping(service, ext, Duration::from_millis(1500));
            }
            Backend::NatPmp { client } => {
                let _ = client.unmap_udp(self.internal_port);
            }
        }
    }
}

fn spawn_renewer(
    backend: Backend,
    internal_port: u16,
    shared: Arc<GuardShared>,
    cfg: &PortMapConfig,
    lease: u32,
) -> Option<JoinHandle<()>> {
    if lease == 0 {
        return None;
    }
    let renew_after = cfg.renew_after;
    let http_timeout = cfg.http_timeout;
    let requested_lease = cfg.lease_secs;
    std::thread::Builder::new()
        .name("syncverse-portmap-renew".to_string())
        .spawn(move || {
            let retry = renew_after.min(Duration::from_secs(30));
            let mut wait = renew_after;
            loop {
                {
                    let mut stopped = shared.stop.lock().unwrap_or_else(|e| e.into_inner());
                    let deadline = Instant::now() + wait;
                    while !*stopped {
                        let now = Instant::now();
                        if now >= deadline {
                            break;
                        }
                        let (g, _) = shared
                            .wake
                            .wait_timeout(stopped, deadline - now)
                            .unwrap_or_else(|e| e.into_inner());
                        stopped = g;
                    }
                    if *stopped {
                        return;
                    }
                }
                let ext = shared.external_port.load(Ordering::Relaxed);
                let result = match &backend {
                    Backend::Igd { service, local_ip } => add_port_mapping(
                        service,
                        ext,
                        internal_port,
                        local_ip,
                        requested_lease,
                        http_timeout,
                    )
                    .and_then(|()| verify_mapping(service, ext, local_ip, http_timeout))
                    .map(|_| ())
                    .map_err(PortMapError::UpnpRenewal),
                    Backend::NatPmp { client } => client
                        .map_udp(internal_port, ext, requested_lease)
                        .map(|g| {
                            shared
                                .external_port
                                .store(g.external_port, Ordering::Relaxed);
                        })
                        .map_err(PortMapError::NatPmpRenewal),
                };
                let mut last = shared.last_error.lock().unwrap_or_else(|e| e.into_inner());
                match result {
                    Ok(()) => {
                        shared.renewals.fetch_add(1, Ordering::Relaxed);
                        *last = None;
                        wait = renew_after;
                    }
                    Err(e) => {
                        *last = Some(e);
                        wait = retry;
                    }
                }
            }
        })
        .ok()
}

/// Spawns a background thread (`"syncverse-upnp"`) that discovers a gateway and maps the
/// given UDP port without stalling playback or watch-party initialisation.
///
/// The returned `mpsc::Receiver` yields `(Option<UpnpMappingGuard>, UpnpStatus)` once
/// discovery and mapping complete or fail. Keep the guard alive for as long as the mapping
/// should exist.
pub fn spawn_upnp_mapping(
    port: u16,
) -> std::sync::mpsc::Receiver<(Option<UpnpMappingGuard>, UpnpStatus)> {
    let (tx, rx) = std::sync::mpsc::channel();
    let tx_worker = tx.clone();
    if let Err(err) = std::thread::Builder::new()
        .name("syncverse-upnp".to_string())
        .spawn(move || {
            let (status, guard) = UpnpMappingGuard::map_port(port);
            let _ = tx_worker.send((guard, status));
        })
    {
        let _ = tx.send((
            None,
            UpnpStatus::Disabled {
                reason: format!("Failed to spawn UPnP thread: {}", err),
            },
        ));
    }
    rx
}

fn io_err(context: &'static str) -> impl Fn(io::Error) -> UpnpError {
    move |e| UpnpError::Io {
        context,
        kind: e.kind(),
    }
}

/// Discovers an Internet Gateway Device via SSDP.
fn discover_igd(
    ssdp_addr: &str,
    timeout: Duration,
    http_timeout: Duration,
) -> Result<IgdService, UpnpError> {
    let socket = UdpSocket::bind("0.0.0.0:0").map_err(io_err("binding the SSDP socket"))?;
    let search_msg = format!(
        "M-SEARCH * HTTP/1.1\r\nHOST: {ssdp_addr}\r\nMAN: \"ssdp:discover\"\r\nMX: 1\r\n\
         ST: urn:schemas-upnp-org:device:InternetGatewayDevice:1\r\n\r\n"
    );
    socket
        .send_to(search_msg.as_bytes(), ssdp_addr)
        .map_err(io_err("sending the SSDP search"))?;

    let deadline = Instant::now() + timeout;
    let mut buf = [0u8; 4096];
    let mut last_err = UpnpError::DiscoveryTimeout;
    loop {
        let now = Instant::now();
        if now >= deadline {
            return Err(last_err);
        }
        socket
            .set_read_timeout(Some((deadline - now).max(Duration::from_millis(1))))
            .map_err(io_err("setting the SSDP timeout"))?;
        let n = match socket.recv_from(&mut buf) {
            Ok((n, _)) => n,
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) =>
            {
                return Err(last_err)
            }
            Err(e) => return Err(io_err("reading the SSDP answer")(e)),
        };
        let response = String::from_utf8_lossy(&buf[..n]);
        let Some(location) = extract_header_value(&response, "LOCATION") else {
            last_err = UpnpError::MissingLocation;
            continue;
        };
        match fetch_and_parse_root_desc(&location, http_timeout) {
            Ok(svc) => return Ok(svc),
            Err(e) => last_err = e,
        }
    }
}

/// Fetches the router's root XML description and extracts the WAN connection control URL.
fn fetch_and_parse_root_desc(
    location_url: &str,
    timeout: Duration,
) -> Result<IgdService, UpnpError> {
    let url_without_proto = location_url
        .trim()
        .strip_prefix("http://")
        .ok_or(UpnpError::UnsupportedScheme)?;
    let slash_idx = url_without_proto
        .find('/')
        .unwrap_or(url_without_proto.len());
    let host = &url_without_proto[..slash_idx];
    let path = if slash_idx < url_without_proto.len() {
        &url_without_proto[slash_idx..]
    } else {
        "/"
    };
    let (status, body) = http_request("GET", host, path, None, &[], timeout)?;
    if status != 200 {
        return Err(UpnpError::InvalidResponse(
            "device description is not HTTP 200",
        ));
    }
    let (service_type, control_url) = parse_service_descriptor(&body)?;
    let control_url = match control_url.strip_prefix("http://") {
        Some(rest) => rest
            .find('/')
            .map_or("/".to_string(), |i| rest[i..].to_string()),
        None if control_url.starts_with('/') => control_url,
        None => format!("/{control_url}"),
    };
    Ok(IgdService {
        host: host.to_string(),
        control_url,
        service_type,
    })
}

/// Parses the service type and control URL from XML descriptor content.
fn parse_service_descriptor(xml: &str) -> Result<(String, String), UpnpError> {
    let services = [
        "urn:schemas-upnp-org:service:WANIPConnection:1",
        "urn:schemas-upnp-org:service:WANIPConnection:2",
        "urn:schemas-upnp-org:service:WANPPPConnection:1",
    ];
    for svc in &services {
        if let Some(pos) = xml.find(svc) {
            if let Some(ctrl) = xml_tag(&xml[pos..], "controlURL") {
                return Ok((svc.to_string(), ctrl));
            }
        }
    }
    Err(UpnpError::NoCompatibleService)
}

/// Returns the trimmed text of the first `<tag>..</tag>` in `xml`.
fn xml_tag(xml: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = xml.find(&open)? + open.len();
    let end = xml[start..].find(&close)?;
    Some(xml[start..start + end].trim().to_string())
}

/// Performs one SOAP action and maps SOAP faults to typed errors; returns the body on success.
fn soap_call(
    service: &IgdService,
    action: &str,
    args: &str,
    timeout: Duration,
) -> Result<String, UpnpError> {
    let body = format!(
        "<?xml version=\"1.0\"?>\r\n<s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\" \
         s:encodingStyle=\"http://schemas.xmlsoap.org/soap/encoding/\"><s:Body>\
         <u:{action} xmlns:u=\"{}\">{args}</u:{action}></s:Body></s:Envelope>",
        service.service_type
    );
    let soap_action = format!("\"{}#{}\"", service.service_type, action);
    let (status, resp) = http_request(
        "POST",
        &service.host,
        &service.control_url,
        Some(&body),
        &[("SOAPAction", soap_action.as_str())],
        timeout,
    )?;
    if status == 200 {
        return Ok(resp);
    }
    match xml_tag(&resp, "errorCode").and_then(|c| c.parse::<u32>().ok()) {
        Some(714) => Err(UpnpError::NoSuchEntry),
        Some(718) => Err(UpnpError::ConflictInMappingEntry),
        Some(725) => Err(UpnpError::OnlyPermanentLeases),
        Some(code) => Err(UpnpError::Soap {
            code,
            description: xml_tag(&resp, "errorDescription").unwrap_or_default(),
        }),
        None => Err(UpnpError::InvalidResponse(
            "HTTP error without a UPnP error code",
        )),
    }
}

fn add_port_mapping(
    service: &IgdService,
    external_port: u16,
    internal_port: u16,
    local_ip: &str,
    lease: u32,
    timeout: Duration,
) -> Result<(), UpnpError> {
    let args = format!(
        "<NewRemoteHost></NewRemoteHost><NewExternalPort>{external_port}</NewExternalPort>\
         <NewProtocol>UDP</NewProtocol><NewInternalPort>{internal_port}</NewInternalPort>\
         <NewInternalClient>{local_ip}</NewInternalClient><NewEnabled>1</NewEnabled>\
         <NewPortMappingDescription>{MAPPING_DESCRIPTION}</NewPortMappingDescription>\
         <NewLeaseDuration>{lease}</NewLeaseDuration>"
    );
    soap_call(service, "AddPortMapping", &args, timeout).map(|_| ())
}

fn delete_port_mapping(
    service: &IgdService,
    external_port: u16,
    timeout: Duration,
) -> Result<(), UpnpError> {
    let args = format!(
        "<NewRemoteHost></NewRemoteHost><NewExternalPort>{external_port}</NewExternalPort>\
         <NewProtocol>UDP</NewProtocol>"
    );
    match soap_call(service, "DeletePortMapping", &args, timeout) {
        Ok(_) | Err(UpnpError::NoSuchEntry) => Ok(()),
        Err(e) => Err(e),
    }
}

/// A mapping as listed by the router.
#[derive(Debug, Clone, PartialEq, Eq)]
struct MappingEntry {
    internal_client: String,
    description: String,
}

fn get_specific_mapping(
    service: &IgdService,
    external_port: u16,
    timeout: Duration,
) -> Result<MappingEntry, UpnpError> {
    let args = format!(
        "<NewRemoteHost></NewRemoteHost><NewExternalPort>{external_port}</NewExternalPort>\
         <NewProtocol>UDP</NewProtocol>"
    );
    let resp = soap_call(service, "GetSpecificPortMappingEntry", &args, timeout)?;
    let internal_client = xml_tag(&resp, "NewInternalClient").ok_or(UpnpError::InvalidResponse(
        "mapping entry without internal client",
    ))?;
    Ok(MappingEntry {
        internal_client,
        description: xml_tag(&resp, "NewPortMappingDescription").unwrap_or_default(),
    })
}

/// Checks that the router lists our mapping. `Ok(true)` verified, `Ok(false)` the router does
/// not implement the query (accepted, reported as unverified), `Err` the mapping is missing
/// or points elsewhere.
fn verify_mapping(
    service: &IgdService,
    external_port: u16,
    local_ip: &str,
    timeout: Duration,
) -> Result<bool, UpnpError> {
    match get_specific_mapping(service, external_port, timeout) {
        Ok(entry) if entry.internal_client == local_ip => Ok(true),
        Ok(_) | Err(UpnpError::NoSuchEntry) => Err(UpnpError::VerificationFailed),
        Err(UpnpError::Soap { .. }) | Err(UpnpError::InvalidResponse(_)) => Ok(false),
        Err(e) => Err(e),
    }
}

/// Queries the router's external public WAN IP address.
fn get_external_ip(service: &IgdService, timeout: Duration) -> Result<String, UpnpError> {
    let resp = soap_call(service, "GetExternalIPAddress", "", timeout)?;
    match xml_tag(&resp, "NewExternalIPAddress") {
        Some(ip) if !ip.is_empty() && ip != "0.0.0.0" => Ok(ip),
        _ => Err(UpnpError::InvalidResponse("no usable external IP address")),
    }
}

/// Minimal HTTP/1.1 client (one request, `Connection: close`). Returns status and body.
fn http_request(
    method: &str,
    host: &str,
    path: &str,
    body: Option<&str>,
    extra: &[(&str, &str)],
    timeout: Duration,
) -> Result<(u16, String), UpnpError> {
    let addr: SocketAddr = host
        .parse()
        .map_err(|_| UpnpError::InvalidResponse("router address is not an IPv4 host:port"))?;
    let mut stream =
        TcpStream::connect_timeout(&addr, timeout).map_err(io_err("connecting to the router"))?;
    stream
        .set_read_timeout(Some(timeout))
        .and_then(|()| stream.set_write_timeout(Some(timeout)))
        .map_err(io_err("setting router timeouts"))?;

    let mut req =
        format!("{method} {path} HTTP/1.1\r\nHost: {host}\r\nUser-Agent: Syncverse/0.4.0\r\n");
    if let Some(b) = body {
        req.push_str(&format!(
            "Content-Type: text/xml; charset=\"utf-8\"\r\nContent-Length: {}\r\n",
            b.len()
        ));
    }
    for (k, v) in extra {
        req.push_str(&format!("{k}: {v}\r\n"));
    }
    req.push_str("Connection: close\r\n\r\n");
    if let Some(b) = body {
        req.push_str(b);
    }
    stream
        .write_all(req.as_bytes())
        .map_err(io_err("sending to the router"))?;

    let mut raw = Vec::new();
    // A reset after a complete answer is common on cheap routers; keep what we have.
    if let Err(e) = stream.read_to_end(&mut raw) {
        if raw.is_empty() {
            return Err(io_err("reading the router answer")(e));
        }
    }
    let text = String::from_utf8_lossy(&raw);
    let status = text
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|s| s.parse::<u16>().ok())
        .ok_or(UpnpError::InvalidResponse("malformed HTTP status line"))?;
    let body = text
        .split_once("\r\n\r\n")
        .map_or(String::new(), |(_, b)| b.to_string());
    Ok((status, body))
}

/// Discovers the local machine's IP on the network interface facing the router.
fn get_local_ip(target_host: &str) -> Option<String> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect(target_host).ok()?;
    match socket.local_addr().ok()? {
        SocketAddr::V4(v4) => Some(v4.ip().to_string()),
        SocketAddr::V6(_) => None,
    }
}

/// Helper to extract header value from HTTP/SSDP headers case-insensitively.
fn extract_header_value(headers: &str, target_key: &str) -> Option<String> {
    for line in headers.lines() {
        if let Some(colon) = line.find(':') {
            let key = line[..colon].trim();
            if key.eq_ignore_ascii_case(target_key) {
                return Some(line[colon + 1..].trim().to_string());
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------------------------
// NAT-PMP (RFC 6886)
// ---------------------------------------------------------------------------------------------

/// A mapping granted by a NAT-PMP gateway.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NatPmpMapping {
    /// External UDP port the gateway assigned (may differ from the one requested).
    pub external_port: u16,
    /// Lifetime granted in seconds.
    pub lifetime_secs: u32,
    /// Gateway "seconds since start of epoch" counter; a drop means the gateway rebooted.
    pub epoch_secs: u32,
}

/// NAT-PMP client for one gateway.
#[derive(Debug, Clone)]
pub struct NatPmpClient {
    gateway: SocketAddr,
    initial_timeout: Duration,
    attempts: u32,
}

impl NatPmpClient {
    /// Creates a client. Retransmission timeout starts at `initial_timeout` and doubles.
    pub fn new(gateway: SocketAddr, initial_timeout: Duration, attempts: u32) -> Self {
        Self {
            gateway,
            initial_timeout,
            attempts: attempts.max(1),
        }
    }

    /// Creates a client for the default IPv4 gateway with RFC timing.
    ///
    /// # Errors
    /// [`NatPmpError::Unsupported`] off Linux, [`NatPmpError::NoGateway`] without a route.
    pub fn for_default_gateway() -> Result<Self, NatPmpError> {
        Ok(Self::new(
            SocketAddr::from((default_gateway()?, NATPMP_PORT)),
            Duration::from_millis(250),
            4,
        ))
    }

    fn exchange(
        &self,
        request: &[u8],
        expect_len: usize,
        expect_op: u8,
    ) -> Result<Vec<u8>, NatPmpError> {
        let socket = UdpSocket::bind("0.0.0.0:0").map_err(|e| NatPmpError::Io(e.kind()))?;
        socket
            .connect(self.gateway)
            .map_err(|e| NatPmpError::Io(e.kind()))?;
        let mut timeout = self.initial_timeout;
        let mut buf = [0u8; 64];
        for _ in 0..self.attempts {
            socket
                .send(request)
                .map_err(|e| NatPmpError::Io(e.kind()))?;
            socket
                .set_read_timeout(Some(timeout))
                .map_err(|e| NatPmpError::Io(e.kind()))?;
            // The socket is connected, so only the gateway can answer.
            match socket.recv(&mut buf) {
                Ok(n) => {
                    if n < 4 || buf[0] != 0 || buf[1] != expect_op {
                        return Err(NatPmpError::BadResponse);
                    }
                    let result = u16::from_be_bytes([buf[2], buf[3]]);
                    if result != 0 {
                        return Err(NatPmpError::ResultCode(NatPmpResultCode::from_code(result)));
                    }
                    if n != expect_len {
                        return Err(NatPmpError::BadResponse);
                    }
                    return Ok(buf[..n].to_vec());
                }
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::WouldBlock
                            | io::ErrorKind::TimedOut
                            | io::ErrorKind::ConnectionRefused
                    ) => {}
                Err(e) => return Err(NatPmpError::Io(e.kind())),
            }
            timeout *= 2;
        }
        Err(NatPmpError::Timeout)
    }

    /// Asks the gateway for its public IPv4 address (opcode 0).
    pub fn external_address(&self) -> Result<Ipv4Addr, NatPmpError> {
        let resp = self.exchange(&[0, 0], 12, 128)?;
        Ok(Ipv4Addr::new(resp[8], resp[9], resp[10], resp[11]))
    }

    /// Requests (or renews) a UDP mapping (opcode 1 = UDP in RFC 6886).
    pub fn map_udp(
        &self,
        internal_port: u16,
        suggested_external: u16,
        lifetime_secs: u32,
    ) -> Result<NatPmpMapping, NatPmpError> {
        let mut req = [0u8; 12];
        req[1] = 1; // UDP
        req[4..6].copy_from_slice(&internal_port.to_be_bytes());
        req[6..8].copy_from_slice(&suggested_external.to_be_bytes());
        req[8..12].copy_from_slice(&lifetime_secs.to_be_bytes());
        let resp = self.exchange(&req, 16, 129)?;
        if u16::from_be_bytes([resp[8], resp[9]]) != internal_port {
            return Err(NatPmpError::BadResponse);
        }
        Ok(NatPmpMapping {
            epoch_secs: u32::from_be_bytes([resp[4], resp[5], resp[6], resp[7]]),
            external_port: u16::from_be_bytes([resp[10], resp[11]]),
            lifetime_secs: u32::from_be_bytes([resp[12], resp[13], resp[14], resp[15]]),
        })
    }

    /// Removes the mapping of `internal_port` (lifetime 0, external port 0 per RFC 6886).
    pub fn unmap_udp(&self, internal_port: u16) -> Result<(), NatPmpError> {
        self.map_udp(internal_port, 0, 0).map(|_| ())
    }
}

/// Finds the default IPv4 gateway. Implemented for Linux (`/proc/net/route`).
///
/// # Errors
/// [`NatPmpError::Unsupported`] on other operating systems.
#[cfg(target_os = "linux")]
pub fn default_gateway() -> Result<Ipv4Addr, NatPmpError> {
    let text = std::fs::read_to_string("/proc/net/route").map_err(|e| NatPmpError::Io(e.kind()))?;
    parse_proc_net_route(&text).ok_or(NatPmpError::NoGateway)
}

/// Finds the default IPv4 gateway. Not implemented on this operating system.
///
/// # Errors
/// Always [`NatPmpError::Unsupported`].
#[cfg(not(target_os = "linux"))]
pub fn default_gateway() -> Result<Ipv4Addr, NatPmpError> {
    Err(NatPmpError::Unsupported)
}

/// Parses the text of `/proc/net/route` and returns the gateway of the default route with
/// the lowest metric (flags must include UP and GATEWAY).
pub fn parse_proc_net_route(text: &str) -> Option<Ipv4Addr> {
    let mut best: Option<(u32, Ipv4Addr)> = None;
    for line in text.lines().skip(1) {
        let cols: Vec<&str> = line.split_whitespace().collect();
        if cols.len() < 8 || cols[1] != "00000000" || cols[7] != "00000000" {
            continue;
        }
        let (Ok(gw), Ok(flags), Ok(metric)) = (
            u32::from_str_radix(cols[2], 16),
            u32::from_str_radix(cols[3], 16),
            cols[6].parse::<u32>(),
        ) else {
            continue;
        };
        if flags & 0x3 != 0x3 || gw == 0 {
            continue;
        }
        // The kernel prints the in_addr as a host-order integer.
        let ip = Ipv4Addr::from(gw.to_ne_bytes());
        if best.is_none_or(|(m, _)| metric < m) {
            best = Some((metric, ip));
        }
    }
    best.map(|(_, ip)| ip)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_header_value() {
        let headers = "HTTP/1.1 200 OK\r\nLocation: http://192.168.1.1:49152/root.xml\r\nServer: Router/1.0\r\n\r\n";
        assert_eq!(
            extract_header_value(headers, "LOCATION"),
            Some("http://192.168.1.1:49152/root.xml".to_string())
        );
        assert_eq!(
            extract_header_value(headers, "location"),
            Some("http://192.168.1.1:49152/root.xml".to_string())
        );
        assert_eq!(extract_header_value(headers, "NON_EXISTENT"), None);
    }

    #[test]
    fn test_parse_service_descriptor() {
        let xml = r#"
            <root xmlns="urn:schemas-upnp-org:device-1-0">
                <service>
                    <serviceType>urn:schemas-upnp-org:service:WANIPConnection:1</serviceType>
                    <controlURL>/ctl/IPConn</controlURL>
                </service>
            </root>
        "#;
        let (svc, ctrl) = parse_service_descriptor(xml).unwrap();
        assert_eq!(svc, "urn:schemas-upnp-org:service:WANIPConnection:1");
        assert_eq!(ctrl, "/ctl/IPConn");
        assert_eq!(
            parse_service_descriptor("<root/>"),
            Err(UpnpError::NoCompatibleService)
        );
    }

    #[test]
    fn test_spawn_upnp_mapping_is_non_blocking() {
        let start = std::time::Instant::now();
        let rx = spawn_upnp_mapping(18242);
        // Spawning must return immediately well below the 1500ms SSDP timeout.
        assert!(
            start.elapsed() < Duration::from_millis(200),
            "spawn_upnp_mapping must not block the caller thread"
        );
        // Channel stays open until the background thread completes or drops.
        drop(rx);
    }

    #[test]
    fn proc_net_route_gateway() {
        let text =
            "Iface\tDestination\tGateway\tFlags\tRefCnt\tUse\tMetric\tMask\tMTU\tWindow\tIRTT\n\
                    wlan0\t00000000\t0101A8C0\t0003\t0\t0\t600\t00000000\t0\t0\t0\n\
                    eth0\t00000000\t0100000A\t0003\t0\t0\t100\t00000000\t0\t0\t0\n\
                    wlan0\t0001A8C0\t00000000\t0001\t0\t0\t600\t00FFFFFF\t0\t0\t0\n";
        if cfg!(target_endian = "little") {
            assert_eq!(parse_proc_net_route(text), Some(Ipv4Addr::new(10, 0, 0, 1)));
        }
        assert_eq!(parse_proc_net_route("Iface\n"), None);
    }
}
