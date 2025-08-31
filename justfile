build:
    cargo build

run:
    cargo run

run-binary: build
    target/debug/simple-file-picker-ratatui-rust

cgdb:
    cgdb -d rust-gdb -p (ps -l | where command =~ 'simple.*rust' | get pid | first)
