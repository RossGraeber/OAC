// SPDX-License-Identifier: Apache-2.0

//! OAC-owned certificate provisioning. Verification and handshakes belong to Zenoh.
//!
//! Local certificates, pairing authorities and issued leaves retain rcgen 0.14.10's
//! default validity: 1975-01-01 00:00:00 UTC through 4096-01-01 00:00:00 UTC.
//! Source: <https://github.com/rustls/rcgen/blob/f4a3b165c467c070d5c74b86acc2c3f0a0be10ab/rcgen/src/certificate.rs>,
//! version 0.14.10, cached first-party source inspected 2026-10-11. No shorter
//! operational rotation interval is implemented. Zenoh refuses expired credentials
//! on join (valid/expired control in `native_tls_refuses_expired_certificate_without_regenerating_state`);
//! configuration enables native `close_link_on_expiration`. There is no automatic
//! renewal or regeneration on expiry, corruption, privacy failure or connection failure.
//!
//! Rotation/recovery requires a coordinated outage: stop the local live holder and
//! every joiner, disable restarts, retire the old private directory, provision fresh
//! key material, then start one holder before restarting joiners on that same material.
//! For LAN, stop all peers in the affected trust group, authorize fresh provisioning
//! offline, install fresh credentials in new owner-only directories, and update every
//! peer's roots, IP SAN configuration and CN allowlist before restarting listeners and
//! then connecting peers. For revocation, replace the pairing authority and all leaves
//! and remove old roots everywhere; leaf reissue under an old CA does not revoke it.
//! Do not overwrite live files or rely on certificate-only replacement with an old key.
//! See the crate's binding document for the full local/LAN sequence and evidence limits.
//! No custom rotation, handshake or revocation protocol is added.
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use rcgen::{CertificateParams, DnType, ExtendedKeyUsagePurpose, KeyPair};

/// Certificate issuer owned by an authorized provisioning/pairing operation, never
/// invoked by an incoming transport message. Keep this authority off the LAN peers.
pub struct PairingAuthority(rcgen::CertifiedIssuer<'static, KeyPair>);

/// A pairing-issued TLS credential. No Debug implementation: contains a private key.
pub struct IssuedCertificate {
    certificate: String,
    private_key: String,
    trusted_roots: String,
}

impl PairingAuthority {
    /// Create an authority for an explicitly approved pairing group.
    pub fn new() -> Result<Self, String> {
        let mut params = CertificateParams::default();
        params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
        params.key_usages = vec![
            rcgen::KeyUsagePurpose::KeyCertSign,
            rcgen::KeyUsagePurpose::CrlSign,
        ];
        params
            .distinguished_name
            .push(DnType::CommonName, "oac-pairing");
        let key = KeyPair::generate().map_err(|_| "cannot generate pairing authority")?;
        rcgen::CertifiedIssuer::self_signed(params, key)
            .map(Self)
            .map_err(|_| "cannot generate pairing authority".into())
    }

    /// Issue only after the operator/core authorized pairing. Use an opaque common name
    /// (not an OAC identifier) and the host's configured IP SANs. TLS is not message identity.
    pub fn issue(
        &self,
        common_name: &str,
        addresses: &[std::net::IpAddr],
    ) -> Result<IssuedCertificate, String> {
        if common_name.is_empty() || addresses.is_empty() {
            return Err("certificate subject and addresses are required".into());
        }
        let mut params = CertificateParams::new(
            addresses
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
        )
        .map_err(|_| "invalid certificate address")?;
        params
            .distinguished_name
            .push(DnType::CommonName, common_name);
        params.extended_key_usages = vec![
            ExtendedKeyUsagePurpose::ServerAuth,
            ExtendedKeyUsagePurpose::ClientAuth,
        ];
        let key = KeyPair::generate().map_err(|_| "cannot generate paired key")?;
        let certificate = params
            .signed_by(&key, &self.0)
            .map_err(|_| "cannot issue paired certificate")?;
        Ok(IssuedCertificate {
            certificate: certificate.pem(),
            private_key: key.serialize_pem(),
            trusted_roots: self.0.pem(),
        })
    }
}

impl IssuedCertificate {
    /// Install into a fresh owner-only directory after authorized provisioning. Refuses
    /// replacement; rotation is an explicit provisioning operation, never trust-on-connect.
    pub fn install(&self, directory: &Path) -> Result<CertificateFiles, String> {
        if let Some(parent) = directory.parent() {
            fs::create_dir_all(parent).map_err(|_| "cannot create certificate parent")?;
        }
        private_directory(directory)?;
        let paths = CertificateFiles {
            certificate: directory.join("certificate.pem"),
            private_key: directory.join("key.pem"),
            trusted_roots: directory.join("roots.pem"),
        };
        for (path, contents) in [
            (&paths.certificate, &self.certificate),
            (&paths.private_key, &self.private_key),
            (&paths.trusted_roots, &self.trusted_roots),
        ] {
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options.open(path).map_err(
                |_| "cannot install paired certificate (existing state is never replaced)",
            )?;
            file.write_all(contents.as_bytes())
                .and_then(|()| file.sync_all())
                .map_err(|_| "cannot persist paired certificate")?;
        }
        Ok(paths)
    }
}

/// Files installed by an authorized pairing/provisioning operation. Private key files
/// must be stored in an owner-only directory. This is not an envelope identity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CertificateFiles {
    pub certificate: PathBuf,
    pub private_key: PathBuf,
    pub trusted_roots: PathBuf,
}

impl CertificateFiles {
    pub(crate) fn validate(&self) -> Result<(), String> {
        let parent = self
            .private_key
            .parent()
            .ok_or("certificate key needs a private directory")?;
        private_directory(parent)?;
        ordinary(&self.private_key, false)
    }
}

const LOCAL_SUBJECT: &str = "oac-local";
pub(crate) fn local_subject() -> &'static str {
    LOCAL_SUBJECT
}

pub(crate) fn default_directory() -> Result<PathBuf, String> {
    #[cfg(windows)]
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
    #[cfg(not(windows))]
    let base = std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state"));
    base.map(|b| b.join("OAC/relay-tls"))
        .ok_or_else(|| "no per-user certificate directory".into())
}

fn ordinary(path: &Path, directory: bool) -> Result<(), String> {
    let meta = fs::symlink_metadata(path).map_err(|_| "cannot inspect certificate storage")?;
    if meta.file_type().is_symlink()
        || (directory && !meta.is_dir())
        || (!directory && !meta.is_file())
    {
        return Err("certificate storage must use ordinary files and directories".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let user = std::process::Command::new("id")
            .arg("-u")
            .output()
            .map_err(|_| "cannot check certificate owner")?;
        let uid = std::str::from_utf8(&user.stdout)
            .ok()
            .and_then(|s| s.trim().parse::<u32>().ok())
            .ok_or("cannot check certificate owner")?;
        if !user.status.success() || meta.uid() != uid {
            return Err("certificate storage belongs to another user".into());
        }
        if meta.permissions().mode() & 0o077 != 0 {
            return Err("certificate storage must be owner-only".into());
        }
    }
    Ok(())
}

/// Protect before writing secrets. Existing storage is checked, never repaired or
/// regenerated on a verification failure. Administrators/root remain an OS trust boundary.
fn private_directory(path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        match fs::DirBuilder::new().mode(0o700).create(path) {
            Ok(()) => (),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (),
            Err(_) => return Err("cannot create private certificate directory".into()),
        }
    }
    #[cfg(windows)]
    {
        // Native Windows ACL provisioning, not transport authentication. .NET Framework
        // passes DirectorySecurity as Win32 SECURITY_ATTRIBUTES at creation and accepts
        // a concurrent existing directory without changing its ACL; validation below
        // still refuses non-private existing state. First-party source inspected 2026-10-11:
        // https://github.com/microsoft/referencesource/blob/ec9fa9ae770d522a5b5f0607898044b7478574a3/mscorlib/system/io/directory.cs
        // The path is data, never interpolated into script code. Do not read any keyring.
        use std::os::windows::process::CommandExt;
        let script = r#"
$ErrorActionPreference='Stop'
$p=$env:OAC_TLS_STORAGE
$sid=[System.Security.Principal.WindowsIdentity]::GetCurrent().User
$a=New-Object System.Security.AccessControl.DirectorySecurity
$a.SetOwner($sid)
$a.SetAccessRuleProtection($true,$false)
$r=New-Object System.Security.AccessControl.FileSystemAccessRule($sid,'FullControl','ContainerInherit,ObjectInherit','None','Allow')
$a.AddAccessRule($r)
$null=[System.IO.Directory]::CreateDirectory($p,$a)
foreach ($d in @((Get-Item -LiteralPath $p)) + @(Get-ChildItem -LiteralPath $p -Force)) {
  if ($d.Attributes -band [System.IO.FileAttributes]::ReparsePoint) { throw 'reparse point' }
  try { $a=Get-Acl -LiteralPath $d.FullName } catch {
    # Concurrent creators remove their private temporary files after publication.
    # Only vanished pending files may be skipped; never skip the directory/bundle,
    # an existing file, or an ACL that can actually be inspected and is non-private.
    if (!$d.PSIsContainer -and $d.Name -like 'pending-*.pem' -and !(Test-Path -LiteralPath $d.FullName)) { continue }
    throw
  }
  if ($a.GetOwner([System.Security.Principal.SecurityIdentifier]) -ne $sid) { throw 'foreign owner' }
  $ownerAllowed=$false
  foreach ($r in $a.GetAccessRules($true,$true,[System.Security.Principal.SecurityIdentifier])) {
    if ($r.AccessControlType -eq 'Allow' -and $r.IdentityReference -ne $sid) { throw 'non-private storage' }
    if ($r.AccessControlType -eq 'Allow' -and $r.IdentityReference -eq $sid) { $ownerAllowed=$true }
  }
  # Refuse missing owner rules, including null/empty DACL representations.
  if (!$ownerAllowed) { throw 'missing owner access rule' }
}
"#;
        let output = std::process::Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command", script])
            .env("OAC_TLS_STORAGE", path)
            .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
            .output()
            .map_err(|_| "cannot check Windows certificate ACL")?;
        if !output.status.success() {
            return Err(
                "certificate directory must be owned by and accessible only to this user".into(),
            );
        }
    }
    ordinary(path, true)
}

/// One self-signed certificate is both the per-user relay trust root and the local mTLS
/// identity; the trust rule is the one stated in the transport binding document (lib.rs).
/// A complete PEM bundle is published with an atomic no-overwrite hard link. Losing
/// creators read the winner; partial/corrupt existing state fails closed in native TLS.
/// Unix directories use mode 0700 at creation; temporary files use O_EXCL and mode 0600.
/// Windows files inherit the directory's owner-only ACL, present from directory creation.
pub(crate) fn local_files(directory: &Path) -> Result<CertificateFiles, String> {
    if let Some(parent) = directory.parent() {
        fs::create_dir_all(parent).map_err(|_| "cannot create certificate parent")?;
    }
    private_directory(directory)?;
    let bundle = directory.join("identity.pem");
    if !bundle.exists() {
        let key = KeyPair::generate().map_err(|_| "cannot generate certificate key")?;
        let mut params = CertificateParams::new(vec!["127.0.0.1".into()])
            .map_err(|_| "invalid local certificate name")?;
        params
            .distinguished_name
            .push(DnType::CommonName, LOCAL_SUBJECT);
        params.extended_key_usages = vec![
            ExtendedKeyUsagePurpose::ServerAuth,
            ExtendedKeyUsagePurpose::ClientAuth,
        ];
        let cert = params
            .self_signed(&key)
            .map_err(|_| "cannot generate local certificate")?;
        let temp = directory.join(format!(
            "pending-{}-{}.pem",
            std::process::id(),
            crate::addressing::Partition::new(&cert.pem())
                .prefix()
                .replace('/', "-")
        ));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options
            .open(&temp)
            .map_err(|_| "cannot create certificate bundle")?;
        let result = (|| {
            file.write_all(cert.pem().as_bytes())
                .map_err(|_| "cannot write certificate")?;
            file.write_all(key.serialize_pem().as_bytes())
                .map_err(|_| "cannot write certificate key")?;
            file.sync_all().map_err(|_| "cannot persist certificate")?;
            match fs::hard_link(&temp, &bundle) {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
                Err(_) => Err("cannot publish persisted certificate"),
            }
        })();
        drop(file);
        let _ = fs::remove_file(&temp);
        result?;
    }
    ordinary(&bundle, false)?;
    Ok(CertificateFiles {
        certificate: bundle.clone(),
        private_key: bundle.clone(),
        trusted_roots: bundle,
    })
}
