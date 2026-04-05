#[cfg(any(target_os = "linux", target_os = "android"))]
pub(crate) fn physmem() -> u64 {
    unsafe {
        let pages = libc::sysconf(libc::_SC_PHYS_PAGES);
        let page_size = libc::sysconf(libc::_SC_PAGESIZE);
        if pages <= 0 || page_size <= 0 {
            return 0;
        }

        (pages as u64).checked_mul(page_size as u64).unwrap_or(0)
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn physmem() -> u64 {
    unsafe {
        let mut value = 0u64;
        let mut size = core::mem::size_of::<u64>();
        let name = b"hw.memsize\0";
        let ret = libc::sysctlbyname(
            name.as_ptr().cast(),
            (&mut value as *mut u64).cast(),
            &mut size,
            core::ptr::null_mut(),
            0,
        );
        if ret == 0 { value } else { 0 }
    }
}

#[cfg(target_os = "windows")]
pub(crate) fn physmem() -> u64 {
    #[repr(C)]
    struct MemoryStatusEx {
        dw_length: u32,
        dw_memory_load: u32,
        ull_total_phys: u64,
        ull_avail_phys: u64,
        ull_total_page_file: u64,
        ull_avail_page_file: u64,
        ull_total_virtual: u64,
        ull_avail_virtual: u64,
        ull_avail_extended_virtual: u64,
    }

    extern "system" {
        fn GlobalMemoryStatusEx(lp_buffer: *mut MemoryStatusEx) -> i32;
    }

    unsafe {
        let mut status = MemoryStatusEx {
            dw_length: core::mem::size_of::<MemoryStatusEx>() as u32,
            dw_memory_load: 0,
            ull_total_phys: 0,
            ull_avail_phys: 0,
            ull_total_page_file: 0,
            ull_avail_page_file: 0,
            ull_total_virtual: 0,
            ull_avail_virtual: 0,
            ull_avail_extended_virtual: 0,
        };
        if GlobalMemoryStatusEx(&mut status) != 0 {
            status.ull_total_phys
        } else {
            0
        }
    }
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
