#!/usr/bin/env bash
# Migrate an existing root filesystem into a LUKS container using a keyfile.
# This script is intentionally cautious: it defaults to --dry-run and requires
# --execute to perform destructive steps.
#
# Steps:
# 1) Preflight: root user, required tools, target block device unused, keyfile exists.
# 2) Optionally: LUKS format target, open mapper, mkfs.ext4.
# 3) Copy root into the new filesystem (excludes /boot, /proc, /sys, /dev, /run, /tmp, /mnt, /media, /lost+found, /var/run, /var/tmp).
#    Progress is emitted as "PROGRESS <percent>" via pv.
# 4) Leaves mapper mounted; caller must handle fstab/crypttab/initramfs updates.

set -euo pipefail

log() { echo "[fde-migrate] $*"; }
err() { echo "[fde-migrate][error] $*" >&2; exit 1; }

require_cmd() {
  command -v "$1" >/dev/null 2>&1 || err "Missing dependency: $1"
}

usage() {
  cat <<'EOF'
Usage: fde_migrate_root.sh --target /dev/mmcblk0p3 --keyfile /path/to/rustyjack.key [--execute]

Flags:
  --target   Block device/partition for the new encrypted root (must be unused)
  --keyfile  Path to 32-byte key (raw or 64 hex chars)
  --execute  Actually perform the migration (otherwise dry-run)
EOF
  exit 1
}

TARGET=""
KEYFILE=""
EXECUTE=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --target) TARGET="$2"; shift 2 ;;
    --keyfile) KEYFILE="$2"; shift 2 ;;
    --execute) EXECUTE=1; shift ;;
    *) usage ;;
  esac
done

[[ -z "$TARGET" || -z "$KEYFILE" ]] && usage

if [[ ${EUID:-$(id -u)} -ne 0 ]]; then
  err "Run as root"
fi

require_cmd lsblk
require_cmd cryptsetup
require_cmd pv
require_cmd tar
require_cmd mkfs.ext4
require_cmd mount
require_cmd umount

if [[ ! -b "$TARGET" ]]; then
  err "Target $TARGET is not a block device"
fi

if findmnt -rn "$TARGET" >/dev/null 2>&1; then
  err "Target $TARGET is currently mounted"
fi

root_src=$(findmnt -n -o SOURCE /)
root_base=$(lsblk -no PKNAME "$root_src" 2>/dev/null || basename "$root_src")
target_base=$(lsblk -no PKNAME "$TARGET" 2>/dev/null || basename "$TARGET")
if [[ "$root_base" == "$target_base" ]]; then
  err "Target appears to be on the same device as root; aborting to avoid self-destruction"
fi

if [[ ! -f "$KEYFILE" ]]; then
  err "Keyfile $KEYFILE not found"
fi

KEY_LEN=$(stat -c%s "$KEYFILE" 2>/dev/null || stat -f%z "$KEYFILE")
if [[ "$KEY_LEN" -ne 32 && "$KEY_LEN" -ne 64 ]]; then
  err "Keyfile must be 32 raw bytes or 64 hex chars; got $KEY_LEN bytes"
fi

MAPPER="rjack_root_enc"
MNT=$(mktemp -d /tmp/rjack_encroot.XXXXXX)
cleanup() {
  if mountpoint -q "$MNT"; then umount "$MNT" || true; fi
  if [[ -e "/dev/mapper/$MAPPER" ]]; then cryptsetup close "$MAPPER" || true; fi
  rmdir "$MNT" 2>/dev/null || true
}
trap cleanup EXIT

log "Preflight OK (target=$TARGET, keyfile=$KEYFILE, execute=$EXECUTE)"

if [[ $EXECUTE -eq 0 ]]; then
  log "Dry run only. No changes made."
  exit 0
fi

log "Formatting $TARGET as LUKS (this will destroy data)"
cryptsetup luksFormat "$TARGET" "$KEYFILE" --type luks2
cryptsetup open "$TARGET" "$MAPPER" --key-file "$KEYFILE"

log "Creating ext4 filesystem on /dev/mapper/$MAPPER"
mkfs.ext4 -L RUSTYROOT "/dev/mapper/$MAPPER"

log "Mounting new root at $MNT"
mount "/dev/mapper/$MAPPER" "$MNT"

log "Estimating data size for progress..."
TOTAL_BYTES=$(du -sb \
  --exclude=/boot \
  --exclude=/proc \
  --exclude=/sys \
  --exclude=/dev \
  --exclude=/run \
  --exclude=/tmp \
  --exclude=/mnt \
  --exclude=/media \
  --exclude=/lost+found \
  / | awk '{print $1}')

log "Copying root into encrypted filesystem (this can take a while)"
tar cf - \
  --one-file-system \
  --exclude=/boot \
  --exclude=/proc \
  --exclude=/sys \
  --exclude=/dev \
  --exclude=/run \
  --exclude=/tmp \
  --exclude=/mnt \
  --exclude=/media \
  --exclude=/lost+found \
  / \
  | pv -n -s "$TOTAL_BYTES" \
  | tar xf - -C "$MNT" >/dev/null 2> >(while read -r p; do echo "PROGRESS $p"; done)

sync
log "Copy complete. New root is at $MNT (mapper=$MAPPER)."
log "Next steps (manual): add crypttab/fstab entries and rebuild initramfs to boot from encrypted root."
