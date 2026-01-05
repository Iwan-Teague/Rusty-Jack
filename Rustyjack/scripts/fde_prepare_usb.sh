#!/usr/bin/env bash
# Prepare a removable USB device as a Rustyjack full-disk-encryption key.
# - Verifies removable USB, refuses to touch the boot/root device.
# - Wipes the device, creates a single FAT32 partition, and writes rustyjack.key (64 hex chars).
# - Prints status to stdout; exits non-zero on error.

set -euo pipefail

log() { echo "[fde] $*"; }
err() { echo "[fde][error] $*" >&2; exit 1; }

require_cmd() {
  command -v "$1" >/dev/null 2>&1 || err "Missing dependency: $1"
}

if [[ ${EUID:-$(id -u)} -ne 0 ]]; then
  err "Run as root"
fi

require_cmd lsblk
require_cmd wipefs
require_cmd sfdisk
require_cmd mkfs.vfat
require_cmd findmnt
require_cmd xxd

DEV=${1:-}
if [[ -z "$DEV" ]]; then
  err "Usage: $0 /dev/sdX"
fi

if [[ ! -b "$DEV" ]]; then
  err "Device $DEV not found or not a block device"
fi

type=$(lsblk -no TYPE "$DEV" 2>/dev/null || true)
if [[ "$type" != "disk" ]]; then
  err "$DEV is not a whole disk device (type=$type)"
fi

removable=$(lsblk -nrpo NAME,RM "$DEV" | awk '{print $2}')
if [[ "$removable" != "1" ]]; then
  err "$DEV is not marked as removable"
fi

transport=$(lsblk -nrpo NAME,TRAN "$DEV" | awk '{print $2}')
if [[ "$transport" != "usb" ]]; then
  err "$DEV is not a USB device (TRAN=$transport)"
fi

root_src=$(findmnt -n -o SOURCE /)
root_base=$(lsblk -no PKNAME "$root_src" 2>/dev/null || basename "$root_src")
dev_base=$(basename "$DEV")
if [[ "$dev_base" == "$root_base" ]]; then
  err "$DEV appears to host the root filesystem; refusing"
fi

log "Selected USB: $DEV (removable usb)"

# Unmount any child partitions
while read -r part; do
  if mountpoint -q "$part"; then
    log "Unmounting $part"
    umount "$part" || err "Failed to unmount $part"
  fi
done < <(lsblk -nrpo NAME "$DEV" | tail -n +2)

log "Wiping signatures on $DEV"
wipefs -a "$DEV"

log "Creating single GPT partition"
printf 'label: gpt\n,;\n' | sfdisk --wipe=always "$DEV" >/dev/null

part="${DEV}"
if [[ "$DEV" =~ [0-9]$ ]]; then
  part="${DEV}p1"
else
  part="${DEV}1"
fi

# Wait for partition node
for _ in {1..10}; do
  [[ -b "$part" ]] && break
  sleep 0.3
done
[[ -b "$part" ]] || err "Partition $part not found after creation"

log "Formatting $part as FAT32"
mkfs.vfat -F32 -n RUSTYKEY "$part" >/dev/null

mnt=$(mktemp -d /tmp/rustyjack_fde.XXXXXX)
cleanup() {
  if mountpoint -q "$mnt"; then
    umount "$mnt" || true
  fi
  rmdir "$mnt" 2>/dev/null || true
}
trap cleanup EXIT

log "Mounting $part at $mnt"
mount "$part" "$mnt"

log "Generating keyfile"
KEY_HEX=$(head -c 32 /dev/urandom | xxd -p -c 64)
echo "$KEY_HEX" >"$mnt/rustyjack.key"
sync

log "Key written to $mnt/rustyjack.key (64 hex chars)"

cleanup
log "USB prepared successfully"
