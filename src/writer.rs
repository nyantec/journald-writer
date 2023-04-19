use std::convert::{TryFrom, TryInto};
use std::fmt;
use std::fs::{rename, OpenOptions};
use std::io::Write;
use std::path::Path;

use anyhow::{bail, Context, Result};
use chrono::TimeZone;
use journald::JournalEntry;

pub(crate) fn write_log_line<W: Write, P: AsRef<Path>>(
	log: JournalEntry,
	writer: &mut W,
	cursor_path: P,
	cursor_update: bool
) -> Result<()> {
	let time = log
		.get_reception_wallclock_time()
		.context("Failed to get wallcklock time from systemd")?
		.timestamp_us;
	let time =
		chrono::NaiveDateTime::from_timestamp(time / 1_000 / 1_000, time as u32 % 1_000 % 1_000);
	let time_utc: chrono::DateTime<chrono::Utc> = chrono::DateTime::from_utc(time, chrono::Utc);
	let time_local = chrono::Local.from_utc_datetime(&time);

	// default to emerge
	let prio = log
		.get_field("PRIORITY")
		.map(|v| v.try_into().ok())
		.flatten()
		.unwrap_or(Priority::Emerg);

	let hostname = log.get_field("_HOSTNAME").unwrap_or("airlink");

	let identifier = log.get_field("SYSLOG_IDENTIFIER").unwrap_or("");

	writeln!(
		writer,
		"{utc_time} {local_time} [{severity}] {unit_name}: {identifier}: {log_line}",
		utc_time = time_utc.to_rfc3339_opts(chrono::SecondsFormat::Secs, false),
		local_time = time_local.to_rfc3339_opts(chrono::SecondsFormat::Secs, false),
		severity = prio,
		unit_name = hostname,
		identifier = identifier,
		log_line = log
			.get_message()
			.context("No log line could be read from systemd")?,
	)
	.context("write to log_writer")?;

	writer.flush().context("Flushing writer")?;

	if (cursor_update) {
		if let Some(cursor) = log.get_field("__CURSOR") {
			write_cursor(cursor, cursor_path)?;
		}
	}

	//write_cursor()

	Ok(())
}

fn write_cursor<P: AsRef<Path>>(cursor: &str, cursor_path: P) -> Result<()> {
	let mut tmp_file = cursor_path.as_ref().to_path_buf();
	tmp_file.set_extension("~");
	let path = tmp_file.display().to_string();
	let mut file = OpenOptions::new()
		.create(true)
		.write(true)
		.append(false)
		.open(&tmp_file)
		.with_context(|| format!("Open cursor file: {}", path))?;
	drop(path);

	file.write_all(cursor.as_bytes())
		.context("Writing cursor")?;

	rename(&tmp_file, cursor_path.as_ref()).context("Moving cursor file")?;

	Ok(())
}

#[repr(u8)]
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Priority {
	Emerg = 0,
	Alert,
	Crit,
	Err,
	Warning,
	Notice,
	Info,
	Debug,
}

impl TryFrom<u8> for Priority {
	type Error = anyhow::Error;

	fn try_from(value: u8) -> std::result::Result<Self, Self::Error> {
		match value {
			0 => Ok(Priority::Emerg),
			1 => Ok(Priority::Alert),
			2 => Ok(Priority::Crit),
			3 => Ok(Priority::Err),
			4 => Ok(Priority::Warning),
			5 => Ok(Priority::Notice),
			6 => Ok(Priority::Info),
			7 => Ok(Priority::Debug),
			_ => bail!("Priority {} not supported", value),
		}
	}
}

impl TryFrom<&str> for Priority {
	type Error = anyhow::Error;

	fn try_from(value: &str) -> std::result::Result<Self, Self::Error> {
		let value: u8 = value.parse()?;
		Self::try_from(value)
	}
}

impl fmt::Display for Priority {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		let msg = match self {
			&Priority::Emerg => "Emergency",
			&Priority::Alert => "Alert",
			&Priority::Crit => "Critical",
			&Priority::Err => "Error",
			&Priority::Warning => "Warning",
			&Priority::Notice => "Notice",
			&Priority::Info => "Informational",
			&Priority::Debug => "Debug",
		};

		write!(f, "{}", msg)
	}
}
