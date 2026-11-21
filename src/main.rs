//! `sillok` binary.
//!
//! The system allocator is used on every platform. A command lives for about
//! a millisecond and allocates little, so a replacement allocator's startup
//! work costs more than its faster allocation paths return.

fn main() {
    std::process::exit(sillok::app::run_from_env());
}
