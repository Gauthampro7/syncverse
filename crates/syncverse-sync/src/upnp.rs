//! UPnP / NAT-PMP Internet Gateway Device (IGD) automatic port-forwarding for Watch-Party 2.0.
//!
//! Provides zero-configuration port forwarding over consumer router WAN links using pure Rust,
//! standard SSDP M-SEARCH discovery, and SOAP IGD AddPortMapping / DeletePortMapping protocol,
//! eliminating manual router configuration for watch-party hosts.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream, UdpSocket};
use std::time::Duration;

/// Default SSDP multicast discovery address for UPnP (`239.255.255.250:1900`).
pub const SSDP_MULTICAST_ADDR: &str = "239.255.255.250:1900";

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

/// Status of UPnP port forwarding for a watch-party session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpnpStatus {
    /// Port was successfully forwarded via UPnP IGD.
    Active {
        /// Public WAN IP address of the router, if discoverable.
        external_ip: Option<String>,
        /// Forwarded UDP port.
        port: u16,
    },
    /// UPnP is disabled on the router, discovery timed out, or mapping failed.
    Disabled {
        /// Diagnostic reason for fallback to direct UDP.
        reason: String,
    },
}

/// Active UPnP port mapping guard that unmaps the port when dropped.
pub struct UpnpMappingGuard {
    service: IgdService,
    port: u16,
    external_ip: Option<String>,
}

impl UpnpMappingGuard {
    /// Returns the public WAN IP discovered from the router, if available.
    pub fn external_ip(&self) -> Option<&str> {
        self.external_ip.as_deref()
    }

    /// Returns the mapped external port.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Attempts to discover an Internet Gateway Device and map the specified UDP port.
    ///
    /// Uses a 1.5-second discovery budget. If the local network does not support UPnP,
    /// returns `UpnpStatus::Disabled` without delaying application startup.
    pub fn map_port(port: u16) -> (UpnpStatus, Option<Self>) {
        let service = match discover_igd(Duration::from_millis(1500)) {
            Ok(svc) => svc,
            Err(err) => {
                return (
                    UpnpStatus::Disabled {
                        reason: err,
                    },
                    None,
                );
            }
        };

        // Determine local IPv4 address
        let local_ip = match get_local_ip(&service.host) {
            Some(ip) => ip,
            None => {
                return (
                    UpnpStatus::Disabled {
                        reason: "Failed to determine local network interface IP".to_string(),
                    },
                    None,
                );
            }
        };

        // Issue AddPortMapping SOAP request
        if let Err(e) = add_port_mapping(&service, port, &local_ip) {
            return (
                UpnpStatus::Disabled {
                    reason: format!("Router rejected AddPortMapping: {}", e),
                },
                None,
            );
        }

        // Query public external WAN IP
        let external_ip = get_external_ip(&service).ok();

        let guard = Self {
            service,
            port,
            external_ip: external_ip.clone(),
        };

        (
            UpnpStatus::Active {
                external_ip,
                port,
            },
            Some(guard),
        )
    }

    /// Explicitly unmaps the forwarded port from the router.
    pub fn release(self) {
        // Dropping executes DeletePortMapping
        drop(self);
    }
}

impl Drop for UpnpMappingGuard {
    fn drop(&mut self) {
        let _ = delete_port_mapping(&self.service, self.port);
    }
}

/// Discovers an Internet Gateway Device via SSDP multicast.
fn discover_igd(timeout: Duration) -> Result<IgdService, String> {
    let socket = UdpSocket::bind("0.0.0.0:0")
        .map_err(|e| format!("Failed to bind local SSDP socket: {}", e))?;
    socket
        .set_read_timeout(Some(timeout))
        .map_err(|e| format!("Failed to set SSDP timeout: {}", e))?;

    let search_msg = format!(
        "M-SEARCH * HTTP/1.1\r\n\
         HOST: {}\r\n\
         MAN: \"ssdp:discover\"\r\n\
         MX: 1\r\n\
         ST: urn:schemas-upnp-org:device:InternetGatewayDevice:1\r\n\
         \r\n",
        SSDP_MULTICAST_ADDR
    );

    let _ = socket.send_to(search_msg.as_bytes(), SSDP_MULTICAST_ADDR);

    let mut buf = [0u8; 4096];
    let (n, _from_addr) = socket
        .recv_from(&mut buf)
        .map_err(|_| "UPnP IGD SSDP discovery timed out (no IGD response)".to_string())?;

    let response = String::from_utf8_lossy(&buf[..n]);
    let location = extract_header_value(&response, "LOCATION")
        .ok_or_else(|| "SSDP response missing LOCATION header".to_string())?;

    // Parse location URL to extract host and XML descriptor path
    fetch_and_parse_root_desc(&location)
}

/// Fetches the router's root XML description and extracts WANIPConnection control URL.
fn fetch_and_parse_root_desc(location_url: &str) -> Result<IgdService, String> {
    let trimmed = location_url.trim();
    let url_without_proto = trimmed
        .strip_prefix("http://")
        .ok_or_else(|| "Only HTTP UPnP descriptors are supported".to_string())?;

    let slash_idx = url_without_proto.find('/').unwrap_or(url_without_proto.len());
    let host = &url_without_proto[..slash_idx];
    let path = if slash_idx < url_without_proto.len() {
        &url_without_proto[slash_idx..]
    } else {
        "/"
    };

    let body = http_get(host, path, Duration::from_millis(1500))?;

    // Look for WANIPConnection or WANPPPConnection service
    let (service_type, control_url) = parse_service_descriptor(&body)?;

    // Ensure control_url is absolute or prepend path
    let normalized_control_url = if control_url.starts_with('/') {
        control_url
    } else {
        format!("/{}", control_url)
    };

    Ok(IgdService {
        host: host.to_string(),
        control_url: normalized_control_url,
        service_type,
    })
}

/// Parses the service type and control URL from XML descriptor content.
fn parse_service_descriptor(xml: &str) -> Result<(String, String), String> {
    let services = [
        "urn:schemas-upnp-org:service:WANIPConnection:1",
        "urn:schemas-upnp-org:service:WANIPConnection:2",
        "urn:schemas-upnp-org:service:WANPPPConnection:1",
    ];

    for svc in &services {
        if let Some(pos) = xml.find(svc) {
            let after_svc = &xml[pos..];
            if let Some(ctrl_pos) = after_svc.find("<controlURL>") {
                let after_ctrl = &after_svc[ctrl_pos + "<controlURL>".len()..];
                if let Some(end_ctrl) = after_ctrl.find("</controlURL>") {
                    let ctrl_url = after_ctrl[..end_ctrl].trim().to_string();
                    return Ok((svc.to_string(), ctrl_url));
                }
            }
        }
    }

    Err("No compatible WANIPConnection or WANPPPConnection service found in IGD XML".to_string())
}

/// Sends a SOAP AddPortMapping request to the router's control URL.
fn add_port_mapping(service: &IgdService, port: u16, local_ip: &str) -> Result<(), String> {
    let soap_body = format!(
        "<?xml version=\"1.0\"?>\r\n\
         <s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\" s:encodingStyle=\"http://schemas.xmlsoap.org/soap/encoding/\">\r\n\
           <s:Body>\r\n\
             <u:AddPortMapping xmlns:u=\"{}\">\r\n\
               <NewRemoteHost></NewRemoteHost>\r\n\
               <NewExternalPort>{}</NewExternalPort>\r\n\
               <NewProtocol>UDP</NewProtocol>\r\n\
               <NewInternalPort>{}</NewInternalPort>\r\n\
               <NewInternalClient>{}</NewInternalClient>\r\n\
               <NewEnabled>1</NewEnabled>\r\n\
               <NewPortMappingDescription>Syncverse Watch Party</NewPortMappingDescription>\r\n\
               <NewLeaseDuration>0</NewLeaseDuration>\r\n\
             </u:AddPortMapping>\r\n\
           </s:Body>\r\n\
         </s:Envelope>",
        service.service_type, port, port, local_ip
    );

    let soap_action = format!("\"{}#AddPortMapping\"", service.service_type);
    let resp = http_post(&service.host, &service.control_url, &soap_action, &soap_body, Duration::from_millis(2000))?;

    if resp.contains("<u:AddPortMappingResponse") || resp.contains("200 OK") {
        Ok(())
    } else {
        Err(format!("AddPortMapping rejected by router: {}", resp))
    }
}

/// Sends a SOAP DeletePortMapping request to unmap the port.
fn delete_port_mapping(service: &IgdService, port: u16) -> Result<(), String> {
    let soap_body = format!(
        "<?xml version=\"1.0\"?>\r\n\
         <s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\" s:encodingStyle=\"http://schemas.xmlsoap.org/soap/encoding/\">\r\n\
           <s:Body>\r\n\
             <u:DeletePortMapping xmlns:u=\"{}\">\r\n\
               <NewRemoteHost></NewRemoteHost>\r\n\
               <NewExternalPort>{}</NewExternalPort>\r\n\
               <NewProtocol>UDP</NewProtocol>\r\n\
             </u:DeletePortMapping>\r\n\
           </s:Body>\r\n\
         </s:Envelope>",
        service.service_type, port
    );

    let soap_action = format!("\"{}#DeletePortMapping\"", service.service_type);
    let _ = http_post(&service.host, &service.control_url, &soap_action, &soap_body, Duration::from_millis(1500));
    Ok(())
}

/// Queries the router's external public WAN IP address.
fn get_external_ip(service: &IgdService) -> Result<String, String> {
    let soap_body = format!(
        "<?xml version=\"1.0\"?>\r\n\
         <s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\" s:encodingStyle=\"http://schemas.xmlsoap.org/soap/encoding/\">\r\n\
           <s:Body>\r\n\
             <u:GetExternalIPAddress xmlns:u=\"{}\"/>\r\n\
           </s:Body>\r\n\
         </s:Envelope>",
        service.service_type
    );

    let soap_action = format!("\"{}#GetExternalIPAddress\"", service.service_type);
    let resp = http_post(&service.host, &service.control_url, &soap_action, &soap_body, Duration::from_millis(1500))?;

    if let Some(start) = resp.find("<NewExternalIPAddress>") {
        let after = &resp[start + "<NewExternalIPAddress>".len()..];
        if let Some(end) = after.find("</NewExternalIPAddress>") {
            let ip = after[..end].trim().to_string();
            if !ip.is_empty() && ip != "0.0.0.0" {
                return Ok(ip);
            }
        }
    }

    Err("Could not parse valid external IP from response".to_string())
}

/// Simple HTTP GET client over TCP stream.
fn http_get(host: &str, path: &str, timeout: Duration) -> Result<String, String> {
    let mut stream = TcpStream::connect(host)
        .map_err(|e| format!("Failed to connect to IGD {}: {}", host, e))?;
    stream.set_read_timeout(Some(timeout)).map_err(|e| e.to_string())?;
    stream.set_write_timeout(Some(timeout)).map_err(|e| e.to_string())?;

    let req = format!(
        "GET {} HTTP/1.1\r\n\
         Host: {}\r\n\
         User-Agent: Syncverse/0.1.9\r\n\
         Connection: close\r\n\
         \r\n",
        path, host
    );
    stream.write_all(req.as_bytes()).map_err(|e| e.to_string())?;

    let mut resp = String::new();
    stream.read_to_string(&mut resp).map_err(|e| e.to_string())?;
    Ok(resp)
}

/// Simple HTTP POST client for SOAP XML payloads.
fn http_post(host: &str, path: &str, soap_action: &str, body: &str, timeout: Duration) -> Result<String, String> {
    let mut stream = TcpStream::connect(host)
        .map_err(|e| format!("Failed to connect to IGD {}: {}", host, e))?;
    stream.set_read_timeout(Some(timeout)).map_err(|e| e.to_string())?;
    stream.set_write_timeout(Some(timeout)).map_err(|e| e.to_string())?;

    let req = format!(
        "POST {} HTTP/1.1\r\n\
         Host: {}\r\n\
         User-Agent: Syncverse/0.1.9\r\n\
         Content-Type: text/xml; charset=\"utf-8\"\r\n\
         Content-Length: {}\r\n\
         SOAPAction: {}\r\n\
         Connection: close\r\n\
         \r\n\
         {}",
        path,
        host,
        body.len(),
        soap_action,
        body
    );
    stream.write_all(req.as_bytes()).map_err(|e| e.to_string())?;

    let mut resp = String::new();
    stream.read_to_string(&mut resp).map_err(|e| e.to_string())?;
    Ok(resp)
}

/// Discovers the local machine's IP on the network interface facing the router.
fn get_local_ip(target_host: &str) -> Option<String> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect(target_host).ok()?;
    let local_addr = socket.local_addr().ok()?;
    match local_addr {
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
    }
}
