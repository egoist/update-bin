use clap::Parser;
use serde_json;
use std::env;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
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

fn find_binary_path(bin_name: &str) -> Result<String, String> {
    let command = if cfg!(target_os = "windows") {
        "where"
    } else {
        "which"
    };
    
    if let Ok(output) = Command::new(command).arg(bin_name).output() {
        if output.status.success() {
            let bin_path_raw = String::from_utf8_lossy(&output.stdout);
            let bin_path = bin_path_raw.trim();
            
            // On Windows, 'where' can return multiple paths, so we take the first one
            let first_path = bin_path.lines().next().unwrap_or(bin_path);
            return Ok(first_path.to_string());
        }
    }
    
    Err(format!("Binary '{}' not found", bin_name))
}

fn detect_package_manager(bin_name: &str) -> Result<PackageManager, String> {
    let bin_path = find_binary_path(bin_name)?;

    // Normalize path separators for comparison
    let normalized_path = bin_path.replace('\\', "/");

    // Check for Homebrew (macOS/Linux only)
    if !cfg!(target_os = "windows") && (normalized_path.contains("/opt/homebrew/") || normalized_path.contains("/usr/local/")) {
        return Ok(PackageManager {
            name: "homebrew".to_string(),
            package_name: map_bin_name_to_homebrew_package_name(bin_name),
        });
    }

    // Check for Bun
    if normalized_path.contains("/.bun/") || (cfg!(target_os = "windows") && normalized_path.to_lowercase().contains("\\appdata\\roaming\\bun\\")) {
        return Ok(PackageManager {
            name: "bun".to_string(),
            package_name: map_bin_name_to_bun_package_name(bin_name),
        });
    }

    // Check for Cargo
    if normalized_path.contains("/.cargo/bin/") || (cfg!(target_os = "windows") && normalized_path.to_lowercase().contains("\\.cargo\\bin\\")) {
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
        let normalized_dir = dir.replace('\\', "/");
        if normalized_path.contains(&normalized_dir) {
            return Ok(PackageManager {
                name: "pnpm".to_string(),
                package_name: map_bin_name_to_pnpm_package_name(bin_name),
            });
        }
    }

    // get npm binary path and get its directory
    if let Ok(npm_bin_path) = find_binary_path("npm") {
        let npm_path = PathBuf::from(&npm_bin_path);
        if let Some(npm_bin_dir) = npm_path.parent() {
            let npm_bin_dir_str = npm_bin_dir.to_string_lossy();
            let normalized_npm_dir = npm_bin_dir_str.replace('\\', "/");
            
            if normalized_path.contains(&normalized_npm_dir) {
                let global_node_modules_dir = npm_bin_dir
                    .parent()
                    .and_then(|p| {
                        if cfg!(target_os = "windows") {
                            Some(p.join("node_modules"))
                        } else {
                            Some(p.join("lib").join("node_modules"))
                        }
                    })
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_default();

                return Ok(PackageManager {
                    name: "npm".to_string(),
                    package_name: map_bin_name_to_npm_package_name(
                        bin_name,
                        &global_node_modules_dir,
                    ),
                });
            }
        }
    }

    // check if installed by yarn
    let yarn_bin_dir = Command::new("yarn")
        .args(&["global", "bin"])
        .output()
        .ok()
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string());
    if let Some(dir) = yarn_bin_dir {
        let normalized_dir = dir.replace('\\', "/");
        if normalized_path.contains(&normalized_dir) {
            return Ok(PackageManager {
                name: "yarn".to_string(),
                package_name: map_bin_name_to_yarn_package_name(bin_name),
            });
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
            let package_json_path = if cfg!(target_os = "windows") {
                format!("{}\\{}\\package.json", global_node_modules_dir, package_name)
            } else {
                format!("{}/{}/package.json", global_node_modules_dir, package_name)
            };
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
                        let package_json_path = if cfg!(target_os = "windows") {
                            format!("{}\\package.json", package_path)
                        } else {
                            format!("{}/package.json", package_path)
                        };
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
        let package_json_path = if cfg!(target_os = "windows") {
            format!("{}\\package.json", global_dir)
        } else {
            format!("{}/package.json", global_dir)
        };
        let package_json_content = std::fs::read_to_string(package_json_path).unwrap_or_default();
        let package_json: serde_json::Value =
            serde_json::from_str(&package_json_content).unwrap_or_default();

        let empty_map = serde_json::Map::new();
        let packages = package_json["dependencies"]
            .as_object()
            .unwrap_or(&empty_map);
        for (package_name, _) in packages {
            let node_modules_dir = if cfg!(target_os = "windows") {
                format!("{}\\node_modules", global_dir)
            } else {
                format!("{}/node_modules", global_dir)
            };
            let package_json_path = if cfg!(target_os = "windows") {
                format!("{}\\{}\\package.json", node_modules_dir, package_name)
            } else {
                format!("{}/{}/package.json", node_modules_dir, package_name)
            };
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
    // Bun's global directory is typically ~/.bun/install/global on Unix, %APPDATA%\bun on Windows
    let bun_global_dir = if cfg!(target_os = "windows") {
        env::var("APPDATA")
            .map(|appdata| format!("{}\\bun", appdata))
            .unwrap_or_else(|_| "~\\AppData\\Roaming\\bun".to_string())
    } else {
        env::var("HOME")
            .map(|home| format!("{}/.bun/install/global", home))
            .unwrap_or_else(|_| "~/.bun/install/global".to_string())
    };
    
    let package_json_path = if cfg!(target_os = "windows") {
        format!("{}\\package.json", bun_global_dir)
    } else {
        format!("{}/package.json", bun_global_dir)
    };
    let package_json_content = std::fs::read_to_string(package_json_path).unwrap_or_default();
    let package_json: serde_json::Value =
        serde_json::from_str(&package_json_content).unwrap_or_default();

    let empty_map = serde_json::Map::new();
    let packages = package_json["dependencies"]
        .as_object()
        .unwrap_or(&empty_map);
    for (package_name, _) in packages {
        let node_modules_dir = if cfg!(target_os = "windows") {
            format!("{}\\node_modules", bun_global_dir)
        } else {
            format!("{}/node_modules", bun_global_dir)
        };
        let package_json_path = if cfg!(target_os = "windows") {
            format!("{}\\{}\\package.json", node_modules_dir, package_name)
        } else {
            format!("{}/{}/package.json", node_modules_dir, package_name)
        };
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

// Similar to map_bin_name_to_npm_package_name but for homebrew
fn map_bin_name_to_homebrew_package_name(bin_name: &str) -> String {
    // Get all installed packages in one call
    let installed_packages = Command::new("brew")
        .args(&["list", "--formula"])
        .output()
        .ok()
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string());
    
    if let Some(installed_list) = installed_packages {
        // Convert to a set for O(1) lookup
        let installed_set: std::collections::HashSet<&str> = installed_list.lines()
            .map(|line| line.trim())
            .filter(|line| !line.is_empty())
            .collect();
        
        // Use `brew which-formula` to find which packages provide the binary
        let candidates = Command::new("brew")
            .args(&["which-formula", bin_name])
            .output()
            .ok()
            .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string());
        
        if let Some(candidates) = candidates {
            if !candidates.is_empty() && !candidates.contains("Error") {
                // Find the first candidate that is installed
                for candidate in candidates.lines() {
                    let candidate = candidate.trim();
                    if candidate.is_empty() {
                        continue;
                    }
                    
                    if installed_set.contains(candidate) {
                        return candidate.to_string();
                    }
                }
            }
        }
    }
    
    // If we can't find the package that provides the binary, fall back to the bin name
    bin_name.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_binary_path_cross_platform() {
        // This test will use the appropriate command based on the OS
        // On Unix systems, it uses "which", on Windows it uses "where"
        
        // Test with a commonly available binary (cargo, since this is a Rust project)
        match find_binary_path("cargo") {
            Ok(path) => {
                assert!(!path.is_empty());
                assert!(path.contains("cargo"));
                
                if cfg!(target_os = "windows") {
                    // On Windows, paths typically contain backslashes and .exe extensions
                    assert!(path.contains(".exe") || path.ends_with("cargo"));
                } else {
                    // On Unix systems, paths use forward slashes
                    assert!(path.contains("/"));
                }
            }
            Err(_) => {
                // If cargo is not found, that's also valid for testing
                // The important part is that the function doesn't panic
            }
        }
    }

    #[test]
    fn test_find_binary_path_nonexistent() {
        // Test with a binary that doesn't exist
        let result = find_binary_path("this-binary-definitely-does-not-exist-12345");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not found"));
    }

    #[test]
    fn test_path_normalization() {
        // Test Windows path normalization
        let windows_path = "C:\\Users\\Test\\.cargo\\bin\\update-bin.exe";
        let normalized = windows_path.replace('\\', "/");
        assert_eq!(normalized, "C:/Users/Test/.cargo/bin/update-bin.exe");
        
        // Test Unix path (should remain unchanged)
        let unix_path = "/home/user/.cargo/bin/update-bin";
        let normalized = unix_path.replace('\\', "/");
        assert_eq!(normalized, "/home/user/.cargo/bin/update-bin");
    }

    #[test]
    fn test_cargo_path_detection() {
        // Test that we can detect cargo paths on both Windows and Unix
        let unix_cargo_path = "/home/user/.cargo/bin/some-binary";
        let windows_cargo_path = "C:\\Users\\User\\.cargo\\bin\\some-binary.exe";
        
        // Test Unix path
        let normalized_unix = unix_cargo_path.replace('\\', "/");
        assert!(normalized_unix.contains("/.cargo/bin/"));
        
        // Test Windows path
        let normalized_windows = windows_cargo_path.replace('\\', "/");
        assert!(normalized_windows.contains("/.cargo/bin/"));
        
        // Test Windows-specific detection
        if cfg!(target_os = "windows") {
            assert!(windows_cargo_path.to_lowercase().contains("\\.cargo\\bin\\"));
        }
    }

    #[test]
    fn test_bun_path_detection() {
        // Test Bun path detection for different platforms
        let unix_bun_path = "/home/user/.bun/bin/some-binary";
        let windows_bun_path = "C:\\Users\\User\\AppData\\Roaming\\bun\\bin\\some-binary.exe";
        
        // Test Unix path
        let normalized_unix = unix_bun_path.replace('\\', "/");
        assert!(normalized_unix.contains("/.bun/"));
        
        // Test Windows path
        let normalized_windows = windows_bun_path.replace('\\', "/");
        assert!(normalized_windows.to_lowercase().contains("appdata/roaming/bun"));
    }

    #[test]
    fn test_get_update_command() {
        // Test that get_update_command returns the correct commands for each package manager
        let test_cases = vec![
            ("homebrew", "test-package", ("brew", vec!["upgrade", "test-package"])),
            ("npm", "test-package", ("npm", vec!["update", "-g", "test-package"])),
            ("pnpm", "test-package", ("pnpm", vec!["update", "-g", "test-package"])),
            ("yarn", "test-package", ("yarn", vec!["global", "upgrade", "test-package"])),
            ("cargo", "test-package", ("cargo", vec!["install", "test-package"])),
            ("bun", "test-package", ("bun", vec!["update", "-g", "test-package"])),
        ];

        for (pm_name, package_name, expected) in test_cases {
            let result = get_update_command(pm_name, package_name);
            assert!(result.is_ok());
            
            let (command, args) = result.unwrap();
            assert_eq!(command, expected.0);
            assert_eq!(args, expected.1.iter().map(|s| s.to_string()).collect::<Vec<String>>());
        }
        
        // Test unsupported package manager
        let result = get_update_command("unsupported", "test");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Unsupported package manager"));
    }

    #[test]
    fn test_home_directory_detection() {
        // Test that we can detect the appropriate directory for different platforms
        if cfg!(target_os = "windows") {
            // On Windows, we should use APPDATA
            if let Ok(appdata) = env::var("APPDATA") {
                let bun_dir = format!("{}\\bun", appdata);
                assert!(bun_dir.contains("bun"));
                assert!(bun_dir.contains("\\"));
            }
        } else {
            // On Unix systems, we should use HOME
            if let Ok(home) = env::var("HOME") {
                let bun_dir = format!("{}/.bun/install/global", home);
                assert!(bun_dir.contains(".bun"));
                assert!(bun_dir.contains("/"));
            }
        }
    }
}