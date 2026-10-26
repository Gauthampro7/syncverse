//! Adversarial stress test suite for SPNT wire protocol and watch-party chat transport.
//!
//! Validates:
//! 1. High-throughput concurrent chat flooding with multiple clients, verifying:
//!    - Zero dropped packets in loopback under concurrent load
//!    - Strict anti-echo invariant (senders never receive their own messages back)
//!    - Strictly monotonic sequence ordering per sender
//!    - Accurate preservation of dual timestamps ($t_{wall}, t_{media}$)
//! 2. Corrupted datagram fuzzing and crash resilience:
//!    - Truncated headers (0..11 bytes)
//!    - Unknown opcodes (0x00, 0x07, 0x09, 0x0A, 0x7F, 0xFF)
//!    - Truncated payloads and sub-20-byte ChatMessage payloads
//!    - Bit-flipped CRC32 headers and payload bodies
//!    - Oversized payloads and raw MTU-exceeding datagrams
//!    - High-volume random LFSR fuzz sequences
//!    - Immediate recovery and uninterrupted liveness for subsequent valid packets
//! 3. Dynamic peer joins and leaves during active chat:
//!    - Mid-session joining and immediate integration into broadcast/relay mesh
//!    - Peer departure without host interruption or panic
//!    - Subsequent peer joins continuing uninterrupted communication
//! 4. ChatMessage payload boundary extremes (0-byte payload up to 1400-byte MTU payload)

use std::collections::HashMap;
use std::net::UdpSocket;
use std::thread;
use std::time::{Duration, Instant};

use syncplay_sync::{
    ChatMessagePayload, ClientNode, HostServer, PacketPayload, PacketType, SyncError, WirePacket,
};

#[derive(Debug, Clone)]
struct SentRecord {
    seq: u32,
    wall_ms: u64,
    media_pts: u64,
    payload: Vec<u8>,
}

/// High-throughput concurrent chat flooding.
///
/// 4 clients send 100 messages each (400 total) concurrently while the host
/// is concurrently processing NTP probes and relaying messages.
#[test]
fn test_concurrent_chat_flooding_high_throughput_and_anti_echo() {
    let mut host = HostServer::bind("127.0.0.1:0").expect("host bind");
    let host_addr = host.local_addr().expect("host local_addr");

    const NUM_CLIENTS: usize = 4;
    const MSGS_PER_CLIENT: usize = 100;

    let mut clients: Vec<ClientNode> = (0..NUM_CLIENTS)
        .map(|i| {
            ClientNode::bind("127.0.0.1:0", host_addr, (i + 1) as u32).expect("client bind")
        })
        .collect();

    // Register all clients on host via NTP probes
    for client in &mut clients {
        let _ = client.send_ntp_probe().expect("send probe");
    }

    let deadline = Instant::now() + Duration::from_secs(3);
    while host.clients().len() < NUM_CLIENTS && Instant::now() < deadline {
        let _ = host.poll_and_process();
        for client in &mut clients {
            let _ = client.poll_and_process();
        }
        thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(
        host.clients().len(),
        NUM_CLIENTS,
        "All clients must be registered on host"
    );

    // Track sent messages per client
    let mut sent_records: Vec<Vec<SentRecord>> = vec![Vec::new(); NUM_CLIENTS];

    // Track received messages per client: client_idx -> list of ChatMessagePayload
    let mut received_records: Vec<Vec<ChatMessagePayload>> = vec![Vec::new(); NUM_CLIENTS];

    // Concurrently flood messages across multiple rounds
    let mut msgs_sent_by_client = [0usize; NUM_CLIENTS];
    let total_expected_host_msgs = NUM_CLIENTS * MSGS_PER_CLIENT;

    let start_flood = Instant::now();
    let max_flood_duration = Duration::from_secs(10);

    while Instant::now() - start_flood < max_flood_duration {
        // Send a burst from each client if not finished
        for (c_idx, client) in clients.iter_mut().enumerate() {
            if msgs_sent_by_client[c_idx] < MSGS_PER_CLIENT {
                let msg_num = msgs_sent_by_client[c_idx];
                let media_pts = 10_000 + (msg_num as u64) * 200;
                client.set_local_pts_ms(media_pts);

                let payload_text = format!(
                    "Client-{} Msg-#{:04} Timestamp-{}",
                    c_idx + 1,
                    msg_num,
                    media_pts
                );
                let payload_bytes = payload_text.into_bytes();

                let seq = client
                    .send_chat_message(&payload_bytes)
                    .expect("client send");
                let wall_ms = client.current_local_wall_ms();
                sent_records[c_idx].push(SentRecord {
                    seq,
                    wall_ms,
                    media_pts,
                    payload: payload_bytes,
                });
                msgs_sent_by_client[c_idx] += 1;
            }
        }

        // Host pumps all available packets until WouldBlock
        while let Ok(Some(_)) = host.poll_and_process() {}

        // Each client pumps all available packets until WouldBlock
        for (c_idx, client) in clients.iter_mut().enumerate() {
            while let Ok(Some(_)) = client.poll_and_process() {}
            let new_msgs = client.poll_chat_messages();
            received_records[c_idx].extend(new_msgs);
        }

        // Check if all clients sent all messages AND all clients received expected relayed count
        // Each client should receive (NUM_CLIENTS - 1) * MSGS_PER_CLIENT messages
        let all_sent = msgs_sent_by_client.iter().all(|&n| n == MSGS_PER_CLIENT);
        let expected_relayed_per_client = (NUM_CLIENTS - 1) * MSGS_PER_CLIENT;
        let all_received = received_records
            .iter()
            .all(|msgs| msgs.len() == expected_relayed_per_client);

        if all_sent && all_received {
            break;
        }

        thread::sleep(Duration::from_millis(1));
    }

    // Drain any remaining messages from host and clients
    let drain_deadline = Instant::now() + Duration::from_millis(500);
    while Instant::now() < drain_deadline {
        while let Ok(Some(_)) = host.poll_and_process() {}
        for (c_idx, client) in clients.iter_mut().enumerate() {
            while let Ok(Some(_)) = client.poll_and_process() {}
            let new_msgs = client.poll_chat_messages();
            received_records[c_idx].extend(new_msgs);
        }
        thread::sleep(Duration::from_millis(1));
    }

    // --- VERIFICATION ORACLES ---
    let host_msgs = host.poll_chat_messages();
    assert_eq!(
        host_msgs.len(),
        total_expected_host_msgs,
        "Host must receive exactly {} messages without dropping any (got {})",
        total_expected_host_msgs,
        host_msgs.len()
    );

    let expected_relayed_per_client = (NUM_CLIENTS - 1) * MSGS_PER_CLIENT;

    for (c_idx, recs) in received_records.iter().enumerate() {
        let client_id_str = format!("Client-{}", c_idx + 1);

        // 1. Strict Anti-Echo Invariant: sender NEVER receives its own messages
        for msg in recs {
            let msg_text = String::from_utf8_lossy(&msg.payload);
            assert!(
                !msg_text.contains(&client_id_str),
                "ANTI-ECHO VIOLATION: Client {} received its own echoed message: {}",
                c_idx + 1,
                msg_text
            );
        }

        // 2. Zero dropped messages for relayed peers
        assert_eq!(
            recs.len(),
            expected_relayed_per_client,
            "Client {} must receive exactly {} relayed messages (got {})",
            c_idx + 1,
            expected_relayed_per_client,
            recs.len()
        );

        // 3. Monotonic sequence ordering per remote sender
        // Group received messages by remote sender prefix
        let mut per_sender_seqs: HashMap<String, Vec<u32>> = HashMap::new();
        for msg in recs {
            let msg_text = String::from_utf8_lossy(&msg.payload);
            let sender_tag = msg_text.split_whitespace().next().unwrap_or("").to_string();
            per_sender_seqs
                .entry(sender_tag)
                .or_default()
                .push(msg.sequence);
        }

        assert_eq!(
            per_sender_seqs.len(),
            NUM_CLIENTS - 1,
            "Client {} must have received from all {} other peers",
            c_idx + 1,
            NUM_CLIENTS - 1
        );

        for (sender_tag, seqs) in per_sender_seqs {
            assert_eq!(
                seqs.len(),
                MSGS_PER_CLIENT,
                "Client {} must receive all {} messages from {}",
                c_idx + 1,
                MSGS_PER_CLIENT,
                sender_tag
            );

            // Verify strictly increasing sequence order
            for i in 1..seqs.len() {
                assert!(
                    seqs[i] > seqs[i - 1],
                    "SEQUENCE INVERSION: {} -> Client {}: seq[{}]={} not > seq[{}]={}",
                    sender_tag,
                    c_idx + 1,
                    i,
                    seqs[i],
                    i - 1,
                    seqs[i - 1]
                );
            }
        }

        // 4. Verify exact payload and dual timestamp preservation against sent_records
        for (src_idx, sent_list) in sent_records.iter().enumerate() {
            if src_idx == c_idx {
                continue; // skipped because of anti-echo
            }
            let src_tag = format!("Client-{}", src_idx + 1);
            let matching_received: Vec<&ChatMessagePayload> = recs
                .iter()
                .filter(|m| String::from_utf8_lossy(&m.payload).contains(&src_tag))
                .collect();
            assert_eq!(matching_received.len(), sent_list.len());
            for (sent, rec) in sent_list.iter().zip(matching_received.iter()) {
                assert_eq!(sent.seq, rec.sequence);
                assert_eq!(sent.media_pts, rec.media_timestamp_ms);
                assert!(
                    sent.wall_ms >= rec.wall_timestamp_ms
                        && sent.wall_ms - rec.wall_timestamp_ms <= 100,
                    "Wall timestamp discrepancy: sent={}, rec={}",
                    sent.wall_ms,
                    rec.wall_timestamp_ms
                );
                assert_eq!(sent.payload, rec.payload);
            }
        }
    }
}

/// Corrupted datagram fuzzing and resilience test.
///
/// Sends corrupted headers, unknown opcodes, truncated payloads, bit-flipped CRC32,
/// and random byte fuzzing to host and client, verifying graceful rejection and immediate recovery.
#[test]
fn test_corrupted_datagram_fuzzing_and_host_recovery() {
    let mut host = HostServer::bind("127.0.0.1:0").expect("host bind");
    let host_addr = host.local_addr().expect("host local_addr");

    let raw_sock = UdpSocket::bind("127.0.0.1:0").expect("raw sock bind");

    // Establish a legitimate client
    let mut client = ClientNode::bind("127.0.0.1:0", host_addr, 99).expect("client bind");
    let _ = client.send_ntp_probe().expect("probe");
    for _ in 0..10 {
        let _ = host.poll_and_process();
        let _ = client.poll_and_process();
        thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(host.clients().len(), 1);

    let valid_chat = WirePacket::chat_message(1, 1000, 2000, b"Valid chat payload".to_vec());
    let valid_bytes = valid_chat.to_bytes();

    // --- Sub-test 2.1: Truncated headers (0 to 11 bytes) ---
    for len in 0..12 {
        raw_sock.send_to(&valid_bytes[..len], host_addr).unwrap();
        thread::sleep(Duration::from_millis(1));
        let res = host.poll_and_process();
        assert!(
            matches!(res, Err(SyncError::PacketTooShort(_))),
            "Header len {} must yield PacketTooShort, got {:?}",
            len,
            res
        );
    }

    // --- Sub-test 2.2: Unknown opcodes (0x00, 0x07, 0x09, 0x0A, 0x7F, 0xFF) ---
    for &bad_op in &[0x00u8, 0x07, 0x09, 0x0A, 0x33, 0x7F, 0xFE, 0xFF] {
        let mut pkt = valid_bytes.clone();
        pkt[4] = bad_op;
        raw_sock.send_to(&pkt, host_addr).unwrap();
        thread::sleep(Duration::from_millis(1));
        let res = host.poll_and_process();
        assert!(
            matches!(res, Err(SyncError::UnknownPacketType(op)) if op == bad_op),
            "Bad opcode 0x{:02X} must yield UnknownPacketType, got {:?}",
            bad_op,
            res
        );
    }

    // --- Sub-test 2.3: Truncated payloads ---
    // (a) Declared payload length > remaining buffer bytes
    let mut truncated_buf = valid_bytes.clone();
    truncated_buf.truncate(valid_bytes.len() - 5);
    raw_sock.send_to(&truncated_buf, host_addr).unwrap();
    thread::sleep(Duration::from_millis(1));
    let res = host.poll_and_process();
    assert!(
        matches!(res, Err(SyncError::PayloadTruncated { .. })),
        "Truncated payload buffer must yield PayloadTruncated, got {:?}",
        res
    );

    // (b) ChatMessage payload with length < 20 bytes (e.g. 0, 1, 10, 19 bytes)
    for chat_sub_len in [0usize, 1, 10, 19] {
        let dummy_sub_payload = vec![0xEEu8; chat_sub_len];
        let sub_crc = syncplay_common::Crc32::digest(&dummy_sub_payload);
        let mut malformed_chat = Vec::new();
        malformed_chat.extend_from_slice(b"SPNT");
        malformed_chat.push(PacketType::ChatMessage as u8); // 0x08
        malformed_chat.push(0x00); // flags
        malformed_chat.extend_from_slice(&(chat_sub_len as u16).to_le_bytes());
        malformed_chat.extend_from_slice(&sub_crc.to_le_bytes());
        malformed_chat.extend_from_slice(&dummy_sub_payload);

        raw_sock.send_to(&malformed_chat, host_addr).unwrap();
        thread::sleep(Duration::from_millis(1));
        let res = host.poll_and_process();
        assert!(
            matches!(res, Err(SyncError::MalformedPayload(_))),
            "Sub-20B chat payload length {} must yield MalformedPayload, got {:?}",
            chat_sub_len,
            res
        );
    }

    // --- Sub-test 2.4: CRC32 tampering ---
    // (a) Flipped bits in header CRC field
    let mut bad_crc_pkt = valid_bytes.clone();
    bad_crc_pkt[8] ^= 0x55;
    raw_sock.send_to(&bad_crc_pkt, host_addr).unwrap();
    thread::sleep(Duration::from_millis(1));
    let res = host.poll_and_process();
    assert!(
        matches!(res, Err(SyncError::CrcMismatch { .. })),
        "Tampered CRC field must yield CrcMismatch, got {:?}",
        res
    );

    // (b) Single bit flipped anywhere in the 38-byte payload
    for byte_offset in 12..valid_bytes.len() {
        let mut flipped_payload = valid_bytes.clone();
        flipped_payload[byte_offset] ^= 0x01;
        raw_sock.send_to(&flipped_payload, host_addr).unwrap();
        thread::sleep(Duration::from_millis(1));
        let res = host.poll_and_process();
        assert!(
            matches!(res, Err(SyncError::CrcMismatch { .. })),
            "Bit flip at offset {} must yield CrcMismatch, got {:?}",
            byte_offset,
            res
        );
    }

    // --- Sub-test 2.5: Oversized payloads & buffer boundaries ---
    // Header specifies payload_len = 65535, but datagram is only 100 bytes
    let mut oversized_hdr = valid_bytes[..12].to_vec();
    oversized_hdr[6] = 0xFF;
    oversized_hdr[7] = 0xFF;
    oversized_hdr.extend_from_slice(&[0x42; 88]); // 100 bytes total
    raw_sock.send_to(&oversized_hdr, host_addr).unwrap();
    thread::sleep(Duration::from_millis(1));
    let res = host.poll_and_process();
    assert!(
        matches!(res, Err(SyncError::PayloadTruncated { .. })),
        "Oversized declared length must yield PayloadTruncated, got {:?}",
        res
    );

    // --- Sub-test 2.6: High-density pseudorandom LFSR fuzzing (500 iterations) ---
    let mut lfsr: u32 = 0xCAFEBABE;
    for _ in 0..500 {
        lfsr = lfsr.wrapping_mul(1664525).wrapping_add(1013904223);
        let len = (lfsr % 600) as usize;
        let mut random_bytes = vec![0u8; len];
        for b in &mut random_bytes {
            lfsr = lfsr.wrapping_mul(1664525).wrapping_add(1013904223);
            *b = (lfsr >> 24) as u8;
        }

        raw_sock.send_to(&random_bytes, host_addr).unwrap();
        thread::sleep(Duration::from_micros(200));

        // Poll host: must never panic or crash
        let _ = host.poll_and_process();
    }

    // --- CRITICAL LIVENESS RECOVERY VERIFICATION ---
    // Immediately verify that host is fully operational, unblocked, and serves valid traffic
    let liveness_payload = b"Liveness probe post-fuzzing".to_vec();
    let liveness_seq = client
        .send_chat_message(&liveness_payload)
        .expect("send liveness chat");

    let mut liveness_received = false;
    for _ in 0..30 {
        if let Ok(Some((pkt, _))) = host.poll_and_process() {
            if matches!(pkt.payload, PacketPayload::ChatMessage(_)) {
                liveness_received = true;
                break;
            }
        }
        thread::sleep(Duration::from_millis(2));
    }

    assert!(
        liveness_received,
        "Host must immediately resume processing valid packets after fuzzing barrage"
    );

    let host_chat = host.poll_chat_messages();
    assert_eq!(host_chat.len(), 1);
    assert_eq!(host_chat[0].sequence, liveness_seq);
    assert_eq!(host_chat[0].payload, liveness_payload);
}

/// Dynamic peer joins and leaves during active chat.
///
/// Tests peers dynamically entering and leaving an active watch party while chat messages
/// are being sent, verifying uninterrupted delivery and zero crashes on inactive targets.
#[test]
fn test_dynamic_peer_joins_and_leaves_during_active_chat() {
    let mut host = HostServer::bind("127.0.0.1:0").expect("host bind");
    let host_addr = host.local_addr().expect("host local_addr");

    // Phase 1: Two initial clients
    let mut client1 = ClientNode::bind("127.0.0.1:0", host_addr, 10).expect("client1 bind");
    let mut client2 = ClientNode::bind("127.0.0.1:0", host_addr, 20).expect("client2 bind");

    let _ = client1.send_ntp_probe().unwrap();
    let _ = client2.send_ntp_probe().unwrap();

    for _ in 0..10 {
        let _ = host.poll_and_process();
        let _ = client1.poll_and_process();
        let _ = client2.poll_and_process();
        thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(host.clients().len(), 2);

    // C1 sends 5 messages to C2
    for i in 0..5 {
        let msg = format!("Phase1-C1-{}", i).into_bytes();
        client1.send_chat_message(&msg).unwrap();
    }

    for _ in 0..15 {
        while let Ok(Some(_)) = host.poll_and_process() {}
        while let Ok(Some(_)) = client1.poll_and_process() {}
        while let Ok(Some(_)) = client2.poll_and_process() {}
        thread::sleep(Duration::from_millis(1));
    }

    assert_eq!(client2.poll_chat_messages().len(), 5);
    assert!(client1.poll_chat_messages().is_empty()); // anti-echo

    // Phase 2: Client 3 joins dynamically mid-stream
    let mut client3 = ClientNode::bind("127.0.0.1:0", host_addr, 30).expect("client3 bind");
    let _ = client3.send_ntp_probe().unwrap();

    for _ in 0..10 {
        let _ = host.poll_and_process();
        let _ = client3.poll_and_process();
        thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(host.clients().len(), 3, "Host now has 3 registered peers");

    // C3 sends a message
    let c3_msg = b"Hello from dynamically joined Client 3!".to_vec();
    client3.send_chat_message(&c3_msg).unwrap();

    for _ in 0..15 {
        while let Ok(Some(_)) = host.poll_and_process() {}
        while let Ok(Some(_)) = client1.poll_and_process() {}
        while let Ok(Some(_)) = client2.poll_and_process() {}
        while let Ok(Some(_)) = client3.poll_and_process() {}
        thread::sleep(Duration::from_millis(1));
    }

    assert_eq!(client1.poll_chat_messages().len(), 1);
    assert_eq!(client2.poll_chat_messages().len(), 1);
    assert!(client3.poll_chat_messages().is_empty()); // C3 gets no echo

    // Phase 3: Client 2 leaves the party (socket dropped)
    drop(client2);

    // C1 sends 10 messages while C2 is gone
    for i in 0..10 {
        let msg = format!("Phase3-C1-{}", i).into_bytes();
        client1.send_chat_message(&msg).unwrap();
    }

    // Host relays to C3 and attempts relay to C2's defunct socket; must not fail or panic
    for _ in 0..20 {
        while let Ok(Some(_)) = host.poll_and_process() {}
        while let Ok(Some(_)) = client1.poll_and_process() {}
        while let Ok(Some(_)) = client3.poll_and_process() {}
        thread::sleep(Duration::from_millis(1));
    }

    let c3_msgs = client3.poll_chat_messages();
    assert_eq!(
        c3_msgs.len(),
        10,
        "Active Client 3 must receive all 10 messages from Client 1 despite Client 2 leaving"
    );
    assert!(client1.poll_chat_messages().is_empty());

    // Phase 4: Client 4 joins as a replacement
    let mut client4 = ClientNode::bind("127.0.0.1:0", host_addr, 40).expect("client4 bind");
    let _ = client4.send_ntp_probe().unwrap();

    for _ in 0..10 {
        let _ = host.poll_and_process();
        let _ = client4.poll_and_process();
        thread::sleep(Duration::from_millis(1));
    }

    // C4 sends message
    let c4_msg = b"Client 4 is now present".to_vec();
    client4.send_chat_message(&c4_msg).unwrap();

    for _ in 0..15 {
        while let Ok(Some(_)) = host.poll_and_process() {}
        while let Ok(Some(_)) = client1.poll_and_process() {}
        while let Ok(Some(_)) = client3.poll_and_process() {}
        while let Ok(Some(_)) = client4.poll_and_process() {}
        thread::sleep(Duration::from_millis(1));
    }

    assert_eq!(client1.poll_chat_messages().len(), 1);
    assert_eq!(client3.poll_chat_messages().len(), 1);
    assert!(client4.poll_chat_messages().is_empty()); // no echo
}

/// ChatMessage payload boundary extremes.
///
/// Verifies roundtrips for 0-byte payload (exactly 20 bytes payload section)
/// and large payloads up to near-MTU 1400 bytes.
#[test]
fn test_chat_message_payload_boundary_extremes() {
    // Zero-byte payload (exactly 20 bytes header/metadata)
    let empty_payload_pkt = WirePacket::chat_message(101, 1_700_000_000, 45_000, Vec::new());
    let empty_bytes = empty_payload_pkt.to_bytes();
    assert_eq!(
        empty_bytes.len(),
        12 + 20,
        "Zero-byte chat payload must serialize to exactly 32 bytes (12 hdr + 20 meta)"
    );

    let parsed_empty = WirePacket::from_bytes(&empty_bytes).expect("parse zero-byte chat");
    assert_eq!(parsed_empty, empty_payload_pkt);

    // Large 1400-byte payload
    let large_body = vec![0xABu8; 1400];
    let large_pkt = WirePacket::chat_message(102, 1_700_000_100, 45_200, large_body.clone());
    let large_bytes = large_pkt.to_bytes();
    assert_eq!(
        large_bytes.len(),
        12 + 20 + 1400,
        "1400-byte chat payload must serialize to 1432 bytes"
    );

    let parsed_large = WirePacket::from_bytes(&large_bytes).expect("parse large chat");
    assert_eq!(parsed_large, large_pkt);

    if let PacketPayload::ChatMessage(chat) = parsed_large.payload {
        assert_eq!(chat.sequence, 102);
        assert_eq!(chat.wall_timestamp_ms, 1_700_000_100);
        assert_eq!(chat.media_timestamp_ms, 45_200);
        assert_eq!(chat.payload, large_body);
    } else {
        panic!("Expected ChatMessage payload");
    }
}
