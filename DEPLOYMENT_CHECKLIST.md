# Rustyjack Daemon Security Updates - Deployment Checklist

**Version:** Post-Stages 0-4  
**Date:** 2026-01-03  
**Target:** Raspberry Pi Zero 2 W / Raspberry Pi OS

---

## Pre-Deployment

### 1. Backup Current System
- [ ] Backup current binaries
  ```bash
  sudo cp /usr/local/bin/rustyjackd /backup/rustyjackd.old
  sudo cp /usr/local/bin/rustyjack-ui /backup/rustyjack-ui.old
  ```
- [ ] Backup systemd configurations
  ```bash
  sudo cp /etc/systemd/system/rustyjackd.service /backup/
  sudo cp /etc/systemd/system/rustyjack-ui.service /backup/
  ```
- [ ] Backup configuration files
  ```bash
  sudo tar -czf /backup/rustyjack-config-$(date +%Y%m%d).tar.gz /var/lib/rustyjack
  ```

### 2. Record Current State
- [ ] Document running services
  ```bash
  systemctl status rustyjackd rustyjack-ui > /backup/service-status-before.txt
  ```
- [ ] Document current users/groups
  ```bash
  groups rustyjack-ui > /backup/groups-before.txt
  getent group rustyjack >> /backup/groups-before.txt
  ```
- [ ] Test current functionality
  ```bash
  rustyjack-client version
  rustyjack-client status
  # Document output
  ```

---

## Build & Test

### 3. Build New Binaries
- [ ] Clean build
  ```bash
  cd /path/to/Rustyjack
  cargo clean
  cargo build --release --workspace 2>&1 | tee build.log
  ```
- [ ] Check for errors
  ```bash
  grep -i "error" build.log
  # Should be empty
  ```
- [ ] Run unit tests
  ```bash
  cd rustyjack-daemon
  cargo test validation::tests 2>&1 | tee validation-tests.log
  cargo test auth::tests 2>&1 | tee auth-tests.log
  ```
- [ ] Verify test results
  ```bash
  grep "test result:" *-tests.log
  # Should show: test result: ok. 19 passed (validation)
  # Should show: test result: ok. 11 passed (auth)
  ```

---

## Deployment

### 4. Stop Services
- [ ] Stop services gracefully
  ```bash
  sudo systemctl stop rustyjack-ui.service
  sudo systemctl stop rustyjackd.service
  ```
- [ ] Verify stopped
  ```bash
  sudo systemctl status rustyjackd rustyjack-ui
  # Should show: inactive (dead)
  ```

### 5. Deploy Binaries
- [ ] Install daemon
  ```bash
  sudo cp target/release/rustyjackd /usr/local/bin/
  sudo chmod +x /usr/local/bin/rustyjackd
  ```
- [ ] Install UI
  ```bash
  sudo cp target/release/rustyjack-ui /usr/local/bin/
  sudo chmod +x /usr/local/bin/rustyjack-ui
  ```
- [ ] Install client (if updated)
  ```bash
  sudo cp target/release/rustyjack-client /usr/local/bin/
  sudo chmod +x /usr/local/bin/rustyjack-client
  ```
- [ ] Verify binaries
  ```bash
  /usr/local/bin/rustyjackd --version
  /usr/local/bin/rustyjack-ui --version
  ```

---

## Configuration

### 6. Create Groups
- [ ] Create admin group
  ```bash
  sudo groupadd rustyjack-admin
  ```
- [ ] Add UI user to admin group
  ```bash
  sudo usermod -aG rustyjack-admin rustyjack-ui
  ```
- [ ] Verify group membership
  ```bash
  groups rustyjack-ui
  # Should include: rustyjack-admin
  ```

### 7. Update Systemd Configuration (Optional)
- [ ] Edit daemon service
  ```bash
  sudo nano /etc/systemd/system/rustyjackd.service
  ```
- [ ] Add timeout configuration (optional)
  ```ini
  [Service]
  Environment=RUSTYJACKD_READ_TIMEOUT_MS=5000
  Environment=RUSTYJACKD_WRITE_TIMEOUT_MS=5000
  ```
- [ ] Add group configuration (optional)
  ```ini
  Environment=RUSTYJACKD_ADMIN_GROUP=rustyjack-admin
  Environment=RUSTYJACKD_OPERATOR_GROUP=rustyjack
  ```
- [ ] Enable dangerous ops (if needed)
  ```ini
  Environment=RUSTYJACKD_DANGEROUS_OPS=true
  ```
- [ ] Reload systemd
  ```bash
  sudo systemctl daemon-reload
  ```

---

## Start Services

### 8. Start Daemon
- [ ] Start daemon
  ```bash
  sudo systemctl start rustyjackd.service
  ```
- [ ] Check status
  ```bash
  sudo systemctl status rustyjackd.service
  # Should show: active (running)
  ```
- [ ] Check logs for errors
  ```bash
  sudo journalctl -u rustyjackd.service -n 50
  # Look for ERROR or warnings
  ```

### 9. Start UI
- [ ] Start UI
  ```bash
  sudo systemctl start rustyjack-ui.service
  ```
- [ ] Check status
  ```bash
  sudo systemctl status rustyjack-ui.service
  # Should show: active (running)
  ```
- [ ] Check logs
  ```bash
  sudo journalctl -u rustyjack-ui.service -n 50
  ```

---

## Verification

### 10. Test Basic Functionality
- [ ] Test version endpoint
  ```bash
  rustyjack-client version
  # Should return version info
  ```
- [ ] Test health endpoint
  ```bash
  rustyjack-client health
  # Should return healthy status
  ```
- [ ] Test status endpoint
  ```bash
  rustyjack-client status
  # Should return daemon status
  ```

### 11. Test Feature Negotiation
- [ ] Check features in client logs (if verbose mode available)
  ```bash
  # Features should include:
  # - job_progress
  # - uds_timeouts
  # - group_based_auth
  # - dangerous_ops_enabled (if configured)
  ```

### 12. Test Authorization
- [ ] Test as root (should be Admin)
  ```bash
  sudo rustyjack-client system-reboot --dry-run
  # Should be allowed
  ```
- [ ] Test as UI user (should be Admin if in group)
  ```bash
  sudo -u rustyjack-ui rustyjack-client system-reboot --dry-run
  # Should be allowed (if rustyjack-ui in rustyjack-admin)
  ```
- [ ] Test as operator (should be Operator)
  ```bash
  # As user in rustyjack group but not rustyjack-admin
  rustyjack-client job-start mount --device /dev/sda1
  # Should be allowed
  
  rustyjack-client system-reboot
  # Should be FORBIDDEN
  ```

### 13. Test Timeouts (Optional)
- [ ] Test read timeout
  ```bash
  # Connect and stall (requires custom test client)
  # Daemon should disconnect after 5s
  ```
- [ ] Monitor timeout events
  ```bash
  sudo journalctl -u rustyjackd.service -f | grep -i timeout
  # Should show timeout if stall occurs
  ```

### 14. Test Job Operations
- [ ] Test sleep job
  ```bash
  rustyjack-client job-start sleep --seconds 5
  # Should complete successfully
  ```
- [ ] Test job cancellation
  ```bash
  JOB_ID=$(rustyjack-client job-start sleep --seconds 60)
  rustyjack-client job-cancel $JOB_ID
  rustyjack-client job-status $JOB_ID
  # Should show state: Cancelled
  ```
- [ ] Test WiFi scan (if hardware available)
  ```bash
  rustyjack-client job-start wifi-scan --interface wlan0
  # Should complete or fail gracefully
  ```

---

## Post-Deployment

### 15. Monitor Logs
- [ ] Watch daemon logs
  ```bash
  sudo journalctl -u rustyjackd.service -f
  ```
- [ ] Watch UI logs
  ```bash
  sudo journalctl -u rustyjack-ui.service -f
  ```
- [ ] Check for errors/warnings
  ```bash
  sudo journalctl -u rustyjackd.service --since today | grep -i error
  sudo journalctl -u rustyjackd.service --since today | grep -i warn
  ```

### 16. Document Issues
- [ ] Note any errors observed
- [ ] Document workarounds applied
- [ ] Report bugs to development team

### 17. Performance Check
- [ ] Check CPU usage
  ```bash
  top -p $(pgrep rustyjackd)
  top -p $(pgrep rustyjack-ui)
  ```
- [ ] Check memory usage
  ```bash
  ps aux | grep rustyjack
  ```
- [ ] Check for memory leaks (run over time)
  ```bash
  # Monitor memory over 24 hours
  ```

---

## Rollback (If Needed)

### 18. Rollback Procedure
- [ ] Stop services
  ```bash
  sudo systemctl stop rustyjack-ui rustyjackd
  ```
- [ ] Restore old binaries
  ```bash
  sudo cp /backup/rustyjackd.old /usr/local/bin/rustyjackd
  sudo cp /backup/rustyjack-ui.old /usr/local/bin/rustyjack-ui
  ```
- [ ] Restore old config
  ```bash
  sudo cp /backup/rustyjackd.service /etc/systemd/system/
  sudo systemctl daemon-reload
  ```
- [ ] Restart services
  ```bash
  sudo systemctl start rustyjackd rustyjack-ui
  ```
- [ ] Verify functionality
  ```bash
  rustyjack-client version
  rustyjack-client status
  ```

---

## Optional: Apply Hardening (Stage 5)

### 19. Test Hardened Configuration
- [ ] Deploy hardened systemd unit
  ```bash
  sudo cp rustyjackd.service.hardened /etc/systemd/system/rustyjackd.service
  sudo systemctl daemon-reload
  ```
- [ ] Restart with hardening
  ```bash
  sudo systemctl restart rustyjackd
  ```
- [ ] Test all operations
  ```bash
  # Test WiFi, mount, portal, etc.
  ```
- [ ] Check for permission denials
  ```bash
  sudo journalctl -u rustyjackd.service | grep -i denied
  sudo journalctl -u rustyjackd.service | grep -i permission
  ```
- [ ] Relax restrictions if needed
  ```bash
  # Edit service file, comment out problematic restrictions
  # Test incrementally
  ```

---

## Sign-Off

### 20. Deployment Complete
- [ ] All tests passed
- [ ] No critical errors in logs
- [ ] Performance acceptable
- [ ] Documentation updated
- [ ] Team notified

**Deployed by:** _______________________  
**Date:** _______________________  
**Version:** _______________________  
**Issues:** _______________________  

---

## Contact

**Issues or questions:**
1. Check logs: `sudo journalctl -u rustyjackd.service`
2. Review documentation: `docs/DEPLOYMENT_GUIDE.md`
3. Report bugs: (your bug tracking system)
