//! `pg_dump` as a [`DumpSource`]: a custom-format dump streamed from the child's stdout.

use std::{
    ffi::OsString,
    fmt, io,
    pin::Pin,
    process::Stdio,
    task::{Context, Poll, ready},
};

use anyhow::Context as _;
use percent_encoding::percent_decode_str;
use tokio::{
    io::{AsyncRead, AsyncReadExt, ReadBuf},
    process::{ChildStdout, Command},
    task::JoinHandle,
};

use super::{DumpReader, DumpSource, truncate};

/// How much of `pg_dump`'s stderr is kept for the error message.
const STDERR_LIMIT: u64 = 16 * 1024;

/// Runs `pg_dump` against a database URL. The password never reaches the command line: it
/// travels in `PGPASSWORD`, the user in `PGUSER`, and the rest of the URL (host, database,
/// `sslmode`, ...) is handed to libpq as the connection URI.
pub struct PgDump {
    program: OsString,
    /// The URL without its user and password.
    uri: String,
    user: Option<String>,
    password: Option<String>,
}

impl fmt::Debug for PgDump {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PgDump").field("program", &self.program).finish_non_exhaustive()
    }
}

impl PgDump {
    /// Dumps the database at `database_url` (`postgres://user:password@host/db?...`) with the
    /// `pg_dump` found on `PATH`.
    pub fn new(database_url: &str) -> anyhow::Result<Self> {
        let mut url =
            reqwest::Url::parse(database_url).context("parsing the backup database URL")?;
        let decode = |text: &str| percent_decode_str(text).decode_utf8_lossy().into_owned();
        let user = Some(url.username()).filter(|name| !name.is_empty()).map(decode);
        let password = url.password().map(decode);
        let _ = url.set_username("");
        let _ = url.set_password(None);
        Ok(Self { program: "pg_dump".into(), uri: url.into(), user, password })
    }

    /// Runs another executable instead of `pg_dump` (for example a versioned path).
    #[must_use]
    pub fn with_program(mut self, program: impl Into<OsString>) -> Self {
        self.program = program.into();
        self
    }
}

impl DumpSource for PgDump {
    fn start(&self) -> anyhow::Result<DumpReader> {
        let mut child = Command::new(&self.program)
            .args(["--format=custom", "--no-owner", "--no-privileges"])
            .arg(format!("--dbname={}", self.uri))
            .envs(self.user.as_ref().map(|user| ("PGUSER", user)))
            .envs(self.password.as_ref().map(|password| ("PGPASSWORD", password)))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            // Dropping the stream (an upload that failed) must not leave pg_dump running.
            .kill_on_drop(true)
            .spawn()
            .map_err(|err| {
                if err.kind() == io::ErrorKind::NotFound {
                    anyhow::anyhow!("pg_dump not found: the image must include postgresql-client")
                } else {
                    anyhow::Error::new(err).context("starting pg_dump")
                }
            })?;
        let stdout = child.stdout.take().context("pg_dump has no stdout")?;
        let stderr = child.stderr.take().context("pg_dump has no stderr")?;
        // Waits for the exit and collects stderr; the stream consults it at the end of stdout.
        let exit = tokio::spawn(async move {
            let mut message = Vec::new();
            let (status, _) = tokio::join!(child.wait(), async {
                let _ = stderr.take(STDERR_LIMIT).read_to_end(&mut message).await;
            });
            match status {
                Ok(status) if status.success() => Ok(()),
                Ok(status) => Err(format!(
                    "pg_dump failed ({status}): {}",
                    truncate(String::from_utf8_lossy(&message).trim(), 1000)
                )),
                Err(err) => Err(format!("waiting for pg_dump failed: {err}")),
            }
        });
        Ok(Box::new(DumpStream { stdout, exit: Some(exit), failed: None }))
    }
}

/// `pg_dump`'s stdout, which reports a non-zero exit as a read error at the end of the stream
/// (a truncated dump must never look like a complete one).
struct DumpStream {
    stdout: ChildStdout,
    /// Resolves with the outcome once `pg_dump` exits; taken when stdout ends.
    exit: Option<JoinHandle<Result<(), String>>>,
    /// The failure to keep reporting if the stream is read again after it.
    failed: Option<String>,
}

impl AsyncRead for DumpStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = &mut *self;
        if let Some(message) = &this.failed {
            return Poll::Ready(Err(io::Error::other(message.clone())));
        }
        let Some(exit) = this.exit.as_mut() else {
            return Poll::Ready(Ok(()));
        };
        // A full buffer reads zero bytes without meaning the end.
        if buf.remaining() == 0 {
            return Poll::Ready(Ok(()));
        }
        let before = buf.filled().len();
        ready!(Pin::new(&mut this.stdout).poll_read(cx, buf))?;
        if buf.filled().len() > before {
            return Poll::Ready(Ok(()));
        }
        // End of stdout: pg_dump is done writing; how did it exit?
        let outcome = ready!(Pin::new(exit).poll(cx));
        this.exit = None;
        match outcome.unwrap_or_else(|err| Err(format!("pg_dump task failed: {err}"))) {
            Ok(()) => Poll::Ready(Ok(())),
            Err(message) => {
                this.failed = Some(message.clone());
                Poll::Ready(Err(io::Error::other(message)))
            }
        }
    }
}

impl Drop for DumpStream {
    fn drop(&mut self) {
        // Aborting drops the task's `Child`, which kills pg_dump (`kill_on_drop`).
        if let Some(exit) = &self.exit {
            exit.abort();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn drain(source: &PgDump) -> anyhow::Result<Vec<u8>> {
        let mut reader = source.start()?;
        let mut out = Vec::new();
        let _ = reader.read_to_end(&mut out).await?;
        Ok(out)
    }

    #[test]
    fn the_password_is_split_off_the_uri() {
        let dump = PgDump::new("postgres://bob:p%40ss%2Fword@db.example:5432/app?sslmode=require")
            .unwrap();
        assert_eq!(dump.uri, "postgres://db.example:5432/app?sslmode=require");
        assert_eq!(dump.user.as_deref(), Some("bob"));
        assert_eq!(dump.password.as_deref(), Some("p@ss/word"));
        assert!(!format!("{dump:?}").contains("word"));
        let bare = PgDump::new("postgres://db.example/app").unwrap();
        assert_eq!((bare.user, bare.password), (None, None));
        let _ = PgDump::new("not a url").unwrap_err();
    }

    #[tokio::test]
    async fn a_missing_binary_has_a_clear_error() {
        let dump = PgDump::new("postgres://db/app").unwrap().with_program("/nonexistent/pg_dump");
        let err = dump.start().err().unwrap();
        assert_eq!(err.to_string(), "pg_dump not found: the image must include postgresql-client");
    }

    #[tokio::test]
    async fn a_non_zero_exit_is_a_read_error() {
        // `false` ignores its arguments, prints nothing and exits 1.
        let dump = PgDump::new("postgres://db/app").unwrap().with_program("false");
        let err = drain(&dump).await.unwrap_err();
        assert!(err.to_string().contains("pg_dump failed (exit status: 1)"), "{err}");
    }
}
