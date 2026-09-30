//! `sillok` binary.

// jemalloc per the house stack. Its background purge thread starts lazily
// and the process usually exits within milliseconds, so the cost is
// negligible, and C code (bundled SQLite) allocates through it too.
#[global_allocator]
static GLOBAL: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;

fn main() {
    std::process::exit(sillok::app::run_from_env());
}
