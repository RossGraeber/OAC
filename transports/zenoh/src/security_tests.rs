// SPDX-License-Identifier: Apache-2.0
//! Native TLS/ACL security checks over loopback; no harnesses, no LAN listeners.
use super::*;
use crate::config::Role;
use crate::ports::{Claimed, PortClaim, claimed};
use oac_core::ids::KeyId;
use oac_core::transport::{Deadline, Destination, Payload, PayloadKind, Transport};
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
/// A configuration on a rendezvous port claimed for as long as it lives (`crate::ports`).
fn local(dir: &StateDirectory) -> Claimed<PeerConfiguration> {
    claimed(|port| PeerConfiguration::rendezvous(port).with_local_certificate_directory(&dir.0))
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
fn concurrent_first_creation_publishes_one_complete_private_identity() {
    for _ in 0..4 {
        let dir = StateDirectory::new();
        let conf = local(&dir);
        let barrier = std::sync::Barrier::new(8);
        let results = std::thread::scope(|scope| {
            let callers: Vec<_> = (0..8)
                .map(|_| {
                    let conf = &conf;
                    let barrier = &barrier;
                    let directory = &dir.0;
                    scope.spawn(move || -> Result<Vec<u8>, String> {
                        barrier.wait();
                        // Real configuration/provisioning path, with an independent
                        // ephemeral listener per caller. Native TLS parses the winner.
                        let native = conf.native(Role::First, 0)?;
                        let peer = zenoh::open(native).wait().map_err(|e| e.to_string())?;
                        let bundle = std::fs::read(directory.join("identity.pem"))
                            .map_err(|e| e.to_string())?;
                        peer.close().wait().map_err(|e| e.to_string())?;
                        Ok(bundle)
                    })
                })
                .collect();
            callers
                .into_iter()
                .map(|c| c.join().unwrap())
                .collect::<Vec<_>>()
        });
        let errors: Vec<_> = results.iter().filter_map(|r| r.as_ref().err()).collect();
        assert!(errors.is_empty(), "{errors:?}");
        let winner = results[0].as_ref().unwrap();
        assert!(results.iter().all(|r| r.as_ref().unwrap() == winner));
        assert_eq!(std::fs::read_dir(&dir.0).unwrap().count(), 1);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for (path, mode) in [(&dir.0, 0o700), (&dir.0.join("identity.pem"), 0o600)] {
                assert_eq!(
                    std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
                    mode
                );
            }
        }
    }
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
        assert!(joiner.start(&key('a'), conf.clone().wrap()).is_err());
        assert_eq!(joiner.connected_peers(), 0);
        squatter.close().wait().unwrap();
    }
}

#[test]
fn same_user_persisted_certificate_survives_holder_replacement_and_native_reconnect() {
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
    replacement.start(&key('c'), conf.clone().wrap()).unwrap();
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
    let claim = PortClaim::claim();
    let port = claim.port();
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
    let claim = PortClaim::claim();
    let port = claim.port();
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
fn native_tls_refuses_expired_certificate_without_regenerating_state() {
    for expired in [false, true] {
        let dir = StateDirectory::new();
        let conf = local(&dir);
        conf.native(Role::First, 0).unwrap();
        let mut params = rcgen::CertificateParams::new(vec!["127.0.0.1".into()]).unwrap();
        params
            .distinguished_name
            .push(rcgen::DnType::CommonName, "oac-local");
        params.extended_key_usages = vec![
            rcgen::ExtendedKeyUsagePurpose::ServerAuth,
            rcgen::ExtendedKeyUsagePurpose::ClientAuth,
        ];
        if expired {
            params.not_before = rcgen::date_time_ymd(2000, 1, 1);
            params.not_after = rcgen::date_time_ymd(2001, 1, 1);
        }
        let credential_key = rcgen::KeyPair::generate().unwrap();
        let certificate = params.self_signed(&credential_key).unwrap();
        let bundle = format!("{}{}", certificate.pem(), credential_key.serialize_pem());
        std::fs::write(dir.0.join("identity.pem"), &bundle).unwrap();
        let holder = PeerTransport::new();
        holder.start(&key('a'), conf.clone().wrap()).unwrap();
        let joiner = PeerTransport::new();
        let result = joiner.start(&key('b'), conf.clone().wrap());
        assert_eq!(result.is_err(), expired);
        assert_eq!(
            std::fs::read_to_string(dir.0.join("identity.pem")).unwrap(),
            bundle
        );
        joiner.shutdown();
        holder.shutdown();
    }
}

#[test]
fn corrupt_existing_certificate_is_not_regenerated_and_native_configuration_is_default_deny() {
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
    // Exactly two allow rules, each limited to its own key and messages: widening either to
    // `oac/**`, or adding a message type (e.g. `liveliness_query`), fails here (#63).
    let partition = crate::addressing::Partition::new(conf.partition());
    let rules = acl["rules"].as_array().unwrap();
    assert_eq!(rules.len(), 2);
    for (rule, id, messages, key) in [
        (
            &rules[0],
            "frames",
            serde_json::json!(["put", "declare_subscriber"]),
            partition.key(),
        ),
        (
            &rules[1],
            "liveness",
            serde_json::json!(["liveliness_token", "declare_liveliness_subscriber"]),
            partition.token_pattern(),
        ),
    ] {
        assert_eq!(rule["id"], id);
        assert_eq!(rule["permission"], "allow");
        assert_eq!(rule["flows"], serde_json::json!(["ingress", "egress"]));
        assert_eq!(rule["messages"], messages, "{id}");
        assert_eq!(rule["key_exprs"], serde_json::json!([key]), "{id}");
    }
    assert_eq!(
        acl["policies"][0]["rules"],
        serde_json::json!(["frames", "liveness"])
    );
    std::fs::write(dir.0.join("identity.pem"), b"corrupt certificate").unwrap();
    assert!(
        PeerTransport::new()
            .start(&key('a'), conf.clone().wrap())
            .is_err()
    );
    assert_eq!(
        std::fs::read(dir.0.join("identity.pem")).unwrap(),
        b"corrupt certificate"
    );
}

#[cfg(windows)]
#[test]
fn existing_windows_null_dacl_certificate_directory_is_refused() {
    // Supported Win32 APIs (desktop API), documentation inspected 2026-10-11:
    // https://learn.microsoft.com/en-us/windows/win32/api/aclapi/nf-aclapi-setnamedsecurityinfow
    // https://learn.microsoft.com/en-us/windows/win32/api/aclapi/nf-aclapi-getnamedsecurityinfow
    use std::os::windows::process::CommandExt;
    let dir = StateDirectory::new();
    let conf = local(&dir);
    conf.native(Role::First, conf.rendezvous_port()).unwrap();
    let script = r#"
$ErrorActionPreference='Stop'
# Set only the DACL through the public Win32 API. PowerShell Set-Acl's null-DACL
# path also requests SACL privileges, which a normal user does not have.
Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
public static class OacTlsTestAcl {
  [DllImport("advapi32.dll", CharSet=CharSet.Unicode, EntryPoint="SetNamedSecurityInfoW")]
  static extern uint Set(string path, int type, uint flags, IntPtr owner, IntPtr group, IntPtr dacl, IntPtr sacl);
  [DllImport("advapi32.dll", CharSet=CharSet.Unicode, EntryPoint="GetNamedSecurityInfoW")]
  static extern uint Get(string path, int type, uint flags, out IntPtr owner, out IntPtr group, out IntPtr dacl, out IntPtr sacl, out IntPtr descriptor);
  [DllImport("kernel32.dll")] static extern IntPtr LocalFree(IntPtr pointer);
  public static void Install(string path) {
    uint error=Set(path,1,0x80000004u,IntPtr.Zero,IntPtr.Zero,IntPtr.Zero,IntPtr.Zero);
    if(error!=0) throw new Exception("cannot install null DACL: "+error);
  }
  public static bool IsNull(string path) {
    IntPtr owner, group, dacl, sacl, descriptor;
    uint error=Get(path,1,4u,out owner,out group,out dacl,out sacl,out descriptor);
    if(error!=0) throw new Exception("cannot inspect null DACL: "+error);
    try { return dacl==IntPtr.Zero; } finally { LocalFree(descriptor); }
  }
}
"@
[OacTlsTestAcl]::Install($env:OAC_TLS_STORAGE)
if (![OacTlsTestAcl]::IsNull($env:OAC_TLS_STORAGE)) { throw 'null DACL control was not installed' }
"#;
    let result = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .env("OAC_TLS_STORAGE", &dir.0)
        .creation_flags(0x0800_0000)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(conf.native(Role::First, conf.rendezvous_port()).is_err());
}

#[cfg(unix)]
#[test]
fn existing_unix_non_private_storage_is_refused_without_repair() {
    use std::os::unix::fs::PermissionsExt;
    for directory_is_public in [true, false] {
        let dir = StateDirectory::new();
        let conf = local(&dir);
        conf.native(Role::First, 0).unwrap();
        let path = if directory_is_public {
            dir.0.clone()
        } else {
            dir.0.join("identity.pem")
        };
        let mode = if directory_is_public { 0o755 } else { 0o644 };
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
        let original = std::fs::read(dir.0.join("identity.pem")).unwrap();
        assert!(conf.native(Role::First, 0).is_err());
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            mode
        );
        assert_eq!(std::fs::read(dir.0.join("identity.pem")).unwrap(), original);
    }
}
