
srcdir = $(realpath .)

debug-dir = "${srcdir}/target/debug"
release-dir = "${srcdir}/target/release"

exec_name = "journald-writer"
debug-bin = "${debug-dir}/${exec_name}"
release-bin = "${release-dir}/${exec_name}"

-include ${debug-bin}.d
-include ${release-bin}.d

${debug-bin}:
	cargo build

${release-bin}:
	cargo build --release

gdbserve: ${debug-bin}
	sudo RUST_LOG=trace,journald=trace gdbserver :1234 ${debug-bin} config.example.yml

build.rs:

build: ${release-bin}

debug: ${debug-bin}

run-debug: ${debug-bin}
	sudo RUST_LOG=trace,journald=trace ${debug-bin} config.example.yml