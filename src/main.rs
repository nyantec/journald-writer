use std::convert::TryFrom;
use std::fs;
use std::fs::read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::{Context, Result};
use getopts::Options;
use journald::reader::{JournalFiles, JournalReader, JournalReaderConfig, JournalSeek};
use log::*;
use log_writer::{LogWriter, LogWriterConfig};
use nix::sys::signal;
use nix::sys::signal::{SigHandler, Signal};

mod writer;

static EXIT_FLAG: AtomicBool = AtomicBool::new(false);

extern "C" fn handle_sig(signal: nix::libc::c_int) {
	println!("got signal");
	let signal = Signal::try_from(signal).unwrap();
	EXIT_FLAG.store(
		signal == Signal::SIGTERM || signal == Signal::SIGHUP,
		Ordering::Relaxed,
	);
	// TODO: flush fd from cookie file
}

fn print_usage(program: &str, opts: Options) {
	let brief = format!("Usage: {} CONFIG [options]", program);
	print!("{}", opts.usage(&brief));
}

fn main() {
	if let Err(e) = main_err() {
		eprintln!("Error:");
		eprintln!("{:?}", e);
		std::process::exit(1);
	}
}

fn main_err() -> Result<()> {
	// init logger
	env_logger::init();

	// declare signal handler
	let handler = SigHandler::Handler(handle_sig);
	// SAFETY: result is not used. There as this function is a save ffi call.
	unsafe { signal::signal(Signal::SIGTERM, handler) }
		.context("Failed to install signal handler.")?;

	let args: Vec<String> = std::env::args().collect();
	let program = args[0].clone();

	let mut opts = Options::new();
	opts.optflag("h", "help", "Display this help text and exit");

	let matches = match opts.parse(&args[1..]) {
		Ok(m) => m,
		Err(f) => {
			panic!("{}", f)
		}
	};

	if matches.opt_present("h") {
		print_usage(&program, opts);
		return Ok(());
	}

	let config_path = if !matches.free.is_empty() {
		matches.free[0].clone()
	} else {
		print_usage(&program, opts);
		return Ok(());
	};
	info!("reading config file {}", config_path);

	let config_str = fs::read_to_string(&config_path).context("Reading config file")?;
	let config: Config = serde_yaml::from_str(&config_str).context("Parsing config file")?;
	
	info!("using configuration: {:?}", config);

	info!(
		"writing logs to {}, with cursor: {}",
		config.log_writer_config.target_dir.display(),
		config.cursor_file.display(),
	);
	run(config)?;

	Ok(())
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct Config {
	pub cursor_file: PathBuf,
	pub log_writer_config: LogWriterConfig,
}

pub fn run(config: Config) -> Result<()> {
	let path = config.log_writer_config.target_dir.display().to_string();
	let mut log_writer = LogWriter::new(config.log_writer_config)
		.with_context(|| format!("Creating log writer at path {}", path))?;
	drop(path);

	let mut reader = open_reader(&config.cursor_file)?;
	let mut iter = reader.as_blocking_iter();

	// This iter is blocking. There as this is blocking for loop.
	// This can mean that an exit request takes until the next log line is read
	for entry in &mut iter {
		let entry = entry.context("iterate over Journal entries")?;
		trace!("found entry: {:?}", entry);
		writer::write_log_line(entry, &mut log_writer, &config.cursor_file)?;

		if EXIT_FLAG.load(Ordering::Relaxed) {
			info!("obeying exit flag");
			break;
		}
	}

	Ok(())
}

fn open_reader<P: AsRef<Path>>(path: P) -> Result<JournalReader> {
	let config = JournalReaderConfig {
		files: JournalFiles::All,
		only_volatile: false,
		only_local: true,
	};

	let reader = JournalReader::open(&config).context("Opening journal")?;
	find_cursor(path, reader)
}

fn find_cursor<P: AsRef<Path>>(path: P, mut reader: JournalReader) -> Result<JournalReader> {
	if let Some(path) = path.as_ref().parent() {
		if !path.exists() {
			trace!("creating cursor directory");
			std::fs::create_dir_all(path).context("Creating cursor directory")?;
		}
	}

	if !path.as_ref().exists() {
		debug!("no cursor file, seeking to tail");
		reader
			.seek(JournalSeek::ThisBoot)
			.context("Seeking to journald tail")?;
		reader
			.previous_entry()
			.context("Getting previous journald entry")?;
		return Ok(reader);
	}

	let cursor =
		String::from_utf8_lossy(&std::fs::read(path.as_ref()).context("reading old cursor")?)
			.into_owned();
	debug!("recovered cursor: {}", cursor);
	reader.seek(JournalSeek::Cursor(cursor))?;
	reader
		.previous_entry()
		.context("Getting previous journald entry")?;

	Ok(reader)
}
