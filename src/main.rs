use clap::Parser;
use std::process::{Command, exit, Stdio};
use std::io::{BufRead, BufReader};

#[derive(Parser)]
#[command(name = "update-bin")]
#[command(about = "Update a binary to its latest version by using the original package manager")]
struct Args {
    bin_name: String,
}

fn main() {
    let args = Args::parse();

    match update_binary(&args.bin_name) {
        Ok(_) => {}
        Err(e) => {
            eprintln!("Error: {}", e);
            exit(1);
        }
    }
}

fn update_binary(bin_name: &str) -> Result<(), String> {
    let package_manager = detect_package_manager(bin_name)?;

    let old_version =
        get_version(bin_name, &package_manager).unwrap_or_else(|_| "unknown".to_string());
    println!("Current version: {}", old_version);

    let (command, args) = get_update_command(&package_manager, bin_name)?;

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

    let status = child.wait().map_err(|e| format!("Failed to wait for {}: {}", command, e))?;

    if !status.success() {
        return Err(format!(
            "Failed to update {} with {}",
            bin_name, package_manager
        ));
    }

    let new_version =
        get_version(bin_name, &package_manager).unwrap_or_else(|_| "unknown".to_string());

    if old_version != new_version {
        println!("Updated to version: {}", new_version);
        println!(
            "✅ Successfully updated {} from {} to {}",
            bin_name, old_version, new_version
        );
    } else {
        println!("ℹ️  {} is already up to date ({})", bin_name, old_version);
    }

    Ok(())
}

fn detect_package_manager(bin_name: &str) -> Result<String, String> {
    if let Ok(output) = Command::new("which").arg(bin_name).output() {
        if !output.status.success() {
            return Err(format!("Binary '{}' not found", bin_name));
        }

        let bin_path_raw = String::from_utf8_lossy(&output.stdout);
        let bin_path = bin_path_raw.trim();

        if bin_path.contains("/opt/homebrew/") || bin_path.contains("/usr/local/") {
            return Ok("homebrew".to_string());
        }

        if bin_path.contains("/.bun/") {
            return Ok("bun".to_string());
        }

        if bin_path.contains("/.cargo/bin/") {
            return Ok("cargo".to_string());
        }

        if bin_path.contains("/.npm/") || bin_path.contains("/node_modules/.bin/") {
            if Command::new("pnpm").arg("--version").output().is_ok() {
                if let Ok(pnpm_output) = Command::new("pnpm")
                    .args(&["list", "-g", "--depth=0"])
                    .output()
                {
                    let pnpm_list = String::from_utf8_lossy(&pnpm_output.stdout);
                    if pnpm_list.contains(bin_name) {
                        return Ok("pnpm".to_string());
                    }
                }
            }
            return Ok("npm".to_string());
        }
    }

    Err(format!(
        "Could not detect package manager for '{}'",
        bin_name
    ))
}

fn get_update_command(
    package_manager: &str,
    bin_name: &str,
) -> Result<(String, Vec<String>), String> {
    match package_manager {
        "homebrew" => Ok((
            "brew".to_string(),
            vec!["upgrade".to_string(), bin_name.to_string()],
        )),
        "bun" => Ok((
            "bun".to_string(),
            vec!["update".to_string(), "-g".to_string(), bin_name.to_string()],
        )),
        "npm" => Ok((
            "npm".to_string(),
            vec!["update".to_string(), "-g".to_string(), bin_name.to_string()],
        )),
        "pnpm" => Ok((
            "pnpm".to_string(),
            vec!["update".to_string(), "-g".to_string(), bin_name.to_string()],
        )),
        "cargo" => Ok((
            "cargo".to_string(),
            vec!["install".to_string(), bin_name.to_string()],
        )),
        _ => Err(format!("Unsupported package manager: {}", package_manager)),
    }
}

fn get_version(bin_name: &str, package_manager: &str) -> Result<String, String> {
    match package_manager {
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

fn get_node_package_version(bin_name: &str, package_manager: &str) -> Result<String, String> {
    let output = Command::new(package_manager)
        .args(&["list", "-g", "--depth=0"])
        .output()
        .map_err(|e| format!("Failed to get {} version: {}", package_manager, e))?;

    if !output.status.success() {
        return get_binary_version(bin_name);
    }

    let list_output = String::from_utf8_lossy(&output.stdout);
    for line in list_output.lines() {
        if line.contains(&format!("{}@", bin_name)) {
            let version = line
                .split('@')
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
