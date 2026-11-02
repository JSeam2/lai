use std::{
    env,
    fs::{self, File, OpenOptions},
    io::{self, BufRead, BufReader, Write},
    os::unix::{
        fs::{FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt},
        io::AsRawFd,
        net::{UnixListener, UnixStream},
    },
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

use anyhow::{Context, ensure};

fn directory() -> anyhow::Result<PathBuf> {
    let home = env::var_os("HOME").context("HOME is not set; set it to your home directory")?;
    Ok(PathBuf::from(home).join(".lai"))
}

pub struct Control {
    _lock: File,
    socket_path: PathBuf,
    socket_identity: Option<(u64, u64)>,
}

impl Control {
    pub fn acquire() -> anyhow::Result<Self> {
        let dir = directory()?;
        fs::create_dir_all(&dir)
            .with_context(|| format!("cannot create {}; check permissions", dir.display()))?;
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(dir.join("serve.lock"))
            .context("cannot open server lock; check ~/.lai permissions")?;
        // The open file holds this advisory lock for the server's lifetime.
        let result = unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        if result != 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::WouldBlock {
                anyhow::bail!("a detached LAI server is already running; use `lai kill` first");
            }
            return Err(error).context("cannot acquire the detached server lock");
        }
        Ok(Self {
            _lock: lock,
            socket_path: dir.join("serve.sock"),
            socket_identity: None,
        })
    }

    pub fn start(&mut self) -> anyhow::Result<Arc<AtomicBool>> {
        match fs::symlink_metadata(&self.socket_path) {
            Ok(metadata) => {
                ensure!(
                    metadata.file_type().is_socket(),
                    "{} exists and is not a socket; move it before starting the server",
                    self.socket_path.display()
                );
                match UnixStream::connect(&self.socket_path) {
                    Ok(_) => anyhow::bail!(
                        "a control socket is already active at {}; stop that server first",
                        self.socket_path.display()
                    ),
                    Err(error) if error.kind() == io::ErrorKind::ConnectionRefused => {
                        fs::remove_file(&self.socket_path)
                            .context("cannot remove the stale server socket")?
                    }
                    Err(error) => {
                        return Err(error).context("cannot check the existing server socket");
                    }
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error).context("cannot inspect the server control socket"),
        }
        let listener = UnixListener::bind(&self.socket_path)
            .context("cannot create the detached server control socket")?;
        let metadata = fs::symlink_metadata(&self.socket_path)?;
        self.socket_identity = Some((metadata.dev(), metadata.ino()));
        fs::set_permissions(&self.socket_path, fs::Permissions::from_mode(0o600))
            .context("cannot secure the server control socket")?;
        let stopped = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stopped);
        thread::spawn(move || {
            for stream in listener.incoming() {
                let mut stream = match stream {
                    Ok(stream) => stream,
                    Err(error) => {
                        log::error!("Server control socket failed: {error}");
                        break;
                    }
                };
                if let Err(error) = stream.set_read_timeout(Some(Duration::from_secs(2))) {
                    log::warn!("Cannot set control timeout: {error}");
                    continue;
                }
                let mut request = String::new();
                let read = BufReader::new(&mut stream).read_line(&mut request);
                if read.is_ok() && request == "shutdown\n" {
                    flag.store(true, Ordering::Release);
                    if let Err(error) = stream.write_all(b"stopping\n") {
                        log::warn!("Cannot acknowledge shutdown: {error}");
                    }
                    break;
                }
            }
        });
        Ok(stopped)
    }
}

impl Drop for Control {
    fn drop(&mut self) {
        if let (Some((dev, ino)), Ok(metadata)) = (
            self.socket_identity,
            fs::symlink_metadata(&self.socket_path),
        ) && metadata.file_type().is_socket()
            && (metadata.dev(), metadata.ino()) == (dev, ino)
            && let Err(error) = fs::remove_file(&self.socket_path)
        {
            log::warn!("Cannot remove server control socket: {error}");
        }
    }
}

pub fn stop() -> anyhow::Result<()> {
    let path = directory()?.join("serve.sock");
    let mut stream = UnixStream::connect(&path).with_context(|| {
        format!(
            "no detached LAI server is reachable at {}; start one with `lai serve -d`",
            path.display()
        )
    })?;
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .context("cannot configure the shutdown timeout")?;
    stream
        .write_all(b"shutdown\n")
        .context("cannot send the shutdown request")?;
    let mut response = String::new();
    BufReader::new(stream)
        .read_line(&mut response)
        .context("the server did not acknowledge shutdown; check ~/.lai/serve.log")?;
    ensure!(
        response == "stopping\n",
        "unexpected response from the server control socket; check ~/.lai/serve.log"
    );
    log::info!("Stopping the detached LAI server; the current request will finish first.");
    Ok(())
}
