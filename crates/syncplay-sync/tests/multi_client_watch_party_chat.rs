//! Comprehensive integration test suite for multi-client watch-party networking and in-band chat.
//!
//! Verifies:
//! 1. Multi-client watch party with 1 host and 3 clients.
//! 2. Concurrent NTP clock synchronization and in-band chat messaging.
//! 3. Bidirectional chat delivery, host relaying (no echo to sender), and host broadcast.
//! 4. Dual-temporal timestamp anchoring ($t_{wall}, t_{media}$) on wire packets.

use std::thread;
use std::time::Duration;

use syncplay_sync::{ClientNode, HostServer, PacketPayload};

#[test]
fn test_multi_client_watch_party_concurrent_ntp_and_chat_relay() {
    let mut host = HostServer::bind("127.0.0.1:0").expect("host bind");
    let host_addr = host.local_addr().expect("host local_addr");

    // Spawn 3 client nodes
    let mut client1 = ClientNode::bind("127.0.0.1:0", host_addr, 1).expect("client1 bind");
    let mut client2 = ClientNode::bind("127.0.0.1:0", host_addr, 2).expect("client2 bind");
    let mut client3 = ClientNode::bind("127.0.0.1:0", host_addr, 3).expect("client3 bind");

    let c1_addr = client1.local_addr().expect("client1 local_addr");
    let c2_addr = client2.local_addr().expect("client2 local_addr");
    let c3_addr = client3.local_addr().expect("client3 local_addr");

    assert_ne!(c1_addr, c2_addr);
    assert_ne!(c2_addr, c3_addr);

    // Host initial state: playing at 1.0x at PTS 10,000ms
    host.set_media_state(10_000, 1.0, false);

    // 1. Concurrent NTP probes from all 3 clients
    for _round in 0..4 {
        let _ = client1.send_ntp_probe().expect("client1 probe");
        let _ = client2.send_ntp_probe().expect("client2 probe");
        let _ = client3.send_ntp_probe().expect("client3 probe");

        for _ in 0..10 {
            let _ = host.poll_and_process();
            let _ = client1.poll_and_process();
            let _ = client2.poll_and_process();
            let _ = client3.poll_and_process();
            thread::sleep(Duration::from_millis(1));
        }
    }

    assert_eq!(host.clients().len(), 3, "All 3 clients must be registered on host");
    assert!(client1.filtered_offset_nanos().is_some(), "Client 1 must have offset");
    assert!(client2.filtered_offset_nanos().is_some(), "Client 2 must have offset");
    assert!(client3.filtered_offset_nanos().is_some(), "Client 3 must have offset");

    // 2. Client 1 sends a chat message
    let payload_c1 = b"Hello watch party from Client 1!".to_vec();
    client1.set_local_pts_ms(10_500);
    let seq1 = client1.send_chat_message(&payload_c1).expect("client1 send");
    assert_eq!(seq1, 1);

    // Poll host to receive and relay
    let mut host_received = false;
    for _ in 0..30 {
        if let Ok(Some((pkt, _))) = host.poll_and_process() {
            if matches!(pkt.payload, PacketPayload::ChatMessage(_)) {
                host_received = true;
                break;
            }
        }
        thread::sleep(Duration::from_millis(2));
    }
    assert!(host_received, "Host must receive chat message from Client 1");

    // Host drains message
    let host_msgs = host.poll_chat_messages();
    assert_eq!(host_msgs.len(), 1);
    assert_eq!(host_msgs[0].payload, payload_c1);
    assert_eq!(host_msgs[0].media_timestamp_ms, 10_500);

    // Clients 2 and 3 must receive the relayed message
    let mut c2_received = false;
    let mut c3_received = false;
    for _ in 0..30 {
        if let Ok(Some(pkt)) = client2.poll_and_process() {
            if matches!(pkt.payload, PacketPayload::ChatMessage(_)) {
                c2_received = true;
            }
        }
        if let Ok(Some(pkt)) = client3.poll_and_process() {
            if matches!(pkt.payload, PacketPayload::ChatMessage(_)) {
                c3_received = true;
            }
        }
        if c2_received && c3_received {
            break;
        }
        thread::sleep(Duration::from_millis(2));
    }
    assert!(c2_received, "Client 2 must receive relayed message from Client 1");
    assert!(c3_received, "Client 3 must receive relayed message from Client 1");

    // Client 1 must NOT receive its own echo
    let _ = client1.poll_and_process();
    assert!(client1.poll_chat_messages().is_empty(), "Sender Client 1 must not receive echo");

    let c2_msgs = client2.poll_chat_messages();
    let c3_msgs = client3.poll_chat_messages();
    assert_eq!(c2_msgs.len(), 1);
    assert_eq!(c3_msgs.len(), 1);
    assert_eq!(c2_msgs[0].payload, payload_c1);
    assert_eq!(c3_msgs[0].payload, payload_c1);

    // 3. Client 2 replies
    let payload_c2 = b"Client 2 replying: sync is great!".to_vec();
    client2.set_local_pts_ms(11_200);
    let _ = client2.send_chat_message(&payload_c2).expect("client2 send");

    // Host processes and relays
    for _ in 0..30 {
        let _ = host.poll_and_process();
        thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(host.poll_chat_messages().len(), 1);

    // Client 1 and Client 3 receive Client 2's message; Client 2 gets no echo
    for _ in 0..30 {
        let _ = client1.poll_and_process();
        let _ = client2.poll_and_process();
        let _ = client3.poll_and_process();
        thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(client1.poll_chat_messages().len(), 1, "Client 1 must receive Client 2's message");
    assert_eq!(client3.poll_chat_messages().len(), 1, "Client 3 must receive Client 2's message");
    assert!(client2.poll_chat_messages().is_empty(), "Client 2 must not receive echo");

    // 4. Host broadcasts announcement to all 3 clients
    let host_announcement = b"Host: Unpause scheduled at PTS 15,000ms!".to_vec();
    let sent_count = host.broadcast_chat_message(&host_announcement).expect("host broadcast");
    assert_eq!(sent_count, 3, "Must send to all 3 clients");

    for _ in 0..30 {
        let _ = client1.poll_and_process();
        let _ = client2.poll_and_process();
        let _ = client3.poll_and_process();
        thread::sleep(Duration::from_millis(2));
    }
    let c1_msgs = client1.poll_chat_messages();
    let c2_msgs = client2.poll_chat_messages();
    let c3_msgs = client3.poll_chat_messages();
    assert_eq!(c1_msgs.len(), 1);
    assert_eq!(c2_msgs.len(), 1);
    assert_eq!(c3_msgs.len(), 1);
    assert_eq!(c1_msgs[0].payload, host_announcement);
    assert_eq!(c2_msgs[0].payload, host_announcement);
    assert_eq!(c3_msgs[0].payload, host_announcement);
}
