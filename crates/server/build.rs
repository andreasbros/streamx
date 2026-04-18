use std::process::Command;
use std::time::SystemTime;

fn main() {
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    // Convert to YYMMDDHHMMSS manually
    let secs = now as i64;
    let days = secs / 86400;
    let time_of_day = secs % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    // Days since epoch to date (simplified)
    let mut y = 1970i64;
    let mut remaining = days;
    loop {
        let days_in_year = if y % 4 == 0 && (y % 100 != 0 || y % 400 == 0) { 366 } else { 365 };
        if remaining < days_in_year { break; }
        remaining -= days_in_year;
        y += 1;
    }
    let leap = y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
    let month_days = [31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut m = 0usize;
    for md in &month_days {
        if remaining < *md { break; }
        remaining -= md;
        m += 1;
    }
    let d = remaining + 1;
    let timestamp = format!("{:02}{:02}{:02}{:02}{:02}{:02}", y % 100, m + 1, d, hours, minutes, seconds);
    let version = format!("0.1.0-{timestamp}");

    // Short hash
    let hash = simple_hash(&format!("{version}-{now}"));
    let short_hash = format!("{:08x}", hash);

    println!("cargo:rustc-env=STREAMX_VERSION={version}");
    println!("cargo:rustc-env=STREAMX_BUILD_HASH={short_hash}");
    println!("cargo:rustc-env=STREAMX_BUILD_TIMESTAMP={now}");

    // Re-run on every build
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../../web/dist/index.html");

    if let Ok(output) = Command::new("git").args(["rev-parse", "--short", "HEAD"]).output() {
        let commit = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !commit.is_empty() {
            println!("cargo:rustc-env=STREAMX_GIT_COMMIT={commit}");
        }
    }
}

fn simple_hash(s: &str) -> u32 {
    let mut h: u32 = 5381;
    for b in s.bytes() {
        h = h.wrapping_mul(33).wrapping_add(b as u32);
    }
    h
}
