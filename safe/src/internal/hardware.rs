#[cfg(any(target_os = "linux", target_os = "android"))]
pub(crate) fn physmem() -> u64 {
    let Ok(meminfo) = std::fs::read_to_string("/proc/meminfo") else {
        return 0;
    };

    for line in meminfo.lines() {
        if let Some(rest) = line.strip_prefix("MemTotal:") {
            let Some(kib) = rest
                .split_whitespace()
                .next()
                .and_then(|value| value.parse::<u64>().ok())
            else {
                break;
            };

            return kib.checked_mul(1024).unwrap_or(0);
        }
    }

    0
}

#[cfg(target_os = "macos")]
pub(crate) fn physmem() -> u64 {
    let Ok(output) = std::process::Command::new("sysctl")
        .args(["-n", "hw.memsize"])
        .output()
    else {
        return 0;
    };

    if !output.status.success() {
        return 0;
    }

    std::str::from_utf8(&output.stdout)
        .ok()
        .and_then(|text| text.trim().parse::<u64>().ok())
        .unwrap_or(0)
}

#[cfg(target_os = "windows")]
pub(crate) fn physmem() -> u64 {
    0
}

#[cfg(not(any(
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    target_os = "windows"
)))]
pub(crate) fn physmem() -> u64 {
    0
}

pub(crate) fn cputhreads() -> u32 {
    std::thread::available_parallelism()
        .ok()
        .map(|threads| threads.get())
        .and_then(|threads| u32::try_from(threads).ok())
        .unwrap_or(0)
}
