# URGENT FIX: Docker Desktop API Version Issue on Windows

## Your Specific Problem

You're getting:
```
request returned 500 Internal Server Error for API route and version
http://%2F%2F.%2Fpipe%2FdockerDesktopLinuxEngine/v1.52/containers/json, check if the server supports the requested API version
```

This means Docker Desktop is running BUT it's having communication issues.

## Quick Fix (2 minutes)

### Option 1: Restart Docker Desktop (Usually Works)

1. **Quit Docker Desktop completely**:
   - Right-click the Docker whale icon in system tray (bottom-right of screen)
   - Click "Quit Docker Desktop"
   - Wait 10 seconds

2. **Start Docker Desktop again**:
   - Search for "Docker Desktop" in Windows Start menu
   - Launch it
   - Wait until the whale icon is solid/green (not animated)
   - This takes 30-60 seconds

3. **Run your build again**:
   ```powershell
   cd C:\Users\teagu\Desktop\Rustyjack
   .\scripts\build_arm32.ps1
   ```

### Option 2: Restart Windows (If Option 1 Doesn't Work)

Sometimes Docker gets into a bad state. A full restart fixes it:
1. Quit Docker Desktop
2. Restart Windows
3. Start Docker Desktop
4. Run build script

## Why This Happens

Docker Desktop on Windows uses named pipes to communicate between the CLI and the Docker daemon. Sometimes the pipe gets into a bad state, causing the 500 error. Restarting Docker Desktop clears this.

## Updated Scripts

The build scripts have been updated to:
1. Check if Docker is responsive (5-second timeout)
2. Provide clear error messages
3. Suggest restarting Docker Desktop

If the scripts still fail after restarting Docker Desktop, the error message will guide you through additional troubleshooting steps.

## Testing Docker

After restarting Docker Desktop, verify it's working:
```powershell
docker version
```

Should output both Client and Server versions without errors.

If this hangs or errors, Docker Desktop isn't fully started yet - wait a bit longer.

## What Changed in the Scripts

- Replaced `docker ps` / `docker info` (which hang on API errors) with `docker version` (more reliable)
- Added 5-second timeout to avoid infinite hangs
- Added specific instructions for restarting Docker Desktop
- Scripts now fail fast with helpful error messages instead of hanging

## Still Having Issues?

See the full troubleshooting guide: `DOCKER_WINDOWS_SETUP.md`

## TL;DR

```powershell
# 1. Right-click Docker whale icon → Quit Docker Desktop
# 2. Wait 10 seconds
# 3. Start Docker Desktop from Start menu
# 4. Wait for green whale icon  
# 5. Run:
cd C:\Users\teagu\Desktop\Rustyjack
.\scripts\build_arm32.ps1
```

That's it!
