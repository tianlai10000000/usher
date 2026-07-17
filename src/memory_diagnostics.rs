use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

const SAMPLE_INTERVAL: Duration = Duration::from_secs(5);

#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct ProcessMemorySnapshot {
    pub rss_current_bytes: u64,
    pub rss_peak_bytes: u64,
    pub phys_footprint_bytes: u64,
    pub virtual_size_bytes: u64,
    pub internal_bytes: u64,
    pub external_bytes: u64,
    pub reusable_bytes: u64,
    pub compressed_bytes: u64,
    pub region_count: u64,
    pub mach_port_count: u64,
    pub thread_count: u64,
    pub running_thread_count: u64,
    pub page_fault_count: u64,
    pub page_in_count: u64,
}

struct DiagnosticState {
    started_at: Instant,
    baseline: ProcessMemorySnapshot,
    previous: ProcessMemorySnapshot,
}

static DIAGNOSTIC_STATE: OnceLock<Mutex<DiagnosticState>> = OnceLock::new();
static SAMPLER_STARTED: AtomicBool = AtomicBool::new(false);
static SAMPLER_STOP: AtomicBool = AtomicBool::new(false);
static SAMPLE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub(crate) fn diagnostics_enabled() -> bool {
    cfg!(debug_assertions)
        || std::env::var("USHER_MEMORY_DIAGNOSTICS")
            .map(|value| matches!(value.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"))
            .unwrap_or(false)
}

/// Capture the baseline before the Slint window and renderer are constructed.
/// Logging starts later, after the existing logger has been initialized.
pub(crate) fn initialize_baseline() {
    if !diagnostics_enabled() {
        return;
    }
    let snapshot = process_memory_snapshot();
    let _ = DIAGNOSTIC_STATE.set(Mutex::new(DiagnosticState {
        started_at: Instant::now(),
        baseline: snapshot,
        previous: snapshot,
    }));
}

pub(crate) fn process_memory_snapshot() -> ProcessMemorySnapshot {
    #[cfg(target_os = "macos")]
    {
        return macos_process_memory_snapshot();
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        return unix_process_memory_snapshot();
    }

    #[allow(unreachable_code)]
    ProcessMemorySnapshot::default()
}

pub(crate) fn log_checkpoint(reason: &str) {
    if !diagnostics_enabled() {
        return;
    }
    log_snapshot(reason, process_memory_snapshot());
}

pub(crate) fn start_periodic_logging() {
    if !diagnostics_enabled() || SAMPLER_STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    SAMPLER_STOP.store(false, Ordering::SeqCst);
    log::info!(
        "[Memory][Diag] periodic sampler started: interval_ms={} debug_build={} slint_backend_env={}",
        SAMPLE_INTERVAL.as_millis(),
        cfg!(debug_assertions),
        std::env::var("SLINT_BACKEND").unwrap_or_else(|_| "auto".to_string()),
    );
    crate::app_runtime::shared_runtime().spawn(async {
        let mut interval = tokio::time::interval(SAMPLE_INTERVAL);
        // The startup checkpoints already cover the immediate sample.
        interval.tick().await;
        loop {
            interval.tick().await;
            if SAMPLER_STOP.load(Ordering::SeqCst) {
                break;
            }
            log_checkpoint("periodic");
        }
        log::info!("[Memory][Diag] periodic sampler stopped");
    });
}

pub(crate) fn stop_periodic_logging() {
    if !SAMPLER_STARTED.load(Ordering::SeqCst) {
        return;
    }
    log_checkpoint("shutdown.before_memory_sampler_stop");
    SAMPLER_STOP.store(true, Ordering::SeqCst);
}

fn log_snapshot(reason: &str, snapshot: ProcessMemorySnapshot) {
    let state = DIAGNOSTIC_STATE.get_or_init(|| {
        Mutex::new(DiagnosticState {
            started_at: Instant::now(),
            baseline: snapshot,
            previous: snapshot,
        })
    });
    let (elapsed_ms, delta_previous, delta_baseline) = match state.lock() {
        Ok(mut state) => {
            let values = (
                state.started_at.elapsed().as_millis(),
                signed_delta(snapshot.rss_current_bytes, state.previous.rss_current_bytes),
                signed_delta(snapshot.rss_current_bytes, state.baseline.rss_current_bytes),
            );
            state.previous = snapshot;
            values
        }
        Err(_) => (0, 0, 0),
    };
    let sequence = SAMPLE_SEQUENCE.fetch_add(1, Ordering::Relaxed) + 1;
    log::info!(
        "[Memory][Diag] seq={} reason={} elapsed_ms={} rss_mib={:.2} rss_delta_last_mib={:+.2} rss_delta_baseline_mib={:+.2} rss_peak_mib={:.2} phys_footprint_mib={:.2} virtual_mib={:.2} internal_mib={:.2} external_mib={:.2} reusable_mib={:.2} compressed_mib={:.2} regions={} mach_ports={} threads={} running_threads={} faults={} pageins={}",
        sequence,
        reason,
        elapsed_ms,
        mib(snapshot.rss_current_bytes),
        mib_signed(delta_previous),
        mib_signed(delta_baseline),
        mib(snapshot.rss_peak_bytes),
        mib(snapshot.phys_footprint_bytes),
        mib(snapshot.virtual_size_bytes),
        mib(snapshot.internal_bytes),
        mib(snapshot.external_bytes),
        mib(snapshot.reusable_bytes),
        mib(snapshot.compressed_bytes),
        snapshot.region_count,
        snapshot.mach_port_count,
        snapshot.thread_count,
        snapshot.running_thread_count,
        snapshot.page_fault_count,
        snapshot.page_in_count,
    );
}

fn signed_delta(current: u64, previous: u64) -> i128 {
    i128::from(current) - i128::from(previous)
}

fn mib(bytes: u64) -> f64 {
    bytes as f64 / 1_048_576.0
}

fn mib_signed(bytes: i128) -> f64 {
    bytes as f64 / 1_048_576.0
}

#[cfg(target_os = "macos")]
fn macos_process_memory_snapshot() -> ProcessMemorySnapshot {
    use std::ffi::c_void;

    #[repr(C)]
    struct TaskVmInfo {
        virtual_size: u64,
        region_count: i32,
        page_size: i32,
        resident_size: u64,
        resident_size_peak: u64,
        device: u64,
        device_peak: u64,
        internal: u64,
        internal_peak: u64,
        external: u64,
        external_peak: u64,
        reusable: u64,
        reusable_peak: u64,
        purgeable_volatile_pmap: u64,
        purgeable_volatile_resident: u64,
        purgeable_volatile_virtual: u64,
        compressed: u64,
        compressed_peak: u64,
        compressed_lifetime: u64,
        phys_footprint: u64,
    }

    #[repr(C)]
    #[derive(Default)]
    struct ProcTaskInfo {
        virtual_size: u64,
        resident_size: u64,
        total_user: u64,
        total_system: u64,
        threads_user: u64,
        threads_system: u64,
        policy: i32,
        faults: i32,
        pageins: i32,
        cow_faults: i32,
        messages_sent: i32,
        messages_received: i32,
        syscalls_mach: i32,
        syscalls_unix: i32,
        context_switches: i32,
        thread_count: i32,
        running_thread_count: i32,
        priority: i32,
    }

    unsafe extern "C" {
        fn mach_task_self() -> u32;
        fn task_info(
            target_task: u32,
            flavor: i32,
            task_info_out: *mut i32,
            task_info_out_count: *mut u32,
        ) -> i32;
        fn proc_pidinfo(
            pid: i32,
            flavor: i32,
            arg: u64,
            buffer: *mut c_void,
            buffer_size: i32,
        ) -> i32;
        fn mach_port_names(
            task: u32,
            names: *mut *mut u32,
            names_count: *mut u32,
            types: *mut *mut u32,
            types_count: *mut u32,
        ) -> i32;
        fn vm_deallocate(target_task: u32, address: usize, size: usize) -> i32;
    }

    const TASK_VM_INFO: i32 = 22;
    const PROC_PIDTASKINFO: i32 = 4;
    const KERN_SUCCESS: i32 = 0;

    let mut snapshot = ProcessMemorySnapshot::default();
    let mut vm_info = std::mem::MaybeUninit::<TaskVmInfo>::uninit();
    let mut count = (std::mem::size_of::<TaskVmInfo>() / std::mem::size_of::<i32>()) as u32;
    let task_result = unsafe {
        task_info(
            mach_task_self(),
            TASK_VM_INFO,
            vm_info.as_mut_ptr() as *mut i32,
            &mut count,
        )
    };
    if task_result == KERN_SUCCESS {
        let info = unsafe { vm_info.assume_init() };
        snapshot.rss_current_bytes = info.resident_size;
        snapshot.rss_peak_bytes = info.resident_size_peak;
        snapshot.phys_footprint_bytes = info.phys_footprint;
        snapshot.virtual_size_bytes = info.virtual_size;
        snapshot.internal_bytes = info.internal;
        snapshot.external_bytes = info.external;
        snapshot.reusable_bytes = info.reusable;
        snapshot.compressed_bytes = info.compressed;
        snapshot.region_count = info.region_count.max(0) as u64;
    }

    let mut task = ProcTaskInfo::default();
    let task_size = std::mem::size_of::<ProcTaskInfo>() as i32;
    let bytes = unsafe {
        proc_pidinfo(
            libc::getpid(),
            PROC_PIDTASKINFO,
            0,
            &mut task as *mut ProcTaskInfo as *mut c_void,
            task_size,
        )
    };
    if bytes == task_size {
        if snapshot.rss_current_bytes == 0 {
            snapshot.rss_current_bytes = task.resident_size;
            snapshot.virtual_size_bytes = task.virtual_size;
        }
        snapshot.thread_count = task.thread_count.max(0) as u64;
        snapshot.running_thread_count = task.running_thread_count.max(0) as u64;
        snapshot.page_fault_count = task.faults.max(0) as u64;
        snapshot.page_in_count = task.pageins.max(0) as u64;
    }

    let task_self = unsafe { mach_task_self() };
    let mut port_names = std::ptr::null_mut::<u32>();
    let mut port_name_count = 0u32;
    let mut port_types = std::ptr::null_mut::<u32>();
    let mut port_type_count = 0u32;
    let port_result = unsafe {
        mach_port_names(
            task_self,
            &mut port_names,
            &mut port_name_count,
            &mut port_types,
            &mut port_type_count,
        )
    };
    if port_result == KERN_SUCCESS {
        snapshot.mach_port_count = port_name_count as u64;
        if !port_names.is_null() {
            let _ = unsafe {
                vm_deallocate(
                    task_self,
                    port_names as usize,
                    port_name_count as usize * std::mem::size_of::<u32>(),
                )
            };
        }
        if !port_types.is_null() {
            let _ = unsafe {
                vm_deallocate(
                    task_self,
                    port_types as usize,
                    port_type_count as usize * std::mem::size_of::<u32>(),
                )
            };
        }
    }

    snapshot
}

#[cfg(test)]
mod tests {
    use super::{mib, mib_signed, signed_delta};

    #[test]
    fn reports_signed_memory_deltas_in_both_directions() {
        assert_eq!(signed_delta(20, 12), 8);
        assert_eq!(signed_delta(12, 20), -8);
    }

    #[test]
    fn converts_bytes_to_mebibytes() {
        assert_eq!(mib(1_048_576), 1.0);
        assert_eq!(mib_signed(-1_048_576), -1.0);
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
fn unix_process_memory_snapshot() -> ProcessMemorySnapshot {
    let mut snapshot = ProcessMemorySnapshot::default();
    if let Ok(statm) = std::fs::read_to_string("/proc/self/statm") {
        let mut fields = statm.split_whitespace();
        let virtual_pages = fields.next().and_then(|value| value.parse::<u64>().ok());
        let resident_pages = fields.next().and_then(|value| value.parse::<u64>().ok());
        let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) }.max(0) as u64;
        snapshot.virtual_size_bytes = virtual_pages.unwrap_or(0).saturating_mul(page_size);
        snapshot.rss_current_bytes = resident_pages.unwrap_or(0).saturating_mul(page_size);
    }
    if let Ok(status) = std::fs::read_to_string("/proc/self/status") {
        snapshot.thread_count = status
            .lines()
            .find_map(|line| line.strip_prefix("Threads:"))
            .and_then(|value| value.trim().parse::<u64>().ok())
            .unwrap_or(0);
    }
    let mut usage = std::mem::MaybeUninit::<libc::rusage>::uninit();
    if unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) } == 0 {
        let usage = unsafe { usage.assume_init() };
        snapshot.rss_peak_bytes = (usage.ru_maxrss.max(0) as u64).saturating_mul(1024);
        snapshot.page_fault_count =
            (usage.ru_minflt.max(0) as u64).saturating_add(usage.ru_majflt.max(0) as u64);
        snapshot.page_in_count = usage.ru_majflt.max(0) as u64;
    }
    snapshot
}
