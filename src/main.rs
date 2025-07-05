use clap::Parser;
use serde_json;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{exit, Command, Stdio};

#[derive(Parser)]
#[command(name = "update-bin")]
#[command(about = "Update a binary to its latest version by using the original package manager")]
struct Args {
    bin_name: String,
    #[arg(long, help = "Display package name and package manager instead of updating")]
    info: bool,
}

fn main() {
    let args = Args::parse();

    if args.info {
        match display_info(&args.bin_name) {
            Ok(_) => {}
            Err(e) => {
                eprintln!("Error: {}", e);
                exit(1);
            }
        }
    } else {
        match update_binary(&args.bin_name) {
            Ok(_) => {}
            Err(e) => {
                eprintln!("Error: {}", e);
                exit(1);
            }
        }
    }
}

fn display_info(bin_name: &str) -> Result<(), String> {
    let package_manager = detect_package_manager(bin_name)?;
    println!("Package name: {}", package_manager.package_name);
    println!("Package manager: {}", package_manager.name);
    Ok(())
}

fn update_binary(bin_name: &str) -> Result<(), String> {
    let package_manager = detect_package_manager(bin_name)?;

    let old_version =
        get_version(bin_name, &package_manager).unwrap_or_else(|_| "unknown".to_string());
    println!("Current version: {}", old_version);

    let (command, args) = get_update_command(&package_manager.name, &package_manager.package_name)?;

    println!(
        "Updating {} with {}",
        package_manager.package_name, package_manager.name
    );

    let mut child = Command::new(&command)
        .args(&args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to run {}: {}", command, e))?;

    if let Some(stdout) = child.stdout.take() {
        let reader = BufReader::new(stdout);
        for line in reader.lines() {
            if let Ok(line) = line {
                println!("\x1b[2m---> {}\x1b[0m", line);
            }
        }
    }

    if let Some(stderr) = child.stderr.take() {
        let reader = BufReader::new(stderr);
        for line in reader.lines() {
            if let Ok(line) = line {
                eprintln!("\x1b[2m---> {}\x1b[0m", line);
            }
        }
    }

    let status = child
        .wait()
        .map_err(|e| format!("Failed to wait for {}: {}", command, e))?;

    if !status.success() {
        return Err(format!(
            "Failed to update {} with {}",
            package_manager.package_name, package_manager.name
        ));
    }

    let new_version =
        get_version(&bin_name, &package_manager).unwrap_or_else(|_| "unknown".to_string());

    if old_version != new_version {
        println!("Updated to version: {}", new_version);
        println!(
            "✅ Successfully updated {} from {} to {}",
            package_manager.package_name, old_version, new_version
        );
    } else {
        println!(
            "ℹ️  {} is already up to date ({})",
            package_manager.package_name, old_version
        );
    }

    Ok(())
}

struct PackageManager {
    name: String,
    package_name: String,
}

fn detect_package_manager(bin_name: &str) -> Result<PackageManager, String> {
    if let Ok(output) = Command::new("which").arg(bin_name).output() {
        if !output.status.success() {
            return Err(format!("Binary '{}' not found", bin_name));
        }

        let bin_path_raw = String::from_utf8_lossy(&output.stdout);
        let bin_path = bin_path_raw.trim();

        if bin_path.contains("/opt/homebrew/") || bin_path.contains("/usr/local/") {
            return Ok(PackageManager {
                name: "homebrew".to_string(),
                package_name: bin_name.to_string(),
            });
        }

        if bin_path.contains("/.bun/") {
            return Ok(PackageManager {
                name: "bun".to_string(),
                package_name: map_bin_name_to_bun_package_name(bin_name),
            });
        }

        if bin_path.contains("/.cargo/bin/") {
            return Ok(PackageManager {
                name: "cargo".to_string(),
                package_name: bin_name.to_string(),
            });
        }

        // check if installed by pnpm
        let global_bin_dir = Command::new("pnpm")
            .args(&["bin", "-g"])
            .output()
            .ok()
            .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string());
        if let Some(dir) = global_bin_dir {
            if bin_path.contains(&dir) {
                return Ok(PackageManager {
                    name: "pnpm".to_string(),
                    package_name: map_bin_name_to_pnpm_package_name(bin_name),
                });
            }
        }

        // get npm binary path by running `which npm` and get its directory
        let npm_bin_path = Command::new("which")
            .arg("npm")
            .output()
            .ok()
            .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string());
        let npm_bin_dir = npm_bin_path.as_ref().map(|path| {
            let mut parts: Vec<&str> = path.split('/').collect();
            parts.pop();
            parts.join("/")
        });

        if let Some(dir) = npm_bin_dir {
            if bin_path.contains(&dir) {
                let global_node_modules_dir = Path::new(&dir)
                    .parent()
                    .unwrap()
                    .join("lib")
                    .join("node_modules")
                    .to_string_lossy()
                    .to_string();

                return Ok(PackageManager {
                    name: "npm".to_string(),
                    package_name: map_bin_name_to_npm_package_name(
                        bin_name,
                        &global_node_modules_dir,
                    ),
                });
            }
        }

        // check if installed by yarn
        let yarn_bin_dir = Command::new("yarn")
            .args(&["global", "bin"])
            .output()
            .ok()
            .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string());
        if let Some(dir) = yarn_bin_dir {
            if bin_path.contains(&dir) {
                return Ok(PackageManager {
                    name: "yarn".to_string(),
                    package_name: map_bin_name_to_yarn_package_name(bin_name),
                });
            }
        }
    }

    Err(format!(
        "Could not detect package manager for '{}'",
        bin_name
    ))
}

fn get_update_command(
    package_manager: &str,
    package_name: &str,
) -> Result<(String, Vec<String>), String> {
    match package_manager {
        "homebrew" => Ok((
            "brew".to_string(),
            vec!["upgrade".to_string(), package_name.to_string()],
        )),
        "bun" => Ok((
            "bun".to_string(),
            vec![
                "update".to_string(),
                "-g".to_string(),
                package_name.to_string(),
            ],
        )),
        "npm" => Ok((
            "npm".to_string(),
            vec![
                "update".to_string(),
                "-g".to_string(),
                package_name.to_string(),
            ],
        )),
        "pnpm" => Ok((
            "pnpm".to_string(),
            vec![
                "update".to_string(),
                "-g".to_string(),
                package_name.to_string(),
            ],
        )),
        "cargo" => Ok((
            "cargo".to_string(),
            vec!["install".to_string(), package_name.to_string()],
        )),
        "yarn" => Ok((
            "yarn".to_string(),
            vec![
                "global".to_string(),
                "upgrade".to_string(),
                package_name.to_string(),
            ],
        )),
        _ => Err(format!("Unsupported package manager: {}", package_manager)),
    }
}

fn get_version(bin_name: &str, package_manager: &PackageManager) -> Result<String, String> {
    match package_manager.name.to_string().as_str() {
        "homebrew" => get_homebrew_version(bin_name),
        "bun" | "npm" | "pnpm" => get_node_package_version(bin_name, package_manager),
        "cargo" => get_cargo_version(bin_name),
        _ => get_binary_version(bin_name),
    }
}

fn get_homebrew_version(bin_name: &str) -> Result<String, String> {
    let output = Command::new("brew")
        .args(&["list", "--versions", bin_name])
        .output()
        .map_err(|e| format!("Failed to get brew version: {}", e))?;

    if !output.status.success() {
        return Err("Package not found in homebrew".to_string());
    }

    let version_line = String::from_utf8_lossy(&output.stdout);
    let version = version_line
        .trim()
        .split_whitespace()
        .nth(1)
        .unwrap_or("unknown")
        .to_string();

    Ok(version)
}

fn get_node_package_version(
    bin_name: &str,
    package_manager: &PackageManager,
) -> Result<String, String> {
    let output = Command::new(&package_manager.name)
        .args(&["list", "-g", "--depth=0"])
        .output()
        .map_err(|e| format!("Failed to get {} version: {}", package_manager.name, e))?;

    if !output.status.success() {
        return get_binary_version(bin_name);
    }

    let list_output = String::from_utf8_lossy(&output.stdout);
    for line in list_output.lines() {
        if line.contains(&format!("{}@", package_manager.package_name)) {
            let version = line
                .split(&format!("{}@", package_manager.package_name))
                .nth(1)
                .unwrap_or("unknown")
                .trim()
                .to_string();
            return Ok(version);
        }
    }

    get_binary_version(bin_name)
}

fn get_cargo_version(bin_name: &str) -> Result<String, String> {
    let output = Command::new("cargo")
        .args(&["install", "--list"])
        .output()
        .map_err(|e| format!("Failed to get cargo version: {}", e))?;

    if !output.status.success() {
        return get_binary_version(bin_name);
    }

    let list_output = String::from_utf8_lossy(&output.stdout);
    for line in list_output.lines() {
        if line.starts_with(&format!("{} ", bin_name)) {
            let version = line
                .split_whitespace()
                .nth(1)
                .and_then(|v| v.strip_prefix("v"))
                .unwrap_or("unknown")
                .trim_end_matches(':')
                .to_string();
            return Ok(version);
        }
    }

    get_binary_version(bin_name)
}

fn get_binary_version(bin_name: &str) -> Result<String, String> {
    let version_flags = ["--version", "-v", "-V", "version"];

    for flag in &version_flags {
        if let Ok(output) = Command::new(bin_name).arg(flag).output() {
            if output.status.success() {
                let version_output = String::from_utf8_lossy(&output.stdout);
                let version = version_output
                    .lines()
                    .next()
                    .unwrap_or("unknown")
                    .trim()
                    .to_string();
                return Ok(version);
            }
        }
    }

    Err("Could not determine version".to_string())
}

// an npm package can be installed as another name other than its package name to the bin directory
// so we need to scan all packages and use the "bin" field (string or object) to determine the actual package name by the bin name
fn map_bin_name_to_npm_package_name(bin_name: &str, global_node_modules_dir: &str) -> String {
    let global_json_content = Command::new("npm")
        .args(&["list", "-g", "--json", "--depth=0"])
        .output()
        .ok()
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string());
    if let Some(global_json_content) = global_json_content {
        let global_json: serde_json::Value =
            serde_json::from_str(&global_json_content).unwrap_or_default();

        let empty_map = serde_json::Map::new();
        let packages = global_json["dependencies"]
            .as_object()
            .unwrap_or(&empty_map);
        for (package_name, _) in packages {
            let package_json_path =
                format!("{}/{}/package.json", global_node_modules_dir, package_name);
            let package_json = std::fs::read_to_string(package_json_path).unwrap_or_default();
            let package_json: serde_json::Value =
                serde_json::from_str(&package_json).unwrap_or_default();
            if let Some(bin) = package_json.get("bin") {
                if bin.is_string() && bin.as_str() == Some(bin_name) {
                    return package_name.to_string();
                }
                if bin.is_object() {
                    for (bin_name_in_json, _) in bin.as_object().unwrap() {
                        if bin_name_in_json == bin_name {
                            return package_name.to_string();
                        }
                    }
                }
            }
        }
    }

    bin_name.to_string()
}

// Similar to map_bin_name_to_npm_package_name but for pnpm
fn map_bin_name_to_pnpm_package_name(bin_name: &str) -> String {
    let global_json_content = Command::new("pnpm")
        .args(&["list", "-g", "--json"])
        .output()
        .ok()
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string());
    if let Some(global_json_content) = global_json_content {
        let global_json: serde_json::Value =
            serde_json::from_str(&global_json_content).unwrap_or_default();

        if let Some(global_array) = global_json.as_array() {
            for global_object in global_array {
                let empty_map = serde_json::Map::new();
                let packages = global_object["dependencies"]
                    .as_object()
                    .unwrap_or(&empty_map);
                for (package_name, package_info) in packages {
                    // Use the path from the package info to find package.json
                    if let Some(package_path) = package_info["path"].as_str() {
                        let package_json_path = format!("{}/package.json", package_path);
                        let package_json =
                            std::fs::read_to_string(package_json_path).unwrap_or_default();
                        let package_json: serde_json::Value =
                            serde_json::from_str(&package_json).unwrap_or_default();
                        if let Some(bin) = package_json.get("bin") {
                            if bin.is_string() && bin.as_str() == Some(bin_name) {
                                return package_name.to_string();
                            }
                            if bin.is_object() {
                                for (bin_name_in_json, _) in bin.as_object().unwrap() {
                                    if bin_name_in_json == bin_name {
                                        return package_name.to_string();
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    bin_name.to_string()
}

// Similar to map_bin_name_to_npm_package_name but for yarn
fn map_bin_name_to_yarn_package_name(bin_name: &str) -> String {
    let yarn_global_dir = Command::new("yarn")
        .args(&["global", "dir"])
        .output()
        .ok()
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string());
    if let Some(global_dir) = yarn_global_dir {
        let package_json_path = format!("{}/package.json", global_dir);
        let package_json_content = std::fs::read_to_string(package_json_path).unwrap_or_default();
        let package_json: serde_json::Value =
            serde_json::from_str(&package_json_content).unwrap_or_default();

        let empty_map = serde_json::Map::new();
        let packages = package_json["dependencies"]
            .as_object()
            .unwrap_or(&empty_map);
        for (package_name, _) in packages {
            let node_modules_dir = format!("{}/node_modules", global_dir);
            let package_json_path = format!("{}/{}/package.json", node_modules_dir, package_name);
            let package_json = std::fs::read_to_string(package_json_path).unwrap_or_default();
            let package_json: serde_json::Value =
                serde_json::from_str(&package_json).unwrap_or_default();
            if let Some(bin) = package_json.get("bin") {
                if bin.is_string() && bin.as_str() == Some(bin_name) {
                    return package_name.to_string();
                }
                if bin.is_object() {
                    for (bin_name_in_json, _) in bin.as_object().unwrap() {
                        if bin_name_in_json == bin_name {
                            return package_name.to_string();
                        }
                    }
                }
            }
        }
    }

    bin_name.to_string()
}

// Similar to map_bin_name_to_npm_package_name but for bun
fn map_bin_name_to_bun_package_name(bin_name: &str) -> String {
    // Bun's global directory is typically ~/.bun/install/global
    let bun_global_dir = std::env::var("HOME")
        .map(|home| format!("{}/.bun/install/global", home))
        .unwrap_or_else(|_| "~/.bun/install/global".to_string());
    
    let package_json_path = format!("{}/package.json", bun_global_dir);
    let package_json_content = std::fs::read_to_string(package_json_path).unwrap_or_default();
    let package_json: serde_json::Value =
        serde_json::from_str(&package_json_content).unwrap_or_default();

    let empty_map = serde_json::Map::new();
    let packages = package_json["dependencies"]
        .as_object()
        .unwrap_or(&empty_map);
    for (package_name, _) in packages {
        let node_modules_dir = format!("{}/node_modules", bun_global_dir);
        let package_json_path = format!("{}/{}/package.json", node_modules_dir, package_name);
        let package_json = std::fs::read_to_string(package_json_path).unwrap_or_default();
        let package_json: serde_json::Value =
            serde_json::from_str(&package_json).unwrap_or_default();
        if let Some(bin) = package_json.get("bin") {
            if bin.is_string() && bin.as_str() == Some(bin_name) {
                return package_name.to_string();
            }
            if bin.is_object() {
                for (bin_name_in_json, _) in bin.as_object().unwrap() {
                    if bin_name_in_json == bin_name {
                        return package_name.to_string();
                    }
                }
            }
        }
    }

    bin_name.to_string()
}