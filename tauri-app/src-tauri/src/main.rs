// Heliox OS — AI System Control Agent
// Tauri v2 application entry point

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod hotkey;
mod tray;

use std::process::{Child, Command};
use std::sync::Mutex;
use tauri::Manager;

/// Global handle to the Python daemon process so we can kill it on exit.
struct DaemonProcess(Mutex<Option<Child>>);

fn get_app_data_dir() -> std::path::PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
    home.join(".heliox-os")
}

fn get_venv_python() -> std::path::PathBuf {
    let venv_dir = get_app_data_dir().join("env");
    #[cfg(target_os = "windows")]
    {
        venv_dir.join("Scripts").join("python.exe")
    }
    #[cfg(not(target_os = "windows"))]
    {
        venv_dir.join("bin").join("python3")
    }
}

fn spawn_daemon() -> Option<Child> {
    let data_dir = get_app_data_dir();
    if !data_dir.exists() {
        let _ = std::fs::create_dir_all(&data_dir);
    }
    
    let python_exe = get_venv_python();
    
    // First-run setup: if the virtualenv python doesn't exist, build it
    if !python_exe.exists() {
        println!("[Heliox OS] First run detected. Creating Python virtual environment...");
        let venv_dir = data_dir.join("env");
        
        // 1. Create VirtualEnv
        let mut venv_cmd = Command::new("python");
        #[cfg(not(target_os = "windows"))]
        let mut venv_cmd = Command::new("python3");
        
        // Suppress window popup on windows for the setup script
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            venv_cmd.creation_flags(0x08000000);
        }

        let status = venv_cmd.args(["-m", "venv", venv_dir.to_str().unwrap()]).status();
        if status.is_err() || !status.unwrap().success() {
            eprintln!("[Heliox OS] Error: Failed to create virtual environment. Is Python installed?");
            return None;
        }

        // 2. Install pilot-daemon
        println!("[Heliox OS] Installing AI backend into virtual environment...");
        let mut pip_exe = venv_dir.join("bin").join("pip");
        #[cfg(target_os = "windows")]
        { pip_exe = venv_dir.join("Scripts").join("pip.exe"); }

        let mut pip_cmd = Command::new(&pip_exe);
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            pip_cmd.creation_flags(0x08000000);
        }
        
        let install_status = pip_cmd
            .args(["install", "pilot-daemon"])
            .status();

        if install_status.is_err() || !install_status.unwrap().success() {
            eprintln!("[Heliox OS] Error: Failed to install pilot-daemon.");
            return None;
        }
        println!("[Heliox OS] First run setup complete.");
    }
    
    // 3. Boot the daemon using the isolated virtual environment safely
    let mut cmd = Command::new(&python_exe);
    cmd.args(["-m", "pilot.server"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }

    let child = cmd.spawn().ok();

    if child.is_some() {
        println!("[Heliox OS] AI daemon spawned successfully from virtualenv");
    } else {
        eprintln!("[Heliox OS] Warning: Could not spawn Python daemon.");
    }

    child
}

fn main() {
    // Spawn the Python daemon before building the Tauri app
    let daemon_child = spawn_daemon();

    tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_shell::init())
        .manage(DaemonProcess(Mutex::new(daemon_child)))
        .setup(|app| {
            let window = app.get_webview_window("main").unwrap();
            
            // Show the window when the user starts the app, rather than hiding it
            window.show().unwrap();
            window.set_focus().unwrap();

            tray::setup_tray(app)?;
            hotkey::register_hotkey(app)?;

            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::Destroyed = event {
                // Kill the daemon when the app window is destroyed
                if let Some(state) = window.try_state::<DaemonProcess>() {
                    if let Ok(mut guard) = state.0.lock() {
                        if let Some(ref mut child) = *guard {
                            let _ = child.kill();
                            println!("[Heliox OS] Python daemon stopped");
                        }
                    }
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::toggle_window,
            commands::get_daemon_status,
            commands::send_to_daemon,
            commands::confirm_action,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Heliox OS");
}
