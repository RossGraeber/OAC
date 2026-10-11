// SPDX-License-Identifier: Apache-2.0
//! Native TLS/ACL security checks over loopback; no harnesses, no LAN listeners.
use super::*;
use crate::config::Role;
use oac_core::ids::KeyId;
use oac_core::transport::{Deadline, Destination, Payload, PayloadKind, Transport};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};
use zenoh::Wait;

static NEXT: AtomicUsize = AtomicUsize::new(0);
struct StateDirectory(PathBuf);
impl StateDirectory {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "oac-tls-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        )))
    }
}
impl Drop for StateDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn key(n: char) -> KeyId {
    KeyId::parse(&n.to_string().repeat(64)).unwrap()
}
fn local(dir: &StateDirectory) -> PeerConfiguration {
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    PeerConfiguration::rendezvous(port).with_local_certificate_directory(&dir.0)
}
fn send_and_receive(sender: &PeerTransport, receiver: &PeerTransport, device: char) {
    let (tx, rx) = mpsc::channel();
    let _sub = receiver
        .subscribe(
            &Destination::Device(key(device)),
            Arc::new(move |inbound| {
                tx.send(inbound.payload.octets().to_vec()).unwrap();
            }),
        )
        .unwrap();
    sender.publish(
        &Destination::Device(key(device)),
        Payload::new(PayloadKind::Sealed, b"security-test".to_vec()),
        Deadline::at(Instant::now() + Duration::from_secs(5)),
    );
    assert_eq!(
        rx.recv_timeout(Duration::from_secs(5)).unwrap(),
        b"security-test"
    );
}

#[test]
fn plain_native_router_and_different_user_tls_holder_are_refused() {
    let trusted = StateDirectory::new();
    let foreign = StateDirectory::new();
    for plaintext in [true, false] {
        let conf = local(&trusted);
        let alien = conf.clone().with_local_certificate_directory(&foreign.0);
        let mut native = alien.native(Role::First, conf.rendezvous_port()).unwrap();
        if plaintext {
            native
                .insert_json5("transport/link/protocols", r#"["tcp"]"#)
                .unwrap();
            native
                .insert_json5(
                    "listen/endpoints",
                    &format!(r#"["tcp/127.0.0.1:{}"]"#, conf.rendezvous_port()),
                )
                .unwrap();
            native
                .insert_json5("access_control/enabled", "false")
                .unwrap();
        }
        let squatter = zenoh::open(native).wait().unwrap();
        let joiner = PeerTransport::new();
        assert!(joiner.start(&key('a'), conf.wrap()).is_err());
        assert_eq!(joiner.connected_peers(), 0);
        squatter.close().wait().unwrap();
    }
}

#[test]
fn same_user_pin_survives_holder_replacement_and_native_reconnect() {
    let directory = StateDirectory::new();
    let conf = local(&directory);
    let first = PeerTransport::new();
    let joiner = PeerTransport::new();
    first.start(&key('a'), conf.clone().wrap()).unwrap();
    let pin = std::fs::read(directory.0.join("identity.pem")).unwrap();
    joiner.start(&key('b'), conf.clone().wrap()).unwrap();
    send_and_receive(&joiner, &first, 'a');
    first.shutdown();
    let replacement = PeerTransport::new();
    replacement.start(&key('c'), conf.wrap()).unwrap();
    assert_eq!(
        std::fs::read(directory.0.join("identity.pem")).unwrap(),
        pin
    );
    // Link establishment precedes native routing convergence. As in the existing
    // replacement test, probe eventual delivery; this is no product retry mechanism.
    let (tx, rx) = mpsc::channel();
    let _sub = joiner
        .subscribe(
            &Destination::Device(key('b')),
            Arc::new(move |_| {
                let _ = tx.send(());
            }),
        )
        .unwrap();
    let until = Instant::now() + Duration::from_secs(20);
    loop {
        replacement.publish(
            &Destination::Device(key('b')),
            Payload::new(PayloadKind::Sealed, b"replacement".to_vec()),
            Deadline::at(Instant::now() + Duration::from_secs(5)),
        );
        if rx.recv_timeout(Duration::from_millis(200)).is_ok() {
            break;
        }
        assert!(
            Instant::now() < until,
            "native reconnect did not resume delivery"
        );
    }
    joiner.shutdown();
    replacement.shutdown();
}

#[test]
fn paired_lan_configuration_tls_and_acl_allow_only_approved_common_names() {
    let authority = PairingAuthority::new().unwrap();
    let a_dir = StateDirectory::new();
    let b_dir = StateDirectory::new();
    let c_dir = StateDirectory::new();
    let ip = "127.0.0.1".parse().unwrap();
    let a = authority
        .issue("peer-a", &[ip])
        .unwrap()
        .install(&a_dir.0)
        .unwrap();
    let b = authority
        .issue("peer-b", &[ip])
        .unwrap()
        .install(&b_dir.0)
        .unwrap();
    let c = authority
        .issue("unapproved", &[ip])
        .unwrap()
        .install(&c_dir.0)
        .unwrap();
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let address = format!("127.0.0.1:{port}").parse().unwrap();
    let ephemeral = "127.0.0.1:0".parse().unwrap();
    let conf_a = PeerConfiguration::lan(address, vec![], a, vec!["peer-b".into()]);
    let conf_b = PeerConfiguration::lan(ephemeral, vec![address], b, vec!["peer-a".into()]);
    let receiver = PeerTransport::new();
    let sender = PeerTransport::new();
    receiver.start(&key('a'), conf_a.wrap()).unwrap();
    sender.start(&key('b'), conf_b.wrap()).unwrap();
    send_and_receive(&sender, &receiver, 'a');
    let stranger_conf = PeerConfiguration::lan(ephemeral, vec![address], c, vec!["peer-a".into()]);
    let stranger = zenoh::open(stranger_conf.native(Role::First, 0).unwrap())
        .wait()
        .unwrap();
    let (tx, rx) = mpsc::channel();
    let _sub = receiver
        .subscribe(
            &Destination::Device(key('a')),
            Arc::new(move |_| {
                tx.send(()).unwrap();
            }),
        )
        .unwrap();
    stranger
        .put(
            crate::addressing::Partition::new(DEFAULT_PARTITION).key(),
            b"denied".to_vec(),
        )
        .encoding(zenoh::bytes::Encoding::ZENOH_BYTES)
        .wait()
        .unwrap();
    assert!(rx.recv_timeout(Duration::from_millis(300)).is_err());
    stranger.close().wait().unwrap();
    sender.shutdown();
    receiver.shutdown();
}

#[test]
fn paired_certificate_wrong_ip_name_is_refused_by_native_tls() {
    let authority = PairingAuthority::new().unwrap();
    let a_dir = StateDirectory::new();
    let b_dir = StateDirectory::new();
    let wrong = authority
        .issue("peer-a", &["127.0.0.2".parse().unwrap()])
        .unwrap()
        .install(&a_dir.0)
        .unwrap();
    let good = authority
        .issue("peer-b", &["127.0.0.1".parse().unwrap()])
        .unwrap()
        .install(&b_dir.0)
        .unwrap();
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let address = format!("127.0.0.1:{port}").parse().unwrap();
    let holder_conf = PeerConfiguration::lan(address, vec![], wrong, vec!["peer-b".into()]);
    let holder = PeerTransport::new();
    holder.start(&key('a'), holder_conf.wrap()).unwrap();
    let joiner_conf = PeerConfiguration::lan(
        "127.0.0.1:0".parse().unwrap(),
        vec![address],
        good,
        vec!["peer-a".into()],
    );
    assert!(
        PeerTransport::new()
            .start(&key('b'), joiner_conf.wrap())
            .is_err()
    );
    holder.shutdown();
}

#[test]
fn native_mtls_refuses_a_joiner_without_a_client_certificate() {
    let dir = StateDirectory::new();
    let conf = local(&dir);
    let holder = PeerTransport::new();
    holder.start(&key('a'), conf.clone().wrap()).unwrap();
    let mut anonymous = conf.native(Role::Joiner, 0).unwrap();
    // Model a native client that does not offer a certificate, independently of
    // OAC's mandatory-client-credential configuration. The holder still requires mTLS.
    anonymous
        .insert_json5("transport/link/tls/enable_mtls", "false")
        .unwrap();
    anonymous
        .insert_json5("transport/link/tls/connect_certificate", "null")
        .unwrap();
    anonymous
        .insert_json5("transport/link/tls/connect_private_key", "null")
        .unwrap();
    assert!(zenoh::open(anonymous).wait().is_err());
    holder.shutdown();
}

#[test]
fn corrupt_existing_pin_is_not_regenerated_and_native_configuration_is_default_deny() {
    let dir = StateDirectory::new();
    let conf = local(&dir);
    let native = conf.native(Role::First, conf.rendezvous_port()).unwrap();
    for setting in [
        "enable_mtls",
        "verify_name_on_connect",
        "close_link_on_expiration",
    ] {
        assert_eq!(
            native
                .get_json(&format!("transport/link/tls/{setting}"))
                .unwrap(),
            "true"
        );
    }
    assert_eq!(
        native.get_json("transport/link/protocols").unwrap(),
        r#"["tls"]"#
    );
    let acl: serde_json::Value =
        serde_json::from_str(&native.get_json("access_control").unwrap()).unwrap();
    assert_eq!(acl["default_permission"], "deny");
    assert_eq!(
        acl["subjects"][0]["cert_common_names"],
        serde_json::json!(["oac-local"])
    );
    // Native config serializes unused optional subject fields as null.
    assert!(acl["subjects"][0]["zids"].is_null());
    std::fs::write(dir.0.join("identity.pem"), b"corrupt pin").unwrap();
    assert!(PeerTransport::new().start(&key('a'), conf.wrap()).is_err());
    assert_eq!(
        std::fs::read(dir.0.join("identity.pem")).unwrap(),
        b"corrupt pin"
    );
}

#[cfg(windows)]
#[test]
fn existing_windows_null_dacl_certificate_directory_is_refused() {
    use std::os::windows::process::CommandExt;
    let dir = StateDirectory::new();
    let conf = local(&dir);
    conf.native(Role::First, conf.rendezvous_port()).unwrap();
    let script = r#"
$ErrorActionPreference='Stop'
$sid=[System.Security.Principal.WindowsIdentity]::GetCurrent().User
$a=New-Object System.Security.AccessControl.DirectorySecurity
$a.SetSecurityDescriptorSddlForm('D:PNO_ACCESS_CONTROL')
$a.SetOwner($sid)
Set-Acl -LiteralPath $env:OAC_TLS_STORAGE -AclObject $a
$observed=Get-Acl -LiteralPath $env:OAC_TLS_STORAGE
if (!$observed.GetSecurityDescriptorSddlForm([System.Security.AccessControl.AccessControlSections]::Access).Contains('NO_ACCESS_CONTROL')) { throw 'null DACL control was not installed' }
"#;
    let result = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .env("OAC_TLS_STORAGE", &dir.0)
        .creation_flags(0x0800_0000)
        .output()
        .unwrap();
    assert!(result.status.success());
    assert!(conf.native(Role::First, conf.rendezvous_port()).is_err());
}
