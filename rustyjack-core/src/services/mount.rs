use std::process::Command;

use serde_json::Value;

use crate::services::error::ServiceError;

#[derive(Debug, Clone)]
pub struct BlockDeviceInfo {
    pub name: String,
    pub size: String,
    pub model: String,
    pub transport: String,
    pub removable: bool,
}

pub fn list_block_devices() -> Result<Vec<BlockDeviceInfo>, ServiceError> {
    let output = Command::new("lsblk")
        .args(["-J", "-p", "-o", "NAME,TYPE,RM,SIZE,MODEL,TRAN"])
        .output()
        .map_err(ServiceError::Io)?;
    if !output.status.success() {
        return Err(ServiceError::External(format!(
            "lsblk failed with status {:?}",
            output.status.code()
        )));
    }

    let parsed: Value = serde_json::from_slice(&output.stdout)
        .map_err(|err| ServiceError::External(format!("parsing lsblk JSON output: {err}")))?;
    let blockdevices = parsed
        .get("blockdevices")
        .and_then(|v| v.as_array())
        .ok_or_else(|| ServiceError::External("lsblk JSON missing blockdevices".to_string()))?;

    let mut devices = Vec::new();
    for dev in blockdevices {
        let dev_type = dev.get("type").and_then(Value::as_str).unwrap_or("");
        if dev_type != "disk" {
            continue;
        }
        let name = dev.get("name").and_then(Value::as_str).unwrap_or("");
        if name.is_empty() {
            continue;
        }

        if name.starts_with("/dev/mmcblk")
            || name.starts_with("/dev/loop")
            || name.starts_with("/dev/ram")
        {
            continue;
        }

        let removable = match dev.get("rm") {
            Some(Value::Bool(v)) => *v,
            Some(Value::Number(v)) => v.as_u64().unwrap_or(0) != 0,
            Some(Value::String(v)) => v == "1" || v.eq_ignore_ascii_case("true"),
            _ => false,
        };
        let size = dev
            .get("size")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let model = dev
            .get("model")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let transport = dev
            .get("tran")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();

        devices.push(BlockDeviceInfo {
            name: name.to_string(),
            size,
            model,
            transport,
            removable,
        });
    }

    Ok(devices)
}

#[derive(Debug, Clone)]
pub struct MountInfo {
    pub device: String,
    pub mountpoint: String,
    pub filesystem: String,
    pub size: String,
}

pub fn list_mounts() -> Result<Vec<MountInfo>, ServiceError> {
    use std::fs;
    
    let mounts = fs::read_to_string("/proc/mounts")
        .map_err(ServiceError::Io)?;
    
    let mut result = Vec::new();
    for line in mounts.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 3 {
            continue;
        }
        
        let device = parts[0];
        let mountpoint = parts[1];
        let filesystem = parts[2];
        
        if !device.starts_with("/dev/") {
            continue;
        }
        
        if device.starts_with("/dev/loop") || device.starts_with("/dev/ram") {
            continue;
        }
        
        result.push(MountInfo {
            device: device.to_string(),
            mountpoint: mountpoint.to_string(),
            filesystem: filesystem.to_string(),
            size: "".to_string(),
        });
    }
    
    Ok(result)
}

pub struct MountRequest {
    pub device: String,
    pub filesystem: Option<String>,
}

pub fn mount<F>(req: MountRequest, mut on_progress: F) -> Result<Value, ServiceError>
where
    F: FnMut(u8, &str),
{
    if req.device.trim().is_empty() {
        return Err(ServiceError::InvalidInput("device".to_string()));
    }
    
    if !req.device.starts_with("/dev/") {
        return Err(ServiceError::InvalidInput("device must start with /dev/".to_string()));
    }
    
    on_progress(10, "Preparing mount");
    
    let device_name = req.device.trim_start_matches("/dev/").replace('/', "_");
    let mountpoint = format!("/media/rustyjack/{}", device_name);
    
    std::fs::create_dir_all(&mountpoint)
        .map_err(ServiceError::Io)?;
    
    on_progress(50, "Mounting device");
    
    let mut cmd = Command::new("mount");
    cmd.arg(&req.device).arg(&mountpoint);
    
    if let Some(ref fs) = req.filesystem {
        cmd.arg("-t").arg(fs);
    }
    
    let output = cmd.output().map_err(ServiceError::Io)?;
    
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(ServiceError::External(format!("mount failed: {}", stderr)));
    }
    
    on_progress(100, "Mounted");
    
    Ok(serde_json::json!({
        "device": req.device,
        "mountpoint": mountpoint,
        "mounted": true
    }))
}

pub struct UnmountRequest {
    pub device: String,
}

pub fn unmount<F>(req: UnmountRequest, mut on_progress: F) -> Result<Value, ServiceError>
where
    F: FnMut(u8, &str),
{
    if req.device.trim().is_empty() {
        return Err(ServiceError::InvalidInput("device".to_string()));
    }
    
    on_progress(10, "Unmounting device");
    
    let output = Command::new("umount")
        .arg(&req.device)
        .output()
        .map_err(ServiceError::Io)?;
    
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(ServiceError::External(format!("umount failed: {}", stderr)));
    }
    
    on_progress(100, "Unmounted");
    
    Ok(serde_json::json!({
        "device": req.device,
        "unmounted": true
    }))
}
