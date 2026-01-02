use rustyjack_ipc::{DaemonError, ErrorCode};

const MAX_INTERFACE_NAME_LEN: usize = 64;
const MAX_SSID_LEN: usize = 32;
const MAX_PSK_LEN: usize = 64;
const MIN_PSK_LEN: usize = 8;
const MAX_DEVICE_PATH_LEN: usize = 256;
const MAX_PORT: u16 = 65535;
const MIN_PORT: u16 = 1;
const MAX_TIMEOUT_MS: u64 = 3_600_000;

pub fn validate_interface_name(interface: &str) -> Result<(), DaemonError> {
    if interface.is_empty() {
        return Err(DaemonError::new(
            ErrorCode::BadRequest,
            "interface name cannot be empty",
            false,
        ));
    }
    if interface.len() > MAX_INTERFACE_NAME_LEN {
        return Err(DaemonError::new(
            ErrorCode::BadRequest,
            "interface name too long",
            false,
        ));
    }
    if !interface
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(DaemonError::new(
            ErrorCode::BadRequest,
            "interface name contains invalid characters",
            false,
        ));
    }
    Ok(())
}

pub fn validate_ssid(ssid: &str) -> Result<(), DaemonError> {
    if ssid.is_empty() {
        return Err(DaemonError::new(
            ErrorCode::BadRequest,
            "SSID cannot be empty",
            false,
        ));
    }
    if ssid.len() > MAX_SSID_LEN {
        return Err(DaemonError::new(
            ErrorCode::BadRequest,
            "SSID too long (max 32 bytes)",
            false,
        ));
    }
    Ok(())
}

pub fn validate_psk(psk: &Option<String>) -> Result<(), DaemonError> {
    if let Some(ref passphrase) = psk {
        if passphrase.len() < MIN_PSK_LEN {
            return Err(DaemonError::new(
                ErrorCode::BadRequest,
                "PSK too short (min 8 characters)",
                false,
            ));
        }
        if passphrase.len() > MAX_PSK_LEN {
            return Err(DaemonError::new(
                ErrorCode::BadRequest,
                "PSK too long (max 64 characters)",
                false,
            ));
        }
    }
    Ok(())
}

pub fn validate_channel(channel: &Option<u8>) -> Result<(), DaemonError> {
    if let Some(ch) = channel {
        if *ch == 0 || *ch > 165 {
            return Err(DaemonError::new(
                ErrorCode::BadRequest,
                "invalid channel (must be 1-165)",
                false,
            ));
        }
    }
    Ok(())
}

pub fn validate_port(port: u16) -> Result<(), DaemonError> {
    if port < MIN_PORT || port > MAX_PORT {
        return Err(DaemonError::new(
            ErrorCode::BadRequest,
            "invalid port number",
            false,
        ));
    }
    if port < 1024 {
        return Err(DaemonError::new(
            ErrorCode::BadRequest,
            "privileged ports (<1024) not allowed",
            false,
        ));
    }
    Ok(())
}

pub fn validate_timeout_ms(timeout_ms: u64) -> Result<(), DaemonError> {
    if timeout_ms == 0 {
        return Err(DaemonError::new(
            ErrorCode::BadRequest,
            "timeout cannot be zero",
            false,
        ));
    }
    if timeout_ms > MAX_TIMEOUT_MS {
        return Err(DaemonError::new(
            ErrorCode::BadRequest,
            "timeout too large (max 1 hour)",
            false,
        ));
    }
    Ok(())
}

pub fn validate_device_path(device: &str) -> Result<(), DaemonError> {
    if device.is_empty() {
        return Err(DaemonError::new(
            ErrorCode::BadRequest,
            "device path cannot be empty",
            false,
        ));
    }
    if device.len() > MAX_DEVICE_PATH_LEN {
        return Err(DaemonError::new(
            ErrorCode::BadRequest,
            "device path too long",
            false,
        ));
    }
    if !device.starts_with('/') {
        return Err(DaemonError::new(
            ErrorCode::BadRequest,
            "device path must be absolute",
            false,
        ));
    }
    if device.contains("..") {
        return Err(DaemonError::new(
            ErrorCode::BadRequest,
            "device path contains directory traversal",
            false,
        ));
    }
    Ok(())
}

pub fn validate_filesystem(filesystem: &Option<String>) -> Result<(), DaemonError> {
    if let Some(ref fs) = filesystem {
        if fs.is_empty() {
            return Err(DaemonError::new(
                ErrorCode::BadRequest,
                "filesystem type cannot be empty",
                false,
            ));
        }
        let valid_filesystems = [
            "ext4", "ext3", "ext2", "vfat", "exfat", "ntfs", "ntfs-3g", "f2fs", "xfs", "btrfs",
        ];
        if !valid_filesystems.contains(&fs.as_str()) {
            return Err(DaemonError::new(
                ErrorCode::BadRequest,
                "unsupported filesystem type",
                false,
            ));
        }
    }
    Ok(())
}
