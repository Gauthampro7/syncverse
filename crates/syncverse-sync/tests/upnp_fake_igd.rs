//! Tests of UPnP IGD and NAT-PMP port mapping against fake gateways on loopback.
//!
//! The fake IGD answers SSDP and SOAP like a simple router; the fake NAT-PMP gateway speaks
//! RFC 6886 on a high loopback port. Real router behaviour is not covered.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use syncverse_sync::upnp::{
    NatPmpClient, NatPmpError, NatPmpResultCode, PortMapConfig, PortMapError, PortMapProtocol,
    UpnpError, MAPPING_DESCRIPTION,
};
use syncverse_sync::{UpnpMappingGuard, UpnpStatus};

// ------------------------------------------------------------------------------------------
// Fake IGD
// ------------------------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct Mapping {
    client: String,
    description: String,
    lease: u32,
}

#[derive(Default)]
struct IgdState {
    mappings: HashMap<u16, Mapping>,
    adds: usize,
    deletes: usize,
    gets: usize,
    /// Error code to return for every AddPortMapping.
    fail_add: Option<u32>,
    /// Reject non-zero leases with 725.
    only_permanent: bool,
}

struct FakeIgd {
    ssdp_addr: SocketAddr,
    state: Arc<Mutex<IgdState>>,
    stop: Arc<AtomicBool>,
    threads: Vec<JoinHandle<()>>,
}

fn tag(body: &str, name: &str) -> Option<String> {
    let open = format!("<{name}>");
    let start = body.find(&open)? + open.len();
    let end = body[start..].find(&format!("</{name}>"))?;
    Some(body[start..start + end].to_string())
}

fn soap_fault(code: u32) -> (u16, String) {
    (
        500,
        format!(
            "<s:Envelope><s:Body><s:Fault><detail><UPnPError><errorCode>{code}</errorCode>\
             <errorDescription>fake {code}</errorDescription></UPnPError></detail></s:Fault>\
             </s:Body></s:Envelope>"
        ),
    )
}

fn handle_http(stream: &mut TcpStream, state: &Mutex<IgdState>) {
    stream
        .set_read_timeout(Some(Duration::from_millis(500)))
        .unwrap();
    let mut raw = Vec::new();
    let mut buf = [0u8; 2048];
    let header_end = loop {
        let Ok(n) = stream.read(&mut buf) else { return };
        if n == 0 {
            return;
        }
        raw.extend_from_slice(&buf[..n]);
        if let Some(p) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
            break p + 4;
        }
    };
    let head = String::from_utf8_lossy(&raw[..header_end]).to_string();
    let content_len: usize = head
        .lines()
        .find_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.eq_ignore_ascii_case("content-length")
                .then(|| v.trim().parse().ok())?
        })
        .unwrap_or(0);
    while raw.len() < header_end + content_len {
        let Ok(n) = stream.read(&mut buf) else { return };
        if n == 0 {
            break;
        }
        raw.extend_from_slice(&buf[..n]);
    }
    let body = String::from_utf8_lossy(&raw[header_end..]).to_string();
    let request_line = head.lines().next().unwrap_or("");

    let (status, resp_body) = if request_line.starts_with("GET /root.xml") {
        (
            200,
            "<root><device><serviceList><service>\
             <serviceType>urn:schemas-upnp-org:service:WANIPConnection:1</serviceType>\
             <controlURL>/ctl</controlURL></service></serviceList></device></root>"
                .to_string(),
        )
    } else if request_line.starts_with("POST /ctl") {
        let action = head
            .lines()
            .find(|l| l.to_ascii_lowercase().starts_with("soapaction"))
            .and_then(|l| l.split('#').nth(1))
            .map(|a| a.trim().trim_matches('"').to_string())
            .unwrap_or_default();
        let mut st = state.lock().unwrap();
        let port = tag(&body, "NewExternalPort").and_then(|p| p.parse::<u16>().ok());
        match action.as_str() {
            "AddPortMapping" => {
                st.adds += 1;
                let lease: u32 = tag(&body, "NewLeaseDuration")
                    .and_then(|l| l.parse().ok())
                    .unwrap_or(0);
                if let Some(code) = st.fail_add {
                    soap_fault(code)
                } else if st.only_permanent && lease != 0 {
                    soap_fault(725)
                } else {
                    st.mappings.insert(
                        port.unwrap(),
                        Mapping {
                            client: tag(&body, "NewInternalClient").unwrap_or_default(),
                            description: tag(&body, "NewPortMappingDescription")
                                .unwrap_or_default(),
                            lease,
                        },
                    );
                    (200, "<u:AddPortMappingResponse/>".to_string())
                }
            }
            "DeletePortMapping" => {
                st.deletes += 1;
                match st.mappings.remove(&port.unwrap()) {
                    Some(_) => (200, "<u:DeletePortMappingResponse/>".to_string()),
                    None => soap_fault(714),
                }
            }
            "GetSpecificPortMappingEntry" => {
                st.gets += 1;
                match st.mappings.get(&port.unwrap()) {
                    Some(m) => (
                        200,
                        format!(
                            "<u:GetSpecificPortMappingEntryResponse><NewInternalPort>{}\
                             </NewInternalPort><NewInternalClient>{}</NewInternalClient>\
                             <NewEnabled>1</NewEnabled><NewPortMappingDescription>{}\
                             </NewPortMappingDescription><NewLeaseDuration>{}</NewLeaseDuration>\
                             </u:GetSpecificPortMappingEntryResponse>",
                            port.unwrap(),
                            m.client,
                            m.description,
                            m.lease
                        ),
                    ),
                    None => soap_fault(714),
                }
            }
            "GetExternalIPAddress" => (
                200,
                "<u:GetExternalIPAddressResponse><NewExternalIPAddress>203.0.113.5\
                 </NewExternalIPAddress></u:GetExternalIPAddressResponse>"
                    .to_string(),
            ),
            _ => soap_fault(401),
        }
    } else {
        (404, String::new())
    };
    let reply = format!(
        "HTTP/1.1 {status} X\r\nContent-Type: text/xml\r\nContent-Length: {}\r\n\
         Connection: close\r\n\r\n{resp_body}",
        resp_body.len()
    );
    let _ = stream.write_all(reply.as_bytes());
}

impl FakeIgd {
    fn start() -> Self {
        let state = Arc::new(Mutex::new(IgdState::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let http_addr = listener.local_addr().unwrap();
        let ssdp = UdpSocket::bind("127.0.0.1:0").unwrap();
        ssdp.set_read_timeout(Some(Duration::from_millis(10)))
            .unwrap();
        let ssdp_addr = ssdp.local_addr().unwrap();

        let (st, sp) = (state.clone(), stop.clone());
        let http = thread::spawn(move || {
            while !sp.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut s, _)) => {
                        s.set_nonblocking(false).unwrap();
                        handle_http(&mut s, &st);
                    }
                    Err(_) => thread::sleep(Duration::from_millis(3)),
                }
            }
        });
        let sp = stop.clone();
        let ssdp_thread = thread::spawn(move || {
            let mut buf = [0u8; 1024];
            while !sp.load(Ordering::Relaxed) {
                if let Ok((_, from)) = ssdp.recv_from(&mut buf) {
                    let reply = format!(
                        "HTTP/1.1 200 OK\r\nST: urn:schemas-upnp-org:device:InternetGatewayDevice:1\r\n\
                         LOCATION: http://{http_addr}/root.xml\r\n\r\n"
                    );
                    let _ = ssdp.send_to(reply.as_bytes(), from);
                }
            }
        });
        Self {
            ssdp_addr,
            state,
            stop,
            threads: vec![http, ssdp_thread],
        }
    }

    fn cfg(&self) -> PortMapConfig {
        PortMapConfig {
            ssdp_addr: self.ssdp_addr.to_string(),
            discovery_timeout: Duration::from_millis(800),
            try_natpmp: false,
            ..PortMapConfig::default()
        }
    }

    fn mapping(&self, port: u16) -> Option<Mapping> {
        self.state.lock().unwrap().mappings.get(&port).cloned()
    }
}

impl Drop for FakeIgd {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        for t in self.threads.drain(..) {
            let _ = t.join();
        }
    }
}

fn wait_until(what: &str, limit: Duration, mut f: impl FnMut() -> bool) {
    let end = Instant::now() + limit;
    while Instant::now() < end {
        if f() {
            return;
        }
        thread::sleep(Duration::from_millis(10));
    }
    panic!("timed out waiting for {what}");
}

#[test]
fn igd_add_verify_renew_and_delete_on_drop() {
    let igd = FakeIgd::start();
    let mut cfg = igd.cfg();
    cfg.renew_after = Duration::from_millis(150); // shortened from 1800 s
    let guard = UpnpMappingGuard::try_map_port(7001, &cfg).unwrap();

    let m = igd.mapping(7001).expect("mapping added");
    assert_eq!(m.lease, 3600, "default lease is 3600 s");
    assert_eq!(m.description, MAPPING_DESCRIPTION);
    assert_eq!(m.client, "127.0.0.1");
    assert!(guard.verified());
    assert_eq!(guard.protocol(), PortMapProtocol::Upnp);
    assert_eq!(guard.external_ip(), Some("203.0.113.5"));
    assert_eq!(guard.port(), 7001);
    assert!(guard
        .status_text()
        .starts_with("UPnP active: 203.0.113.5:7001"));
    assert!(igd.state.lock().unwrap().gets >= 1, "verified through Get");

    wait_until("two renewals", Duration::from_secs(3), || {
        guard.renewals() >= 2
    });
    assert!(igd.state.lock().unwrap().adds >= 3);
    assert!(guard.last_error().is_none());

    drop(guard);
    assert!(igd.mapping(7001).is_none(), "deleted on drop");
    assert_eq!(igd.state.lock().unwrap().deletes, 1);
}

#[test]
fn default_timings_match_spec() {
    let cfg = PortMapConfig::default();
    assert_eq!(cfg.lease_secs, 3600);
    assert_eq!(cfg.renew_after, Duration::from_secs(1800));
}

#[test]
fn renewal_failure_is_reported() {
    let igd = FakeIgd::start();
    let mut cfg = igd.cfg();
    cfg.renew_after = Duration::from_millis(100);
    let guard = UpnpMappingGuard::try_map_port(7002, &cfg).unwrap();
    igd.state.lock().unwrap().fail_add = Some(501);
    wait_until("renewal error", Duration::from_secs(2), || {
        guard.last_error().is_some()
    });
    assert!(matches!(
        guard.last_error(),
        Some(PortMapError::UpnpRenewal(UpnpError::Soap { code: 501, .. }))
    ));
    assert!(guard.status_text().contains("last renewal failed"));
}

#[test]
fn stale_own_mapping_is_cleaned_foreign_mapping_is_left_alone() {
    let igd = FakeIgd::start();
    igd.state.lock().unwrap().mappings.insert(
        7003,
        Mapping {
            client: "192.168.1.99".into(),
            description: MAPPING_DESCRIPTION.into(),
            lease: 0,
        },
    );
    let guard = UpnpMappingGuard::try_map_port(7003, &igd.cfg()).unwrap();
    assert_eq!(
        igd.state.lock().unwrap().deletes,
        1,
        "stale entry removed first"
    );
    assert_eq!(igd.mapping(7003).unwrap().client, "127.0.0.1");
    drop(guard);

    igd.state.lock().unwrap().mappings.insert(
        7004,
        Mapping {
            client: "192.168.1.50".into(),
            description: "someone else's game".into(),
            lease: 0,
        },
    );
    let err = UpnpMappingGuard::try_map_port(7004, &igd.cfg())
        .err()
        .unwrap();
    assert_eq!(err, PortMapError::Upnp(UpnpError::PortUsedByOther));
    assert_eq!(
        igd.mapping(7004).unwrap().description,
        "someone else's game"
    );
}

#[test]
fn router_errors_become_typed_errors() {
    let igd = FakeIgd::start();
    for (code, expected) in [
        (718, UpnpError::ConflictInMappingEntry),
        (
            402,
            UpnpError::Soap {
                code: 402,
                description: "fake 402".into(),
            },
        ),
        (
            725,
            UpnpError::OnlyPermanentLeases, // permanent retry also fails with 725 here
        ),
    ] {
        igd.state.lock().unwrap().fail_add = Some(code);
        let err = UpnpMappingGuard::try_map_port(7005, &igd.cfg())
            .err()
            .unwrap();
        assert_eq!(err, PortMapError::Upnp(expected), "code {code}");
    }
    // The status API carries a readable reason instead of a String error type.
    igd.state.lock().unwrap().fail_add = Some(718);
    let (status, guard) = UpnpMappingGuard::map_port_with(7005, &igd.cfg());
    assert!(guard.is_none());
    match status {
        UpnpStatus::Disabled { reason } => assert!(reason.contains("718"), "{reason}"),
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn permanent_lease_fallback_when_router_refuses_leases() {
    let igd = FakeIgd::start();
    igd.state.lock().unwrap().only_permanent = true;
    let guard = UpnpMappingGuard::try_map_port(7006, &igd.cfg()).unwrap();
    assert_eq!(guard.lease_secs(), 0);
    assert_eq!(igd.mapping(7006).unwrap().lease, 0);
    thread::sleep(Duration::from_millis(100));
    assert_eq!(guard.renewals(), 0, "permanent mappings are not renewed");
    drop(guard);
    assert!(igd.mapping(7006).is_none());
}

#[test]
fn no_router_means_discovery_timeout() {
    let silent = UdpSocket::bind("127.0.0.1:0").unwrap();
    let cfg = PortMapConfig {
        ssdp_addr: silent.local_addr().unwrap().to_string(),
        discovery_timeout: Duration::from_millis(200),
        try_natpmp: false,
        ..PortMapConfig::default()
    };
    let err = UpnpMappingGuard::try_map_port(7007, &cfg).err().unwrap();
    assert_eq!(err, PortMapError::Upnp(UpnpError::DiscoveryTimeout));
}

// ------------------------------------------------------------------------------------------
// Fake NAT-PMP gateway
// ------------------------------------------------------------------------------------------

#[derive(Default)]
struct PmpState {
    /// internal port -> (external port, expiry)
    mappings: HashMap<u16, (u16, Instant)>,
    requests: usize,
    map_requests: usize,
    result_code: u16,
}

struct FakePmp {
    addr: SocketAddr,
    state: Arc<Mutex<PmpState>>,
    drop_first: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl FakePmp {
    fn start(drop_first: usize) -> Self {
        let sock = UdpSocket::bind("127.0.0.1:0").unwrap();
        sock.set_read_timeout(Some(Duration::from_millis(10)))
            .unwrap();
        let addr = sock.local_addr().unwrap();
        let state = Arc::new(Mutex::new(PmpState::default()));
        let drop_first = Arc::new(AtomicUsize::new(drop_first));
        let stop = Arc::new(AtomicBool::new(false));
        let (st, df, sp) = (state.clone(), drop_first.clone(), stop.clone());
        let handle = thread::spawn(move || {
            let mut buf = [0u8; 64];
            while !sp.load(Ordering::Relaxed) {
                let Ok((n, from)) = sock.recv_from(&mut buf) else {
                    continue;
                };
                let mut s = st.lock().unwrap();
                s.requests += 1;
                if df.load(Ordering::SeqCst) > 0 {
                    df.fetch_sub(1, Ordering::SeqCst);
                    continue;
                }
                if n < 2 || buf[0] != 0 {
                    continue;
                }
                let op = buf[1];
                let mut reply = vec![0u8, 128 + op];
                reply.extend_from_slice(&s.result_code.to_be_bytes());
                reply.extend_from_slice(&1234u32.to_be_bytes());
                match op {
                    0 => reply.extend_from_slice(&[203, 0, 113, 9]),
                    1 if n == 12 => {
                        s.map_requests += 1;
                        let internal = u16::from_be_bytes([buf[4], buf[5]]);
                        let suggested = u16::from_be_bytes([buf[6], buf[7]]);
                        let lifetime = u32::from_be_bytes([buf[8], buf[9], buf[10], buf[11]]);
                        let mut external = if suggested == 0 {
                            internal + 1000
                        } else {
                            suggested
                        };
                        if s.result_code == 0 {
                            if lifetime == 0 {
                                s.mappings.remove(&internal);
                                external = 0;
                            } else {
                                s.mappings.insert(
                                    internal,
                                    (
                                        external,
                                        Instant::now() + Duration::from_secs(u64::from(lifetime)),
                                    ),
                                );
                            }
                        }
                        reply.extend_from_slice(&internal.to_be_bytes());
                        reply.extend_from_slice(&external.to_be_bytes());
                        reply.extend_from_slice(&lifetime.to_be_bytes());
                    }
                    _ => continue,
                }
                let _ = sock.send_to(&reply, from);
            }
        });
        Self {
            addr,
            state,
            drop_first,
            stop,
            handle: Some(handle),
        }
    }

    fn is_mapped(&self, internal: u16) -> bool {
        self.state
            .lock()
            .unwrap()
            .mappings
            .get(&internal)
            .is_some_and(|(_, expiry)| *expiry > Instant::now())
    }

    fn client(&self) -> NatPmpClient {
        NatPmpClient::new(self.addr, Duration::from_millis(40), 4)
    }
}

impl Drop for FakePmp {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
        let _ = &self.drop_first;
    }
}

#[test]
fn natpmp_external_address_map_and_unmap() {
    let gw = FakePmp::start(0);
    let client = gw.client();
    assert_eq!(
        client.external_address().unwrap(),
        Ipv4Addr::new(203, 0, 113, 9)
    );
    let m = client.map_udp(7100, 7100, 3600).unwrap();
    assert_eq!(m.external_port, 7100);
    assert_eq!(m.lifetime_secs, 3600);
    assert_eq!(m.epoch_secs, 1234);
    assert!(gw.is_mapped(7100));
    client.unmap_udp(7100).unwrap();
    assert!(!gw.is_mapped(7100));
}

#[test]
fn natpmp_retries_and_reports_refusal() {
    let gw = FakePmp::start(2);
    let client = gw.client();
    assert!(
        client.map_udp(7101, 7101, 60).is_ok(),
        "survives 2 lost requests"
    );
    assert_eq!(gw.state.lock().unwrap().requests, 3);

    gw.state.lock().unwrap().result_code = 2;
    assert_eq!(
        client.map_udp(7102, 7102, 60),
        Err(NatPmpError::ResultCode(NatPmpResultCode::NotAuthorized))
    );

    let silent = UdpSocket::bind("127.0.0.1:0").unwrap();
    let dead = NatPmpClient::new(silent.local_addr().unwrap(), Duration::from_millis(20), 3);
    assert_eq!(dead.map_udp(7103, 7103, 60), Err(NatPmpError::Timeout));
}

#[test]
fn natpmp_mapping_expires_unless_renewed() {
    let gw = FakePmp::start(0);

    // Without renewal the mapping expires after its lifetime.
    gw.client().map_udp(7110, 7110, 1).unwrap();
    assert!(gw.is_mapped(7110));
    thread::sleep(Duration::from_millis(1200));
    assert!(!gw.is_mapped(7110), "expired");

    // Through the guard: fallback from a silent UPnP discovery, lease 1 s renewed every 300 ms.
    let silent = UdpSocket::bind("127.0.0.1:0").unwrap();
    let cfg = PortMapConfig {
        ssdp_addr: silent.local_addr().unwrap().to_string(),
        discovery_timeout: Duration::from_millis(150),
        lease_secs: 1,
        renew_after: Duration::from_millis(300),
        natpmp_gateway: Some(gw.addr),
        natpmp_initial_timeout: Duration::from_millis(40),
        ..PortMapConfig::default()
    };
    let guard = UpnpMappingGuard::try_map_port(7111, &cfg).unwrap();
    assert_eq!(guard.protocol(), PortMapProtocol::NatPmp);
    assert_eq!(guard.external_ip(), Some("203.0.113.9"));
    assert_eq!(guard.port(), 7111);
    assert_eq!(guard.lease_secs(), 1);
    thread::sleep(Duration::from_millis(1600));
    assert!(gw.is_mapped(7111), "kept alive by renewals");
    assert!(guard.renewals() >= 3, "renewals: {}", guard.renewals());
    drop(guard);
    assert!(!gw.is_mapped(7111), "removed on drop");
}

#[test]
fn both_methods_failing_reports_both_reasons() {
    let silent = UdpSocket::bind("127.0.0.1:0").unwrap();
    let refusing = FakePmp::start(0);
    refusing.state.lock().unwrap().result_code = 3;
    let cfg = PortMapConfig {
        ssdp_addr: silent.local_addr().unwrap().to_string(),
        discovery_timeout: Duration::from_millis(100),
        natpmp_gateway: Some(refusing.addr),
        natpmp_initial_timeout: Duration::from_millis(40),
        ..PortMapConfig::default()
    };
    let err = UpnpMappingGuard::try_map_port(7120, &cfg).err().unwrap();
    assert_eq!(
        err,
        PortMapError::Both {
            upnp: UpnpError::DiscoveryTimeout,
            natpmp: NatPmpError::ResultCode(NatPmpResultCode::NetworkFailure),
        }
    );
    let text = err.to_string();
    assert!(text.contains("UPnP") && text.contains("NAT-PMP"), "{text}");
}

#[cfg(not(target_os = "linux"))]
#[test]
fn natpmp_gateway_lookup_is_unsupported_off_linux() {
    assert_eq!(
        syncverse_sync::upnp::default_gateway(),
        Err(NatPmpError::Unsupported)
    );
}

#[cfg(target_os = "linux")]
#[test]
fn natpmp_gateway_lookup_reads_proc_net_route() {
    // The machine may have no default route (CI sandbox); both outcomes are valid answers.
    match syncverse_sync::upnp::default_gateway() {
        Ok(ip) => assert!(!ip.is_unspecified()),
        Err(e) => assert_eq!(e, NatPmpError::NoGateway),
    }
}
