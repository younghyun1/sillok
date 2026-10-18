//! `sillok` binary.

// Keep jemalloc on its supported desktop targets. Windows uses the system
// allocator so installation does not require jemalloc's Unix build tools.
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[global_allocator]
static GLOBAL: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;

fn main() {
    std::process::exit(sillok::app::run_from_env());
}
