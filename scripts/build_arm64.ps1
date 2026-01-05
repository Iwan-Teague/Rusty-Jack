#!/usr/bin/env pwsh
$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$RepoRoot = Split-Path -Parent $ScriptDir
& "$RepoRoot\tests\compile\build_arm64.ps1" $args
